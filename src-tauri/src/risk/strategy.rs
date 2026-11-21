//! Strategy risk layer: stopped strategies cannot open, per-strategy
//! order value caps, and per-strategy daily loss limits.

use rust_decimal::Decimal;

use crate::models::{OrderRequest, RiskContext, RiskLayer};

use super::{exposure_increase, order_notional, LayerOutcome, RiskLimits, RiskState};

pub fn check(
    limits: &RiskLimits,
    state: &RiskState,
    _context: &RiskContext,
    request: &OrderRequest,
) -> LayerOutcome {
    if !request.reduce_only && limits.disabled_strategies.contains(&request.strategy_id) {
        return super::reject(
            RiskLayer::Strategy,
            format!("strategy {} is stopped", request.strategy_id),
        );
    }
    if let Some(strategy_limits) = limits.strategies.get(&request.strategy_id) {
        if let Some(max_order) = strategy_limits.max_order_notional {
            if let Some(notional) = order_notional(state, request) {
                if notional > max_order {
                    return super::reject(
                        RiskLayer::Strategy,
                        format!("order value {notional} exceeds strategy cap {max_order}"),
                    );
                }
            }
        }
        if let Some(max_loss) = strategy_limits.max_daily_loss {
            let increase = exposure_increase(state, request).unwrap_or(Decimal::ZERO);
            if increase > Decimal::ZERO {
                let pnl = state
                    .strategy_daily_pnl
                    .get(&request.strategy_id)
                    .copied()
                    .unwrap_or(Decimal::ZERO);
                if pnl <= -max_loss {
                    return super::reject(
                        RiskLayer::Strategy,
                        format!("strategy daily loss limit reached: pnl {pnl} <= -{max_loss}"),
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::tests::{funded_state, sample_context, sample_request};
    use crate::risk::StrategyLimits;
    use rust_decimal_macros::dec;

    #[test]
    fn stopped_strategy_cannot_open_but_can_reduce() {
        let mut limits = RiskLimits::default();
        limits.disabled_strategies.insert("strat-1".into());
        let state = funded_state();
        assert!(check(&limits, &state, &sample_context(), &sample_request()).is_err());
        let mut closing = sample_request();
        closing.reduce_only = true;
        assert!(check(&limits, &state, &sample_context(), &closing).is_ok());
    }

    #[test]
    fn strategy_daily_loss_stops_openings() {
        let mut limits = RiskLimits::default();
        limits.strategies.insert(
            "strat-1".into(),
            StrategyLimits {
                max_order_notional: None,
                max_daily_loss: Some(dec!(100)),
            },
        );
        let mut state = funded_state();
        state
            .strategy_daily_pnl
            .insert("strat-1".into(), dec!(-150));
        assert!(check(&limits, &state, &sample_context(), &sample_request()).is_err());
    }
}
