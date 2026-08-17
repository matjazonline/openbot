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
