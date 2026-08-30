//! Immutable query sessions and cancellable execution jobs.

use crate::environment::QueryPlan;
use crate::error::{AppError, AppResult};
use crate::guardrail::ValidatedQuery;
use crate::model::{ExportInfo, PageResult};
use crate::{export, query};
use duckdb::InterruptHandle;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

pub struct QueryManager {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<(String, u64), Arc<QuerySession>>>,
}

pub struct QuerySession {
    pub id: u64,
    pub workspace_revision: u64,
    plan: QueryPlan,
    query: ValidatedQuery,
    cancelled: AtomicBool,
    active: Mutex<Vec<Weak<InterruptHandle>>>,
}

impl QueryManager {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn create(
        &self,
        window: &str,
        plan: QueryPlan,
        query: ValidatedQuery,
    ) -> AppResult<Arc<QuerySession>> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let session = Arc::new(QuerySession {
            id,
            workspace_revision: plan.revision,
            plan,
            query,
            cancelled: AtomicBool::new(false),
            active: Mutex::new(Vec::new()),
        });
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| AppError::lock_poisoned())?;
        sessions.insert((window.to_string(), id), session.clone());
        Ok(session)
    }

    pub fn get(&self, window: &str, id: u64) -> AppResult<Arc<QuerySession>> {
        self.sessions
            .lock()
            .map_err(|_| AppError::lock_poisoned())?
            .get(&(window.to_string(), id))
            .cloned()
            .ok_or_else(|| {
                AppError::QuerySession(
                    "This result is no longer available. Run the query again.".into(),
                )
            })
    }

    pub fn cancel(&self, window: &str, id: u64) {
        if let Ok(mut sessions) = self.sessions.lock()
            && let Some(session) = sessions.remove(&(window.to_string(), id))
        {
            session.cancel();
        }
    }

    pub fn remove_window(&self, window: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            let keys = sessions
                .keys()
                .filter(|(label, _)| label == window)
                .cloned()
                .collect::<Vec<_>>();
            for key in keys {
                if let Some(session) = sessions.remove(&key) {
                    session.cancel();
                }
            }
        }
    }
}

impl QuerySession {
    pub fn page(&self, page: u64, page_size: u64) -> AppResult<PageResult> {
        let connection = self.connection()?;
        query::run_page(&connection, &self.query, page, page_size)
    }

    pub fn count(&self) -> AppResult<u64> {
        let connection = self.connection()?;
        query::count_rows(&connection, &self.query)
    }

    pub fn export(&self, destination: &Path) -> AppResult<ExportInfo> {
        let connection = self.connection()?;
        export::export(&connection, &self.query, destination)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(mut active) = self.active.lock() {
            active.retain(|weak| {
                if let Some(handle) = weak.upgrade() {
                    handle.interrupt();
                    true
                } else {
                    false
                }
            });
        }
    }

    fn connection(&self) -> AppResult<duckdb::Connection> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(AppError::QuerySession("The query was cancelled.".into()));
        }
        let connection = self.plan.open_connection()?;
        let handle = connection.interrupt_handle();
        let mut active = self.active.lock().map_err(|_| AppError::lock_poisoned())?;
        active.retain(|weak| weak.strong_count() > 0);
        active.push(Arc::downgrade(&handle));
        drop(active);
        if self.cancelled.load(Ordering::Acquire) {
            handle.interrupt();
            return Err(AppError::QuerySession("The query was cancelled.".into()));
        }
        Ok(connection)
    }
}
