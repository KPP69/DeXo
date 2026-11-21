//! Trading center: order, fill and position records, trade history,
//! persistence and startup reconciliation. SQLite is the local record; the
//! exchange is authoritative for real positions and fills.
//!
//! Phase 6 provides the strict-migration database foundation
//! (`database`), the order/fill/position record stores, and startup
//! reconciliation against the exchange.

pub mod database;
pub mod fills;
pub mod orders;
pub mod positions;
pub mod reconciliation;
