//! The unified strategy trait (Phase 2).
//!
//! A strategy only reads through the data center's `DataAccess`, decides,
//! and returns standardised `OrderRequest`s. It never calls an exchange,
//! never sees credentials or account files, and never bypasses risk
//! control: the execution layer hands every returned request to the risk
//! center before anything is submitted.

use async_trait::async_trait;

use crate::data::DataAccess;
use crate::models::{OrderRequest, StrategyError};

#[async_trait]
pub trait Strategy: Send + Sync {
    /// Stable strategy instance identifier, e.g. "funding-arb-hl1".
    fn id(&self) -> &str;

    /// The account this strategy instance trades on.
    fn account_id(&self) -> &str;

    /// Called once when the strategy is started by the execution layer.
    async fn on_start(&self) -> Result<(), StrategyError> {
        Ok(())
    }

    /// Called once when the strategy is stopped; must not emit orders.
    async fn on_stop(&self) -> Result<(), StrategyError> {
        Ok(())
    }

    /// One evaluation pass: read data, decide, return order intents
    /// (possibly none). Returning an error stops this pass, not the
    /// application; the execution layer logs it and applies the
    /// strategy's error policy.
    async fn evaluate(&self, data: &dyn DataAccess) -> Result<Vec<OrderRequest>, StrategyError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Balance, DataError, FundingInfo, OrderBook, Position};
    use rust_decimal_macros::dec;

    struct StubData;

    #[async_trait]
    impl DataAccess for StubData {
        async fn funding(&self, exchange: &str, symbol: &str) -> Result<FundingInfo, DataError> {
            Ok(FundingInfo {
                exchange: exchange.to_string(),
                symbol: symbol.to_string(),
                rate: dec!(0.001),
                next_funding_at: None,
                observed_at: 1,
            })
        }

        async fn order_book(&self, _: &str, _: &str) -> Result<OrderBook, DataError> {
            Err(DataError::NotFound("no book in stub".into()))
        }

        async fn balance(&self, _: &str) -> Result<Balance, DataError> {
            Err(DataError::NotFound("no balance in stub".into()))
        }

        async fn positions(&self, _: &str) -> Result<Vec<Position>, DataError> {
            Ok(Vec::new())
        }
    }

    struct ThresholdStrategy;

    #[async_trait]
    impl Strategy for ThresholdStrategy {
        fn id(&self) -> &str {
            "threshold-test"
        }

        fn account_id(&self) -> &str {
            "mock-1"
        }

        async fn evaluate(
            &self,
            data: &dyn DataAccess,
        ) -> Result<Vec<OrderRequest>, StrategyError> {
            let funding = data.funding("mock", "BTC").await?;
            if funding.rate > dec!(0.0005) {
                return Ok(vec![OrderRequest {
                    account_id: "mock-1".into(),
                    strategy_id: "threshold-test".into(),
                    exchange: "mock".into(),
                    symbol: "BTC".into(),
                    side: crate::models::Side::Sell,
                    order_type: crate::models::OrderType::Market,
                    quantity: dec!(0.1),
                    price: None,
                    reduce_only: false,
                    client_order_id: "dexo-strat-1".into(),
                    created_at: 2,
                }]);
            }
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn strategy_emits_requests_through_data_access_only() {
        let strategy = ThresholdStrategy;
        let requests = strategy
            .evaluate(&StubData)
            .await
            .expect("evaluation succeeds");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].account_id, strategy.account_id());
        assert_eq!(requests[0].strategy_id, strategy.id());
    }
}
