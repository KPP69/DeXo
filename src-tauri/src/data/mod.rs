//! Data center: the single source strategies use for market data, order
//! books, funding rates, balances and positions, with caching and
//! timestamps. Stale data must be detectable and must not drive new orders.
//! Implemented in Phase 5.

pub mod account;
pub mod cache;
pub mod funding;
pub mod market;
pub mod positions;
