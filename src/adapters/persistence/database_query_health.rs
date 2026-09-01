//! PostgreSQL's cumulative normalized-query statistics for the operator dashboard.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::FromRow;
use tracing::warn;

use crate::{
    adapters::persistence::PostgresPersistence,
    services::database_query_health::{
        DatabaseQueryHealthError, DatabaseQueryHealthPersistence, DatabaseQueryHealthSnapshot,
        NormalizedSql, QueryHealthEntry, QueryHealthFailureCategory, StatementTracking,
    },
};

/// One statement performs the whole snapshot so a single-flight refresh is one database read.
/// `dbid` is constrained to the current database before aggregation. PostgreSQL keeps separate
/// rows for users and top-level status; grouping by `queryid` combines those with weighted timing.
const DATABASE_QUERY_HEALTH_SQL: &str = r#"
    WITH collection AS (
        SELECT info.stats_reset AS statistics_observed_since,
               info.dealloc::bigint AS deallocations,
               (SELECT setting FROM pg_settings WHERE name = 'pg_stat_statements.track') AS track,
               (SELECT setting = 'on'
                  FROM pg_settings
                 WHERE name = 'pg_stat_statements.track_utility') AS utility_tracking,
               (SELECT CASE WHEN setting::bigint < 0 THEN NULL ELSE setting::bigint END
                  FROM pg_settings
                 WHERE name = 'log_min_duration_statement') AS slow_query_logging_threshold_ms,
               (SELECT setting::bigint = 0
                  FROM pg_settings
                 WHERE name = 'log_parameter_max_length')
               AND
               (SELECT setting::bigint = 0
                  FROM pg_settings
                 WHERE name = 'log_parameter_max_length_on_error') AS bind_parameters_redacted
          FROM pg_stat_statements_info AS info
    ),
    aggregated AS (
        SELECT statement.queryid,
               MIN(LEFT(statement.query, 16384)) AS normalized_sql,
               BOOL_OR(octet_length(statement.query) > 16384) AS sql_truncated,
               SUM(statement.calls)::bigint AS calls,
               SUM(statement.total_exec_time)::float8 AS total_exec_time_ms,
               (SUM(statement.total_exec_time) / NULLIF(SUM(statement.calls), 0))::float8
                   AS mean_exec_time_ms,
               MAX(statement.max_exec_time)::float8 AS max_exec_time_ms,
               SUM(statement.rows)::bigint AS rows,
               SUM(statement.shared_blks_read)::bigint AS shared_blocks_read,
               SUM(statement.shared_blks_hit)::bigint AS shared_blocks_hit,
               SUM(statement.temp_blks_written)::bigint AS temporary_blocks_written,
               SUM(statement.wal_bytes)::float8 AS wal_bytes
          FROM pg_stat_statements AS statement
         WHERE statement.dbid = (
                   SELECT database.oid
                     FROM pg_database AS database
                    WHERE database.datname = current_database()
               )
         GROUP BY statement.queryid
    ),
    ranked AS (
        (SELECT 'total'::text AS ranking,
                ROW_NUMBER() OVER (
                    ORDER BY aggregated.total_exec_time_ms DESC, aggregated.queryid ASC
                )::bigint AS rank,
                aggregated.*
           FROM aggregated
          ORDER BY aggregated.total_exec_time_ms DESC, aggregated.queryid ASC
          LIMIT 5)
        UNION ALL
        (SELECT 'mean'::text AS ranking,
                ROW_NUMBER() OVER (
                    ORDER BY aggregated.mean_exec_time_ms DESC, aggregated.queryid ASC
                )::bigint AS rank,
                aggregated.*
           FROM aggregated
          WHERE aggregated.calls >= 5
          ORDER BY aggregated.mean_exec_time_ms DESC, aggregated.queryid ASC
          LIMIT 5)
    )
    SELECT collection.statistics_observed_since,
           collection.deallocations,
           collection.track,
           collection.utility_tracking,
           collection.slow_query_logging_threshold_ms,
           collection.bind_parameters_redacted,
           ranked.ranking,
           ranked.rank,
           ranked.queryid,
           ranked.normalized_sql,
           ranked.sql_truncated,
           ranked.calls,
           ranked.total_exec_time_ms,
           ranked.mean_exec_time_ms,
           ranked.max_exec_time_ms,
           ranked.rows,
           ranked.shared_blocks_read,
           ranked.shared_blocks_hit,
           ranked.temporary_blocks_written,
           ranked.wal_bytes
      FROM collection
      LEFT JOIN ranked ON TRUE
     ORDER BY CASE ranked.ranking WHEN 'total' THEN 0 ELSE 1 END, ranked.rank ASC"#;

#[derive(FromRow)]
struct QueryHealthRow {
    statistics_observed_since: DateTime<Utc>,
    deallocations: i64,
    track: String,
    utility_tracking: bool,
    slow_query_logging_threshold_ms: Option<i64>,
    bind_parameters_redacted: bool,
    ranking: Option<String>,
    rank: Option<i64>,
    queryid: Option<i64>,
    normalized_sql: Option<String>,
    sql_truncated: Option<bool>,
    calls: Option<i64>,
    total_exec_time_ms: Option<f64>,
    mean_exec_time_ms: Option<f64>,
    max_exec_time_ms: Option<f64>,
    rows: Option<i64>,
    shared_blocks_read: Option<i64>,
    shared_blocks_hit: Option<i64>,
    temporary_blocks_written: Option<i64>,
    wal_bytes: Option<f64>,
}

impl QueryHealthRow {
    fn entry(&self) -> Option<QueryHealthEntry> {
        Some(QueryHealthEntry {
            query_id: self.queryid?,
            sql: NormalizedSql::bounded(
                self.normalized_sql.as_deref()?,
                self.sql_truncated.unwrap_or(false),
            ),
            calls: self.calls?,
            total_exec_time_ms: self.total_exec_time_ms?,
            mean_exec_time_ms: self.mean_exec_time_ms?,
            max_exec_time_ms: self.max_exec_time_ms?,
            rows: self.rows?,
            shared_blocks_read: self.shared_blocks_read?,
            shared_blocks_hit: self.shared_blocks_hit?,
            temporary_blocks_written: self.temporary_blocks_written?,
            wal_bytes: self.wal_bytes?,
        })
    }
}

fn failure(error: sqlx::Error) -> DatabaseQueryHealthError {
    let category = match error.as_database_error().and_then(|error| error.code()) {
        Some(code) if matches!(code.as_ref(), "42P01" | "42704" | "55000") => {
            QueryHealthFailureCategory::ExtensionUnavailable
        }
        Some(code) if code == "42501" => QueryHealthFailureCategory::PermissionDenied,
        _ => QueryHealthFailureCategory::DatabaseUnavailable,
    };
    warn!(%error, category = category.label(), "Database query statistics read failed");
    DatabaseQueryHealthError { category }
}

fn malformed_snapshot() -> DatabaseQueryHealthError {
    warn!("Database query statistics returned an incomplete row");
    DatabaseQueryHealthError {
        category: QueryHealthFailureCategory::DatabaseUnavailable,
    }
}

#[async_trait]
impl DatabaseQueryHealthPersistence for PostgresPersistence {
    async fn database_query_health(
        &self,
    ) -> Result<DatabaseQueryHealthSnapshot, DatabaseQueryHealthError> {
        let rows = sqlx::query_as::<_, QueryHealthRow>(DATABASE_QUERY_HEALTH_SQL)
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;
        let metadata = rows.first().ok_or_else(malformed_snapshot)?;
        let mut top_by_total_time = Vec::with_capacity(5);
        let mut top_by_mean_time = Vec::with_capacity(5);
        for row in &rows {
            let Some(ranking) = row.ranking.as_deref() else {
                continue;
            };
            let entry = row.entry().ok_or_else(malformed_snapshot)?;
            match ranking {
                "total" => top_by_total_time.push((row.rank.unwrap_or(i64::MAX), entry)),
                "mean" => top_by_mean_time.push((row.rank.unwrap_or(i64::MAX), entry)),
                _ => return Err(malformed_snapshot()),
            }
        }
        top_by_total_time.sort_by_key(|(rank, _)| *rank);
        top_by_mean_time.sort_by_key(|(rank, _)| *rank);

        Ok(DatabaseQueryHealthSnapshot {
            statistics_observed_since: metadata.statistics_observed_since,
            deallocations: metadata.deallocations,
            track: StatementTracking::parse(&metadata.track),
            utility_tracking: metadata.utility_tracking,
            slow_query_logging_threshold_ms: metadata.slow_query_logging_threshold_ms,
            bind_parameters_redacted: metadata.bind_parameters_redacted,
            top_by_total_time: top_by_total_time
                .into_iter()
                .map(|(_, entry)| entry)
                .collect(),
            top_by_mean_time: top_by_mean_time
                .into_iter()
                .map(|(_, entry)| entry)
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::test_pool;

    #[test]
    fn query_is_current_database_scoped_and_rankings_are_deterministic() {
        assert!(DATABASE_QUERY_HEALTH_SQL.contains("statement.dbid ="));
        assert!(DATABASE_QUERY_HEALTH_SQL.contains("current_database()"));
        assert!(DATABASE_QUERY_HEALTH_SQL.contains("GROUP BY statement.queryid"));
        assert!(
            DATABASE_QUERY_HEALTH_SQL
                .contains("SUM(statement.total_exec_time) / NULLIF(SUM(statement.calls), 0)")
        );
        assert!(DATABASE_QUERY_HEALTH_SQL.contains("aggregated.calls >= 5"));
        assert_eq!(DATABASE_QUERY_HEALTH_SQL.matches("LIMIT 5").count(), 2);
        assert!(DATABASE_QUERY_HEALTH_SQL.contains("aggregated.queryid ASC"));
    }

    #[test]
    fn sql_text_is_bounded_again_at_the_adapter_boundary() {
        let text = "é".repeat(10_000);
        let row = QueryHealthRow {
            statistics_observed_since: Utc::now(),
            deallocations: 0,
            track: "top".into(),
            utility_tracking: false,
            slow_query_logging_threshold_ms: None,
            bind_parameters_redacted: true,
            ranking: Some("total".into()),
            rank: Some(1),
            queryid: Some(1),
            normalized_sql: Some(text),
            sql_truncated: Some(false),
            calls: Some(1),
            total_exec_time_ms: Some(1.0),
            mean_exec_time_ms: Some(1.0),
            max_exec_time_ms: Some(1.0),
            rows: Some(1),
            shared_blocks_read: Some(1),
            shared_blocks_hit: Some(1),
            temporary_blocks_written: Some(1),
            wal_bytes: Some(1.0),
        };
        let entry = row.entry().unwrap();
        assert!(entry.sql.as_str().len() <= 16 * 1024);
        assert!(entry.sql.truncated());
    }

    #[tokio::test]
    async fn database_snapshot_executes_or_reports_that_the_library_needs_preloading() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        match persistence.database_query_health().await {
            Ok(snapshot) => {
                assert_eq!(snapshot.track, StatementTracking::Top);
                assert!(!snapshot.utility_tracking);
                assert!(snapshot.top_by_total_time.len() <= 5);
                assert!(snapshot.top_by_mean_time.len() <= 5);
                assert!(
                    snapshot
                        .top_by_mean_time
                        .iter()
                        .all(|entry| entry.calls >= 5)
                );
            }
            Err(error) => assert_eq!(
                error.category,
                QueryHealthFailureCategory::ExtensionUnavailable,
                "a configured CI/production database must return a snapshot; an un-preloaded local database has one explicit fallback"
            ),
        }
    }
}
