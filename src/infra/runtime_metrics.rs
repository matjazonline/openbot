use std::{fs, path::PathBuf, time::Instant};

use chrono::Utc;

use crate::{
    entities::runtime_metrics::{MachineIdentity, RuntimeMetricObservation},
    services::runtime_metrics::{
        ActiveTaskExecutions, MemoryProviderActivity, RuntimeMetricSource,
    },
};

const PROC_STATUS: &str = "/proc/self/status";
const PROC_STAT: &str = "/proc/stat";
const CGROUP_V2_MEMORY_MAX: &str = "/sys/fs/cgroup/memory.max";
const CGROUP_V1_MEMORY_LIMIT: &str = "/sys/fs/cgroup/memory/memory.limit_in_bytes";
const CGROUP_V2_CPU_STAT: &str = "/sys/fs/cgroup/cpu.stat";
const CGROUP_V1_CPU_STAT: &str = "/sys/fs/cgroup/cpu/cpu.stat";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CpuCounters {
    total: u64,
    idle: u64,
    steal: u64,
}

#[derive(Debug, Clone)]
struct RuntimeMetricPaths {
    process_status: PathBuf,
    proc_stat: PathBuf,
    memory_limits: Vec<PathBuf>,
    cpu_stats: Vec<PathBuf>,
}

impl Default for RuntimeMetricPaths {
    fn default() -> Self {
        Self {
            process_status: PROC_STATUS.into(),
            proc_stat: PROC_STAT.into(),
            memory_limits: vec![CGROUP_V2_MEMORY_MAX.into(), CGROUP_V1_MEMORY_LIMIT.into()],
            cpu_stats: vec![CGROUP_V2_CPU_STAT.into(), CGROUP_V1_CPU_STAT.into()],
        }
    }
}

/// Linux-backed collector. Every read is optional so development on macOS and unsupported cgroup
/// layouts still produce database/pool samples with unavailable host values.
pub struct LinuxRuntimeMetricSource {
    identity: MachineIdentity,
    paths: RuntimeMetricPaths,
    previous_cpu: Option<CpuCounters>,
    previous_throttle: Option<(u64, Instant)>,
    active_task_executions: ActiveTaskExecutions,
    task_worker_concurrency_limit: i32,
    hydradb: MemoryProviderActivity,
}

impl LinuxRuntimeMetricSource {
    pub fn new(
        identity: MachineIdentity,
        active_task_executions: ActiveTaskExecutions,
        task_worker_concurrency_limit: usize,
        hydradb: MemoryProviderActivity,
    ) -> Self {
        Self {
            identity,
            paths: RuntimeMetricPaths::default(),
            previous_cpu: None,
            previous_throttle: None,
            active_task_executions,
            hydradb,
            task_worker_concurrency_limit: i32::try_from(task_worker_concurrency_limit)
                .expect("task worker concurrency fits an i32"),
        }
    }

    fn read_first(paths: &[PathBuf]) -> Option<String> {
        paths.iter().find_map(|path| fs::read_to_string(path).ok())
    }
}

impl RuntimeMetricSource for LinuxRuntimeMetricSource {
    fn observe(&mut self) -> RuntimeMetricObservation {
        let process_rss_bytes = fs::read_to_string(&self.paths.process_status)
            .ok()
            .and_then(|contents| parse_process_rss_bytes(&contents));
        let memory_limit_bytes = Self::read_first(&self.paths.memory_limits)
            .as_deref()
            .and_then(parse_memory_limit_bytes);

        let current_cpu = fs::read_to_string(&self.paths.proc_stat)
            .ok()
            .and_then(|contents| parse_proc_stat(&contents));
        let (cpu_utilization_percent, cpu_steal_percent) =
            cpu_percentages(self.previous_cpu, current_cpu);
        self.previous_cpu = current_cpu;

        let now = Instant::now();
        let current_throttle = Self::read_first(&self.paths.cpu_stats)
            .as_deref()
            .and_then(parse_throttled_microseconds);
        let cpu_throttle_percent =
            throttle_percentage(self.previous_throttle, current_throttle, now);
        self.previous_throttle = current_throttle.map(|counter| (counter, now));

        RuntimeMetricObservation {
            identity: self.identity.clone(),
            sampled_at: Utc::now(),
            process_rss_bytes,
            memory_limit_bytes,
            cpu_utilization_percent,
            cpu_steal_percent,
            cpu_throttle_percent,
            active_task_executions: i32::try_from(self.active_task_executions.current())
                .unwrap_or(self.task_worker_concurrency_limit)
                .min(self.task_worker_concurrency_limit),
            task_worker_concurrency_limit: self.task_worker_concurrency_limit,
            hydradb: self.hydradb.drain(),
        }
    }
}

fn parse_process_rss_bytes(contents: &str) -> Option<i64> {
    let line = contents.lines().find(|line| line.starts_with("VmRSS:"))?;
    let mut fields = line.split_whitespace();
    (fields.next()? == "VmRSS:").then_some(())?;
    let kibibytes = fields.next()?.parse::<i64>().ok()?;
    (fields.next()? == "kB").then_some(())?;
    kibibytes.checked_mul(1024)
}

fn parse_memory_limit_bytes(contents: &str) -> Option<i64> {
    let value = contents.trim();
    if value == "max" {
        return None;
    }
    value.parse::<i64>().ok().filter(|limit| *limit >= 0)
}

fn parse_proc_stat(contents: &str) -> Option<CpuCounters> {
    let fields: Vec<u64> = contents
        .lines()
        .find(|line| line.starts_with("cpu "))?
        .split_whitespace()
        .skip(1)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if fields.len() < 8 {
        return None;
    }

    Some(CpuCounters {
        // guest and guest_nice (fields 8 and 9) are already included in user and nice.
        total: fields[..8].iter().copied().sum(),
        idle: fields[3].checked_add(fields[4])?,
        steal: fields[7],
    })
}

fn cpu_percentages(
    previous: Option<CpuCounters>,
    current: Option<CpuCounters>,
) -> (Option<f64>, Option<f64>) {
    let (Some(previous), Some(current)) = (previous, current) else {
        return (None, None);
    };
    let Some(total) = current.total.checked_sub(previous.total) else {
        return (None, None);
    };
    let Some(idle) = current.idle.checked_sub(previous.idle) else {
        return (None, None);
    };
    let Some(steal) = current.steal.checked_sub(previous.steal) else {
        return (None, None);
    };
    if total == 0 || idle > total || steal > total {
        return (None, None);
    }

    (
        Some((total - idle) as f64 * 100.0 / total as f64),
        Some(steal as f64 * 100.0 / total as f64),
    )
}

fn parse_throttled_microseconds(contents: &str) -> Option<u64> {
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        match (fields.next(), fields.next()) {
            (Some("throttled_usec"), Some(value)) => return value.parse().ok(),
            // cgroup v1 reports nanoseconds.
            (Some("throttled_time"), Some(value)) => {
                return value
                    .parse::<u64>()
                    .ok()
                    .map(|nanoseconds| nanoseconds / 1000);
            }
            _ => {}
        }
    }
    None
}

fn throttle_percentage(
    previous: Option<(u64, Instant)>,
    current: Option<u64>,
    now: Instant,
) -> Option<f64> {
    let (Some((previous_counter, previous_at)), Some(current)) = (previous, current) else {
        return None;
    };
    let throttled = current.checked_sub(previous_counter)?;
    let elapsed_microseconds = now.duration_since(previous_at).as_micros();
    (elapsed_microseconds > 0).then(|| throttled as f64 * 100.0 / elapsed_microseconds as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::Duration};
    use uuid::Uuid;

    #[test]
    fn process_status_parser_reads_vmrss_in_bytes() {
        let fixture = "Name:\tmail_agents\nVmPeak:\t  999 kB\nVmRSS:\t  12345 kB\n";
        assert_eq!(parse_process_rss_bytes(fixture), Some(12_641_280));
        assert_eq!(parse_process_rss_bytes("VmRSS: nope kB"), None);
        assert_eq!(parse_process_rss_bytes("Name: mail_agents"), None);
    }

    #[test]
    fn proc_stat_parser_and_delta_handle_first_sample_and_reset() {
        let first = parse_proc_stat("cpu  100 10 30 400 20 5 5 10 0 0\ncpu0 1").unwrap();
        let second = parse_proc_stat("cpu  130 10 50 460 20 5 5 20 0 0").unwrap();
        assert_eq!(cpu_percentages(None, Some(first)), (None, None));
        let (busy, steal) = cpu_percentages(Some(first), Some(second));
        assert!((busy.unwrap() - 50.0).abs() < 0.001);
        assert!((steal.unwrap() - 8.333_333).abs() < 0.001);
        assert_eq!(cpu_percentages(Some(second), Some(first)), (None, None));
        assert_eq!(parse_proc_stat("cpu malformed"), None);
    }

    #[test]
    fn cgroup_parsers_accept_v1_and_v2_and_reject_malformed_values() {
        assert_eq!(
            parse_memory_limit_bytes("1073741824\n"),
            Some(1_073_741_824)
        );
        assert_eq!(parse_memory_limit_bytes("max\n"), None);
        assert_eq!(parse_memory_limit_bytes("many"), None);
        assert_eq!(
            parse_throttled_microseconds("nr_periods 2\nthrottled_usec 45\n"),
            Some(45)
        );
        assert_eq!(
            parse_throttled_microseconds("throttled_time 45000\n"),
            Some(45)
        );
        assert_eq!(parse_throttled_microseconds("throttled_usec nope\n"), None);
    }

    #[test]
    fn throttle_delta_is_unavailable_first_and_after_a_counter_reset() {
        let now = Instant::now();
        assert_eq!(throttle_percentage(None, Some(10), now), None);
        assert_eq!(
            throttle_percentage(Some((20, now)), Some(10), now + Duration::from_secs(1)),
            None
        );
        assert_eq!(
            throttle_percentage(Some((10, now)), Some(260_010), now + Duration::from_secs(1)),
            Some(26.0)
        );
    }

    #[test]
    fn missing_linux_files_leave_host_values_unavailable() {
        let missing = std::env::temp_dir().join(format!("runtime-metrics-{}", Uuid::new_v4()));
        fs::create_dir(&missing).unwrap();
        let mut source = LinuxRuntimeMetricSource {
            identity: MachineIdentity {
                id: crate::entities::runtime_metrics::MachineId::new("test"),
                region: None,
            },
            paths: RuntimeMetricPaths {
                process_status: missing.join("status"),
                proc_stat: missing.join("stat"),
                memory_limits: vec![missing.join("memory.max")],
                cpu_stats: vec![missing.join("cpu.stat")],
            },
            previous_cpu: None,
            previous_throttle: None,
            active_task_executions: ActiveTaskExecutions::default(),
            task_worker_concurrency_limit: 4,
            hydradb: MemoryProviderActivity::default(),
        };

        let gauge = source.active_task_executions.clone();
        let _active = gauge.enter();
        let observation = source.observe();
        assert_eq!(observation.process_rss_bytes, None);
        assert_eq!(observation.memory_limit_bytes, None);
        assert_eq!(observation.cpu_utilization_percent, None);
        assert_eq!(observation.cpu_throttle_percent, None);
        assert_eq!(observation.active_task_executions, 1);
        assert_eq!(observation.task_worker_concurrency_limit, 4);
        assert_eq!(observation.hydradb, Default::default());

        fs::remove_dir(missing).unwrap();
    }
}
