//! Fill records. Fills are facts reported by the exchange; recording
//! the same fill twice (for example when reconciliation re-reads the
//! fill history) is explicitly idempotent: `record_fill` reports
//! whether it inserted, and a duplicate is a no-op, never an overwrite.

use std::str::FromStr;

use rust_decimal::Decimal;

use crate::models::{Fill, Side};

use super::database::{Database, DatabaseError};

/// Stable deduplication key for a fill. Exchange fill ids are unique
/// per venue; fills without one fall back to order + time + price +
/// quantity, which is the best identity available for them.
fn fill_key(fill: &Fill) -> String {
    match &fill.exchange_fill_id {
        Some(id) => format!("{}:{id}", fill.exchange),
        None => format!(
            "{}:{}:{}:{}:{}",
            fill.exchange, fill.client_order_id, fill.filled_at, fill.price, fill.quantity
        ),
    }
}

fn row_to_fill(row: &rusqlite::Row) -> Result<Fill, DatabaseError> {
    let side_raw: String = row.get(7).map_err(DatabaseError::StoreFailed)?;
    let qty_raw: String = row.get(8).map_err(DatabaseError::StoreFailed)?;
    let price_raw: String = row.get(9).map_err(DatabaseError::StoreFailed)?;
    let fee_raw: String = row.get(10).map_err(DatabaseError::StoreFailed)?;
    let parse = |raw: String, field: &str| {
        Decimal::from_str(&raw).map_err(|_| {
            DatabaseError::CorruptRecord(format!("unparseable decimal in {field}: '{raw}'"))
        })
    };
    Ok(Fill {
        client_order_id: row.get(1).map_err(DatabaseError::StoreFailed)?,
        exchange_order_id: row.get(2).map_err(DatabaseError::StoreFailed)?,
        exchange_fill_id: row.get(3).map_err(DatabaseError::StoreFailed)?,
        account_id: row.get(4).map_err(DatabaseError::StoreFailed)?,
        exchange: row.get(5).map_err(DatabaseError::StoreFailed)?,
        symbol: row.get(6).map_err(DatabaseError::StoreFailed)?,
        side: match side_raw.as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            other => {
                return Err(DatabaseError::CorruptRecord(format!(
                    "unknown fill side '{other}'"
                )))
            }
        },
        quantity: parse(qty_raw, "fills.quantity")?,
        price: parse(price_raw, "fills.price")?,
        fee: parse(fee_raw, "fills.fee")?,
        filled_at: row.get(11).map_err(DatabaseError::StoreFailed)?,
    })
}

const FILL_COLUMNS: &str = "fill_key, client_order_id, exchange_order_id, exchange_fill_id, \
    account_id, exchange, symbol, side, quantity, price, fee, filled_at";

impl Database {
    /// Records a fill. Returns `true` when inserted, `false` when the
    /// same fill was already recorded (idempotent re-read).
    pub fn record_fill(&self, fill: &Fill) -> Result<bool, DatabaseError> {
        let key = fill_key(fill);
        let mut stmt = self
            .conn()
            .prepare("SELECT 1 FROM fills WHERE fill_key = ?1")
            .map_err(DatabaseError::StoreFailed)?;
        let exists = stmt.exists([&key]).map_err(DatabaseError::StoreFailed)?;
        if exists {
            return Ok(false);
        }
        self.conn()
            .execute(
                &format!("INSERT INTO fills ({FILL_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"),
                rusqlite::params![
                    key,
                    fill.client_order_id,
                    fill.exchange_order_id,
                    fill.exchange_fill_id,
                    fill.account_id,
                    fill.exchange,
                    fill.symbol,
                    match fill.side {
                        Side::Buy => "buy",
                        Side::Sell => "sell",
                    },
                    fill.quantity.to_string(),
                    fill.price.to_string(),
                    fill.fee.to_string(),
                    fill.filled_at,
                ],
            )
            .map_err(DatabaseError::StoreFailed)?;
        Ok(true)
    }

    pub fn list_fills(&self, client_order_id: &str) -> Result<Vec<Fill>, DatabaseError> {
        let sql = format!(
            "SELECT {FILL_COLUMNS} FROM fills WHERE client_order_id = ?1 ORDER BY filled_at ASC"
        );
        let mut stmt = self
            .conn()
            .prepare(&sql)
            .map_err(DatabaseError::StoreFailed)?;
        let mut rows = stmt
            .query([client_order_id])
            .map_err(DatabaseError::StoreFailed)?;
        let mut fills = Vec::new();
        while let Some(row) = rows.next().map_err(DatabaseError::StoreFailed)? {
            fills.push(row_to_fill(row)?);
        }
        Ok(fills)
    }

    pub fn list_fills_since(
        &self,
        account_id: &str,
        since: crate::models::TimestampMs,
    ) -> Result<Vec<Fill>, DatabaseError> {
        let sql = format!(
            "SELECT {FILL_COLUMNS} FROM fills WHERE account_id = ?1 AND filled_at >= ?2 \
             ORDER BY filled_at ASC"
        );
        let mut stmt = self
            .conn()
            .prepare(&sql)
            .map_err(DatabaseError::StoreFailed)?;
        let mut rows = stmt
            .query(rusqlite::params![account_id, since])
            .map_err(DatabaseError::StoreFailed)?;
        let mut fills = Vec::new();
        while let Some(row) = rows.next().map_err(DatabaseError::StoreFailed)? {
            fills.push(row_to_fill(row)?);
        }
        Ok(fills)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::database::initialise;
    use rust_decimal_macros::dec;

    fn sample_fill() -> Fill {
        Fill {
            client_order_id: "0xabc".into(),
            exchange_order_id: Some("42".into()),
            exchange_fill_id: Some("fill-1".into()),
            account_id: "acc-1".into(),
            exchange: "hyperliquid".into(),
            symbol: "BTC".into(),
            side: Side::Buy,
            quantity: dec!(0.25),
            price: dec!(60000),
            fee: dec!(0.9),
            filled_at: 5_000,
        }
    }

    #[test]
    fn fills_round_trip_and_deduplicate() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = initialise(&tmp.path().join("dexo.db")).expect("db");
        assert!(db.record_fill(&sample_fill()).expect("first insert"));
        assert!(!db.record_fill(&sample_fill()).expect("duplicate"));
        let fills = db.list_fills("0xabc").expect("list");
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].quantity, dec!(0.25));
        assert_eq!(fills[0].fee, dec!(0.9));
        assert_eq!(db.list_fills_since("acc-1", 6_000).expect("since").len(), 0);
        assert_eq!(db.list_fills_since("acc-1", 5_000).expect("since").len(), 1);
    }
}
