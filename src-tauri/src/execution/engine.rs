//! Execution engine (Phase 7): the single entry point strategies and
//! the UI use for trading actions. It delegates the lifecycle to the
//! order manager and startup recovery to `recovery`, so there is
//! exactly one code path that can place, cancel or converge an order.

use std::sync::Arc;

use crate::models::{Order, OrderRequest, TimestampMs};

use super::order_manager::{ExecutionError, OrderManager};
use super::recovery::{recover_account, RecoveryReport};

pub struct ExecutionEngine {
    manager: Arc<OrderManager>,
}

impl ExecutionEngine {
    pub fn new(manager: Arc<OrderManager>) -> Self {
        Self { manager }
    }

    pub fn manager(&self) -> Arc<OrderManager> {
        self.manager.clone()
    }

    pub async fn submit_order(&self, request: &OrderRequest) -> Result<Order, ExecutionError> {
        self.manager.submit(request).await
    }

    pub async fn cancel_order(
        &self,
        account_id: &str,
        client_order_id: &str,
    ) -> Result<Order, ExecutionError> {
        self.manager.cancel(account_id, client_order_id).await
    }

    pub async fn query_order(
        &self,
        account_id: &str,
        client_order_id: &str,
    ) -> Result<Order, ExecutionError> {
        self.manager.query(account_id, client_order_id).await
    }

    /// Startup recovery for one account: converge every unfinished
    /// local order against the exchange before new openings.
    pub async fn recover_account(
        &self,
        account_id: &str,
        now: TimestampMs,
    ) -> Result<RecoveryReport, ExecutionError> {
        let adapter = self.manager.adapter(account_id)?;
        recover_account(&self.manager.database(), &adapter, now).await
    }
}
