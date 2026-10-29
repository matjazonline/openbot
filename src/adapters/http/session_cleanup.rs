use super::*;

const CLEANUP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

impl SessionAuthority {
    /// Owned and joined by serve; cancellation drops an active cleanup transaction before exit.
    pub async fn run_cleanup(self: Arc<Self>, mut shutdown: tokio::sync::broadcast::Receiver<()>) {
        let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = shutdown.recv() => break,
                _ = interval.tick() => {}
            }
            let result = tokio::select! {
                biased;
                _ = shutdown.recv() => break,
                result = session_store(self.store.cleanup_sessions()) => result,
            };
            if let Some(monitoring) = &self.monitoring {
                match result {
                    Ok(batch) => {
                        monitoring.increment_counter("session_cleanup_removed", batch.removed, &[]);
                        monitoring.record_gauge(
                            "session_cleanup_backlog",
                            batch.backlog as f64,
                            &[],
                        );
                        monitoring.record_gauge(
                            "session_cleanup_backlog_capped",
                            if batch.backlog > 1000 { 1.0 } else { 0.0 },
                            &[],
                        );
                        monitoring.record_gauge(
                            "session_cleanup_last_success_unix_seconds",
                            Utc::now().timestamp() as f64,
                            &[],
                        );
                    }
                    Err(_) => monitoring.increment_counter("session_cleanup_failures", 1, &[]),
                }
            } else if result.is_err() {
                tracing::warn!("Session cleanup unavailable");
            }
        }
    }
}

#[cfg(test)]
#[path = "session_cleanup_tests.rs"]
mod tests;
