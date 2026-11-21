//! Strategy center. A strategy only reads data through the data center,
//! decides, and emits standardised order intents to the execution layer.
//! It never calls exchange APIs, reads private keys or account files, or
//! bypasses risk control. Implemented in Phase 9.

pub mod traits;

pub mod funding_arbitrage;
