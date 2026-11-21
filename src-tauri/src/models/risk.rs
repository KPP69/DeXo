//! Risk models: the verdict type every risk layer returns, and the
//! context an order check is evaluated against.

use serde::{Deserialize, Serialize};

use super::TimestampMs;

/// The four risk layers, in evaluation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLayer {
    Global,
    Account,
    Strategy,
    Order,
}

/// Outcome of a risk check. A rejection always names the layer that
/// rejected and a human-readable reason; it is never a bare boolean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "verdict")]
pub enum RiskResult {
    Approved,
    Rejected { layer: RiskLayer, reason: String },
}

impl RiskResult {
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }

    pub fn rejected(layer: RiskLayer, reason: impl Into<String>) -> Self {
        Self::Rejected {
            layer,
            reason: reason.into(),
        }
    }
}

/// Everything a risk layer may consult for one order check. Kept free of
/// credentials and raw exchange payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskContext {
    pub account_id: String,
    pub strategy_id: String,
    /// Global Kill Switch state: when active, opening orders are rejected
    /// at the global layer while reduce-only / close orders still pass to
    /// the remaining layers.
    pub kill_switch_active: bool,
    pub evaluated_at: TimestampMs,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_carries_layer_and_reason() {
        let result = RiskResult::rejected(RiskLayer::Global, "kill switch active");
        assert!(!result.is_approved());
        match result {
            RiskResult::Rejected { layer, reason } => {
                assert_eq!(layer, RiskLayer::Global);
                assert_eq!(reason, "kill switch active");
            }
            RiskResult::Approved => panic!("must be rejected"),
        }
        assert!(RiskResult::Approved.is_approved());
    }
}
