//! Data center: the single source strategies use for market data, order
//! books, funding rates, balances and positions, with caching and
//! timestamps. Stale data must be detectable and must not drive new orders.
//! The `DataAccess` interface is defined in Phase 2; the caching
//! implementation lands in Phase 5.

pub mod account;
pub mod cache;
pub mod funding;
pub mod market;
pub mod positions;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use crate::exchanges::traits::ExchangeAdapter;
use crate::models::{Balance, DataError, FundingInfo, OrderBook, Position, TimestampMs};

pub use cache::TtlCache;

/// The only data interface strategies may use. Implementations must
/// enforce freshness: data older than the configured limit for its kind
/// returns `DataError::StaleData` instead of a value, so expired data can
/// never silently drive new orders.
#[async_trait]
pub trait DataAccess: Send + Sync {
    async fn funding(&self, exchange: &str, symbol: &str) -> Result<FundingInfo, DataError>;

    async fn order_book(&self, exchange: &str, symbol: &str) -> Result<OrderBook, DataError>;

    async fn balance(&self, account_id: &str) -> Result<Balance, DataError>;

    async fn positions(&self, account_id: &str) -> Result<Vec<Position>, DataError>;
}

/// How long each kind of data may be served from cache before it must be
/// refreshed. Past the limit, data is stale and must not drive orders.
#[derive(Debug, Clone, Copy)]
pub struct FreshnessLimits {
    pub funding_ms: TimestampMs,
    pub order_book_ms: TimestampMs,
    pub balance_ms: TimestampMs,
    pub positions_ms: TimestampMs,
}

impl Default for FreshnessLimits {
    fn default() -> Self {
        Self {
            funding_ms: 30_000,
            order_book_ms: 5_000,
            balance_ms: 15_000,
            positions_ms: 15_000,
        }
    }
}

fn system_now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as TimestampMs)
        .unwrap_or(0)
}

/// The data center (Phase 5): serves strategies through `DataAccess`,
/// backed by per-account exchange adapters and timestamped caches.
///
/// Read semantics, identical for every kind of data:
/// 1. A cached value younger than its freshness limit is served as-is.
/// 2. Otherwise the value is refreshed from the adapter; success updates
///    the cache and returns the fresh value.
/// 3. If the refresh fails and only stale data exists, the read fails
///    with `DataError::StaleData` — stale data is never silently served
///    to a strategy. With no cached data at all, the adapter's error is
///    returned as `DataError::Backend`.
pub struct DataCenter {
    adapters: HashMap<String, Arc<dyn ExchangeAdapter>>,
    limits: FreshnessLimits,
    clock: Arc<dyn Fn() -> TimestampMs + Send + Sync>,
    funding_cache: TtlCache<(String, String), FundingInfo>,
    book_cache: TtlCache<(String, String), OrderBook>,
    balance_cache: TtlCache<String, Balance>,
    positions_cache: TtlCache<String, Vec<Position>>,
}

impl DataCenter {
    pub fn new(adapters: Vec<Arc<dyn ExchangeAdapter>>, limits: FreshnessLimits) -> Self {
        Self::with_clock(adapters, limits, Arc::new(system_now_ms))
    }

    pub fn with_clock(
        adapters: Vec<Arc<dyn ExchangeAdapter>>,
        limits: FreshnessLimits,
        clock: Arc<dyn Fn() -> TimestampMs + Send + Sync>,
    ) -> Self {
        Self {
            adapters: adapters
                .into_iter()
                .map(|a| (a.account_id().to_string(), a))
                .collect(),
            limits,
            clock,
            funding_cache: TtlCache::new(),
            book_cache: TtlCache::new(),
            balance_cache: TtlCache::new(),
            positions_cache: TtlCache::new(),
        }
    }

    fn now(&self) -> TimestampMs {
        (self.clock)()
    }

    fn adapter_for_account(&self, account_id: &str) -> Result<Arc<dyn ExchangeAdapter>, DataError> {
        self.adapters
            .get(account_id)
            .cloned()
            .ok_or_else(|| DataError::NotFound(format!("no adapter for account '{account_id}'")))
    }

    fn adapter_for_exchange(&self, exchange: &str) -> Result<Arc<dyn ExchangeAdapter>, DataError> {
        self.adapters
            .values()
            .find(|a| a.name() == exchange)
            .cloned()
            .ok_or_else(|| DataError::NotFound(format!("no adapter for exchange '{exchange}'")))
    }

    /// Shared read path implementing the semantics documented on the
    /// struct: fresh cache hit, else refresh, else an explicit error.
    async fn read_through<K, V, F, Fut>(
        &self,
        cache: &TtlCache<K, V>,
        key: K,
        ttl_ms: TimestampMs,
        label: &str,
        fetch: F,
    ) -> Result<V, DataError>
    where
        K: Eq + std::hash::Hash,
        V: Clone,
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<V, crate::models::ExchangeError>>,
    {
        let now = self.now();
        if let Some(value) = cache.get_fresh(&key, now, ttl_ms) {
            return Ok(value);
        }
        match fetch().await {
            Ok(value) => {
                cache.put(key, value.clone(), self.now());
                Ok(value)
            }
            Err(error) => {
                if let Some(stale) = cache.get_any(&key) {
                    Err(DataError::StaleData(format!(
                        "{label} is {} ms old (limit {ttl_ms} ms) and refresh failed: {error}",
                        stale.age_at(self.now())
                    )))
                } else {
                    Err(DataError::from(error))
                }
            }
        }
    }
}

#[async_trait]
impl DataAccess for DataCenter {
    async fn funding(&self, exchange: &str, symbol: &str) -> Result<FundingInfo, DataError> {
        let adapter = self.adapter_for_exchange(exchange)?;
        let key = (exchange.to_string(), symbol.to_string());
        let symbol = symbol.to_string();
        self.read_through(
            &self.funding_cache,
            key,
            self.limits.funding_ms,
            "funding data",
            move || async move { adapter.get_funding(&symbol).await },
        )
        .await
    }

    async fn order_book(&self, exchange: &str, symbol: &str) -> Result<OrderBook, DataError> {
        let adapter = self.adapter_for_exchange(exchange)?;
        let key = (exchange.to_string(), symbol.to_string());
        let symbol = symbol.to_string();
        self.read_through(
            &self.book_cache,
            key,
            self.limits.order_book_ms,
            "order book data",
            move || async move { adapter.get_orderbook(&symbol).await },
        )
        .await
    }

    async fn balance(&self, account_id: &str) -> Result<Balance, DataError> {
        let adapter = self.adapter_for_account(account_id)?;
        self.read_through(
            &self.balance_cache,
            account_id.to_string(),
            self.limits.balance_ms,
            "balance data",
            move || async move { adapter.get_balance().await },
        )
        .await
    }

    async fn positions(&self, account_id: &str) -> Result<Vec<Position>, DataError> {
        let adapter = self.adapter_for_account(account_id)?;
        self.read_through(
            &self.positions_cache,
            account_id.to_string(),
            self.limits.positions_ms,
            "position data",
            move || async move { adapter.get_positions().await },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ExchangeError, Fill, Order, OrderRequest};
    use rust_decimal_macros::dec;
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

    struct CountingAdapter {
        calls: AtomicUsize,
        failing: AtomicBool,
    }

    #[async_trait]
    impl ExchangeAdapter for CountingAdapter {
        fn name(&self) -> &str {
            "mockex"
        }

        fn account_id(&self) -> &str {
            "acc-1"
        }

        async fn get_funding(&self, symbol: &str) -> Result<FundingInfo, ExchangeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.failing.load(Ordering::SeqCst) {
                return Err(ExchangeError::Network("venue down".into()));
            }
            Ok(FundingInfo {
                exchange: "mockex".into(),
                symbol: symbol.to_string(),
                rate: dec!(0.0001),
                next_funding_at: None,
                observed_at: 0,
            })
        }

        async fn get_orderbook(&self, symbol: &str) -> Result<OrderBook, ExchangeError> {
            Ok(OrderBook {
                exchange: "mockex".into(),
                symbol: symbol.to_string(),
                bids: Vec::new(),
                asks: Vec::new(),
                observed_at: 0,
            })
        }

        async fn get_positions(&self) -> Result<Vec<Position>, ExchangeError> {
            Ok(Vec::new())
        }

        async fn get_balance(&self) -> Result<Balance, ExchangeError> {
            Ok(Balance {
                account_id: "acc-1".into(),
                exchange: "mockex".into(),
                currency: "USDC".into(),
                total: dec!(100),
                available: dec!(100),
                updated_at: 0,
            })
        }

        async fn place_order(&self, _: &OrderRequest) -> Result<Order, ExchangeError> {
            Err(ExchangeError::Unsupported(
                "no trading in data tests".into(),
            ))
        }

        async fn get_order(&self, _: &str) -> Result<Order, ExchangeError> {
            Err(ExchangeError::Unsupported(
                "no trading in data tests".into(),
            ))
        }

        async fn cancel_order(&self, _: &str) -> Result<Order, ExchangeError> {
            Err(ExchangeError::Unsupported(
                "no trading in data tests".into(),
            ))
        }

        async fn get_fills(&self, _: Option<TimestampMs>) -> Result<Vec<Fill>, ExchangeError> {
            Ok(Vec::new())
        }
    }

    fn center(adapter: Arc<CountingAdapter>, clock: Arc<AtomicI64>) -> DataCenter {
        DataCenter::with_clock(
            vec![adapter],
            FreshnessLimits {
                funding_ms: 1_000,
                order_book_ms: 1_000,
                balance_ms: 1_000,
                positions_ms: 1_000,
            },
            Arc::new(move || clock.load(Ordering::SeqCst)),
        )
    }

    #[tokio::test]
    async fn fresh_cache_hits_do_not_refetch() {
        let adapter = Arc::new(CountingAdapter {
            calls: AtomicUsize::new(0),
            failing: AtomicBool::new(false),
        });
        let clock = Arc::new(AtomicI64::new(10_000));
        let center = center(adapter.clone(), clock.clone());
        center.funding("mockex", "BTC").await.expect("first");
        clock.store(10_500, Ordering::SeqCst);
        center.funding("mockex", "BTC").await.expect("cached");
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn expired_entries_are_refetched() {
        let adapter = Arc::new(CountingAdapter {
            calls: AtomicUsize::new(0),
            failing: AtomicBool::new(false),
        });
        let clock = Arc::new(AtomicI64::new(10_000));
        let center = center(adapter.clone(), clock.clone());
        center.funding("mockex", "BTC").await.expect("first");
        clock.store(11_500, Ordering::SeqCst);
        center.funding("mockex", "BTC").await.expect("refetch");
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn stale_data_is_never_served_when_refresh_fails() {
        let adapter = Arc::new(CountingAdapter {
            calls: AtomicUsize::new(0),
            failing: AtomicBool::new(false),
        });
        let clock = Arc::new(AtomicI64::new(10_000));
        let center = center(adapter.clone(), clock.clone());
        center.funding("mockex", "BTC").await.expect("first");
        clock.store(12_000, Ordering::SeqCst);
        adapter.failing.store(true, Ordering::SeqCst);
        let result = center.funding("mockex", "BTC").await;
        assert!(matches!(result, Err(DataError::StaleData(_))));
    }

    #[tokio::test]
    async fn missing_data_with_failing_adapter_is_a_backend_error() {
        let adapter = Arc::new(CountingAdapter {
            calls: AtomicUsize::new(0),
            failing: AtomicBool::new(true),
        });
        let clock = Arc::new(AtomicI64::new(10_000));
        let center = center(adapter, clock);
        let result = center.funding("mockex", "BTC").await;
        assert!(matches!(result, Err(DataError::Backend(_))));
    }

    #[tokio::test]
    async fn unknown_exchange_and_account_are_not_found() {
        let adapter = Arc::new(CountingAdapter {
            calls: AtomicUsize::new(0),
            failing: AtomicBool::new(false),
        });
        let clock = Arc::new(AtomicI64::new(10_000));
        let center = center(adapter, clock);
        assert!(matches!(
            center.funding("nope", "BTC").await,
            Err(DataError::NotFound(_))
        ));
        assert!(matches!(
            center.balance("ghost").await,
            Err(DataError::NotFound(_))
        ));
    }
}
