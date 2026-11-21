//! Order risk layer: the order itself — positive size, a valid limit
//! price, minimum order value, price deviation against the reference
//! price, and reference-data freshness (stale data cannot justify an
//! order; this is where the data center's staleness protection bites).

use rust_decimal::Decimal;

use crate::models::{OrderRequest, OrderType, RiskContext, RiskLayer};

use super::{order_notional, LayerOutcome, RiskLimits, RiskState};

pub fn check(
    limits: &RiskLimits,
    state: &RiskState,
    context: &RiskContext,
    request: &OrderRequest,
) -> LayerOutcome {
    if request.quantity <= Decimal::ZERO {
        return super::reject(RiskLayer::Order, "quantity must be positive");
    }
    if request.order_type == OrderType::Limit {
        match request.price {
            Some(price) if price > Decimal::ZERO => {}
            _ => return super::reject(RiskLayer::Order, "limit order needs a positive price"),
        }
    }
    // Reference data must exist and be fresh, for both order types:
    // market orders are valued and deviation-checked against it, and
    // limit orders are deviation-checked against it.
    let (reference, observed_at) = state
        .reference_price(&request.exchange, &request.symbol)
        .ok_or_else(|| crate::models::RiskResult::Rejected {
            layer: RiskLayer::Order,
            reason: format!(
                "no reference price for {} {}",
                request.exchange, request.symbol
            ),
        })?;
    let age = context.evaluated_at - observed_at;
    if age > limits.global.max_data_age_ms {
        return super::reject(
            RiskLayer::Order,
            format!(
                "reference data is {age} ms old (limit {} ms)",
                limits.global.max_data_age_ms
            ),
        );
    }
    if reference <= Decimal::ZERO {
        return super::reject(RiskLayer::Order, "reference price is not positive");
    }
    let notional =
        order_notional(state, request).ok_or_else(|| crate::models::RiskResult::Rejected {
            layer: RiskLayer::Order,
            reason: "order cannot be valued".to_string(),
        })?;
    if notional < limits.global.min_order_notional {
        return super::reject(
            RiskLayer::Order,
            format!(
                "order value {notional} below minimum {}",
                limits.global.min_order_notional
            ),
        );
    }
    if let (OrderType::Limit, Some(price)) = (request.order_type, request.price) {
        let deviation = (price - reference).abs() / reference;
        if deviation > limits.global.max_price_deviation {
            return super::reject(
                RiskLayer::Order,
                format!(
                    "limit price {price} deviates {deviation} from reference {reference} (limit {})",
                    limits.global.max_price_deviation
                ),
            );
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
    fn stale_reference_data_is_rejected() {
        let mut state = funded_state();
        state.reference_prices.insert(
            ("hyperliquid".into(), "BTC".into()),
            (dec!(60000), 10_000 - 60_000),
        );
        let result = check(
            &RiskLimits::default(),
            &state,
            &sample_context(),
            &sample_request(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn deviating_limit_price_is_rejected() {
        let mut request = sample_request();
        request.price = Some(dec!(80000)); // +33% vs reference 60000
        let result = check(
            &RiskLimits::default(),
            &funded_state(),
            &sample_context(),
            &request,
        );
        assert!(result.is_err());
    }

    #[test]
    fn dust_orders_are_rejected() {
        let mut request = sample_request();
        request.quantity = dec!(0.0001); // value 6 < minimum 10
        request.price = Some(dec!(60000));
        let result = check(
            &RiskLimits::default(),
            &funded_state(),
            &sample_context(),
            &request,
        );
        assert!(result.is_err());
    }
}
