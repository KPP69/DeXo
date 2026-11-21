//! Startup recovery (Phase 7): before any new opening is allowed,
//! every locally unfinished order is queried on the exchange and the
//! local record is converged to the authoritative state.
//!
//! - Found remotely → converge (counted as resolved when the order
//!   reaches a terminal state, still open otherwise).
//! - Missing remotely → an anomaly is recorded; an `Unknown` order is
//!   moved to `Reconciling` to mark it as under review. Nothing is
//!   resubmitted and nothing is force-closed from here.
//! - Any other exchange error aborts recovery loudly: starting to
//!   trade on a half-recovered account is worse than not starting.

use std::sync::{Arc, Mutex};

use crate::exchanges::traits::ExchangeAdapter;
use crate::models::{OrderStatus, TimestampMs};
use crate::trading::database::Database;
use crate::trading::reconciliation::{Anomaly, AnomalyKind};

use super::order_manager::{apply_remote_order, is_not_found, ExecutionError};

#[derive(Debug, Default)]
pub struct RecoveryReport {
    pub account_id: String,
    pub checked: usize,
    /// Orders whose local record changed to match the exchange.
    pub converged: usize,
    /// Orders confirmed still open on the exchange.
    pub still_open: usize,
    /// Orders that reached a terminal state during recovery.
    pub resolved_terminal: usize,
    /// Locally open orders the exchange does not know.
    pub missing_remotely: usize,
}

pub async fn recover_account(
    db: &Arc<Mutex<Database>>,
    adapter: &Arc<dyn ExchangeAdapter>,
    now: TimestampMs,
) -> Result<RecoveryReport, ExecutionError> {
    let account_id = adapter.account_id().to_string();
    let open_orders = {
        let db = db.lock().expect("database mutex poisoned");
        db.list_orders(&account_id, true)?
    };
    let mut report = RecoveryReport {
        account_id: account_id.clone(),
        checked: open_orders.len(),
        ..Default::default()
    };
    for local in open_orders {
        match adapter.get_order(&local.client_order_id).await {
            Ok(remote) => {
                let db = db.lock().expect("database mutex poisoned");
                let after = apply_remote_order(&db, &remote)?;
                if after.status != local.status || after.filled_quantity != local.filled_quantity {
                    report.converged += 1;
                }
                if after.status.is_terminal() {
                    report.resolved_terminal += 1;
                } else {
                    report.still_open += 1;
                }
            }
            Err(e) if is_not_found(&e) => {
                report.missing_remotely += 1;
                let db = db.lock().expect("database mutex poisoned");
                db.record_anomaly(
                    &Anomaly {
                        kind: AnomalyKind::LocalOrderMissingRemotely,
                        account_id: account_id.clone(),
                        detail: format!(
                            "recovery: order {} ({}) is open locally but unknown to the exchange",
                            local.client_order_id,
                            crate::trading::orders::status_code(local.status)
                        ),
                    },
                    now,
                )?;
                if local.status == OrderStatus::Unknown {
                    let mut reviewing = local.clone();
                    reviewing.status = OrderStatus::Reconciling;
                    reviewing.updated_at = now;
                    db.upsert_order(&reviewing)?;
                }
            }
            Err(e) => return Err(ExecutionError::Exchange(e)),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        Balance, Fill, FundingInfo, Order, OrderBook, OrderRequest, OrderType, Position, Side,
    };
    use crate::trading::database::initialise;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    struct VenueAdapter {
        orders: HashMap<String, Order>,
    }

    fn order(cloid: &str, status: OrderStatus) -> Order {
        Order {
            client_order_id: cloid.into(),
            exchange_order_id: Some("7".into()),
            account_id: "acc-1".into(),
            strategy_id: String::new(),
            exchange: "mockex".into(),
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

    #[async_trait]
    impl ExchangeAdapter for VenueAdapter {
        fn name(&self) -> &str {
            "mockex"
        }

        fn account_id(&self) -> &str {
            "acc-1"
        }

        async fn get_funding(&self, _: &str) -> Result<FundingInfo, crate::models::ExchangeError> {
            Err(crate::models::ExchangeError::Unsupported("n/a".into()))
        }

        async fn get_orderbook(&self, _: &str) -> Result<OrderBook, crate::models::ExchangeError> {
            Err(crate::models::ExchangeError::Unsupported("n/a".into()))
        }

        async fn get_positions(&self) -> Result<Vec<Position>, crate::models::ExchangeError> {
            Ok(Vec::new())
        }

        async fn get_balance(&self) -> Result<Balance, crate::models::ExchangeError> {
            Err(crate::models::ExchangeError::Unsupported("n/a".into()))
        }

        async fn place_order(
            &self,
            _: &OrderRequest,
        ) -> Result<Order, crate::models::ExchangeError> {
            Err(crate::models::ExchangeError::Unsupported("n/a".into()))
        }

        async fn get_order(&self, cloid: &str) -> Result<Order, crate::models::ExchangeError> {
            self.orders
                .get(cloid)
                .cloned()
                .ok_or_else(|| crate::models::ExchangeError::Remote {
                    code: Some(crate::exchanges::hyperliquid::ORDER_NOT_FOUND_CODE.into()),
                    message: "unknown order".into(),
                })
        }

        async fn cancel_order(&self, _: &str) -> Result<Order, crate::models::ExchangeError> {
            Err(crate::models::ExchangeError::Unsupported("n/a".into()))
        }

        async fn get_fills(
            &self,
            _: Option<TimestampMs>,
        ) -> Result<Vec<Fill>, crate::models::ExchangeError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn recovery_converges_filled_and_flags_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = initialise(&tmp.path().join("dexo.db")).expect("db");
        // Local view before the "restart": one Unknown, one Open that
        // has since filled, one Open that vanished.
        db.upsert_order(&order("0xunknown", OrderStatus::Unknown))
            .expect("seed unknown");
        db.upsert_order(&order("0xfilled", OrderStatus::Open))
            .expect("seed open");
        db.upsert_order(&order("0xgone", OrderStatus::Open))
            .expect("seed gone");
        let db = Arc::new(Mutex::new(db));
        let venue: Arc<dyn ExchangeAdapter> = Arc::new(VenueAdapter {
            orders: HashMap::from([
                ("0xunknown".into(), order("0xunknown", OrderStatus::Open)),
                ("0xfilled".into(), order("0xfilled", OrderStatus::Filled)),
            ]),
        });
        let report = recover_account(&db, &venue, 5_000).await.expect("recover");
        assert_eq!(report.checked, 3);
        assert_eq!(report.missing_remotely, 1);
        assert_eq!(report.resolved_terminal, 1);
        let db = db.lock().expect("db");
        let filled = db.get_order("0xfilled").expect("read").expect("present");
        assert_eq!(filled.status, OrderStatus::Filled);
        let anomalies = db.list_anomalies("acc-1").expect("anomalies");
        assert_eq!(anomalies.len(), 1);
        assert_eq!(
            anomalies[0].anomaly.kind,
            AnomalyKind::LocalOrderMissingRemotely
        );
    }
}
