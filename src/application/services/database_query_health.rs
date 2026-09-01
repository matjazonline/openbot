//! Cached, operator-facing query statistics.
//!
//! The HTTP adapter decides who may ask. This service keeps the expensive PostgreSQL statistics
//! read shared across tabs and turns adapter failures into a deliberately small, display-safe
//! category. The raw database error never crosses this boundary.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::{sync::Mutex, time::Instant};

/// Displayed SQL is bounded at the persistence boundary before it enters the cache or renderer.
pub const MAX_NORMALIZED_SQL_BYTES: usize = 16 * 1024;

const SUCCESS_TTL: Duration = Duration::from_secs(60);
const FAILURE_TTL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryHealthFailureCategory {
    ExtensionUnavailable,
    PermissionDenied,
    DatabaseUnavailable,
}

impl QueryHealthFailureCategory {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ExtensionUnavailable => "extension unavailable",
            Self::PermissionDenied => "permission denied",
            Self::DatabaseUnavailable => "database unavailable",
        }
    }
}

/// A persistence failure carries no source text by design. The PostgreSQL adapter logs its source
/// error before returning one of these categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatabaseQueryHealthError {
    pub category: QueryHealthFailureCategory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedSql {
    text: String,
    truncated: bool,
}

impl NormalizedSql {
    /// Bound UTF-8 by bytes without splitting a code point.
    pub fn bounded(value: &str, database_reported_truncation: bool) -> Self {
        let mut end = value.len().min(MAX_NORMALIZED_SQL_BYTES);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            text: value[..end].to_string(),
            truncated: database_reported_truncation || end < value.len(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryHealthEntry {
    pub query_id: i64,
    pub sql: NormalizedSql,
    pub calls: i64,
    pub total_exec_time_ms: f64,
    pub mean_exec_time_ms: f64,
    pub max_exec_time_ms: f64,
    pub rows: i64,
    pub shared_blocks_read: i64,
    pub shared_blocks_hit: i64,
    pub temporary_blocks_written: i64,
    pub wal_bytes: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementTracking {
    None,
    Top,
    All,
    Unknown,
}

impl StatementTracking {
    pub const fn parse(value: &str) -> Self {
        match value.as_bytes() {
            b"none" => Self::None,
            b"top" => Self::Top,
            b"all" => Self::All,
            _ => Self::Unknown,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Top => "top",
            Self::All => "all",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseQueryHealthSnapshot {
    pub statistics_observed_since: DateTime<Utc>,
    pub deallocations: i64,
    pub track: StatementTracking,
    pub utility_tracking: bool,
    pub slow_query_logging_threshold_ms: Option<i64>,
    pub bind_parameters_redacted: bool,
    pub top_by_total_time: Vec<QueryHealthEntry>,
    pub top_by_mean_time: Vec<QueryHealthEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DatabaseQueryHealth {
    Available(DatabaseQueryHealthSnapshot),
    Unavailable(QueryHealthFailureCategory),
}

#[async_trait]
pub trait DatabaseQueryHealthPersistence: Send + Sync {
    async fn database_query_health(
        &self,
    ) -> Result<DatabaseQueryHealthSnapshot, DatabaseQueryHealthError>;
}

#[derive(Clone)]
struct CachedReading {
    value: DatabaseQueryHealth,
    expires_at: Instant,
}

/// One mutex deliberately covers both the cache check and refresh. A refresh happens at most once;
/// concurrent callers wait for it and then reuse the same reading.
pub struct DatabaseQueryHealthService {
    persistence: Arc<dyn DatabaseQueryHealthPersistence>,
    cache: Mutex<Option<CachedReading>>,
}

impl DatabaseQueryHealthService {
    pub fn new(persistence: Arc<dyn DatabaseQueryHealthPersistence>) -> Self {
        Self {
            persistence,
            cache: Mutex::new(None),
        }
    }

    pub async fn snapshot(&self) -> DatabaseQueryHealth {
        let mut cache = self.cache.lock().await;
        let now = Instant::now();
        if let Some(reading) = cache.as_ref().filter(|reading| reading.expires_at > now) {
            return reading.value.clone();
        }

        let (value, ttl) = match self.persistence.database_query_health().await {
            Ok(snapshot) => (DatabaseQueryHealth::Available(snapshot), SUCCESS_TTL),
            Err(error) => (
                DatabaseQueryHealth::Unavailable(error.category),
                FAILURE_TTL,
            ),
        };
        *cache = Some(CachedReading {
            value: value.clone(),
            expires_at: Instant::now() + ttl,
        });
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct Reads {
        count: AtomicUsize,
        fail: AtomicBool,
    }

    impl Reads {
        fn new(fail: bool) -> Self {
            Self {
                count: AtomicUsize::new(0),
                fail: AtomicBool::new(fail),
            }
        }
    }

    #[async_trait]
    impl DatabaseQueryHealthPersistence for Reads {
        async fn database_query_health(
            &self,
        ) -> Result<DatabaseQueryHealthSnapshot, DatabaseQueryHealthError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            if self.fail.load(Ordering::SeqCst) {
                Err(DatabaseQueryHealthError {
                    category: QueryHealthFailureCategory::PermissionDenied,
                })
            } else {
                Ok(DatabaseQueryHealthSnapshot {
                    statistics_observed_since: Utc::now(),
                    deallocations: 0,
                    track: StatementTracking::Top,
                    utility_tracking: false,
                    slow_query_logging_threshold_ms: None,
                    bind_parameters_redacted: true,
                    top_by_total_time: Vec::new(),
                    top_by_mean_time: Vec::new(),
                })
            }
        }
    }

    #[test]
    fn normalized_sql_is_bounded_by_bytes_on_a_character_boundary() {
        let oversized = format!("{}é", "x".repeat(MAX_NORMALIZED_SQL_BYTES - 1));
        let bounded = NormalizedSql::bounded(&oversized, false);
        assert!(bounded.as_str().len() <= MAX_NORMALIZED_SQL_BYTES);
        assert!(bounded.truncated());
        assert!(std::str::from_utf8(bounded.as_str().as_bytes()).is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn successes_are_cached_for_sixty_seconds_and_refresh_after_expiry() {
        let persistence = Arc::new(Reads::new(false));
        let service = DatabaseQueryHealthService::new(persistence.clone());

        service.snapshot().await;
        tokio::time::advance(Duration::from_secs(59)).await;
        service.snapshot().await;
        assert_eq!(persistence.count.load(Ordering::SeqCst), 1);

        tokio::time::advance(Duration::from_secs(2)).await;
        service.snapshot().await;
        assert_eq!(persistence.count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn failures_are_cached_for_fifteen_seconds() {
        let persistence = Arc::new(Reads::new(true));
        let service = DatabaseQueryHealthService::new(persistence.clone());

        assert!(matches!(
            service.snapshot().await,
            DatabaseQueryHealth::Unavailable(QueryHealthFailureCategory::PermissionDenied)
        ));
        tokio::time::advance(Duration::from_secs(14)).await;
        service.snapshot().await;
        assert_eq!(persistence.count.load(Ordering::SeqCst), 1);

        tokio::time::advance(Duration::from_secs(2)).await;
        service.snapshot().await;
        assert_eq!(persistence.count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn concurrent_misses_share_one_refresh() {
        let persistence = Arc::new(Reads::new(false));
        let service = Arc::new(DatabaseQueryHealthService::new(persistence.clone()));
        let mut readers = Vec::new();
        for _ in 0..20 {
            let service = service.clone();
            readers.push(tokio::spawn(async move { service.snapshot().await }));
        }
        for reader in readers {
            assert!(matches!(
                reader.await.unwrap(),
                DatabaseQueryHealth::Available(_)
            ));
        }
        assert_eq!(persistence.count.load(Ordering::SeqCst), 1);
    }
}
