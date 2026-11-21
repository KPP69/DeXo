//! Global risk layer: Kill Switch, maximum daily loss circuit
//! breaker, and the total exposure cap across all accounts.

use rust_decimal::Decimal;

use crate::models::{OrderRequest, RiskContext, RiskLayer};

use super::{exposure_increase, LayerOutcome, RiskLimits, RiskState};

pub fn check(
    limits: &RiskLimits,
    state: &RiskState,
    _context: &RiskContext,
    request: &OrderRequest,
    kill_switch_active: bool,
    kill_reason: &str,
) -> LayerOutcome {
    // Kill Switch: blocks new openings only. Reduce-only orders (and,
    // outside this trait, cancels and closes) keep working; nothing
    // is liquidated by the switch.
    if kill_switch_active && !request.reduce_only {
        return super::reject(
            RiskLayer::Global,
            format!("kill switch active: {kill_reason}"),
        );
    }
    // Exposure-dependent checks only bind orders that add exposure.
    let increase = exposure_increase(state, request).unwrap_or(Decimal::ZERO);
    if increase > Decimal::ZERO {
        if let Some(max_loss) = limits.global.max_daily_loss {
            if state.daily_pnl <= -max_loss {
                return super::reject(
                    RiskLayer::Global,
                    format!(
                        "daily loss limit reached: pnl {} <= -{}",
                        state.daily_pnl, max_loss
                    ),
                );
            }
        }
        if let Some(max_exposure) = limits.global.max_total_exposure {
            let total = state.total_exposure() + increase;
            if total > max_exposure {
                return super::reject(
                    RiskLayer::Global,
                    format!("total exposure {total} would exceed cap {max_exposure}"),
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::tests::{funded_state, sample_context, sample_request};
    use rust_decimal_macros::dec;

    #[test]
    fn daily_loss_breaker_blocks_openings_not_reductions() {
        let mut limits = RiskLimits::default();
        limits.global.max_daily_loss = Some(dec!(500));
        let mut state = funded_state();
        state.daily_pnl = dec!(-600);
        let result = check(
            &limits,
            &state,
            &sample_context(),
            &sample_request(),
            false,
            "",
        );
        assert!(result.is_err());
        let mut closing = sample_request();
        closing.reduce_only = true;
        closing.side = crate::models::Side::Sell;
        assert!(check(&limits, &state, &sample_context(), &closing, false, "").is_ok());
    }

    #[test]
    fn total_exposure_cap_is_enforced() {
        let mut limits = RiskLimits::default();
        limits.global.max_total_exposure = Some(dec!(5000));
        // Sample order adds 0.1 BTC × 60000 = 6000 of exposure.
        let result = check(
            &limits,
            &funded_state(),
            &sample_context(),
            &sample_request(),
            false,
            "",
        );
        assert!(result.is_err());
    }
}
