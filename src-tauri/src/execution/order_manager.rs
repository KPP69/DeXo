//! Order lifecycle management (Phase 7, single leg).
//!
//! The one rule everything here serves: **an ambiguous outcome is
//! resolved by querying the exchange with the client_order_id, never
//! by resubmitting.** A submission whose result is unknown (timeout,
//! network failure, rate limiting, unclassifiable error) moves the
//! local order to `Unknown` and triggers a query; only a definite
//! venue rejection moves it to `Failed`.
//!
//! Concurrency: submissions and cancels for the same
//! (account, symbol) pair are serialised through a per-pair async
//! lock; different pairs proceed in parallel. The lock map itself is
//! only held long enough to clone the pair's lock. Database access
//! goes through a mutex that is never held across an await.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex as AsyncMutex;

use crate::exchanges::hyperliquid::ORDER_NOT_FOUND_CODE;
use crate::exchanges::traits::ExchangeAdapter;
use crate::models::{
    ExchangeError, Order, OrderRequest, OrderStatus, RiskContext, RiskLayer, RiskResult,
    TimestampMs,
};
use crate::risk::RiskCheck;
use crate::trading::database::{Database, DatabaseError};

#[derive(Debug)]
pub enum ExecutionError {
    Database(DatabaseError),
    Exchange(ExchangeError),
    RiskRejected { layer: RiskLayer, reason: String },
    OrderNotFound(String),
    AdapterMissing(String),
    InvalidState(String),
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(e) => write!(f, "execution database error: {e}"),
            Self::Exchange(e) => write!(f, "execution exchange error: {e}"),
            Self::RiskRejected { layer, reason } => {
                write!(f, "risk rejected at {layer:?}: {reason}")
            }
            Self::OrderNotFound(id) => write!(f, "order not found locally: {id}"),
            Self::AdapterMissing(id) => write!(f, "no adapter for account: {id}"),
            Self::InvalidState(m) => write!(f, "invalid order state handling: {m}"),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<DatabaseError> for ExecutionError {
    fn from(e: DatabaseError) -> Self {
        Self::Database(e)
    }
}

impl From<ExchangeError> for ExecutionError {
    fn from(e: ExchangeError) -> Self {
        Self::Exchange(e)
    }
}

fn system_now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as TimestampMs)
        .unwrap_or(0)
}

pub fn is_not_found(error: &ExchangeError) -> bool {
    matches!(error, ExchangeError::Remote { code: Some(code), .. } if code == ORDER_NOT_FOUND_CODE)
}

/// Converges the local record toward the exchange's authoritative
/// order. Legal transitions are applied directly; a remote `Cancelled`
/// behind a local `Open`/`PartiallyFilled` goes through the required
/// `CancelPending` hop. Anything else is refused and the local record
/// is left untouched (reconciliation/recovery will flag it).
pub fn apply_remote_order(db: &Database, remote: &Order) -> Result<Order, ExecutionError> {
    match db.get_order(&remote.client_order_id)? {
        None => {
            db.upsert_order(remote)?;
        }
        Some(local) => {
            if local.status == remote.status || local.status.can_transition_to(remote.status) {
                db.upsert_order(remote)?;
            } else if remote.status == OrderStatus::Cancelled
                && local.status.can_transition_to(OrderStatus::CancelPending)
            {
                let mut hop = local.clone();
                hop.status = OrderStatus::CancelPending;
                hop.updated_at = remote.updated_at;
                db.upsert_order(&hop)?;
                db.upsert_order(remote)?;
            } else {
                return Err(ExecutionError::InvalidState(format!(
                    "order {}: local {} cannot converge to remote {}",
                    remote.client_order_id,
                    crate::trading::orders::status_code(local.status),
                    crate::trading::orders::status_code(remote.status)
                )));
            }
        }
    }
    db.get_order(&remote.client_order_id)?
        .ok_or_else(|| ExecutionError::OrderNotFound(remote.client_order_id.clone()))
}

type PairLockMap = Mutex<HashMap<(String, String), Arc<AsyncMutex<()>>>>;

pub struct OrderManager {
    db: Arc<Mutex<Database>>,
    adapters: HashMap<String, Arc<dyn ExchangeAdapter>>,
    risk: Arc<dyn RiskCheck>,
    pair_locks: PairLockMap,
    kill_switch: AtomicBool,
    submit_timeout: std::time::Duration,
    clock: Arc<dyn Fn() -> TimestampMs + Send + Sync>,
}

impl OrderManager {
    pub fn new(
        db: Arc<Mutex<Database>>,
        adapters: Vec<Arc<dyn ExchangeAdapter>>,
        risk: Arc<dyn RiskCheck>,
    ) -> Self {
        Self {
            db,
            adapters: adapters
                .into_iter()
                .map(|a| (a.account_id().to_string(), a))
                .collect(),
            risk,
            pair_locks: Mutex::new(HashMap::new()),
            kill_switch: AtomicBool::new(false),
            submit_timeout: std::time::Duration::from_secs(10),
            clock: Arc::new(system_now_ms),
        }
    }

    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.submit_timeout = timeout;
        self
    }

    /// Driven by the risk center (Phase 8). While active, the risk
    /// context reports it and the global layer rejects opening orders.
    pub fn set_kill_switch(&self, active: bool) {
        self.kill_switch.store(active, Ordering::SeqCst);
    }

    pub fn kill_switch_active(&self) -> bool {
        self.kill_switch.load(Ordering::SeqCst)
    }

    pub fn database(&self) -> Arc<Mutex<Database>> {
        self.db.clone()
    }

    pub fn adapter(&self, account_id: &str) -> Result<Arc<dyn ExchangeAdapter>, ExecutionError> {
        self.adapters
            .get(account_id)
            .cloned()
            .ok_or_else(|| ExecutionError::AdapterMissing(account_id.to_string()))
    }

    fn now(&self) -> TimestampMs {
        (self.clock)()
    }

    fn pair_lock(&self, account_id: &str, symbol: &str) -> Arc<AsyncMutex<()>> {
        let key = (account_id.to_string(), symbol.to_string());
        let mut locks = self.pair_locks.lock().expect("pair lock map poisoned");
        locks
            .entry(key)
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    fn with_db<T>(
        &self,
        f: impl FnOnce(&Database) -> Result<T, ExecutionError>,
    ) -> Result<T, ExecutionError> {
        let db = self.db.lock().expect("database mutex poisoned");
        f(&db)
    }

    fn transition(&self, order: &Order, status: OrderStatus) -> Result<Order, ExecutionError> {
        let mut next = order.clone();
        next.status = status;
        next.updated_at = self.now();
        self.with_db(|db| {
            db.upsert_order(&next)?;
            Ok(next.clone())
        })
    }

    /// Submits one order through the full lifecycle: persist, risk
    /// check, submit, and — on any ambiguity — query by
    /// client_order_id instead of resubmitting. Re-submitting an
    /// already-known client_order_id returns the stored order
    /// (idempotency: a duplicate request never reaches the venue
    /// twice).
    pub async fn submit(&self, request: &OrderRequest) -> Result<Order, ExecutionError> {
        let adapter = self.adapter(&request.account_id)?;
        let lock = self.pair_lock(&request.account_id, &request.symbol);
        let _guard = lock.lock().await;

        if let Some(existing) = self.with_db(|db| Ok(db.get_order(&request.client_order_id)?))? {
            return Ok(existing);
        }

        let mut order = Order::from_request(request, self.now());
        self.with_db(|db| db.upsert_order(&order).map_err(ExecutionError::from))?;

        let context = RiskContext {
            account_id: request.account_id.clone(),
            strategy_id: request.strategy_id.clone(),
            kill_switch_active: self.kill_switch_active(),
            evaluated_at: self.now(),
        };
        match self.risk.check_order(&context, request) {
            RiskResult::Approved => {}
            RiskResult::Rejected { layer, reason } => {
                let _ = self.transition(&order, OrderStatus::Failed);
                return Err(ExecutionError::RiskRejected { layer, reason });
            }
        }

        order = self.transition(&order, OrderStatus::RiskChecked)?;
        order = self.transition(&order, OrderStatus::Submitting)?;

        let placed = tokio::time::timeout(self.submit_timeout, adapter.place_order(request)).await;
        match placed {
            Ok(Ok(remote)) => self.with_db(|db| apply_remote_order(db, &remote)),
            Ok(Err(e)) if e.outcome_is_ambiguous() || matches!(e, ExchangeError::RateLimited) => {
                order = self.transition(&order, OrderStatus::Unknown)?;
                self.resolve_via_query(&adapter, order).await
            }
            Ok(Err(e)) => {
                let _ = self.transition(&order, OrderStatus::Failed);
                Err(ExecutionError::Exchange(e))
            }
            Err(_elapsed) => {
                order = self.transition(&order, OrderStatus::Unknown)?;
                self.resolve_via_query(&adapter, order).await
            }
        }
    }

    /// After an ambiguous submission: ask the venue what actually
    /// happened. Found → converge and return. Not found → the order
    /// stays `Unknown` locally and is returned as such (recovery will
    /// keep checking); it is never resubmitted from here.
    async fn resolve_via_query(
        &self,
        adapter: &Arc<dyn ExchangeAdapter>,
        order: Order,
    ) -> Result<Order, ExecutionError> {
        match adapter.get_order(&order.client_order_id).await {
            Ok(remote) => self.with_db(|db| apply_remote_order(db, &remote)),
            Err(e) if is_not_found(&e) => Ok(order),
            Err(e) => Err(ExecutionError::Exchange(e)),
        }
    }

    /// Cancels one locally known order. Timeouts move the order to
    /// `CancelPending` and resolve by query, exactly like submissions.
    pub async fn cancel(
        &self,
        account_id: &str,
        client_order_id: &str,
    ) -> Result<Order, ExecutionError> {
        let local = self
            .with_db(|db| Ok(db.get_order(client_order_id)?))?
            .ok_or_else(|| ExecutionError::OrderNotFound(client_order_id.to_string()))?;
        if local.status.is_terminal() {
            return Ok(local);
        }
        let adapter = self.adapter(account_id)?;
        let lock = self.pair_lock(account_id, &local.symbol);
        let _guard = lock.lock().await;

        let cancelled =
            tokio::time::timeout(self.submit_timeout, adapter.cancel_order(client_order_id)).await;
        match cancelled {
            Ok(Ok(remote)) => self.with_db(|db| apply_remote_order(db, &remote)),
            Ok(Err(e)) if is_not_found(&e) => Err(ExecutionError::Exchange(e)),
            Ok(Err(_)) | Err(_) => {
                let pending = if local.status.can_transition_to(OrderStatus::CancelPending) {
                    self.transition(&local, OrderStatus::CancelPending)?
                } else {
                    local
                };
                match adapter.get_order(client_order_id).await {
                    Ok(remote) => self.with_db(|db| apply_remote_order(db, &remote)),
                    Err(_) => Ok(pending),
                }
            }
        }
    }

    /// Queries the venue for one order and converges the local record.
    pub async fn query(
        &self,
        account_id: &str,
        client_order_id: &str,
    ) -> Result<Order, ExecutionError> {
        let adapter = self.adapter(account_id)?;
        let remote = adapter.get_order(client_order_id).await?;
        self.with_db(|db| apply_remote_order(db, &remote))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        Balance, Fill, FundingInfo, OrderBook, OrderType, Position, RiskLayer, Side,
    };
    use crate::trading::database::initialise;
    use async_trait::async_trait;
    use rust_decimal_macros::dec;
    use std::sync::atomic::AtomicUsize;

    struct AllowAll;
    impl RiskCheck for AllowAll {
        fn check_order(&self, _: &RiskContext, _: &OrderRequest) -> RiskResult {
            RiskResult::Approved
        }
    }

    struct DenyAll;
    impl RiskCheck for DenyAll {
        fn check_order(&self, _: &RiskContext, _: &OrderRequest) -> RiskResult {
            RiskResult::rejected(RiskLayer::Global, "test denial")
        }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum PlaceScript {
        Rest,
        TimeoutThenRest,
        TimeoutThenMissing,
        DefiniteReject,
    }

    struct ScriptedAdapter {
        script: PlaceScript,
        place_calls: AtomicUsize,
        resting: Mutex<HashMap<String, Order>>,
    }

    impl ScriptedAdapter {
        fn new(script: PlaceScript) -> Self {
            Self {
                script,
                place_calls: AtomicUsize::new(0),
                resting: Mutex::new(HashMap::new()),
            }
        }

        fn venue_order(&self, request: &OrderRequest, status: OrderStatus) -> Order {
            let mut order = Order::from_request(request, 2_000);
            order.status = status;
            order.exchange_order_id = Some("99".into());
            order
        }
    }

    #[async_trait]
    impl ExchangeAdapter for ScriptedAdapter {
        fn name(&self) -> &str {
            "mockex"
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
            Ok(Vec::new())
        }

        async fn get_balance(&self) -> Result<Balance, ExchangeError> {
            Err(ExchangeError::Unsupported("n/a".into()))
        }

        async fn place_order(&self, request: &OrderRequest) -> Result<Order, ExchangeError> {
            self.place_calls.fetch_add(1, Ordering::SeqCst);
            match self.script {
                PlaceScript::Rest => {
                    let order = self.venue_order(request, OrderStatus::Open);
                    self.resting
                        .lock()
                        .expect("resting")
                        .insert(request.client_order_id.clone(), order.clone());
                    Ok(order)
                }
                PlaceScript::TimeoutThenRest => {
                    // The venue DID take the order; the reply was lost.
                    let order = self.venue_order(request, OrderStatus::Open);
                    self.resting
                        .lock()
                        .expect("resting")
                        .insert(request.client_order_id.clone(), order);
                    Err(ExchangeError::Timeout)
                }
                PlaceScript::TimeoutThenMissing => Err(ExchangeError::Timeout),
                PlaceScript::DefiniteReject => {
                    Err(ExchangeError::InvalidRequest("bad price".into()))
                }
            }
        }

        async fn get_order(&self, cloid: &str) -> Result<Order, ExchangeError> {
            self.resting
                .lock()
                .expect("resting")
                .get(cloid)
                .cloned()
                .ok_or_else(|| ExchangeError::Remote {
                    code: Some(ORDER_NOT_FOUND_CODE.into()),
                    message: "unknown order".into(),
                })
        }

        async fn cancel_order(&self, cloid: &str) -> Result<Order, ExchangeError> {
            let mut resting = self.resting.lock().expect("resting");
            match resting.get_mut(cloid) {
                Some(order) => {
                    order.status = OrderStatus::Cancelled;
                    Ok(order.clone())
                }
                None => Err(ExchangeError::Remote {
                    code: Some(ORDER_NOT_FOUND_CODE.into()),
                    message: "unknown order".into(),
                }),
            }
        }

        async fn get_fills(&self, _: Option<TimestampMs>) -> Result<Vec<Fill>, ExchangeError> {
            Ok(Vec::new())
        }
    }

    fn request(cloid: &str) -> OrderRequest {
        OrderRequest {
            account_id: "acc-1".into(),
            strategy_id: String::new(),
            exchange: "mockex".into(),
            symbol: "BTC".into(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: dec!(0.1),
            price: Some(dec!(60000)),
            reduce_only: false,
            client_order_id: cloid.into(),
            created_at: 1_000,
        }
    }

    fn manager(
        adapter: Arc<ScriptedAdapter>,
        risk: Arc<dyn RiskCheck>,
    ) -> (tempfile::TempDir, OrderManager) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = initialise(&tmp.path().join("dexo.db")).expect("db");
        let manager = OrderManager::new(
            Arc::new(Mutex::new(db)),
            vec![adapter as Arc<dyn ExchangeAdapter>],
            risk,
        );
        (tmp, manager)
    }

    #[tokio::test]
    async fn successful_submission_rests_open() {
        let adapter = Arc::new(ScriptedAdapter::new(PlaceScript::Rest));
        let (_tmp, manager) = manager(adapter, Arc::new(AllowAll));
        let order = manager.submit(&request("0x1")).await.expect("submit");
        assert_eq!(order.status, OrderStatus::Open);
        assert_eq!(order.exchange_order_id.as_deref(), Some("99"));
    }

    #[tokio::test]
    async fn timeout_is_resolved_by_query_never_by_resubmission() {
        let adapter = Arc::new(ScriptedAdapter::new(PlaceScript::TimeoutThenRest));
        let (_tmp, manager) = manager(adapter.clone(), Arc::new(AllowAll));
        let order = manager.submit(&request("0x2")).await.expect("submit");
        assert_eq!(order.status, OrderStatus::Open, "query found the order");
        assert_eq!(
            adapter.place_calls.load(Ordering::SeqCst),
            1,
            "exactly one submission reached the venue"
        );
    }

    #[tokio::test]
    async fn timeout_with_no_remote_order_stays_unknown() {
        let adapter = Arc::new(ScriptedAdapter::new(PlaceScript::TimeoutThenMissing));
        let (_tmp, manager) = manager(adapter.clone(), Arc::new(AllowAll));
        let order = manager.submit(&request("0x3")).await.expect("submit");
        assert_eq!(order.status, OrderStatus::Unknown);
        assert_eq!(adapter.place_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn definite_rejection_marks_failed() {
        let adapter = Arc::new(ScriptedAdapter::new(PlaceScript::DefiniteReject));
        let (_tmp, manager) = manager(adapter, Arc::new(AllowAll));
        let result = manager.submit(&request("0x4")).await;
        assert!(matches!(result, Err(ExecutionError::Exchange(_))));
        let stored = manager
            .with_db(|db| Ok(db.get_order("0x4")?))
            .expect("db")
            .expect("stored");
        assert_eq!(stored.status, OrderStatus::Failed);
    }

    #[tokio::test]
    async fn risk_rejection_never_reaches_the_venue() {
        let adapter = Arc::new(ScriptedAdapter::new(PlaceScript::Rest));
        let (_tmp, manager) = manager(adapter.clone(), Arc::new(DenyAll));
        let result = manager.submit(&request("0x5")).await;
        assert!(matches!(result, Err(ExecutionError::RiskRejected { .. })));
        assert_eq!(adapter.place_calls.load(Ordering::SeqCst), 0);
        let stored = manager
            .with_db(|db| Ok(db.get_order("0x5")?))
            .expect("db")
            .expect("stored");
        assert_eq!(stored.status, OrderStatus::Failed);
    }

    #[tokio::test]
    async fn duplicate_submission_is_idempotent() {
        let adapter = Arc::new(ScriptedAdapter::new(PlaceScript::Rest));
        let (_tmp, manager) = manager(adapter.clone(), Arc::new(AllowAll));
        manager.submit(&request("0x6")).await.expect("first");
        let again = manager.submit(&request("0x6")).await.expect("duplicate");
        assert_eq!(again.status, OrderStatus::Open);
        assert_eq!(adapter.place_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancel_converges_through_cancel_pending() {
        let adapter = Arc::new(ScriptedAdapter::new(PlaceScript::Rest));
        let (_tmp, manager) = manager(adapter, Arc::new(AllowAll));
        manager.submit(&request("0x7")).await.expect("submit");
        let cancelled = manager.cancel("acc-1", "0x7").await.expect("cancel");
        assert_eq!(cancelled.status, OrderStatus::Cancelled);
        // Cancelling again is a no-op returning the terminal record.
        let again = manager.cancel("acc-1", "0x7").await.expect("re-cancel");
        assert_eq!(again.status, OrderStatus::Cancelled);
    }
}
