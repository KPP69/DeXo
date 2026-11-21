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

use async_trait::async_trait;

use crate::models::{Balance, DataError, FundingInfo, OrderBook, Position};

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
