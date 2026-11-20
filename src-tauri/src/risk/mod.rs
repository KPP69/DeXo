//! Risk center: four layers (global, account, strategy, order) plus a
//! global Kill Switch. The Kill Switch blocks new position opening while
//! keeping cancel / reduce / close capabilities; it is not a forced
//! liquidation. Implemented in Phase 8.

pub mod account;
pub mod global;
pub mod order;
pub mod strategy;
