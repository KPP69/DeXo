//! Hyperliquid adapter (Phase 4): mainnet only, API wallet (agent) only.
//!
//! Verified against the official API documentation on 2026-11-21 and
//! implemented on top of hypersdk 0.3.1 (see Cargo.toml for the
//! evaluation note). Design points:
//!
//! - The main wallet never enters this process. Queries use the master
//!   account address; signing uses only the agent (API wallet) key,
//!   which the user authorises once outside DeXo (approveAgent in the
//!   Hyperliquid frontend).
//! - Writes are gated twice: a signer must be configured AND writes must
//!   be explicitly enabled for the account (`writes_enabled`, default
//!   false). Until both hold, order operations return an explicit error
//!   instead of touching the venue.
//! - Hyperliquid has no native market order: a market order is an IOC
//!   limit order at a protection price, so `OrderType::Market` requires
//!   `OrderRequest::price` as the worst acceptable price.
//! - `client_order_id` values for this venue are cloids: `0x` followed
//!   by 32 hex characters (128 bit). Use [`new_client_order_id`] to
//!   generate them.
//! - HIP-3 namespace venues (for example Entropy, dex `io`) are served
//!   by this same adapter with `dex` set; the product-level exchange id
//!   stays distinct (e.g. "entropy").
//! - hypersdk 0.3.1 exposes no HTTP l2Book, so the order book snapshot
//!   uses a direct `/info` request; everything else goes through the SDK.
//! - SDK info errors arrive as plain text (`ApiError`), so error
//!   classification is by message content — coarse, and documented here
//!   rather than hidden.

use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use hypersdk::hypercore::types::{
    BatchCancelCloid, BatchOrder, CancelByCloid, OrderGrouping, OrderResponseStatus,
    OrderTypePlacement, TimeInForce,
};
use hypersdk::hypercore::{Cloid, HttpClient, OidOrCloid, PerpMarket, PrivateKeySigner};
use hypersdk::Address;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::Deserialize;

use crate::accounts::credential_store::Credentials;
use crate::exchanges::traits::ExchangeAdapter;
use crate::models::{
    Balance, BookLevel, ExchangeError, Fill, FundingInfo, Order, OrderBook, OrderRequest,
    OrderStatus, OrderType, Position, Side, TimestampMs,
};

const MAINNET_INFO_URL: &str = "https://api.hyperliquid.xyz/info";
/// Credential field holding the agent (API wallet) private key.
pub const AGENT_KEY_FIELD: &str = "agent_private_key";

/// Error code used when the venue has no order for a cloid. Shared so
/// reconciliation can recognise the case without stringly-typed drift.
pub const ORDER_NOT_FOUND_CODE: &str = "ORDER_NOT_FOUND";

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Generates a cloid-shaped client order id: `0x` + 32 hex chars,
/// combining the current time with a process-local counter so ids are
/// unique within a run and roughly time-ordered.
pub fn new_client_order_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    let stamp = (now_ms() as u128) << 64;
    format!("0x{:032x}", stamp | (seq & 0xffff_ffff))
}

/// Parses a cloid string (`0x` + 32 hex chars) into the SDK's B128 form.
pub fn parse_cloid(client_order_id: &str) -> Result<Cloid, ExchangeError> {
    let hex = client_order_id.strip_prefix("0x").ok_or_else(|| {
        ExchangeError::InvalidRequest(format!(
            "client_order_id '{client_order_id}' is not a Hyperliquid cloid (expected 0x + 32 hex chars)"
        ))
    })?;
    if hex.len() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ExchangeError::InvalidRequest(format!(
            "client_order_id '{client_order_id}' is not a Hyperliquid cloid (expected 0x + 32 hex chars)"
        )));
    }
    let mut bytes = [0u8; 16];
    for i in 0..16 {
        bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| ExchangeError::InvalidRequest("invalid cloid hex".into()))?;
    }
    Ok(Cloid::from(bytes))
}

fn format_cloid(cloid: Cloid) -> String {
    format!("{cloid:#x}")
}

/// Classifies the SDK's info errors. Coarse by necessity: hypersdk
/// 0.3.1 returns `anyhow::Error` with plain-text messages for /info, so
/// classification is by message content.
fn map_api_error(error: impl std::fmt::Display) -> ExchangeError {
    let text = error.to_string();
    let lower = text.to_lowercase();
    if lower.contains("rate limit") || lower.contains("429") || lower.contains("too many requests")
    {
        ExchangeError::RateLimited
    } else if lower.contains("timed out") || lower.contains("timeout") {
        ExchangeError::Timeout
    } else if lower.contains("error sending request") || lower.contains("connection") {
        ExchangeError::Network(text)
    } else {
        ExchangeError::Remote {
            code: None,
            message: text,
        }
    }
}

/// Classifies a failed write action. A failed submission is ambiguous
/// unless the venue clearly rejected it, so the default is `Unknown`:
/// the caller must query before doing anything else.
fn map_action_error<T: std::fmt::Debug>(
    error: hypersdk::hypercore::ActionError<T>,
) -> ExchangeError {
    let text = error.message().to_string();
    let lower = text.to_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        ExchangeError::Timeout
    } else if lower.contains("rate limit") {
        ExchangeError::RateLimited
    } else {
        ExchangeError::Unknown(text)
    }
}

pub struct HyperliquidAdapter {
    exchange_id: String,
    account_id: String,
    user_address: Address,
    signer: Option<PrivateKeySigner>,
    writes_enabled: bool,
    dex: Option<String>,
    client: HttpClient,
    http: reqwest::Client,
    markets: RwLock<Option<Vec<PerpMarket>>>,
    last_nonce: AtomicU64,
}

impl HyperliquidAdapter {
    /// Builds a mainnet adapter. `credentials` may carry the agent key
    /// under [`AGENT_KEY_FIELD`]; without it the adapter is read-only.
    /// `dex` selects a HIP-3 namespace (e.g. "io" for Entropy); the
    /// product exchange id is supplied separately as `exchange_id`.
    pub fn new(
        exchange_id: &str,
        account_id: &str,
        user_address: &str,
        credentials: Option<&Credentials>,
        dex: Option<String>,
    ) -> Result<Self, ExchangeError> {
        let user_address = Address::from_str(user_address).map_err(|_| {
            ExchangeError::InvalidRequest(format!("invalid account address '{user_address}'"))
        })?;
        let signer =
            match credentials.and_then(|c| c.get(AGENT_KEY_FIELD)) {
                Some(key) => Some(PrivateKeySigner::from_str(key).map_err(|_| {
                    ExchangeError::Authentication("invalid agent private key".into())
                })?),
                None => None,
            };
        Ok(Self {
            exchange_id: exchange_id.to_string(),
            account_id: account_id.to_string(),
            user_address,
            signer,
            writes_enabled: false,
            dex,
            client: hypersdk::hypercore::mainnet(),
            http: reqwest::Client::new(),
            markets: RwLock::new(None),
            last_nonce: AtomicU64::new(0),
        })
    }

    /// Explicitly enables or disables order submission for this adapter.
    /// Defaults to disabled; the execution layer turns this on only for
    /// accounts the user has explicitly enabled for live trading.
    pub fn with_writes_enabled(mut self, enabled: bool) -> Self {
        self.writes_enabled = enabled;
        self
    }

    pub fn writes_enabled(&self) -> bool {
        self.writes_enabled && self.signer.is_some()
    }

    fn require_signer(&self) -> Result<&PrivateKeySigner, ExchangeError> {
        if !self.writes_enabled {
            return Err(ExchangeError::Unsupported(
                "order submission is disabled for this account; it must be explicitly enabled before any order is sent".into(),
            ));
        }
        self.signer.as_ref().ok_or_else(|| {
            ExchangeError::Authentication("no API wallet (agent key) configured".into())
        })
    }

    fn next_nonce(&self) -> u64 {
        loop {
            let last = self.last_nonce.load(Ordering::SeqCst);
            let next = now_ms().max(last + 1);
            if self
                .last_nonce
                .compare_exchange(last, next, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return next;
            }
        }
    }

    async fn markets(&self) -> Result<Vec<PerpMarket>, ExchangeError> {
        if let Some(markets) = self
            .markets
            .read()
            .map_err(|_| ExchangeError::Unknown("market cache lock poisoned".into()))?
            .clone()
        {
            return Ok(markets);
        }
        let markets = match &self.dex {
            None => self.client.perps().await.map_err(map_api_error)?,
            Some(dex_name) => {
                let dexes = self.client.perp_dexes().await.map_err(map_api_error)?;
                let dex = dexes
                    .into_iter()
                    .find(|d| d.name() == dex_name)
                    .ok_or_else(|| {
                        ExchangeError::InvalidRequest(format!("unknown HIP-3 dex '{dex_name}'"))
                    })?;
                self.client.perps_from(dex).await.map_err(map_api_error)?
            }
        };
        *self
            .markets
            .write()
            .map_err(|_| ExchangeError::Unknown("market cache lock poisoned".into()))? =
            Some(markets.clone());
        Ok(markets)
    }

    async fn market_for(&self, symbol: &str) -> Result<PerpMarket, ExchangeError> {
        self.markets()
            .await?
            .into_iter()
            .find(|m| m.name == symbol)
            .ok_or_else(|| {
                ExchangeError::InvalidRequest(format!(
                    "unknown symbol '{symbol}' on {}",
                    self.exchange_id
                ))
            })
    }
}

/// Maps the venue's order status plus fill progress onto the unified
/// status. `filled` is derived from original minus remaining size.
fn map_order_status(
    status: hypersdk::hypercore::types::OrderStatus,
    filled: Decimal,
) -> OrderStatus {
    use hypersdk::hypercore::types::OrderStatus as Venue;
    match status {
        Venue::Open | Venue::Triggered => {
            if filled > Decimal::ZERO {
                OrderStatus::PartiallyFilled
            } else {
                OrderStatus::Open
            }
        }
        Venue::Filled => OrderStatus::Filled,
        Venue::Rejected => OrderStatus::Failed,
        Venue::ScheduledCancel => OrderStatus::CancelPending,
        _ => OrderStatus::Cancelled,
    }
}

fn map_sdk_side(side: hypersdk::hypercore::types::Side) -> Side {
    match side {
        hypersdk::hypercore::types::Side::Bid => Side::Buy,
        hypersdk::hypercore::types::Side::Ask => Side::Sell,
    }
}

#[derive(Debug, Deserialize)]
struct BookLevelRaw {
    px: String,
    sz: String,
}

#[derive(Debug, Deserialize)]
struct L2BookRaw {
    time: u64,
    levels: [Vec<BookLevelRaw>; 2],
}

fn parse_l2_book(
    exchange_id: &str,
    symbol: &str,
    raw: &serde_json::Value,
) -> Result<OrderBook, ExchangeError> {
    let book: L2BookRaw =
        serde_json::from_value(raw.clone()).map_err(|e| ExchangeError::Remote {
            code: None,
            message: format!("unexpected l2Book response: {e}"),
        })?;
    let parse_levels = |levels: &[BookLevelRaw]| -> Result<Vec<BookLevel>, ExchangeError> {
        levels
            .iter()
            .map(|level| {
                Ok(BookLevel {
                    price: Decimal::from_str(&level.px).map_err(|_| ExchangeError::Remote {
                        code: None,
                        message: "invalid price in l2Book".into(),
                    })?,
                    quantity: Decimal::from_str(&level.sz).map_err(|_| ExchangeError::Remote {
                        code: None,
                        message: "invalid size in l2Book".into(),
                    })?,
                })
            })
            .collect()
    };
    let [bids, asks] = &book.levels;
    Ok(OrderBook {
        exchange: exchange_id.to_string(),
        symbol: symbol.to_string(),
        bids: parse_levels(bids)?,
        asks: parse_levels(asks)?,
        observed_at: book.time as TimestampMs,
    })
}

/// Extracts the current funding rate for `symbol` from a
/// metaAndAssetCtxs response value: `[meta, assetCtxs]`, aligned by the
/// universe order. Funding settles hourly, so the next funding time is
/// the next hour boundary after `now`.
fn parse_funding(
    exchange_id: &str,
    symbol: &str,
    response: &serde_json::Value,
    now: TimestampMs,
) -> Result<FundingInfo, ExchangeError> {
    let parts = response.as_array().ok_or_else(|| ExchangeError::Remote {
        code: None,
        message: "unexpected metaAndAssetCtxs response".into(),
    })?;
    let (meta, ctxs) = match parts.as_slice() {
        [meta, ctxs] => (meta, ctxs),
        _ => {
            return Err(ExchangeError::Remote {
                code: None,
                message: "unexpected metaAndAssetCtxs response".into(),
            })
        }
    };
    let universe = meta
        .get("universe")
        .and_then(|u| u.as_array())
        .ok_or_else(|| ExchangeError::Remote {
            code: None,
            message: "metaAndAssetCtxs missing universe".into(),
        })?;
    let index = universe
        .iter()
        .position(|entry| entry.get("name").and_then(|n| n.as_str()) == Some(symbol))
        .ok_or_else(|| ExchangeError::InvalidRequest(format!("unknown symbol '{symbol}'")))?;
    let ctx = ctxs
        .as_array()
        .and_then(|arr| arr.get(index))
        .ok_or_else(|| ExchangeError::Remote {
            code: None,
            message: "metaAndAssetCtxs missing asset context".into(),
        })?;
    let rate_text =
        ctx.get("funding")
            .and_then(|f| f.as_str())
            .ok_or_else(|| ExchangeError::Remote {
                code: None,
                message: "asset context missing funding rate".into(),
            })?;
    let rate = Decimal::from_str(rate_text).map_err(|_| ExchangeError::Remote {
        code: None,
        message: "invalid funding rate".into(),
    })?;
    let next_funding_at = ((now / 3_600_000) + 1) * 3_600_000;
    Ok(FundingInfo {
        exchange: exchange_id.to_string(),
        symbol: symbol.to_string(),
        rate,
        next_funding_at: Some(next_funding_at),
        observed_at: now,
    })
}

#[async_trait]
impl ExchangeAdapter for HyperliquidAdapter {
    fn name(&self) -> &str {
        &self.exchange_id
    }

    fn account_id(&self) -> &str {
        &self.account_id
    }

    async fn get_funding(&self, symbol: &str) -> Result<FundingInfo, ExchangeError> {
        let response = self
            .client
            .meta_and_asset_ctxs(self.dex.clone())
            .await
            .map_err(map_api_error)?;
        parse_funding(
            &self.exchange_id,
            symbol,
            &response,
            now_ms() as TimestampMs,
        )
    }

    async fn get_orderbook(&self, symbol: &str) -> Result<OrderBook, ExchangeError> {
        // hypersdk 0.3.1 has no HTTP l2Book; use the same /info endpoint
        // directly for this one snapshot call.
        let response = self
            .http
            .post(MAINNET_INFO_URL)
            .json(&serde_json::json!({"type": "l2Book", "coin": symbol}))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ExchangeError::Timeout
                } else if e.is_connect() {
                    ExchangeError::Network(e.to_string())
                } else {
                    ExchangeError::Unknown(e.to_string())
                }
            })?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ExchangeError::RateLimited);
        }
        let raw: serde_json::Value = response.json().await.map_err(|e| ExchangeError::Remote {
            code: None,
            message: format!("invalid l2Book response: {e}"),
        })?;
        parse_l2_book(&self.exchange_id, symbol, &raw)
    }

    async fn get_positions(&self) -> Result<Vec<Position>, ExchangeError> {
        let state = self
            .client
            .clearinghouse_state(self.user_address, self.dex.clone())
            .await
            .map_err(map_api_error)?;
        let now = now_ms() as TimestampMs;
        Ok(state
            .asset_positions
            .iter()
            .map(|asset| {
                let pos = &asset.position;
                Position {
                    account_id: self.account_id.clone(),
                    exchange: self.exchange_id.clone(),
                    symbol: pos.coin.clone(),
                    quantity: pos.szi,
                    entry_price: pos.entry_px,
                    leverage: Some(Decimal::from(pos.leverage.value)),
                    unrealized_pnl: Some(pos.unrealized_pnl),
                    updated_at: now,
                }
            })
            .collect())
    }

    async fn get_balance(&self) -> Result<Balance, ExchangeError> {
        let state = self
            .client
            .clearinghouse_state(self.user_address, self.dex.clone())
            .await
            .map_err(map_api_error)?;
        Ok(Balance {
            account_id: self.account_id.clone(),
            exchange: self.exchange_id.clone(),
            currency: "USDC".to_string(),
            total: state.cross_margin_summary.account_value,
            available: state.withdrawable,
            updated_at: now_ms() as TimestampMs,
        })
    }

    async fn place_order(&self, request: &OrderRequest) -> Result<Order, ExchangeError> {
        let signer = self.require_signer()?;
        if request.exchange != self.exchange_id {
            return Err(ExchangeError::InvalidRequest(format!(
                "order targets exchange '{}' but this adapter serves '{}'",
                request.exchange, self.exchange_id
            )));
        }
        let market = self.market_for(&request.symbol).await?;
        let price = request.price.ok_or_else(|| {
            ExchangeError::InvalidRequest(
                "a price is required: for market orders it is the worst acceptable (protection) price".into(),
            )
        })?;
        let rounded_price = market.round_price(price).ok_or_else(|| {
            ExchangeError::InvalidRequest(format!(
                "price {price} cannot be rounded to a valid tick"
            ))
        })?;
        let size = request
            .quantity
            .round_dp_with_strategy(market.sz_decimals as u32, RoundingStrategy::ToZero);
        if size <= Decimal::ZERO {
            return Err(ExchangeError::InvalidRequest(format!(
                "quantity {} rounds to zero at {} size decimals",
                request.quantity, market.sz_decimals
            )));
        }
        let tif = match request.order_type {
            OrderType::Limit => TimeInForce::Gtc,
            OrderType::Market => TimeInForce::Ioc,
        };
        let sdk_request = hypersdk::hypercore::types::OrderRequest {
            asset: market.index,
            is_buy: matches!(request.side, Side::Buy),
            limit_px: rounded_price,
            sz: size,
            reduce_only: request.reduce_only,
            order_type: OrderTypePlacement::Limit { tif },
            cloid: parse_cloid(&request.client_order_id)?,
        };
        let batch = BatchOrder {
            orders: vec![sdk_request],
            grouping: OrderGrouping::Na,
            builder: None,
        };
        let statuses = self
            .client
            .place(signer, batch, self.next_nonce(), None, None)
            .await
            .map_err(map_action_error)?;
        let status = statuses
            .into_iter()
            .next()
            .ok_or_else(|| ExchangeError::Unknown("venue returned no order status".into()))?;
        let mut order = Order::from_request(request, now_ms() as TimestampMs);
        match status {
            OrderResponseStatus::Resting { oid, .. } => {
                order.status = OrderStatus::Open;
                order.exchange_order_id = Some(oid.to_string());
            }
            OrderResponseStatus::WaitingForFill => {
                order.status = OrderStatus::Open;
            }
            OrderResponseStatus::Filled {
                total_sz,
                avg_px,
                oid,
            } => {
                order.status = OrderStatus::Filled;
                order.exchange_order_id = Some(oid.to_string());
                order.filled_quantity = total_sz;
                order.average_fill_price = Some(avg_px);
            }
            OrderResponseStatus::Success | OrderResponseStatus::WaitingForTrigger => {
                order.status = OrderStatus::Open;
            }
            OrderResponseStatus::Error(message) => {
                return Err(ExchangeError::Remote {
                    code: None,
                    message,
                });
            }
        }
        Ok(order)
    }

    async fn get_order(&self, client_order_id: &str) -> Result<Order, ExchangeError> {
        let cloid = parse_cloid(client_order_id)?;
        let update = self
            .client
            .order_status(self.user_address, OidOrCloid::Right(cloid))
            .await
            .map_err(map_api_error)?
            .ok_or_else(|| ExchangeError::Remote {
                code: Some(ORDER_NOT_FOUND_CODE.into()),
                message: format!("no order found for cloid {client_order_id}"),
            })?;
        let basic = &update.order;
        let filled = basic.orig_sz - basic.sz;
        Ok(Order {
            client_order_id: basic
                .cloid
                .map(format_cloid)
                .unwrap_or_else(|| client_order_id.to_string()),
            exchange_order_id: Some(basic.oid.to_string()),
            account_id: self.account_id.clone(),
            strategy_id: String::new(),
            exchange: self.exchange_id.clone(),
            symbol: basic.coin.clone(),
            side: map_sdk_side(basic.side),
            order_type: if matches!(basic.tif, Some(TimeInForce::Ioc)) {
                OrderType::Market
            } else {
                OrderType::Limit
            },
            quantity: basic.orig_sz,
            price: Some(basic.limit_px),
            reduce_only: basic.reduce_only,
            status: map_order_status(update.status, filled),
            filled_quantity: filled.max(Decimal::ZERO),
            average_fill_price: None,
            created_at: basic.timestamp as TimestampMs,
            updated_at: update.status_timestamp as TimestampMs,
        })
    }

    async fn cancel_order(&self, client_order_id: &str) -> Result<Order, ExchangeError> {
        let signer = self.require_signer()?;
        let current = self.get_order(client_order_id).await?;
        if current.status.is_terminal() {
            return Ok(current);
        }
        let market = self.market_for(&current.symbol).await?;
        let batch = BatchCancelCloid {
            cancels: vec![CancelByCloid {
                asset: market.index as u32,
                cloid: parse_cloid(client_order_id)?,
            }],
            fast: false,
        };
        let statuses = self
            .client
            .cancel_by_cloid(signer, batch, self.next_nonce(), None, None)
            .await
            .map_err(map_action_error)?;
        match statuses.into_iter().next() {
            Some(OrderResponseStatus::Error(message)) => {
                return Err(ExchangeError::Remote {
                    code: None,
                    message,
                });
            }
            Some(_) => {}
            None => {
                return Err(ExchangeError::Unknown(
                    "venue returned no cancel status".into(),
                ))
            }
        }
        // Confirm the resulting state from the venue; if the confirmation
        // query itself fails, report CancelPending rather than guessing.
        match self.get_order(client_order_id).await {
            Ok(order) => Ok(order),
            Err(_) => {
                let mut order = current;
                order.status = OrderStatus::CancelPending;
                order.updated_at = now_ms() as TimestampMs;
                Ok(order)
            }
        }
    }

    async fn get_fills(&self, since: Option<TimestampMs>) -> Result<Vec<Fill>, ExchangeError> {
        let start = since.unwrap_or(0).max(0) as u64;
        let fills = self
            .client
            .user_fills_by_time(self.user_address, start, None)
            .await
            .map_err(map_api_error)?;
        Ok(fills
            .iter()
            .map(|fill| Fill {
                client_order_id: fill.cloid.map(format_cloid).unwrap_or_default(),
                exchange_order_id: Some(fill.oid.to_string()),
                exchange_fill_id: Some(fill.tid.to_string()),
                account_id: self.account_id.clone(),
                exchange: self.exchange_id.clone(),
                symbol: fill.coin.clone(),
                side: map_sdk_side(fill.side),
                quantity: fill.sz,
                price: fill.px,
                fee: fill.fee,
                filled_at: fill.time as TimestampMs,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn cloid_roundtrip_and_validation() {
        let id = new_client_order_id();
        assert!(id.starts_with("0x"));
        assert_eq!(id.len(), 34);
        let parsed = parse_cloid(&id).expect("generated id parses");
        assert_eq!(format_cloid(parsed), id);
        assert!(parse_cloid("dexo-plain-string").is_err());
        assert!(parse_cloid("0x1234").is_err());
        assert!(parse_cloid("0xzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    }

    #[test]
    fn generated_cloids_are_unique() {
        assert_ne!(new_client_order_id(), new_client_order_id());
    }

    #[test]
    fn funding_parses_from_meta_and_asset_ctxs() {
        let response = serde_json::json!([
            {"universe": [{"name": "BTC"}, {"name": "ETH"}]},
            [{"funding": "0.0000125"}, {"funding": "-0.00002"}]
        ]);
        let funding = parse_funding("hyperliquid", "ETH", &response, 3_599_000).expect("parse");
        assert_eq!(funding.rate, dec!(-0.00002));
        assert_eq!(funding.next_funding_at, Some(3_600_000));
        assert!(parse_funding("hyperliquid", "DOGE", &response, 0).is_err());
    }

    #[test]
    fn order_book_parses_from_l2_book() {
        let raw = serde_json::json!({
            "coin": "BTC",
            "time": 123,
            "levels": [
                [{"px": "90000.0", "sz": "1.5", "n": 3}],
                [{"px": "90010.0", "sz": "2.0", "n": 1}]
            ]
        });
        let book = parse_l2_book("hyperliquid", "BTC", &raw).expect("parse");
        assert_eq!(book.bids[0].price, dec!(90000));
        assert_eq!(book.asks[0].quantity, dec!(2));
        assert_eq!(book.observed_at, 123);
    }

    #[test]
    fn order_status_mapping_covers_partial_and_terminal() {
        use hypersdk::hypercore::types::OrderStatus as Venue;
        assert_eq!(
            map_order_status(Venue::Open, Decimal::ZERO),
            OrderStatus::Open
        );
        assert_eq!(
            map_order_status(Venue::Open, dec!(0.5)),
            OrderStatus::PartiallyFilled
        );
        assert_eq!(
            map_order_status(Venue::Filled, dec!(1)),
            OrderStatus::Filled
        );
        assert_eq!(
            map_order_status(Venue::Rejected, Decimal::ZERO),
            OrderStatus::Failed
        );
        assert_eq!(
            map_order_status(Venue::MarginCanceled, Decimal::ZERO),
            OrderStatus::Cancelled
        );
    }

    #[test]
    fn writes_are_disabled_by_default() {
        let adapter = HyperliquidAdapter::new(
            "hyperliquid",
            "hl1",
            "0x0000000000000000000000000000000000000000",
            None,
            None,
        )
        .expect("adapter builds");
        assert!(!adapter.writes_enabled());
        assert!(adapter.require_signer().is_err());
    }
}
