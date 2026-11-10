//! In-process counters and the Prometheus text of `GET /admin/v1/metrics` (p06 gate A2-1).
//!
//! Counters are per process (sum them across instances); the gauges come from one bounded,
//! read-only database query at scrape time and are database-wide (read them from any one
//! instance). Every label value comes from a closed set fixed in code (an operation id, an error
//! code, a state, an outcome code, a category), never from a tenant, namespace or content.
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::AtomicI64;

/// Unix seconds of the last successful maintenance pass (0 before the first).
pub static LAST_MAINTENANCE_UNIX: AtomicI64 = AtomicI64::new(0);

/// A counter's name and its label pairs.
type CounterKey = (&'static str, Vec<(&'static str, String)>);

/// One gauge row of a scrape: name, help, labels, value.
pub type GaugeRow<'a> = (&'a str, &'a str, Vec<(&'a str, String)>, f64);

pub struct Metrics {
    counters: Mutex<BTreeMap<CounterKey, u64>>,
}

/// The process registry.
pub static METRICS: Metrics = Metrics {
    counters: Mutex::new(BTreeMap::new()),
};

/// Counter names and their one-line help (each is rendered with `# HELP`/`# TYPE`).
const HELP: &[(&str, &str)] = &[
    (
        "memory_requests_total",
        "Requests by registry operation (or unrouted) and outcome code",
    ),
    (
        "memory_generation_settled_total",
        "Finished or retried generation settlements by state and outcome code",
    ),
    (
        "memory_generation_dispatched_total",
        "Generation operations committed as dispatched",
    ),
    (
        "memory_generation_retries_total",
        "Definite provider rejections scheduled for another dispatch",
    ),
    (
        "memory_generation_lease_lost_total",
        "Generation leases lost, by reason",
    ),
    (
        "memory_generation_reaped_total",
        "Operations the reaper released, abandoned or expired",
    ),
    (
        "memory_provider_tokens_total",
        "Provider tokens reported by settled calls, by kind",
    ),
    (
        "memory_generation_usage_exceeded_reservation_total",
        "Settlements whose reported usage exceeded the reservation",
    ),
    ("memory_index_commits_total", "Indexer commits by result"),
    (
        "memory_search_semantic_partial_total",
        "Searches answered without the semantic leg",
    ),
    ("memory_context_packs_total", "Context packs assembled"),
    (
        "memory_context_omitted_total",
        "Candidates left out of context packs by the budget",
    ),
    (
        "memory_retention_rows_total",
        "Rows removed or nulled by maintenance, by category",
    ),
    (
        "memory_maintenance_errors_total",
        "Maintenance passes that failed",
    ),
    (
        "memory_provider_auth_failures_total",
        "Provider rejections of the service's credentials, by provider error type",
    ),
];

impl Metrics {
    pub fn inc(&self, name: &'static str, labels: &[(&'static str, &str)], by: u64) {
        if by == 0 {
            return;
        }
        let key = (
            name,
            labels
                .iter()
                .map(|(key, value)| (*key, (*value).to_owned()))
                .collect(),
        );
        if let Ok(mut counters) = self.counters.lock() {
            *counters.entry(key).or_insert(0) += by;
        }
    }

    pub fn get(&self, name: &'static str, labels: &[(&'static str, &str)]) -> u64 {
        let key = (
            name,
            labels
                .iter()
                .map(|(key, value)| (*key, (*value).to_owned()))
                .collect::<Vec<_>>(),
        );
        self.counters
            .lock()
            .ok()
            .and_then(|counters| counters.get(&key).copied())
            .unwrap_or(0)
    }

    /// The counters followed by `gauges`, in Prometheus text format 0.0.4.
    pub fn render(&self, gauges: &[GaugeRow<'_>]) -> String {
        let mut out = String::new();
        let counters = self
            .counters
            .lock()
            .map(|counters| counters.clone())
            .unwrap_or_default();
        for (name, help) in HELP {
            out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} counter\n"));
            for ((metric, labels), value) in &counters {
                if metric == name {
                    out.push_str(&format!("{name}{} {value}\n", render_labels(labels)));
                }
            }
        }
        let mut seen: Vec<&str> = Vec::new();
        for (name, help, labels, value) in gauges {
            if !seen.contains(name) {
                out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} gauge\n"));
                seen.push(name);
            }
            let labels: Vec<(&str, String)> = labels.clone();
            out.push_str(&format!("{name}{} {value}\n", render_labels(&labels)));
        }
        out
    }
}

fn render_labels<K: AsRef<str>>(labels: &[(K, String)]) -> String {
    if labels.is_empty() {
        return String::new();
    }
    let inner: Vec<String> = labels
        .iter()
        .map(|(key, value)| format!("{}=\"{}\"", key.as_ref(), value.replace('"', "")))
        .collect();
    format!("{{{}}}", inner.join(","))
}

/// The database-wide gauges of one scrape (06A.3): queue depth and age, indexing, unknown
/// outcomes, fairness and maintenance freshness. `None` when the bounded query failed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Gauges {
    pub accepted: i64,
    pub dispatched: i64,
    pub oldest_accepted_seconds: f64,
    pub unknown_last_day: i64,
    pub tenants_waiting: i64,
    /// The most accepted operations any one tenant has queued (fairness: claims rotate across
    /// tenants, so one tenant's backlog cannot hold the others back).
    pub max_tenant_queue: i64,
    pub index_pending: i64,
    pub oldest_index_pending_seconds: f64,
    pub index_failed: i64,
}
