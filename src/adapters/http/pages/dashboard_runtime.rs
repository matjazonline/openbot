//! Operator-only machine panels kept separate from the already substantial task dashboard.

use super::{
    chart::{self, ChartKind, Series, TimeChart, YUnit},
    dashboard::{
        DashboardPage, Stat, chart_footer, panel, process_panel, relative_time, stat, stat_row,
    },
};
use crate::entities::{
    dashboard::DashboardWindow,
    runtime_metrics::{RuntimeMetricSnapshot, TASK_WORKER_RECOMMENDATION_POLICY},
};

/// Deployment-wide information is rendered only when the route supplied an operator-authorized
/// snapshot. An empty snapshot still renders the section and its em dashes; `None` means hidden.
pub(super) fn machine_section(page: &DashboardPage<'_>) -> String {
    let Some(runtime) = page.runtime else {
        return String::new();
    };
    let current = runtime.current.as_ref();
    let rss = current.and_then(|sample| sample.process_rss_bytes);
    let limit = current.and_then(|sample| sample.memory_limit_bytes);
    let usage = match (rss, limit.filter(|limit| *limit > 0)) {
        (Some(rss), Some(limit)) => format!("{:.0}% of limit", rss as f64 * 100.0 / limit as f64),
        _ => "limit context unavailable".to_string(),
    };
    let region = page
        .machine
        .region
        .as_ref()
        .map_or("—".to_string(), ToString::to_string);
    let freshness = current
        .map(|sample| relative_time(sample.sampled_at))
        .unwrap_or_else(|| "—".to_string());

    let process = page.process.map(process_panel).unwrap_or_default();
    format!(
        r##"<div>
            <h2 class="mb-1 text-lg font-semibold">This machine</h2>
            <p class="mb-3 text-sm opacity-60">
                Deployment infrastructure for the machine serving this connection. Not company-scoped.
            </p>
            <div class="flex flex-col gap-3">
                {identity}
                {memory}
                {task_workers}
                {cpu}
                {database}
                {process}
            </div>
        </div>"##,
        identity = stat_row(
            "Machine",
            &format!(
                "{id}{region}{freshness}",
                id = stat(Stat::new(
                    "Machine ID",
                    page.machine.id.as_str(),
                    "serving this page"
                )),
                region = stat(Stat::new("Region", &region, "runtime region")),
                freshness = stat(Stat::new("Last sample", &freshness, "10-second sampler")),
            ),
        ),
        memory = stat_row(
            "Memory",
            &format!(
                "{current}{peak}{limit}",
                current = stat(Stat::new("RSS now", &bytes(rss), &usage)),
                peak = stat(Stat::new(
                    "Peak RSS",
                    &bytes(runtime.peak_rss_bytes),
                    page.window.label(),
                )),
                limit = stat(Stat::new(
                    "Memory limit",
                    &bytes(limit),
                    "detected cgroup limit"
                )),
            ),
        ),
        task_workers = task_worker_capacity(runtime),
        cpu = runtime_cpu_panel(runtime, page.window),
        database = runtime_database_panel(runtime, page.window),
    )
}

fn task_worker_capacity(runtime: &RuntimeMetricSnapshot) -> String {
    let current = runtime.current.as_ref();
    let active = current
        .map(|sample| sample.active_task_executions.to_string())
        .unwrap_or_else(|| "—".to_string());
    let configured = current
        .map(|sample| sample.task_worker_concurrency_limit.to_string())
        .unwrap_or_else(|| "—".to_string());
    let suggested = runtime
        .suggested_task_worker_concurrency
        .as_ref()
        .map(|recommendation| recommendation.maximum.to_string())
        .unwrap_or_else(|| "—".to_string());
    let supporting_samples = runtime
        .suggested_task_worker_concurrency
        .as_ref()
        .map(|recommendation| recommendation.supporting_samples.to_string())
        .unwrap_or_else(|| "—".to_string());
    let evidence_caption = if runtime.suggested_task_worker_concurrency.is_some() {
        "complete samples at that level".to_string()
    } else {
        format!(
            "needs {}h and {} samples per level",
            TASK_WORKER_RECOMMENDATION_POLICY.window_hours,
            TASK_WORKER_RECOMMENDATION_POLICY.minimum_samples_per_level,
        )
    };

    format!(
        "{row}{criteria}",
        row = stat_row(
            "Task worker capacity",
            &format!(
                "{active}{configured}{suggested}{evidence}",
                active = stat(Stat::new("Active tasks", &active, "executing now")),
                configured = stat(Stat::new(
                    "Configured limit",
                    &configured,
                    "TASK_WORKER_CONCURRENCY",
                )),
                suggested = stat(Stat::new(
                    "Suggested maximum",
                    &suggested,
                    "safe observed TASK_WORKER_CONCURRENCY",
                )),
                evidence = stat(Stat::new(
                    "Evidence",
                    &supporting_samples,
                    &evidence_caption,
                )),
            ),
        ),
        criteria = chart_footer(&format!(
            "Uses {hours}h p95: CPU <{cpu:.0}%, steal + throttle <{pressure:.0}%, RSS <{rss:.0}% of limit, database acquire <{database:.0}ms, and {reserved} pool connections reserved",
            hours = TASK_WORKER_RECOMMENDATION_POLICY.window_hours,
            cpu = TASK_WORKER_RECOMMENDATION_POLICY.cpu_p95_percent,
            pressure = TASK_WORKER_RECOMMENDATION_POLICY.cpu_pressure_p95_percent,
            rss = TASK_WORKER_RECOMMENDATION_POLICY.rss_p95_percent,
            database = TASK_WORKER_RECOMMENDATION_POLICY.database_acquire_p95_ms,
            reserved = TASK_WORKER_RECOMMENDATION_POLICY.reserved_pool_connections,
        )),
    )
}

fn runtime_cpu_panel(runtime: &RuntimeMetricSnapshot, window: DashboardWindow) -> String {
    let measured = runtime.buckets.iter().any(|bucket| {
        bucket.cpu_utilization_percent.is_some()
            || bucket.cpu_steal_percent.is_some()
            || bucket.cpu_throttle_percent.is_some()
    });
    if !measured {
        return panel(
            "CPU utilization, steal and throttling",
            r##"<div class="px-4 py-6 text-sm opacity-60">— Host CPU counters are unavailable or need a second sample.</div>"##,
        );
    }

    let series = [
        Series {
            label: "utilization",
            color: "var(--color-primary)",
            values: runtime
                .buckets
                .iter()
                .map(|bucket| bucket.cpu_utilization_percent)
                .collect(),
        },
        Series {
            label: "steal",
            color: "var(--color-warning)",
            values: runtime
                .buckets
                .iter()
                .map(|bucket| bucket.cpu_steal_percent)
                .collect(),
        },
        Series {
            label: "throttle",
            color: "var(--color-error)",
            values: runtime
                .buckets
                .iter()
                .map(|bucket| bucket.cpu_throttle_percent)
                .collect(),
        },
    ];
    let buckets: Vec<_> = runtime.buckets.iter().map(|bucket| bucket.bucket).collect();
    panel(
        "CPU utilization, steal and throttling",
        &format!(
            "{chart}{footer}",
            chart = chart::time_chart(&TimeChart {
                buckets: &buckets,
                series: &series,
                kind: ChartKind::Line,
                unit: YUnit::Percent,
                tick_format: window.tick_format(),
            }),
            footer = chart_footer(
                "Steal may represent quota throttling or contention with work on the host",
            ),
        ),
    )
}

fn runtime_database_panel(runtime: &RuntimeMetricSnapshot, window: DashboardWindow) -> String {
    let current = runtime.current.as_ref();
    let pool_value = current
        .map(|sample| format!("{} / {}", sample.pool_active, sample.pool_size))
        .unwrap_or_else(|| "—".to_string());
    let pool_caption = current
        .map(|sample| format!("active · {} idle", sample.pool_idle))
        .unwrap_or_else(|| "pool usage unavailable".to_string());
    let acquire = current
        .map(|sample| format!("{:.1} ms", sample.database_acquire_duration_ms))
        .unwrap_or_else(|| "—".to_string());
    let acquire_caption = current.map_or("no sample", |sample| {
        if sample.database_acquire_succeeded {
            "acquired"
        } else {
            "acquisition failed"
        }
    });
    let summary = stat_row(
        "Database pool",
        &format!(
            "{pool}{acquire}",
            pool = stat(Stat::new("Pool active", &pool_value, &pool_caption)),
            acquire = stat(Stat::new("Latest acquire", &acquire, acquire_caption)),
        ),
    );
    let measured = runtime
        .buckets
        .iter()
        .any(|bucket| bucket.database_acquire_p50_ms.is_some());
    if !measured {
        return format!(
            "{summary}{}",
            panel(
                "Database acquire latency",
                r##"<div class="px-4 py-6 text-sm opacity-60">— No acquisition probe has been persisted yet.</div>"##,
            )
        );
    }

    let series = [
        Series {
            label: "p95",
            color: "var(--color-primary)",
            values: runtime
                .buckets
                .iter()
                .map(|bucket| bucket.database_acquire_p95_ms)
                .collect(),
        },
        Series {
            label: "p50",
            color: "color-mix(in oklab, var(--color-primary) 60%, var(--color-base-100))",
            values: runtime
                .buckets
                .iter()
                .map(|bucket| bucket.database_acquire_p50_ms)
                .collect(),
        },
    ];
    let buckets: Vec<_> = runtime.buckets.iter().map(|bucket| bucket.bucket).collect();
    format!(
        "{summary}{}",
        panel(
            "Database acquire latency",
            &format!(
                "{chart}{footer}",
                chart = chart::time_chart(&TimeChart {
                    buckets: &buckets,
                    series: &series,
                    kind: ChartKind::Line,
                    unit: YUnit::Millis,
                    tick_format: window.tick_format(),
                }),
                footer = chart_footer(
                    "Periodic pool-contention probe; this is not the duration of every query",
                ),
            ),
        )
    )
}

fn bytes(value: Option<i64>) -> String {
    let Some(value) = value else {
        return "—".to_string();
    };
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * MIB;
    if value as f64 >= GIB {
        format!("{:.1} GiB", value as f64 / GIB)
    } else {
        format!("{:.0} MiB", value as f64 / MIB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::runtime_metrics::{
        MachineId, MachineIdentity, RuntimeMetricSample, TaskWorkerConcurrencyRecommendation,
    };
    use chrono::Utc;

    #[test]
    fn worker_capacity_outputs_the_configured_and_safe_observed_maximum() {
        let snapshot = RuntimeMetricSnapshot {
            current: Some(RuntimeMetricSample {
                identity: MachineIdentity {
                    id: MachineId::new("machine"),
                    region: None,
                },
                sampled_at: Utc::now(),
                process_rss_bytes: Some(1),
                memory_limit_bytes: Some(2),
                cpu_utilization_percent: Some(10.0),
                cpu_steal_percent: Some(1.0),
                cpu_throttle_percent: Some(1.0),
                active_task_executions: 2,
                task_worker_concurrency_limit: 4,
                database_acquire_duration_ms: 5.0,
                database_acquire_succeeded: true,
                pool_size: 10,
                pool_idle: 7,
                pool_active: 3,
            }),
            suggested_task_worker_concurrency: Some(TaskWorkerConcurrencyRecommendation {
                maximum: 3,
                supporting_samples: 120,
            }),
            ..RuntimeMetricSnapshot::default()
        };

        let html = task_worker_capacity(&snapshot);
        assert!(html.contains("Configured limit"), "{html}");
        assert!(html.contains(">4<"), "{html}");
        assert!(html.contains("Suggested maximum"), "{html}");
        assert!(html.contains(">3<"), "{html}");
        assert!(html.contains(">120<"), "{html}");
        assert!(html.contains("CPU &lt;80%"), "criteria are escaped: {html}");
    }
}
