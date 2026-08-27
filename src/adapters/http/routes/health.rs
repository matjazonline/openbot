use std::time::Duration;

use axum::{Router, extract::State, response::Json, routing::get};
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::adapters::http::app_state::AppState;

/// Bounded well under the platform health check's own timeout: the pool's
/// acquire timeout is 10s, so an unbounded probe would outlive the check and
/// get the machine marked unhealthy for what is only a degraded dependency.
const DB_PROBE_TIMEOUT: Duration = Duration::from_secs(1);

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

/// Liveness probe. The status code is unconditionally 200 as long as the
/// process is serving: a database outage degrades this service rather than
/// killing it, so reporting it as dead would only make the platform recycle a
/// process that is recovering on its own. The database state is reported in
/// the body instead, for observability.
async fn health(State(db): State<PgPool>) -> Json<Value> {
    let database = match tokio::time::timeout(DB_PROBE_TIMEOUT, probe_db(&db)).await {
        Ok(Ok(())) => "up",
        Ok(Err(err)) => {
            tracing::warn!("health check database probe failed: {err}");
            "down"
        }
        Err(_) => {
            tracing::warn!("health check database probe timed out");
            "down"
        }
    };

    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "database": database,
    }))
}

async fn probe_db(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(db).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// A closed local port stands in for a stopped/unreachable database
    /// machine: the probe must report failure quickly rather than hanging
    /// out to the pool's 10s+ connect/acquire timeouts.
    #[tokio::test]
    async fn probe_reports_down_promptly_when_database_is_unreachable() {
        let pool = PgPool::connect_lazy("postgres://user:pass@127.0.0.1:1/mail_agents_test")
            .expect("lazy pool construction never touches the network");

        let start = Instant::now();
        let result = tokio::time::timeout(DB_PROBE_TIMEOUT, probe_db(&pool)).await;
        let elapsed = start.elapsed();

        assert!(
            matches!(result, Ok(Err(_)) | Err(_)),
            "expected the probe to fail against an unreachable database"
        );
        assert!(
            elapsed < DB_PROBE_TIMEOUT + Duration::from_millis(500),
            "probe took {elapsed:?}, expected it bounded near {DB_PROBE_TIMEOUT:?}"
        );
    }
}
