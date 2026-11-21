//! Risk center: four layers (global, account, strategy, order) plus a
//! global Kill Switch. The Kill Switch blocks new position opening
//! while keeping cancel / reduce / close capabilities; it is not a
//! forced liquidation.
//!
//! Phase 8 implements the layers and the orchestrating `RiskCenter`.
//! Checks stay synchronous (the Phase 2 `RiskCheck` contract): the
//! center evaluates against a state snapshot — balances, positions,
//! daily PnL, reference prices — that the application refreshes from
//! the data center via `update_state`. A check never touches the
//! network or the database, and an order whose inputs cannot be
//! verified (no balance, no fresh reference price) is rejected rather
//! than waved through.

pub mod account;
pub mod global;
pub mod order;
pub mod strategy;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

use rust_decimal::Decimal;

use crate::models::{
    Balance, OrderRequest, OrderType, Position, RiskContext, RiskLayer, RiskResult, Side,
    TimestampMs,
};

/// One risk layer's check (interface defined in Phase 2, layers
/// implemented in Phase 8). Checks are synchronous and pure: a layer
/// consults only the request and the context, so every layer is unit
/// testable without an exchange or a database. The execution layer runs
/// the layers in order — global, account, strategy, order — and the
/// first rejection stops the order.
pub trait RiskCheck: Send + Sync {
    fn check_order(&self, context: &RiskContext, request: &OrderRequest) -> RiskResult;
}

#[derive(Debug, Clone)]
pub struct GlobalLimits {
    /// Cap on total exposure (sum of |position values| plus the new
    /// order's exposure increase) across all accounts. None = no cap.
    pub max_total_exposure: Option<Decimal>,
    /// When the day's PnL reaches this loss, opening orders stop
    /// (global circuit breaker). None = disabled.
    pub max_daily_loss: Option<Decimal>,
    /// Minimum order value in quote currency (Hyperliquid's venue
    /// minimum is 10).
    pub min_order_notional: Decimal,
    /// Maximum allowed distance between a limit price and the
    /// reference price, as a fraction (0.10 = 10%).
    pub max_price_deviation: Decimal,
    /// Reference data older than this cannot justify an order.
    pub max_data_age_ms: TimestampMs,
}

#[derive(Debug, Clone)]
pub struct AccountLimits {
    /// Cap on the account's total exposure after this order.
    pub max_exposure: Option<Decimal>,
    /// Highest leverage the account may run; also the multiple used
    /// for the balance check (order value increase must fit within
    /// available balance × max_leverage, i.e. its margin fits).
    pub max_leverage: Decimal,
    /// Per-order value cap for this account.
    pub max_order_notional: Option<Decimal>,
}

#[derive(Debug, Clone, Default)]
pub struct StrategyLimits {
    pub max_order_notional: Option<Decimal>,
    /// When the strategy's daily PnL reaches this loss, its opening
    /// orders stop.
    pub max_daily_loss: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct RiskLimits {
    pub global: GlobalLimits,
    pub default_account: AccountLimits,
    pub accounts: HashMap<String, AccountLimits>,
    pub strategies: HashMap<String, StrategyLimits>,
    /// Strategies that may not open orders at all (stopped strategies).
    pub disabled_strategies: HashSet<String>,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            global: GlobalLimits {
                max_total_exposure: None,
                max_daily_loss: None,
                min_order_notional: Decimal::new(10, 0),
                max_price_deviation: Decimal::new(10, 2),
                max_data_age_ms: 30_000,
            },
            default_account: AccountLimits {
                max_exposure: None,
                max_leverage: Decimal::new(20, 0),
                max_order_notional: None,
            },
            accounts: HashMap::new(),
            strategies: HashMap::new(),
            disabled_strategies: HashSet::new(),
        }
    }
}

impl RiskLimits {
    pub fn account_limits(&self, account_id: &str) -> &AccountLimits {
        self.accounts
            .get(account_id)
            .unwrap_or(&self.default_account)
    }
}

/// Snapshot the layers evaluate against. Refreshed by the application
/// from the data center; a missing entry means "unknown", and layers
/// that need it reject (unknown is not treated as zero risk).
#[derive(Debug, Clone, Default)]
pub struct RiskState {
    pub balances: HashMap<String, Balance>,
    pub positions: HashMap<String, Vec<Position>>,
    pub daily_pnl: Decimal,
    pub strategy_daily_pnl: HashMap<String, Decimal>,
    /// (exchange, symbol) → (reference price, observed_at).
    pub reference_prices: HashMap<(String, String), (Decimal, TimestampMs)>,
}

impl RiskState {
    pub fn reference_price(&self, exchange: &str, symbol: &str) -> Option<(Decimal, TimestampMs)> {
        self.reference_prices
            .get(&(exchange.to_string(), symbol.to_string()))
            .copied()
    }

    /// Signed current quantity for one account + symbol (0 if flat).
    pub fn position_quantity(&self, account_id: &str, exchange: &str, symbol: &str) -> Decimal {
        self.positions
            .get(account_id)
            .and_then(|list| {
                list.iter()
                    .find(|p| p.exchange == exchange && p.symbol == symbol)
            })
            .map(|p| p.quantity)
            .unwrap_or(Decimal::ZERO)
    }

    /// Value of one position at the reference price, falling back to
    /// its entry price; unknown when neither exists.
    pub fn position_value(&self, position: &Position) -> Option<Decimal> {
        let price = self
            .reference_price(&position.exchange, &position.symbol)
            .map(|(price, _)| price)
            .or(position.entry_price)?;
        Some((position.quantity * price).abs())
    }

    /// Total exposure of one account (sum of |position values|).
    /// Positions that cannot be valued are skipped — the traded
    /// symbol's own data validity is enforced by the order layer.
    pub fn account_exposure(&self, account_id: &str) -> Decimal {
        self.positions
            .get(account_id)
            .map(|list| list.iter().filter_map(|p| self.position_value(p)).sum())
            .unwrap_or(Decimal::ZERO)
    }

    pub fn total_exposure(&self) -> Decimal {
        self.positions
            .keys()
            .map(|account| self.account_exposure(account))
            .sum()
    }
}

/// The order's value at its effective price (limit price, else the
/// reference price for market orders). None when it cannot be valued.
pub fn order_notional(state: &RiskState, request: &OrderRequest) -> Option<Decimal> {
    let price = match request.order_type {
        OrderType::Limit => request.price,
        OrderType::Market => state
            .reference_price(&request.exchange, &request.symbol)
            .map(|(price, _)| price),
    }?;
    Some(request.quantity * price)
}

/// How much this order increases exposure, valued at the reference
/// price: (|q + d| − |q|) × price, floored at zero for pure
/// reductions. None when it cannot be computed.
pub fn exposure_increase(state: &RiskState, request: &OrderRequest) -> Option<Decimal> {
    if request.reduce_only {
        // Reduce-only orders cannot increase exposure by construction
        // (the venue rejects any part that would).
        return Some(Decimal::ZERO);
    }
    let (price, _) = state.reference_price(&request.exchange, &request.symbol)?;
    let current = state.position_quantity(&request.account_id, &request.exchange, &request.symbol);
    let delta = match request.side {
        Side::Buy => request.quantity,
        Side::Sell => -request.quantity,
    };
    let change = (current + delta).abs() - current.abs();
    Some((change * price).max(Decimal::ZERO))
}

/// One recorded rejection: rejections are queryable (Phase 8
/// acceptance), newest last, bounded to the most recent 200.
#[derive(Debug, Clone)]
pub struct RejectionRecord {
    pub at: TimestampMs,
    pub account_id: String,
    pub strategy_id: String,
    pub client_order_id: String,
    pub layer: RiskLayer,
    pub reason: String,
}

const MAX_REJECTION_RECORDS: usize = 200;

/// The risk center: owns the limits, the state snapshot, the Kill
/// Switch and the rejection log, and runs the four layers in order.
pub struct RiskCenter {
    limits: RiskLimits,
    state: RwLock<RiskState>,
    kill_switch: AtomicBool,
    kill_detail: Mutex<Option<(String, TimestampMs)>>,
    rejections: Mutex<VecDeque<RejectionRecord>>,
}

impl RiskCenter {
    pub fn new(limits: RiskLimits) -> Self {
        Self {
            limits,
            state: RwLock::new(RiskState::default()),
            kill_switch: AtomicBool::new(false),
            kill_detail: Mutex::new(None),
            rejections: Mutex::new(VecDeque::new()),
        }
    }

    pub fn limits(&self) -> &RiskLimits {
        &self.limits
    }

    pub fn update_state(&self, state: RiskState) {
        *self.state.write().expect("risk state poisoned") = state;
    }

    pub fn snapshot(&self) -> RiskState {
        self.state.read().expect("risk state poisoned").clone()
    }

    /// Engages the global Kill Switch with a recorded reason. Opening
    /// orders are rejected from this moment; reduce-only orders,
    /// cancels and closes are unaffected, and nothing is liquidated.
    pub fn activate_kill_switch(&self, reason: impl Into<String>, now: TimestampMs) {
        *self.kill_detail.lock().expect("kill detail poisoned") = Some((reason.into(), now));
        self.kill_switch.store(true, Ordering::SeqCst);
    }

    /// Manual release only — nothing in the system clears the switch
    /// automatically.
    pub fn clear_kill_switch(&self) {
        self.kill_switch.store(false, Ordering::SeqCst);
        *self.kill_detail.lock().expect("kill detail poisoned") = None;
    }

    pub fn kill_switch_active(&self) -> bool {
        self.kill_switch.load(Ordering::SeqCst)
    }

    /// (reason, activated_at) while the switch is engaged.
    pub fn kill_switch_status(&self) -> Option<(String, TimestampMs)> {
        if self.kill_switch_active() {
            self.kill_detail
                .lock()
                .expect("kill detail poisoned")
                .clone()
        } else {
            None
        }
    }

    pub fn recent_rejections(&self) -> Vec<RejectionRecord> {
        self.rejections
            .lock()
            .expect("rejections poisoned")
            .iter()
            .cloned()
            .collect()
    }

    fn record_rejection(&self, context: &RiskContext, request: &OrderRequest, result: &RiskResult) {
        if let RiskResult::Rejected { layer, reason } = result {
            let mut log = self.rejections.lock().expect("rejections poisoned");
            if log.len() >= MAX_REJECTION_RECORDS {
                log.pop_front();
            }
            log.push_back(RejectionRecord {
                at: context.evaluated_at,
                account_id: request.account_id.clone(),
                strategy_id: request.strategy_id.clone(),
                client_order_id: request.client_order_id.clone(),
                layer: *layer,
                reason: reason.clone(),
            });
        }
    }
}

impl RiskCheck for RiskCenter {
    fn check_order(&self, context: &RiskContext, request: &OrderRequest) -> RiskResult {
        let state = self.snapshot();
        let kill = self.kill_switch_active() || context.kill_switch_active;
        let kill_reason = self
            .kill_switch_status()
            .map(|(reason, _)| reason)
            .unwrap_or_else(|| "kill switch active".to_string());
        let outcome = global::check(&self.limits, &state, context, request, kill, &kill_reason)
            .and_then(|()| account::check(&self.limits, &state, context, request))
            .and_then(|()| strategy::check(&self.limits, &state, context, request))
            .and_then(|()| order::check(&self.limits, &state, context, request));
        let result = match outcome {
            Ok(()) => RiskResult::Approved,
            Err(rejected) => rejected,
        };
        self.record_rejection(context, request, &result);
        result
    }
}

/// Helper so layers read as sequential checks: `Err` carries the
/// rejection to return.
pub(crate) type LayerOutcome = Result<(), RiskResult>;

pub(crate) fn reject(layer: RiskLayer, reason: impl Into<String>) -> LayerOutcome {
    Err(RiskResult::Rejected {
        layer,
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{OrderType, Side};
    use rust_decimal_macros::dec;

    pub(crate) fn sample_request() -> OrderRequest {
        OrderRequest {
            account_id: "acc-1".into(),
            strategy_id: "strat-1".into(),
            exchange: "hyperliquid".into(),
            symbol: "BTC".into(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: dec!(0.1),
            price: Some(dec!(60000)),
            reduce_only: false,
            client_order_id: "0x1".into(),
            created_at: 1_000,
        }
    }

    pub(crate) fn sample_context() -> RiskContext {
        RiskContext {
            account_id: "acc-1".into(),
            strategy_id: "strat-1".into(),
            kill_switch_active: false,
            evaluated_at: 10_000,
        }
    }

    pub(crate) fn funded_state() -> RiskState {
        let mut state = RiskState::default();
        state.balances.insert(
            "acc-1".into(),
            Balance {
                account_id: "acc-1".into(),
                exchange: "hyperliquid".into(),
                currency: "USDC".into(),
                total: dec!(10000),
                available: dec!(10000),
                updated_at: 9_000,
            },
        );
        state
            .reference_prices
            .insert(("hyperliquid".into(), "BTC".into()), (dec!(60000), 9_500));
        state
    }

    #[test]
    fn well_formed_order_passes_all_layers() {
        let center = RiskCenter::new(RiskLimits::default());
        center.update_state(funded_state());
        assert!(center
            .check_order(&sample_context(), &sample_request())
            .is_approved());
        assert!(center.recent_rejections().is_empty());
    }

    #[test]
    fn kill_switch_blocks_openings_but_not_reductions() {
        let center = RiskCenter::new(RiskLimits::default());
        center.update_state(funded_state());
        center.activate_kill_switch("manual stop", 9_900);
        let opening = center.check_order(&sample_context(), &sample_request());
        assert!(matches!(
            opening,
            RiskResult::Rejected {
                layer: RiskLayer::Global,
                ..
            }
        ));
        let mut closing = sample_request();
        closing.reduce_only = true;
        closing.side = Side::Sell;
        assert!(center
            .check_order(&sample_context(), &closing)
            .is_approved());
        // Rejections are queryable with their reason.
        let log = center.recent_rejections();
        assert_eq!(log.len(), 1);
        assert!(log[0].reason.contains("manual stop"));
        // Manual release restores openings.
        center.clear_kill_switch();
        assert!(center
            .check_order(&sample_context(), &sample_request())
            .is_approved());
    }

    #[test]
    fn first_rejecting_layer_wins() {
        // Kill switch (global) and a disabled strategy both apply;
        // the global layer must be the one reported.
        let mut limits = RiskLimits::default();
        limits.disabled_strategies.insert("strat-1".into());
        let center = RiskCenter::new(limits);
        center.update_state(funded_state());
        center.activate_kill_switch("stop", 9_900);
        let result = center.check_order(&sample_context(), &sample_request());
        assert!(matches!(
            result,
            RiskResult::Rejected {
                layer: RiskLayer::Global,
                ..
            }
        ));
    }
}
