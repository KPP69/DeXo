//! Local position records: append-only snapshots, never in-place
//! overwrites. The latest snapshot per (exchange, symbol) is the local
//! view; the exchange remains authoritative and reconciliation compares
//! against it (see `reconciliation`).

use std::str::FromStr;

use rust_decimal::Decimal;

use crate::models::Position;

use super::database::{Database, DatabaseError};

fn parse_opt(raw: Option<String>, field: &str) -> Result<Option<Decimal>, DatabaseError> {
    match raw {
        None => Ok(None),
        Some(text) => Decimal::from_str(&text).map(Some).map_err(|_| {
            DatabaseError::CorruptRecord(format!("unparseable decimal in {field}: '{text}'"))
        }),
    }
}

impl Database {
    pub fn record_position_snapshot(&self, position: &Position) -> Result<(), DatabaseError> {
        self.conn()
            .execute(
                "INSERT INTO position_snapshots (account_id, exchange, symbol, quantity, \
                 entry_price, leverage, unrealized_pnl, observed_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    position.account_id,
                    position.exchange,
                    position.symbol,
                    position.quantity.to_string(),
                    position.entry_price.map(|d| d.to_string()),
                    position.leverage.map(|d| d.to_string()),
                    position.unrealized_pnl.map(|d| d.to_string()),
                    position.updated_at,
                ],
            )
            .map_err(DatabaseError::StoreFailed)?;
        Ok(())
    }

    /// The newest snapshot per (exchange, symbol) for one account.
    pub fn latest_positions(&self, account_id: &str) -> Result<Vec<Position>, DatabaseError> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT account_id, exchange, symbol, quantity, entry_price, leverage, \
                 unrealized_pnl, observed_at FROM position_snapshots \
                 WHERE account_id = ?1 AND id IN ( \
                     SELECT MAX(id) FROM position_snapshots \
                     WHERE account_id = ?1 GROUP BY exchange, symbol \
                 ) ORDER BY exchange, symbol",
            )
            .map_err(DatabaseError::StoreFailed)?;
        let mut rows = stmt
            .query([account_id])
            .map_err(DatabaseError::StoreFailed)?;
        let mut positions = Vec::new();
        while let Some(row) = rows.next().map_err(DatabaseError::StoreFailed)? {
            let qty_raw: String = row.get(3).map_err(DatabaseError::StoreFailed)?;
            positions.push(Position {
                account_id: row.get(0).map_err(DatabaseError::StoreFailed)?,
                exchange: row.get(1).map_err(DatabaseError::StoreFailed)?,
                symbol: row.get(2).map_err(DatabaseError::StoreFailed)?,
                quantity: Decimal::from_str(&qty_raw).map_err(|_| {
                    DatabaseError::CorruptRecord(format!(
                        "unparseable decimal in position_snapshots.quantity: '{qty_raw}'"
                    ))
                })?,
                entry_price: parse_opt(
                    row.get(4).map_err(DatabaseError::StoreFailed)?,
                    "position_snapshots.entry_price",
                )?,
                leverage: parse_opt(
                    row.get(5).map_err(DatabaseError::StoreFailed)?,
                    "position_snapshots.leverage",
                )?,
                unrealized_pnl: parse_opt(
                    row.get(6).map_err(DatabaseError::StoreFailed)?,
                    "position_snapshots.unrealized_pnl",
                )?,
                updated_at: row.get(7).map_err(DatabaseError::StoreFailed)?,
            });
        }
        Ok(positions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::database::initialise;
    use rust_decimal_macros::dec;

    fn position(symbol: &str, quantity: Decimal, at: i64) -> Position {
        Position {
            account_id: "acc-1".into(),
            exchange: "hyperliquid".into(),
            symbol: symbol.into(),
            quantity,
            entry_price: Some(dec!(60000)),
            leverage: Some(dec!(5)),
            unrealized_pnl: None,
            updated_at: at,
        }
    }

    #[test]
    fn latest_snapshot_wins_and_history_is_kept() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = initialise(&tmp.path().join("dexo.db")).expect("db");
        db.record_position_snapshot(&position("BTC", dec!(0.5), 1_000))
            .expect("snap 1");
        db.record_position_snapshot(&position("BTC", dec!(0.3), 2_000))
            .expect("snap 2");
        db.record_position_snapshot(&position("ETH", dec!(-2), 2_000))
            .expect("snap 3");
        let latest = db.latest_positions("acc-1").expect("latest");
        assert_eq!(latest.len(), 2);
        let btc = latest.iter().find(|p| p.symbol == "BTC").expect("btc");
        assert_eq!(btc.quantity, dec!(0.3));
        assert_eq!(btc.updated_at, 2_000);
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM position_snapshots", [], |r| r.get(0))
            .expect("count");
        assert_eq!(count, 3, "snapshots are append-only history");
    }
}
