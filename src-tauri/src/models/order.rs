//! Order models and the controlled order state machine.
//!
//! State transitions are explicit: an order may only move along the edges
//! in [`OrderStatus::can_transition_to`]. A network timeout never moves an
//! order to `Failed` directly; it moves to `Unknown`, from which the real
//! state must be established by querying the exchange before any further
//! action (see DEVELOPMENT_GUIDE.md section 7).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::TimestampMs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Market,
    Limit,
}

/// The unified order lifecycle. Variants are named exactly as in the
/// development guide so logs and database records stay comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderStatus {
    Created,
    RiskChecked,
    Submitting,
    Unknown,
    Open,
    PartiallyFilled,
    Filled,
    CancelPending,
    Cancelled,
    Failed,
    RollbackPending,
    RollbackSubmitted,
    RollbackCompleted,
    Reconciling,
}

impl OrderStatus {
    /// Terminal states never transition further.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Filled | Self::Cancelled | Self::Failed | Self::RollbackCompleted
        )
    }

    /// Whether the order may still gain fills.
    pub fn is_live(&self) -> bool {
        matches!(
            self,
            Self::Open | Self::PartiallyFilled | Self::CancelPending
        )
    }

    /// The controlled transition table. Anything not listed here is a bug
    /// and must be rejected by callers, never silently applied.
    pub fn can_transition_to(&self, next: OrderStatus) -> bool {
        use OrderStatus::*;
        let allowed: &[OrderStatus] = match self {
            Created => &[RiskChecked, Failed],
            RiskChecked => &[Submitting, Failed],
            Submitting => &[Open, PartiallyFilled, Filled, Unknown, Failed],
            Unknown => &[
                Open,
                PartiallyFilled,
                Filled,
                Cancelled,
                Failed,
                Reconciling,
            ],
            Open => &[PartiallyFilled, Filled, CancelPending, Reconciling],
            PartiallyFilled => &[Filled, CancelPending, Reconciling],
            CancelPending => &[Cancelled, Filled, PartiallyFilled],
            Reconciling => &[
                Open,
                PartiallyFilled,
                Filled,
                Cancelled,
                Failed,
                RollbackPending,
            ],
            RollbackPending => &[RollbackSubmitted, Failed],
            RollbackSubmitted => &[RollbackCompleted, Failed],
            Filled | Cancelled | Failed | RollbackCompleted => &[],
        };
        allowed.contains(&next)
    }
}

/// A standardised order intent produced by a strategy and handed to the
/// execution layer. It is not yet an exchange order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderRequest {
    pub account_id: String,
    pub strategy_id: String,
    pub exchange: String,
    pub symbol: String,
    pub side: Side,
    pub order_type: OrderType,
    /// Base-asset quantity, always positive; direction comes from `side`.
    pub quantity: Decimal,
    /// Required for `OrderType::Limit`, ignored for market orders.
    pub price: Option<Decimal>,
    pub reduce_only: bool,
    /// Stable idempotency key generated before submission; used for
    /// de-duplication, timeout queries and recovery.
    pub client_order_id: String,
    pub created_at: TimestampMs,
}

/// An order tracked by the trading center, from creation to terminal
/// state. The exchange remains authoritative for the real fill state;
/// this record is the local, recoverable view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub client_order_id: String,
    pub exchange_order_id: Option<String>,
    pub account_id: String,
    pub strategy_id: String,
    pub exchange: String,
    pub symbol: String,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub price: Option<Decimal>,
    pub reduce_only: bool,
    pub status: OrderStatus,
    pub filled_quantity: Decimal,
    pub average_fill_price: Option<Decimal>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

impl Order {
    /// Builds a fresh `Created` order from a risk-checked request.
    pub fn from_request(request: &OrderRequest, now: TimestampMs) -> Self {
        Self {
            client_order_id: request.client_order_id.clone(),
            exchange_order_id: None,
            account_id: request.account_id.clone(),
            strategy_id: request.strategy_id.clone(),
            exchange: request.exchange.clone(),
            symbol: request.symbol.clone(),
            side: request.side,
            order_type: request.order_type,
            quantity: request.quantity,
            price: request.price,
            reduce_only: request.reduce_only,
            status: OrderStatus::Created,
            filled_quantity: Decimal::ZERO,
            average_fill_price: None,
            created_at: request.created_at,
            updated_at: now,
        }
    }

    /// Quantity not yet filled, based on actual fills — rollback and
    /// exposure calculations must use this, never the planned quantity
    /// alone.
    pub fn remaining_quantity(&self) -> Decimal {
        self.quantity - self.filled_quantity
    }
}

/// A single executed fill reported by an exchange.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub client_order_id: String,
    pub exchange_order_id: Option<String>,
    pub exchange_fill_id: Option<String>,
    pub account_id: String,
    pub exchange: String,
    pub symbol: String,
    pub side: Side,
    pub quantity: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
    pub filled_at: TimestampMs,
}

/// A completed trade record (one full open/close cycle leg as recorded by
/// the trading center for statistics and audit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trade {
    pub id: String,
    pub account_id: String,
    pub strategy_id: String,
    pub exchange: String,
    pub symbol: String,
    pub side: Side,
    pub quantity: Decimal,
    pub price: Decimal,
    pub realised_pnl: Option<Decimal>,
    pub closed_at: TimestampMs,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn happy_path_transitions_are_allowed() {
        use OrderStatus::*;
        let path = [
            Created,
            RiskChecked,
            Submitting,
            Open,
            PartiallyFilled,
            Filled,
        ];
        for pair in path.windows(2) {
            assert!(
                pair[0].can_transition_to(pair[1]),
                "{:?} -> {:?} must be allowed",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn timeout_goes_to_unknown_never_straight_to_failed_from_open() {
        use OrderStatus::*;
        assert!(Submitting.can_transition_to(Unknown));
        assert!(Unknown.can_transition_to(Reconciling));
        // An open order whose status is merely unconfirmed must be
        // reconciled, not written off or resubmitted.
        assert!(!Open.can_transition_to(Failed));
        assert!(!Open.can_transition_to(Submitting));
    }

    #[test]
    fn terminal_states_have_no_outgoing_transitions() {
        use OrderStatus::*;
        for state in [Filled, Cancelled, Failed, RollbackCompleted] {
            assert!(state.is_terminal());
            assert!(!state.can_transition_to(Open));
            assert!(!state.can_transition_to(Reconciling));
        }
    }

    #[test]
    fn rollback_path_is_reachable_from_reconciling() {
        use OrderStatus::*;
        assert!(Reconciling.can_transition_to(RollbackPending));
        assert!(RollbackPending.can_transition_to(RollbackSubmitted));
        assert!(RollbackSubmitted.can_transition_to(RollbackCompleted));
    }

    #[test]
    fn remaining_quantity_uses_actual_fills() {
        let request = OrderRequest {
            account_id: "hl1".into(),
            strategy_id: "funding-arb".into(),
            exchange: "hyperliquid".into(),
            symbol: "BTC".into(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: dec!(1.0),
            price: Some(dec!(90000)),
            reduce_only: false,
            client_order_id: "dexo-test-1".into(),
            created_at: 10,
        };
        let mut order = Order::from_request(&request, 11);
        assert_eq!(order.status, OrderStatus::Created);
        assert_eq!(order.remaining_quantity(), dec!(1.0));
        order.filled_quantity = dec!(0.4);
        assert_eq!(order.remaining_quantity(), dec!(0.6));
    }

    #[test]
    fn order_status_serialises_as_screaming_snake_case() {
        let json = serde_json::to_string(&OrderStatus::PartiallyFilled).expect("serialise");
        assert_eq!(json, "\"PARTIALLY_FILLED\"");
    }
}
