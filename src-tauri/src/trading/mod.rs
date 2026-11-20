//! Trading center: order, fill and position records, trade history,
//! persistence and startup reconciliation. SQLite is the local record; the
//! exchange is authoritative for real positions and fills.
//!
//! Phase 1 provides only the database foundation (`database`); the record
//! modules land in Phase 6.

pub mod database;
pub mod fills;
pub mod orders;
pub mod positions;
pub mod reconciliation;
