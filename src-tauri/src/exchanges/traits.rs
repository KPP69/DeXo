//! The unified exchange Adapter trait (Phase 2).
//!
//! Design decisions (binding for every adapter):
//! - One adapter instance is bound to one account by the account center
//!   (Phase 3), which owns the credentials. Trait methods therefore do
//!   not take `account_id` or credentials; results still carry
//!   `account_id` so records stay attributable.
//! - Unsupported capabilities return `ExchangeError::Unsupported`,
//!   never a fabricated success or an empty default.
//! - `place_order` takes the standardised `OrderRequest`, whose
//!   `client_order_id` is the idempotency key. A timeout surfaces as
//!   `ExchangeError::Timeout` and the caller must establish the real
//!   state through `get_order` before doing anything else.
//! - Namespace venues (for example Entropy on Hyperliquid HIP-3) are
//!   served by the parent venue's adapter; there is no separate adapter
//!   type for them.

use async_trait::async_trait;

use crate::models::{
    Balance, ExchangeError, Fill, FundingInfo, Order, OrderBook, OrderRequest, Position,
    TimestampMs,
};

#[async_trait]
pub trait ExchangeAdapter: Send + Sync {
    /// `Exchange::id` this adapter serves, e.g. "hyperliquid".
    fn name(&self) -> &str;

    /// The account this instance is bound to.
    fn account_id(&self) -> &str;

    async fn get_funding(&self, symbol: &str) -> Result<FundingInfo, ExchangeError>;

    async fn get_orderbook(&self, symbol: &str) -> Result<OrderBook, ExchangeError>;

    async fn get_positions(&self) -> Result<Vec<Position>, ExchangeError>;

    async fn get_balance(&self) -> Result<Balance, ExchangeError>;

    /// Submits an order. On success returns the tracked order as the
    /// venue acknowledged it; the venue's state remains authoritative.
    async fn place_order(&self, request: &OrderRequest) -> Result<Order, ExchangeError>;

    /// Queries one order by its `client_order_id` (falling back to the
    /// venue's id mapping the adapter keeps). This is the mandatory
    /// first step after any ambiguous submission outcome.
    async fn get_order(&self, client_order_id: &str) -> Result<Order, ExchangeError>;

    async fn cancel_order(&self, client_order_id: &str) -> Result<Order, ExchangeError>;

    /// Fills since `since` (epoch milliseconds), or all fills the venue
    /// returns by default when `None`.
    async fn get_fills(&self, since: Option<TimestampMs>) -> Result<Vec<Fill>, ExchangeError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{OrderStatus, OrderType, Side};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    /// A minimal in-memory adapter proving the trait is implementable and
    /// that a venue can be driven entirely through the unified models.
    struct MockExchange {
        order: std::sync::Mutex<Option<Order>>,
    }

    #[async_trait]
    impl ExchangeAdapter for MockExchange {
        fn name(&self) -> &str {
            "mock"
        }

        fn account_id(&self) -> &str {
            "mock-1"
        }

        async fn get_funding(&self, symbol: &str) -> Result<FundingInfo, ExchangeError> {
            Ok(FundingInfo {
                exchange: "mock".into(),
                symbol: symbol.to_string(),
                rate: dec!(0.0001),
                next_funding_at: None,
                observed_at: 1,
            })
        }

        async fn get_orderbook(&self, _symbol: &str) -> Result<OrderBook, ExchangeError> {
            Err(ExchangeError::Unsupported(
                "order book not implemented in mock".into(),
            ))
        }

        async fn get_positions(&self) -> Result<Vec<Position>, ExchangeError> {
            Ok(Vec::new())
        }

        async fn get_balance(&self) -> Result<Balance, ExchangeError> {
            Ok(Balance {
                account_id: "mock-1".into(),
                exchange: "mock".into(),
                currency: "USDC".into(),
                total: dec!(1000),
                available: dec!(1000),
                updated_at: 1,
            })
        }

        async fn place_order(&self, request: &OrderRequest) -> Result<Order, ExchangeError> {
            let mut order = Order::from_request(request, 2);
            order.status = OrderStatus::Filled;
            order.filled_quantity = request.quantity;
            order.average_fill_price = request.price;
            *self.order.lock().expect("mock lock") = Some(order.clone());
            Ok(order)
        }

        async fn get_order(&self, client_order_id: &str) -> Result<Order, ExchangeError> {
            self.order
                .lock()
                .expect("mock lock")
                .clone()
                .filter(|o| o.client_order_id == client_order_id)
                .ok_or_else(|| ExchangeError::InvalidRequest("unknown order".into()))
        }

        async fn cancel_order(&self, client_order_id: &str) -> Result<Order, ExchangeError> {
            let mut order = self.get_order(client_order_id).await?;
            order.status = OrderStatus::Cancelled;
            Ok(order)
        }

        async fn get_fills(&self, _since: Option<TimestampMs>) -> Result<Vec<Fill>, ExchangeError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn unified_models_flow_through_the_trait() {
        let adapter = MockExchange {
            order: std::sync::Mutex::new(None),
        };
        let request = OrderRequest {
            account_id: "mock-1".into(),
            strategy_id: "test".into(),
            exchange: "mock".into(),
            symbol: "BTC".into(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: dec!(0.5),
            price: Some(dec!(90000)),
            reduce_only: false,
            client_order_id: "dexo-mock-1".into(),
            created_at: 1,
        };
        let placed = adapter.place_order(&request).await.expect("place");
        assert_eq!(placed.status, OrderStatus::Filled);
        assert_eq!(placed.remaining_quantity(), Decimal::ZERO);
        let fetched = adapter.get_order("dexo-mock-1").await.expect("query");
        assert_eq!(fetched.client_order_id, "dexo-mock-1");
        // Unsupported capability is an explicit error, not a fake success.
        assert!(matches!(
            adapter.get_orderbook("BTC").await,
            Err(ExchangeError::Unsupported(_))
        ));
    }
}
