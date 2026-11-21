//! Account risk layer: balance availability, leverage, per-account
//! exposure and per-order value caps.

use rust_decimal::Decimal;

use crate::models::{OrderRequest, RiskContext, RiskLayer};

use super::{exposure_increase, order_notional, LayerOutcome, RiskLimits, RiskState};

pub fn check(
    limits: &RiskLimits,
    state: &RiskState,
    _context: &RiskContext,
    request: &OrderRequest,
) -> LayerOutcome {
    let account_limits = limits.account_limits(&request.account_id);
    let increase = exposure_increase(state, request).unwrap_or(Decimal::ZERO);

    // An opening order cannot be judged without a known balance;
    // unknown is not treated as sufficient. (Reduce-only orders do not
    // need margin and skip this.)
    let balance = if request.reduce_only {
        state.balances.get(&request.account_id)
    } else {
        Some(state.balances.get(&request.account_id).ok_or_else(|| {
            crate::models::RiskResult::Rejected {
                layer: RiskLayer::Account,
                reason: format!("balance unknown for account {}", request.account_id),
            }
        })?)
    };

    if increase > Decimal::ZERO {
        let balance = balance.expect("balance required when exposure increases");
        // Margin feasibility: the added value's margin (value divided
        // by the leverage cap) must fit in the available balance.
        if account_limits.max_leverage > Decimal::ZERO
            && increase > balance.available * account_limits.max_leverage
        {
            return super::reject(
                RiskLayer::Account,
                format!(
                    "order value increase {increase} exceeds available {} at max leverage {}",
                    balance.available, account_limits.max_leverage
                ),
            );
        }
        if let Some(max_exposure) = account_limits.max_exposure {
            let total = state.account_exposure(&request.account_id) + increase;
            if total > max_exposure {
                return super::reject(
                    RiskLayer::Account,
                    format!("account exposure {total} would exceed cap {max_exposure}"),
                );
            }
        }
        // Existing leverage on the traded symbol must be within cap.
        if let Some(position) = state.positions.get(&request.account_id).and_then(|list| {
            list.iter()
                .find(|p| p.exchange == request.exchange && p.symbol == request.symbol)
        }) {
            if let Some(leverage) = position.leverage {
                if leverage > account_limits.max_leverage {
                    return super::reject(
                        RiskLayer::Account,
                        format!(
                            "existing leverage {leverage} exceeds cap {}",
                            account_limits.max_leverage
                        ),
                    );
                }
            }
        }
    }

    if let Some(max_order) = account_limits.max_order_notional {
        if let Some(notional) = order_notional(state, request) {
            if notional > max_order {
                return super::reject(
                    RiskLayer::Account,
                    format!("order value {notional} exceeds account cap {max_order}"),
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
    fn unknown_balance_blocks_openings() {
        let state = RiskState::default();
        let result = check(
            &RiskLimits::default(),
            &state,
            &sample_context(),
            &sample_request(),
        );
        assert!(matches!(
            result,
            Err(crate::models::RiskResult::Rejected {
                layer: RiskLayer::Account,
                ..
            })
        ));
    }

    #[test]
    fn margin_check_uses_available_balance_and_leverage_cap() {
        // Available 100 USDC at max 20x supports at most 2000 of new
        // value; the sample order adds 6000.
        let mut state = funded_state();
        state.balances.get_mut("acc-1").expect("balance").available = dec!(100);
        let result = check(
            &RiskLimits::default(),
            &state,
            &sample_context(),
            &sample_request(),
        );
        assert!(result.is_err());
    }
}
