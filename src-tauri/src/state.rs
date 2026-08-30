//! Application state shared across Tauri command invocations.
//!
//! State is **per window**: every window (keyed by its Tauri label) owns an
//! independent [`Environment`]. Windows never share mutable data, so opening,
//! querying, or closing files in one window can never affect another.

use crate::cache::CacheStore;
use crate::environment::Environment;
use crate::error::{AppError, AppResult};
use crate::query_session::QueryManager;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Tunable defaults for the app.
#[derive(Debug, Clone, Copy)]
pub struct AppConfig {
    /// Default rows per page (the product spec's "top 2000 rows").
    pub default_page_size: u64,
    /// Upper bound on the on-disk cache before LRU eviction kicks in.
    pub cache_cap_bytes: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_page_size: 2000,
            cache_cap_bytes: 8 * 1024 * 1024 * 1024, // 8 GiB
        }
    }
}

/// Managed by Tauri; one instance for the whole app.
///
/// Each window's [`Environment`] sits behind its own short-lived metadata lock;
/// query jobs capture immutable plans and release it before opening DuckDB.
/// The outer lock is held only long enough to look up or create the window Arc.
pub struct AppState {
    envs: Mutex<HashMap<String, Arc<Mutex<Environment>>>>,
    /// Source of unique labels for windows opened at runtime.
    next_window_id: AtomicU64,
    pub config: AppConfig,
    cache: CacheStore,
    pub sessions: QueryManager,
}

impl AppState {
    pub fn new() -> AppResult<Self> {
        let config = AppConfig::default();
        let cache = CacheStore::production(config.cache_cap_bytes)?;
        Ok(Self::with_cache(cache, config))
    }

    pub(crate) fn with_cache(cache: CacheStore, config: AppConfig) -> Self {
        Self {
            envs: Mutex::new(HashMap::new()),
            next_window_id: AtomicU64::new(1),
            config,
            cache,
            sessions: QueryManager::new(),
        }
    }

    /// The environment for `label`, creating an empty one on first use.
    pub fn env(&self, label: &str) -> AppResult<Arc<Mutex<Environment>>> {
        let mut map = self.envs.lock().map_err(|_| AppError::lock_poisoned())?;
        if let Some(arc) = map.get(label) {
            return Ok(arc.clone());
        }
        let arc = Arc::new(Mutex::new(Environment::new(self.cache.clone())));
        map.insert(label.to_string(), arc.clone());
        Ok(arc)
    }

    /// Drop a window's environment (called when the window is destroyed). Its
    /// connection and read-only attachments are released; other windows keep
    /// their own, even if they had the same file open.
    pub fn remove_env(&self, label: &str) {
        self.sessions.remove_window(label);
        if let Ok(mut map) = self.envs.lock() {
            map.remove(label);
        }
    }

    /// A fresh, unique window label (e.g. `win-1`, `win-2`, …).
    pub fn next_window_label(&self) -> String {
        format!(
            "win-{}",
            self.next_window_id.fetch_add(1, Ordering::Relaxed)
        )
    }
}
