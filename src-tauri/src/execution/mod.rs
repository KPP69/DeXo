//! Execution layer: connects accounts, strategies, the data center, risk
//! control, exchange adapters and the trading center, and owns the full
//! order lifecycle including dual-leg execution and recovery.
//! Implemented in Phase 7 (single leg) and Phase 10 (dual leg).

pub mod arbitrage_executor;
pub mod engine;
pub mod order_manager;
pub mod recovery;
