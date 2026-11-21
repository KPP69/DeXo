//! Shared data models: the unified structures every exchange adapter maps
//! to and every center exchanges. Defined in Phase 2.
//!
//! Rules that apply to every model here:
//! - Money, prices, quantities and fees are `Decimal` (rust_decimal),
//!   never `f64`.
//! - Timestamps are Unix epoch milliseconds (`i64`).
//! - Every order-related record carries `account_id`, `strategy_id`,
//!   `exchange`, `symbol` and `client_order_id` where applicable;
//!   `client_order_id` is the idempotency, timeout-query and recovery key.

pub mod account;
pub mod error;
pub mod market;
pub mod order;
pub mod risk;

pub use account::{Account, Balance, Exchange, Position};
pub use error::{DataError, ExchangeError, StrategyError};
pub use market::{BookLevel, FundingInfo, OrderBook};
pub use order::{Fill, Order, OrderRequest, OrderStatus, OrderType, Side, Trade};
pub use risk::{RiskContext, RiskLayer, RiskResult};

/// Unix epoch milliseconds. All timestamps in DeXo use this unit.
pub type TimestampMs = i64;
