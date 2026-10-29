use super::*;
use std::{io::Write, sync::Mutex};

struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn configured_filter(config: &str) -> &str {
    config
        .lines()
        .find_map(|line| {
            let (key, value) = line.trim().split_once('=')?;
            (key.trim() == "RUST_LOG").then(|| value.trim().trim_matches('"'))
        })
        .expect("supplied configuration includes RUST_LOG")
}

#[test]
fn security_session_metrics_export_under_startup_fly_and_example_filters() {
    for filter in [
        DEFAULT_LOG_FILTER,
        configured_filter(include_str!("../../fly.toml")),
        configured_filter(include_str!("../../.env.example")),
    ] {
        let output = Arc::new(Mutex::new(Vec::new()));
        let sink = output.clone();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new(filter))
            .with(
                fmt::layer()
                    .without_time()
                    .with_ansi(false)
                    .with_target(false)
                    .with_writer(move || Capture(sink.clone())),
            );
        tracing::subscriber::with_default(subscriber, || {
            let monitor = TracingMonitor::new();
            monitor.increment_counter(
                "session_validation_failures",
                1,
                &[("reason", "store_unavailable")],
            );
            monitor.record_gauge("session_cleanup_backlog", 1001.0, &[]);
            tracing::info!(target: "mail_agents::session_probe", "application-marker");
            tracing::info!(target: "monitoring::smtp", "excluded-smtp-marker");
            tracing::info!(target: "monitoring::ai", "excluded-ai-marker");
        });
        let captured = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        assert!(
            captured.contains("application-marker"),
            "{filter}: {captured}"
        );
        assert!(
            captured.contains("session_validation_failures"),
            "{filter}: {captured}"
        );
        assert!(
            captured.contains("store_unavailable"),
            "{filter}: {captured}"
        );
        assert!(
            captured.contains("session_cleanup_backlog"),
            "{filter}: {captured}"
        );
        assert!(!captured.contains("excluded-smtp-marker"));
        assert!(!captured.contains("excluded-ai-marker"));
    }
}
