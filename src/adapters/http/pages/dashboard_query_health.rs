//! Operator-only `pg_stat_statements` rendering for the system dashboard.

use super::{
    dashboard::{Stat, panel, stat, stat_row},
    escape_html_text,
};
use crate::services::database_query_health::{
    DatabaseQueryHealth, DatabaseQueryHealthSnapshot, QueryHealthEntry, StatementTracking,
};

pub(super) fn database_query_health_section(health: Option<&DatabaseQueryHealth>) -> String {
    let Some(health) = health else {
        return String::new();
    };
    match health {
        DatabaseQueryHealth::Unavailable(category) => format!(
            r##"<div>
                <h2 class="mb-1 text-lg font-semibold">Database query health</h2>
                <p class="mb-3 text-sm opacity-60">Cumulative normalized statements for the current database.</p>
                <div class="alert alert-warning">
                    <span><strong>Query statistics unavailable.</strong> Collection category: {category}.</span>
                </div>
            </div>"##,
            category = escape_html_text(category.label()),
        ),
        DatabaseQueryHealth::Available(snapshot) => available(snapshot),
    }
}

fn available(snapshot: &DatabaseQueryHealthSnapshot) -> String {
    let warnings = collection_warnings(snapshot);
    format!(
        r##"<div>
            <h2 class="mb-1 text-lg font-semibold">Database query health</h2>
            <p class="mb-3 text-sm opacity-60">
                Cumulative normalized statements for the current database. Rankings identify what to investigate;
                use the controlled statistics and EXPLAIN workflow for plans and bind-value classes.
            </p>
            <div class="flex flex-col gap-3">
                {metadata}
                {warnings}
                {total}
                {mean}
            </div>
        </div>"##,
        metadata = metadata(snapshot),
        warnings = warnings,
        total = ranking_panel(
            "Top queries by total execution time",
            &snapshot.top_by_total_time
        ),
        mean = ranking_panel(
            "Top queries by weighted mean execution time (5+ calls)",
            &snapshot.top_by_mean_time,
        ),
    )
}

fn metadata(snapshot: &DatabaseQueryHealthSnapshot) -> String {
    let observed_since = snapshot
        .statistics_observed_since
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string();
    let slow_logging = snapshot
        .slow_query_logging_threshold_ms
        .map(|threshold| format!("{threshold} ms"))
        .unwrap_or_else(|| "Off".to_string());
    let redaction = if snapshot.bind_parameters_redacted {
        "Redacted"
    } else {
        "Unsafe"
    };
    stat_row(
        "Collection",
        &format!(
            "{status}{window}{dealloc}{slow}{redaction}",
            status = stat(Stat::new(
                "pg_stat_statements",
                "Enabled",
                &format!(
                    "track={} · utility {}",
                    snapshot.track.label(),
                    if snapshot.utility_tracking {
                        "on"
                    } else {
                        "off"
                    },
                ),
            )),
            window = stat(Stat::new(
                "Observed since",
                &observed_since,
                "statistics reset",
            )),
            dealloc = stat(
                Stat::new(
                    "Deallocations",
                    &snapshot.deallocations.to_string(),
                    "statistics entries evicted",
                )
                .alarming(snapshot.deallocations > 0),
            ),
            slow = stat(Stat::new(
                "Slow-query logging",
                &slow_logging,
                "effective database setting",
            )),
            redaction = stat(
                Stat::new(
                    "Bind parameters",
                    redaction,
                    "statement and error log limits",
                )
                .alarming(!snapshot.bind_parameters_redacted),
            ),
        ),
    )
}

fn collection_warnings(snapshot: &DatabaseQueryHealthSnapshot) -> String {
    let mut warnings = Vec::new();
    if snapshot.deallocations > 0 {
        warnings.push(format!(
            "{} statement entries were deallocated during this observation window; rankings may omit displaced queries.",
            snapshot.deallocations
        ));
    }
    if snapshot.track != StatementTracking::Top {
        warnings.push(format!(
            "pg_stat_statements.track is {}, not the expected top setting.",
            snapshot.track.label()
        ));
    }
    if snapshot.utility_tracking {
        warnings
            .push("Utility tracking is on; the expected production setting is off.".to_string());
    }
    if snapshot.slow_query_logging_threshold_ms.is_some() && !snapshot.bind_parameters_redacted {
        warnings.push(
            "Slow-query logging is enabled while bind-parameter redaction is unsafe; disable logging or restore both zero parameter limits."
                .to_string(),
        );
    }
    if warnings.is_empty() {
        return String::new();
    }
    format!(
        r##"<div class="alert alert-warning"><ul class="list-disc pl-5">{items}</ul></div>"##,
        items = warnings
            .iter()
            .map(|warning| format!("<li>{}</li>", escape_html_text(warning)))
            .collect::<String>(),
    )
}

fn ranking_panel(heading: &str, entries: &[QueryHealthEntry]) -> String {
    if entries.is_empty() {
        return panel(
            heading,
            r##"<div class="px-4 py-6 text-sm opacity-60">No qualifying statements have been observed.</div>"##,
        );
    }
    let rows = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| query_row(index + 1, entry))
        .collect::<String>();
    panel(
        heading,
        &format!(r##"<div class="divide-y divide-base-300">{rows}</div>"##),
    )
}

fn query_row(rank: usize, entry: &QueryHealthEntry) -> String {
    let truncation = entry.sql.truncated().then_some({
        r##"<p class="mt-2 text-xs font-semibold text-warning">SQL truncated to the 16 KiB display limit.</p>"##
    });
    format!(
        r##"<div class="p-4">
            <div class="mb-3 flex flex-wrap items-baseline justify-between gap-2">
                <span class="font-semibold">#{rank} · Query {query_id}</span>
                <span class="text-xs opacity-60">Normalized SQL; bind values are not collected</span>
            </div>
            <div class="grid grid-cols-2 gap-3 sm:grid-cols-4 xl:grid-cols-6">
                {calls}{total}{mean}{max}{rows}{reads}{hits}{temp}{wal}
            </div>
            <details class="mt-3 rounded-box bg-base-200 p-3">
                <summary class="cursor-pointer text-sm font-semibold">Full normalized SQL</summary>
                <pre class="mt-3 max-h-80 overflow-auto whitespace-pre-wrap break-words text-xs"><code>{sql}</code></pre>
                {truncation}
            </details>
        </div>"##,
        rank = rank,
        query_id = entry.query_id,
        calls = metric("Calls", &entry.calls.to_string()),
        total = metric("Total", &milliseconds(entry.total_exec_time_ms)),
        mean = metric("Mean", &milliseconds(entry.mean_exec_time_ms)),
        max = metric("Max", &milliseconds(entry.max_exec_time_ms)),
        rows = metric("Rows", &entry.rows.to_string()),
        reads = metric("Shared reads", &entry.shared_blocks_read.to_string()),
        hits = metric("Shared hits", &entry.shared_blocks_hit.to_string()),
        temp = metric("Temp writes", &entry.temporary_blocks_written.to_string()),
        wal = metric("WAL bytes", &bytes(entry.wal_bytes)),
        sql = escape_html_text(entry.sql.as_str()),
        truncation = truncation.unwrap_or_default(),
    )
}

fn metric(label: &str, value: &str) -> String {
    format!(
        r##"<div><div class="text-xs uppercase tracking-wide opacity-50">{label}</div><div class="font-mono text-sm">{value}</div></div>"##,
        label = escape_html_text(label),
        value = escape_html_text(value),
    )
}

fn milliseconds(value: f64) -> String {
    format!("{value:.2} ms")
}

fn bytes(value: f64) -> String {
    if value >= 1_048_576.0 {
        format!("{:.1} MiB", value / 1_048_576.0)
    } else if value >= 1024.0 {
        format!("{:.1} KiB", value / 1024.0)
    } else {
        format!("{value:.0} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::database_query_health::{NormalizedSql, QueryHealthFailureCategory};
    use chrono::{TimeZone, Utc};

    fn snapshot(sql: &str, truncated: bool) -> DatabaseQueryHealthSnapshot {
        let entry = QueryHealthEntry {
            query_id: 42,
            sql: NormalizedSql::bounded(sql, truncated),
            calls: 7,
            total_exec_time_ms: 12.0,
            mean_exec_time_ms: 12.0 / 7.0,
            max_exec_time_ms: 3.0,
            rows: 9,
            shared_blocks_read: 1,
            shared_blocks_hit: 2,
            temporary_blocks_written: 3,
            wal_bytes: 4.0,
        };
        DatabaseQueryHealthSnapshot {
            statistics_observed_since: Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap(),
            deallocations: 0,
            track: StatementTracking::Top,
            utility_tracking: false,
            slow_query_logging_threshold_ms: None,
            bind_parameters_redacted: true,
            top_by_total_time: vec![entry.clone()],
            top_by_mean_time: vec![entry],
        }
    }

    #[test]
    fn hidden_unavailable_and_available_are_distinct_states() {
        assert!(database_query_health_section(None).is_empty());
        let unavailable =
            DatabaseQueryHealth::Unavailable(QueryHealthFailureCategory::ExtensionUnavailable);
        let html = database_query_health_section(Some(&unavailable));
        assert!(html.contains("Query statistics unavailable"));
        assert!(html.contains("extension unavailable"));
    }

    #[test]
    fn normalized_sql_is_escaped_and_truncation_is_visible() {
        let script_start = concat!("<scr", "ipt>");
        let query = format!("SELECT '&{script_start}' WHERE secret = $1");
        let health = DatabaseQueryHealth::Available(snapshot(&query, true));
        let html = database_query_health_section(Some(&health));
        assert!(!html.contains(script_start));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("secret = $1"));
        assert!(!html.contains("customer-secret-bind-value"));
        assert!(html.contains("truncated to the 16 KiB"));
        assert!(html.contains("Query 42"));
    }

    #[test]
    fn deallocation_and_unsafe_logging_configuration_warn() {
        let mut snapshot = snapshot("SELECT $1", false);
        snapshot.deallocations = 3;
        snapshot.slow_query_logging_threshold_ms = Some(200);
        snapshot.bind_parameters_redacted = false;
        let health = DatabaseQueryHealth::Available(snapshot);
        let html = database_query_health_section(Some(&health));
        assert!(html.contains("3 statement entries were deallocated"));
        assert!(html.contains("bind-parameter redaction is unsafe"));
    }
}
