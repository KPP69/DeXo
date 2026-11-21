//! Order records with controlled state transitions.
//!
//! Orders are keyed by `client_order_id` (the idempotency key). Updates
//! go through the Phase 2 state machine: a stored order may only move
//! to a status reachable via `OrderStatus::can_transition_to`, or stay
//! in the same status (field refresh). Anything else is rejected with
//! `DatabaseError::InvalidTransition` — the local record must never be
//! silently overwritten into an impossible state (Phase 6 acceptance:
//! no unreasonable state overwrites). The exchange stays authoritative;
//! reconciliation converges the local record through legal transitions
//! only and flags the rest as anomalies.

use std::str::FromStr;

use rust_decimal::Decimal;

use crate::models::{Order, OrderStatus, OrderType, Side};

use super::database::{Database, DatabaseError};

pub fn status_code(status: OrderStatus) -> &'static str {
    use OrderStatus::*;
    match status {
        Created => "created",
        RiskChecked => "risk_checked",
        Submitting => "submitting",
        Unknown => "unknown",
        Open => "open",
        PartiallyFilled => "partially_filled",
        Filled => "filled",
        CancelPending => "cancel_pending",
        Cancelled => "cancelled",
        Failed => "failed",
        RollbackPending => "rollback_pending",
        RollbackSubmitted => "rollback_submitted",
        RollbackCompleted => "rollback_completed",
        Reconciling => "reconciling",
    }
}

pub fn parse_status(code: &str) -> Result<OrderStatus, DatabaseError> {
    use OrderStatus::*;
    Ok(match code {
        "created" => Created,
        "risk_checked" => RiskChecked,
        "submitting" => Submitting,
        "unknown" => Unknown,
        "open" => Open,
        "partially_filled" => PartiallyFilled,
        "filled" => Filled,
        "cancel_pending" => CancelPending,
        "cancelled" => Cancelled,
        "failed" => Failed,
        "rollback_pending" => RollbackPending,
        "rollback_submitted" => RollbackSubmitted,
        "rollback_completed" => RollbackCompleted,
        "reconciling" => Reconciling,
        other => {
            return Err(DatabaseError::CorruptRecord(format!(
                "unknown order status '{other}'"
            )))
        }
    })
}

fn side_code(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn parse_side(code: &str) -> Result<Side, DatabaseError> {
    match code {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        other => Err(DatabaseError::CorruptRecord(format!(
            "unknown order side '{other}'"
        ))),
    }
}

fn order_type_code(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::Market => "market",
        OrderType::Limit => "limit",
    }
}

fn parse_order_type(code: &str) -> Result<OrderType, DatabaseError> {
    match code {
        "market" => Ok(OrderType::Market),
        "limit" => Ok(OrderType::Limit),
        other => Err(DatabaseError::CorruptRecord(format!(
            "unknown order type '{other}'"
        ))),
    }
}

fn parse_decimal(raw: Option<String>, field: &str) -> Result<Option<Decimal>, DatabaseError> {
    match raw {
        None => Ok(None),
        Some(text) => Decimal::from_str(&text).map(Some).map_err(|_| {
            DatabaseError::CorruptRecord(format!("unparseable decimal in {field}: '{text}'"))
        }),
    }
}

fn parse_required_decimal(raw: String, field: &str) -> Result<Decimal, DatabaseError> {
    parse_decimal(Some(raw), field)?
        .ok_or_else(|| DatabaseError::CorruptRecord(format!("missing required decimal in {field}")))
}

const ORDER_COLUMNS: &str = "client_order_id, exchange_order_id, account_id, strategy_id, \
    exchange, symbol, side, order_type, price, quantity, filled_quantity, \
    average_fill_price, reduce_only, status, created_at, updated_at";

fn row_to_order(row: &rusqlite::Row) -> Result<Order, DatabaseError> {
    let side_raw: String = row.get(6).map_err(DatabaseError::StoreFailed)?;
    let type_raw: String = row.get(7).map_err(DatabaseError::StoreFailed)?;
    let price_raw: Option<String> = row.get(8).map_err(DatabaseError::StoreFailed)?;
    let qty_raw: String = row.get(9).map_err(DatabaseError::StoreFailed)?;
    let filled_raw: String = row.get(10).map_err(DatabaseError::StoreFailed)?;
    let avg_raw: Option<String> = row.get(11).map_err(DatabaseError::StoreFailed)?;
    let status_raw: String = row.get(13).map_err(DatabaseError::StoreFailed)?;
    Ok(Order {
        client_order_id: row.get(0).map_err(DatabaseError::StoreFailed)?,
        exchange_order_id: row.get(1).map_err(DatabaseError::StoreFailed)?,
        account_id: row.get(2).map_err(DatabaseError::StoreFailed)?,
        strategy_id: row.get(3).map_err(DatabaseError::StoreFailed)?,
        exchange: row.get(4).map_err(DatabaseError::StoreFailed)?,
        symbol: row.get(5).map_err(DatabaseError::StoreFailed)?,
        side: parse_side(&side_raw)?,
        order_type: parse_order_type(&type_raw)?,
        price: parse_decimal(price_raw, "orders.price")?,
        quantity: parse_required_decimal(qty_raw, "orders.quantity")?,
        reduce_only: row.get::<_, i64>(12).map_err(DatabaseError::StoreFailed)? != 0,
        status: parse_status(&status_raw)?,
        filled_quantity: parse_required_decimal(filled_raw, "orders.filled_quantity")?,
        average_fill_price: parse_decimal(avg_raw, "orders.average_fill_price")?,
        created_at: row.get(14).map_err(DatabaseError::StoreFailed)?,
        updated_at: row.get(15).map_err(DatabaseError::StoreFailed)?,
    })
}

impl Database {
    /// Inserts a new order, or updates the stored one when the status
    /// transition is legal (same status, or reachable per the state
    /// machine). Illegal transitions are rejected, never applied.
    pub fn upsert_order(&self, order: &Order) -> Result<(), DatabaseError> {
        let existing = self.stored_status(&order.client_order_id)?;
        match existing {
            None => {
                self.conn()
                    .execute(
                        &format!("INSERT INTO orders ({ORDER_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)"),
                        rusqlite::params![
                            order.client_order_id,
                            order.exchange_order_id,
                            order.account_id,
                            order.strategy_id,
                            order.exchange,
                            order.symbol,
                            side_code(order.side),
                            order_type_code(order.order_type),
                            order.price.map(|d| d.to_string()),
                            order.quantity.to_string(),
                            order.filled_quantity.to_string(),
                            order.average_fill_price.map(|d| d.to_string()),
                            order.reduce_only as i64,
                            status_code(order.status),
                            order.created_at,
                            order.updated_at,
                        ],
                    )
                    .map_err(DatabaseError::StoreFailed)?;
                Ok(())
            }
            Some(current) => {
                if current != order.status && !current.can_transition_to(order.status) {
                    return Err(DatabaseError::InvalidTransition(format!(
                        "order {} cannot move from {} to {}",
                        order.client_order_id,
                        status_code(current),
                        status_code(order.status)
                    )));
                }
                self.conn()
                    .execute(
                        "UPDATE orders SET exchange_order_id = ?2, price = ?3, \
                         filled_quantity = ?4, average_fill_price = ?5, status = ?6, \
                         updated_at = ?7 WHERE client_order_id = ?1",
                        rusqlite::params![
                            order.client_order_id,
                            order.exchange_order_id,
                            order.price.map(|d| d.to_string()),
                            order.filled_quantity.to_string(),
                            order.average_fill_price.map(|d| d.to_string()),
                            status_code(order.status),
                            order.updated_at,
                        ],
                    )
                    .map_err(DatabaseError::StoreFailed)?;
                Ok(())
            }
        }
    }

    fn stored_status(&self, client_order_id: &str) -> Result<Option<OrderStatus>, DatabaseError> {
        let mut stmt = self
            .conn()
            .prepare("SELECT status FROM orders WHERE client_order_id = ?1")
            .map_err(DatabaseError::StoreFailed)?;
        let mut rows = stmt
            .query([client_order_id])
            .map_err(DatabaseError::StoreFailed)?;
        match rows.next().map_err(DatabaseError::StoreFailed)? {
            Some(row) => {
                let code: String = row.get(0).map_err(DatabaseError::StoreFailed)?;
                Ok(Some(parse_status(&code)?))
            }
            None => Ok(None),
        }
    }

    pub fn get_order(&self, client_order_id: &str) -> Result<Option<Order>, DatabaseError> {
        let sql = format!("SELECT {ORDER_COLUMNS} FROM orders WHERE client_order_id = ?1");
        let mut stmt = self
            .conn()
            .prepare(&sql)
            .map_err(DatabaseError::StoreFailed)?;
        let mut rows = stmt
            .query([client_order_id])
            .map_err(DatabaseError::StoreFailed)?;
        match rows.next().map_err(DatabaseError::StoreFailed)? {
            Some(row) => Ok(Some(row_to_order(row)?)),
            None => Ok(None),
        }
    }

    /// Orders of one account, oldest first. With `open_only`, only
    /// non-terminal orders (the ones startup recovery cares about).
    pub fn list_orders(
        &self,
        account_id: &str,
        open_only: bool,
    ) -> Result<Vec<Order>, DatabaseError> {
        let sql = if open_only {
            format!(
                "SELECT {ORDER_COLUMNS} FROM orders WHERE account_id = ?1 \
                 AND status NOT IN ('filled', 'cancelled', 'failed', 'rollback_completed') \
                 ORDER BY created_at ASC"
            )
        } else {
            format!(
                "SELECT {ORDER_COLUMNS} FROM orders WHERE account_id = ?1 ORDER BY created_at ASC"
            )
        };
        let mut stmt = self
            .conn()
            .prepare(&sql)
            .map_err(DatabaseError::StoreFailed)?;
        let mut rows = stmt
            .query([account_id])
            .map_err(DatabaseError::StoreFailed)?;
        let mut orders = Vec::new();
        while let Some(row) = rows.next().map_err(DatabaseError::StoreFailed)? {
            orders.push(row_to_order(row)?);
        }
        Ok(orders)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::database::initialise;
    use rust_decimal_macros::dec;

    fn test_db() -> (tempfile::TempDir, Database) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = initialise(&tmp.path().join("dexo.db")).expect("db");
        (tmp, db)
    }

    fn sample_order(status: OrderStatus) -> Order {
        Order {
            client_order_id: "0xabc".into(),
            exchange_order_id: None,
            account_id: "acc-1".into(),
            strategy_id: String::new(),
            exchange: "hyperliquid".into(),
            symbol: "BTC".into(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: dec!(0.5),
            price: Some(dec!(60000)),
            reduce_only: false,
            status,
            filled_quantity: Decimal::ZERO,
            average_fill_price: None,
            created_at: 1_000,
            updated_at: 1_000,
        }
    }

    #[test]
    fn order_round_trips_through_the_store() {
        let (_tmp, db) = test_db();
        db.upsert_order(&sample_order(OrderStatus::Created))
            .expect("insert");
        let stored = db.get_order("0xabc").expect("read").expect("present");
        assert_eq!(stored.quantity, dec!(0.5));
        assert_eq!(stored.price, Some(dec!(60000)));
        assert_eq!(stored.status, OrderStatus::Created);
        assert_eq!(stored.side, Side::Buy);
    }

    #[test]
    fn legal_transitions_update_and_illegal_ones_are_rejected() {
        let (_tmp, db) = test_db();
        db.upsert_order(&sample_order(OrderStatus::Created))
            .expect("insert");
        let mut next = sample_order(OrderStatus::RiskChecked);
        next.updated_at = 2_000;
        db.upsert_order(&next).expect("created -> risk_checked");
        // Open is not reachable from RiskChecked (must go via Submitting).
        let illegal = sample_order(OrderStatus::Open);
        let result = db.upsert_order(&illegal);
        assert!(matches!(result, Err(DatabaseError::InvalidTransition(_))));
        // The stored record is untouched by the rejected update.
        let stored = db.get_order("0xabc").expect("read").expect("present");
        assert_eq!(stored.status, OrderStatus::RiskChecked);
    }

    #[test]
    fn same_status_refresh_updates_fill_fields() {
        let (_tmp, db) = test_db();
        db.upsert_order(&sample_order(OrderStatus::Open))
            .expect("insert");
        let mut refreshed = sample_order(OrderStatus::Open);
        refreshed.filled_quantity = dec!(0.2);
        refreshed.average_fill_price = Some(dec!(60010));
        db.upsert_order(&refreshed).expect("same-status refresh");
        let stored = db.get_order("0xabc").expect("read").expect("present");
        assert_eq!(stored.filled_quantity, dec!(0.2));
        assert_eq!(stored.average_fill_price, Some(dec!(60010)));
    }

    #[test]
    fn open_only_listing_skips_terminal_orders() {
        let (_tmp, db) = test_db();
        let mut open = sample_order(OrderStatus::Open);
        open.client_order_id = "0x1".into();
        db.upsert_order(&open).expect("open");
        let mut done = sample_order(OrderStatus::Filled);
        done.client_order_id = "0x2".into();
        db.upsert_order(&done).expect("filled");
        let open_orders = db.list_orders("acc-1", true).expect("list open");
        assert_eq!(open_orders.len(), 1);
        assert_eq!(open_orders[0].client_order_id, "0x1");
        assert_eq!(db.list_orders("acc-1", false).expect("list all").len(), 2);
    }

    #[test]
    fn status_codes_round_trip_for_every_variant() {
        use OrderStatus::*;
        for status in [
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
        ] {
            assert_eq!(parse_status(status_code(status)).expect("parse"), status);
        }
    }
}
