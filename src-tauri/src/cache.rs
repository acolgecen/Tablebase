//! Process-wide, immutable cache of ingested DuckDB artifacts.
//!
//! Cache entries are keyed by the canonical source fingerprint, import options,
//! and a schema version. Creation is serialized per key, written to a temporary
//! database, and atomically installed so readers never observe a partial file.
//! Active environments and query sessions hold leases; eviction skips leased
//! entries and uses explicit access timestamps instead of database mtimes.

use crate::error::{AppError, AppResult};
use crate::ingest;
use crate::model::{ColumnInfo, ImportOptions};
use crate::query;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone)]
pub struct CacheStore {
    inner: Arc<CacheInner>,
}

struct CacheInner {
    root: PathBuf,
    max_bytes: u64,
    entry_locks: Mutex<HashMap<String, Weak<Mutex<()>>>>,
    leases: Mutex<HashMap<PathBuf, usize>>,
    next_temp_id: AtomicU64,
}

pub struct CacheArtifact {
    pub path: PathBuf,
    pub columns: Vec<ColumnInfo>,
    pub row_count: u64,
    pub cached: bool,
    pub lease: CacheLease,
}

pub struct CacheLease {
    inner: Arc<CacheInner>,
    path: PathBuf,
}

impl Clone for CacheLease {
    fn clone(&self) -> Self {
        increment_lease(&self.inner, &self.path);
        Self {
            inner: self.inner.clone(),
            path: self.path.clone(),
        }
    }
}

impl Drop for CacheLease {
    fn drop(&mut self) {
        if let Ok(mut leases) = self.inner.leases.lock()
            && let Some(count) = leases.get_mut(&self.path)
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                leases.remove(&self.path);
            }
        }
    }
}

impl CacheStore {
    pub fn production(max_bytes: u64) -> AppResult<Self> {
        let base = dirs::data_dir().ok_or_else(|| {
            AppError::Cache("could not locate the application data directory".into())
        })?;
        Self::at(base.join("Tablebase").join("cache"), max_bytes)
    }

    /// Construct a cache at an injected root. Tests use a temporary directory;
    /// production uses the application-support directory above.
    pub fn at(root: PathBuf, max_bytes: u64) -> AppResult<Self> {
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            inner: Arc::new(CacheInner {
                root,
                max_bytes,
                entry_locks: Mutex::new(HashMap::new()),
                leases: Mutex::new(HashMap::new()),
                next_temp_id: AtomicU64::new(0),
            }),
        })
    }

    /// Return a valid artifact, ingesting atomically when absent, corrupt, or
    /// explicitly refreshed. Import options are part of the cache identity.
    pub fn materialize(
        &self,
        source: &Path,
        options: &ImportOptions,
        force: bool,
    ) -> AppResult<CacheArtifact> {
        let canonical = std::fs::canonicalize(source)?;
        let key = cache_key(&canonical, options)?;
        let entry_lock = {
            let mut locks = self
                .inner
                .entry_locks
                .lock()
                .map_err(|_| AppError::lock_poisoned())?;
            if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(key.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        let _guard = entry_lock.lock().map_err(|_| AppError::lock_poisoned())?;

        let target = self.inner.root.join(format!("{key}.duckdb"));
        let existing = if target.exists() {
            inspect(&target).ok()
        } else {
            None
        };

        let (columns, row_count, cached) = if let Some((columns, row_count)) = existing {
            // Identical source fingerprint + options are deterministic. A
            // forced refresh can safely reuse this immutable artifact; changed
            // source metadata or options produce a different key.
            (columns, row_count, !force)
        } else {
            let temp_id = self.inner.next_temp_id.fetch_add(1, Ordering::Relaxed);
            let temp = self.inner.root.join(format!(
                ".{key}.{}.{}.tmp.duckdb",
                std::process::id(),
                temp_id
            ));
            let result = match ingest::ingest(&canonical, &temp, options) {
                Ok(result) => result,
                Err(error) => {
                    let _ = std::fs::remove_file(&temp);
                    return Err(error);
                }
            };
            if let Err(error) = std::fs::rename(&temp, &target) {
                let _ = std::fs::remove_file(&temp);
                return Err(AppError::Cache(format!(
                    "could not install cache artifact: {error}"
                )));
            }
            (result.columns, result.row_count, false)
        };

        self.touch(&target);
        let lease = self.lease(&target)?;
        self.evict_lru()?;

        Ok(CacheArtifact {
            path: target,
            columns,
            row_count,
            cached,
            lease,
        })
    }

    fn lease(&self, path: &Path) -> AppResult<CacheLease> {
        let mut leases = self
            .inner
            .leases
            .lock()
            .map_err(|_| AppError::lock_poisoned())?;
        *leases.entry(path.to_path_buf()).or_insert(0) += 1;
        Ok(CacheLease {
            inner: self.inner.clone(),
            path: path.to_path_buf(),
        })
    }

    fn touch(&self, database: &Path) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let _ = std::fs::write(access_path(database), now.to_string());
    }

    fn evict_lru(&self) -> AppResult<()> {
        let leased = self
            .inner
            .leases
            .lock()
            .map_err(|_| AppError::lock_poisoned())?
            .clone();
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&self.inner.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("duckdb")
                || path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }
            let metadata = entry.metadata()?;
            let accessed = std::fs::read_to_string(access_path(&path))
                .ok()
                .and_then(|value| value.parse::<u128>().ok())
                .unwrap_or(0);
            entries.push((path, metadata.len(), accessed));
        }

        let mut total = entries.iter().map(|(_, size, _)| *size).sum::<u64>();
        entries.sort_by_key(|(_, _, accessed)| *accessed);
        for (path, size, _) in entries {
            if total <= self.inner.max_bytes {
                break;
            }
            if leased.get(&path).copied().unwrap_or(0) > 0 {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                let _ = std::fs::remove_file(access_path(&path));
                total = total.saturating_sub(size);
            }
        }
        Ok(())
    }
}

fn increment_lease(inner: &Arc<CacheInner>, path: &Path) {
    if let Ok(mut leases) = inner.leases.lock() {
        *leases.entry(path.to_path_buf()).or_insert(0) += 1;
    }
}

fn cache_key(path: &Path, options: &ImportOptions) -> AppResult<String> {
    let metadata = std::fs::metadata(path)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(CACHE_SCHEMA_VERSION.to_le_bytes());
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(metadata.len().to_le_bytes());
    hasher.update(modified.to_le_bytes());
    hasher.update(options.delimiter.as_deref().unwrap_or("<auto>").as_bytes());
    hasher.update([options.header.map(u8::from).unwrap_or(2)]);
    Ok(hasher
        .finalize()
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn inspect(database: &Path) -> AppResult<(Vec<ColumnInfo>, u64)> {
    let connection = query::open_readonly(database)?;
    let columns = ingest::columns_of(&connection, ingest::TABLE)?;
    let count: i64 = connection.query_row(
        &format!("SELECT count(*) FROM {}", ingest::TABLE),
        [],
        |row| row.get(0),
    )?;
    Ok((columns, count.max(0) as u64))
}

fn access_path(database: &Path) -> PathBuf {
    database.with_extension("access")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "tablebase_cache_{}_{}_{}",
            std::process::id(),
            name,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("data.csv");
        std::fs::write(&source, "id,name\n1,alice\n").unwrap();
        (root, source)
    }

    #[test]
    fn options_are_part_of_cache_identity() {
        let (root, source) = fixture("options");
        let store = CacheStore::at(root.join("cache"), u64::MAX).unwrap();
        let first = store
            .materialize(&source, &ImportOptions::default(), false)
            .unwrap();
        assert!(!first.cached);
        let hit = store
            .materialize(&source, &ImportOptions::default(), false)
            .unwrap();
        assert!(hit.cached);
        assert_eq!(first.path, hit.path);

        let explicit = store
            .materialize(
                &source,
                &ImportOptions {
                    delimiter: Some(",".into()),
                    header: Some(true),
                },
                false,
            )
            .unwrap();
        assert_ne!(first.path, explicit.path);
        drop((first, hit, explicit));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_ingestion_publishes_one_valid_artifact() {
        let (root, source) = fixture("concurrent");
        let store = CacheStore::at(root.join("cache"), u64::MAX).unwrap();
        let threads = (0..4)
            .map(|_| {
                let store = store.clone();
                let source = source.clone();
                std::thread::spawn(move || {
                    store
                        .materialize(&source, &ImportOptions::default(), false)
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let artifacts = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert!(artifacts.iter().all(|artifact| artifact.row_count == 1));
        assert!(
            artifacts
                .iter()
                .all(|artifact| artifact.path == artifacts[0].path)
        );
        drop(artifacts);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn eviction_never_removes_a_leased_artifact() {
        let (root, first_source) = fixture("leases");
        let second_source = root.join("second.csv");
        std::fs::write(&second_source, "id,name\n2,bob\n").unwrap();
        let store = CacheStore::at(root.join("cache"), 1).unwrap();
        let first = store
            .materialize(&first_source, &ImportOptions::default(), false)
            .unwrap();
        let first_path = first.path.clone();
        assert!(first_path.exists());
        drop(first);

        let second = store
            .materialize(&second_source, &ImportOptions::default(), false)
            .unwrap();
        assert!(second.path.exists());
        assert!(!first_path.exists());
        drop(second);
        std::fs::remove_dir_all(root).unwrap();
    }
}
