//! Risk center: four layers (global, account, strategy, order) plus a
//! global Kill Switch. The Kill Switch blocks new position opening while
//! keeping cancel / reduce / close capabilities; it is not a forced
//! liquidation. Implemented in Phase 8.

pub mod account;
pub mod global;
pub mod order;
pub mod strategy;

use crate::models::{OrderRequest, RiskContext, RiskResult};

/// One risk layer's check (interface defined in Phase 2, layers
/// implemented in Phase 8). Checks are synchronous and pure: a layer
/// consults only the request and the context, so every layer is unit
/// testable without an exchange or a database. The execution layer runs
/// the layers in order — global, account, strategy, order — and the
/// first rejection stops the order.
pub trait RiskCheck: Send + Sync {
    fn check_order(&self, context: &RiskContext, request: &OrderRequest) -> RiskResult;
}
