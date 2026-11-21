//! Startup reconciliation: compare the local record with the exchange
//! (the authoritative source) and record every difference as an
//! anomaly for review.
//!
//! What it does, per account:
//! - Every locally open order is queried on the exchange by
//!   client_order_id. A vanished order is an anomaly (never silently
//!   dropped locally). A differing order converges the local record —
//!   but only through legal state-machine transitions; anything else is
//!   an anomaly for the recovery flow, not an overwrite.
//! - Exchange positions are compared with the latest local snapshots;
//!   mismatches and unrecorded positions are anomalies, and the remote
//!   position is snapshotted locally to converge the view.
//! - A local non-zero snapshot with no remote counterpart is an
//!   anomaly.
//!
//! Any exchange error other than "order not found" aborts the run with
//! an error: a half-finished reconciliation must be loud, not partial
//! and silent.

use rust_decimal::Decimal;

use crate::exchanges::hyperliquid::ORDER_NOT_FOUND_CODE;
use crate::exchanges::traits::ExchangeAdapter;
use crate::models::{ExchangeError, TimestampMs};

use super::database::{Database, DatabaseError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnomalyKind {
    /// Local open order does not exist on the exchange.
    LocalOrderMissingRemotely,
    /// Local and remote order state differ (status or filled quantity).
    OrderStateMismatch,
    /// Position size differs, or a recorded position vanished remotely.
    PositionMismatch,
    /// The exchange holds a position we have no local snapshot of.
    PositionNotRecorded,
}

impl AnomalyKind {
    pub fn code(&self) -> &'static str {
        match self {
            Self::LocalOrderMissingRemotely => "local_order_missing_remotely",
            Self::OrderStateMismatch => "order_state_mismatch",
            Self::PositionMismatch => "position_mismatch",
            Self::PositionNotRecorded => "position_not_recorded",
        }
    }

    pub fn parse(code: &str) -> Result<Self, DatabaseError> {
        Ok(match code {
            "local_order_missing_remotely" => Self::LocalOrderMissingRemotely,
            "order_state_mismatch" => Self::OrderStateMismatch,
            "position_mismatch" => Self::PositionMismatch,
            "position_not_recorded" => Self::PositionNotRecorded,
            other => {
                return Err(DatabaseError::CorruptRecord(format!(
                    "unknown anomaly kind '{other}'"
                )))
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct Anomaly {
    pub kind: AnomalyKind,
    pub account_id: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct StoredAnomaly {
    pub id: i64,
    pub anomaly: Anomaly,
    pub detected_at: TimestampMs,
    pub resolved: bool,
}

#[derive(Debug)]
pub struct ReconciliationReport {
    pub account_id: String,
    pub checked_at: TimestampMs,
    pub anomalies: Vec<Anomaly>,
}

#[derive(Debug)]
pub enum ReconcileError {
    Database(DatabaseError),
    Exchange(ExchangeError),
}

impl std::fmt::Display for ReconcileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(e) => write!(f, "reconciliation database error: {e}"),
            Self::Exchange(e) => write!(f, "reconciliation exchange error: {e}"),
        }
    }
}

impl std::error::Error for ReconcileError {}

impl From<DatabaseError> for ReconcileError {
    fn from(e: DatabaseError) -> Self {
        Self::Database(e)
    }
}

impl From<ExchangeError> for ReconcileError {
    fn from(e: ExchangeError) -> Self {
        Self::Exchange(e)
    }
}

impl Database {
    pub fn record_anomaly(
        &self,
        anomaly: &Anomaly,
        detected_at: TimestampMs,
    ) -> Result<(), DatabaseError> {
        self.conn()
            .execute(
                "INSERT INTO anomalies (kind, account_id, detail, detected_at) \
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    anomaly.kind.code(),
                    anomaly.account_id,
                    anomaly.detail,
                    detected_at,
                ],
            )
            .map_err(DatabaseError::StoreFailed)?;
        Ok(())
    }

    pub fn list_anomalies(&self, account_id: &str) -> Result<Vec<StoredAnomaly>, DatabaseError> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT id, kind, detail, detected_at, resolved FROM anomalies \
                 WHERE account_id = ?1 ORDER BY id ASC",
            )
            .map_err(DatabaseError::StoreFailed)?;
        let mut rows = stmt
            .query([account_id])
            .map_err(DatabaseError::StoreFailed)?;
        let mut stored = Vec::new();
        while let Some(row) = rows.next().map_err(DatabaseError::StoreFailed)? {
            let kind_raw: String = row.get(1).map_err(DatabaseError::StoreFailed)?;
            stored.push(StoredAnomaly {
                id: row.get(0).map_err(DatabaseError::StoreFailed)?,
                anomaly: Anomaly {
                    kind: AnomalyKind::parse(&kind_raw)?,
                    account_id: account_id.to_string(),
                    detail: row.get(2).map_err(DatabaseError::StoreFailed)?,
                },
                detected_at: row.get(3).map_err(DatabaseError::StoreFailed)?,
                resolved: row.get::<_, i64>(4).map_err(DatabaseError::StoreFailed)? != 0,
            });
        }
        Ok(stored)
    }
}

fn is_not_found(error: &ExchangeError) -> bool {
    matches!(error, ExchangeError::Remote { code: Some(code), .. } if code == ORDER_NOT_FOUND_CODE)
}

/// Runs one reconciliation pass for the adapter's account and persists
/// every anomaly it finds. See the module docs for the exact rules.
pub async fn reconcile_account(
    db: &Database,
    adapter: &dyn ExchangeAdapter,
    now: TimestampMs,
) -> Result<ReconciliationReport, ReconcileError> {
    let account_id = adapter.account_id().to_string();
    let mut anomalies: Vec<Anomaly> = Vec::new();
    let mut flag = |kind: AnomalyKind, detail: String| {
        anomalies.push(Anomaly {
            kind,
            account_id: account_id.clone(),
            detail,
        });
    };

    // 1) Local open orders vs the exchange.
    for local in db.list_orders(&account_id, true)? {
        match adapter.get_order(&local.client_order_id).await {
            Ok(remote) => {
                if remote.status != local.status || remote.filled_quantity != local.filled_quantity
                {
                    flag(
                        AnomalyKind::OrderStateMismatch,
                        format!(
                            "order {}: local status/filled {}/{}, remote {}/{}",
                            local.client_order_id,
                            crate::trading::orders::status_code(local.status),
                            local.filled_quantity,
                            crate::trading::orders::status_code(remote.status),
                            remote.filled_quantity
                        ),
                    );
                    // Converge to the authoritative record when the
                    // state machine allows it; a rejected transition is
                    // left for the recovery flow, not forced.
                    let _ = db.upsert_order(&remote);
                }
            }
            Err(e) if is_not_found(&e) => {
                flag(
                    AnomalyKind::LocalOrderMissingRemotely,
                    format!(
                        "order {} is open locally but unknown to the exchange",
                        local.client_order_id
                    ),
                );
            }
            Err(e) => return Err(ReconcileError::Exchange(e)),
        }
    }

    // 2) Exchange positions vs latest local snapshots.
    let remote_positions = adapter.get_positions().await?;
    let local_positions = db.latest_positions(&account_id)?;
    for remote in &remote_positions {
        match local_positions
            .iter()
            .find(|p| p.exchange == remote.exchange && p.symbol == remote.symbol)
        {
            None => {
                flag(
                    AnomalyKind::PositionNotRecorded,
                    format!(
                        "{} {}: remote quantity {} has no local snapshot",
                        remote.exchange, remote.symbol, remote.quantity
                    ),
                );
                db.record_position_snapshot(remote)?;
            }
            Some(local) if local.quantity != remote.quantity => {
                flag(
                    AnomalyKind::PositionMismatch,
                    format!(
                        "{} {}: local quantity {}, remote {}",
                        remote.exchange, remote.symbol, local.quantity, remote.quantity
                    ),
                );
                db.record_position_snapshot(remote)?;
            }
            Some(_) => {}
        }
    }
    for local in &local_positions {
        if local.quantity != Decimal::ZERO
            && !remote_positions
                .iter()
                .any(|p| p.exchange == local.exchange && p.symbol == local.symbol)
        {
            flag(
                AnomalyKind::PositionMismatch,
                format!(
                    "{} {}: local quantity {} but no remote position",
                    local.exchange, local.symbol, local.quantity
                ),
            );
        }
    }

    for anomaly in &anomalies {
        db.record_anomaly(anomaly, now)?;
    }
    Ok(ReconciliationReport {
        account_id,
        checked_at: now,
        anomalies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        Balance, Fill, FundingInfo, Order, OrderBook, OrderRequest, OrderStatus, OrderType,
        Position, Side,
    };
    use crate::trading::database::initialise;
    use async_trait::async_trait;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    struct MockAdapter {
        orders: HashMap<String, Order>,
        positions: Vec<Position>,
    }

    fn order(cloid: &str, status: OrderStatus, filled: Decimal) -> Order {
        Order {
            client_order_id: cloid.into(),
            exchange_order_id: Some("7".into()),
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
            filled_quantity: filled,
            average_fill_price: None,
            created_at: 1_000,
            updated_at: 1_000,
        }
    }

    #[async_trait]
    impl ExchangeAdapter for MockAdapter {
        fn name(&self) -> &str {
            "hyperliquid"
        }

        fn account_id(&self) -> &str {
            "acc-1"
        }

        async fn get_funding(&self, _: &str) -> Result<FundingInfo, ExchangeError> {
            Err(ExchangeError::Unsupported("n/a".into()))
        }

        async fn get_orderbook(&self, _: &str) -> Result<OrderBook, ExchangeError> {
            Err(ExchangeError::Unsupported("n/a".into()))
        }

        async fn get_positions(&self) -> Result<Vec<Position>, ExchangeError> {
            Ok(self.positions.clone())
        }

        async fn get_balance(&self) -> Result<Balance, ExchangeError> {
            Err(ExchangeError::Unsupported("n/a".into()))
        }

        async fn place_order(&self, _: &OrderRequest) -> Result<Order, ExchangeError> {
            Err(ExchangeError::Unsupported("n/a".into()))
        }

        async fn get_order(&self, cloid: &str) -> Result<Order, ExchangeError> {
            self.orders
                .get(cloid)
                .cloned()
                .ok_or_else(|| ExchangeError::Remote {
                    code: Some(ORDER_NOT_FOUND_CODE.into()),
                    message: "unknown order".into(),
                })
        }

        async fn cancel_order(&self, _: &str) -> Result<Order, ExchangeError> {
            Err(ExchangeError::Unsupported("n/a".into()))
        }

        async fn get_fills(&self, _: Option<TimestampMs>) -> Result<Vec<Fill>, ExchangeError> {
            Ok(Vec::new())
        }
    }

    fn position(symbol: &str, quantity: Decimal) -> Position {
        Position {
            account_id: "acc-1".into(),
            exchange: "hyperliquid".into(),
            symbol: symbol.into(),
            quantity,
            entry_price: None,
            leverage: None,
            unrealized_pnl: None,
            updated_at: 2_000,
        }
    }

    #[tokio::test]
    async fn matching_state_produces_no_anomalies() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = initialise(&tmp.path().join("dexo.db")).expect("db");
        db.upsert_order(&order("0x1", OrderStatus::Open, Decimal::ZERO))
            .expect("seed order");
        db.record_position_snapshot(&position("BTC", dec!(0.5)))
            .expect("seed position");
        let adapter = MockAdapter {
            orders: HashMap::from([("0x1".into(), order("0x1", OrderStatus::Open, Decimal::ZERO))]),
            positions: vec![position("BTC", dec!(0.5))],
        };
        let report = reconcile_account(&db, &adapter, 3_000)
            .await
            .expect("reconcile");
        assert!(report.anomalies.is_empty());
        assert!(db.list_anomalies("acc-1").expect("anomalies").is_empty());
    }

    #[tokio::test]
    async fn vanished_order_and_position_drift_are_flagged_and_persisted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = initialise(&tmp.path().join("dexo.db")).expect("db");
        db.upsert_order(&order("0xgone", OrderStatus::Open, Decimal::ZERO))
            .expect("seed order");
        db.record_position_snapshot(&position("BTC", dec!(0.5)))
            .expect("seed position");
        let adapter = MockAdapter {
            orders: HashMap::new(),
            positions: vec![position("BTC", dec!(0.2)), position("ETH", dec!(3))],
        };
        let report = reconcile_account(&db, &adapter, 3_000)
            .await
            .expect("reconcile");
        let kinds: Vec<AnomalyKind> = report.anomalies.iter().map(|a| a.kind.clone()).collect();
        assert!(kinds.contains(&AnomalyKind::LocalOrderMissingRemotely));
        assert!(kinds.contains(&AnomalyKind::PositionMismatch));
        assert!(kinds.contains(&AnomalyKind::PositionNotRecorded));
        // Persisted for later review.
        assert_eq!(
            db.list_anomalies("acc-1").expect("anomalies").len(),
            report.anomalies.len()
        );
        // Local position view converged to the remote snapshot.
        let latest = db.latest_positions("acc-1").expect("latest");
        let btc = latest.iter().find(|p| p.symbol == "BTC").expect("btc");
        assert_eq!(btc.quantity, dec!(0.2));
    }
}
