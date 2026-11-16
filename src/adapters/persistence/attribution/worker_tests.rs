use super::*;
use crate::{
    adapters::http::{
        attribution::{Bridge, worker},
        session::SessionAuthority,
    },
    infra::{attribution::AttributionConfig, config::AppConfig},
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::mpsc};
use tokio_util::sync::CancellationToken;

fn bridge_for(f: &Fixture, origin: &str) -> Arc<Bridge> {
    let registration = AttributionConfig::from_lookup(|name| match name {
        "OPENBOT_ATTRIBUTION_ORIGIN" => Some(origin.into()),
        "OPENBOT_ATTRIBUTION_CLIENT_SECRET" => Some(URL_SAFE_NO_PAD.encode([91u8; 32])),
        "OPENBOT_ATTRIBUTION_LIFECYCLE_SECRET" => Some(URL_SAFE_NO_PAD.encode([92u8; 32])),
        _ => None,
    })
    .unwrap()
    .unwrap();
    let store = Arc::new(PostgresPersistence::new(f.store.pool.clone()));
    let sessions = Arc::new(SessionAuthority::new(&AppConfig::for_test(), store.clone()));
    Arc::new(Bridge::new(Arc::new(registration), store.clone(), store, sessions).unwrap())
}

#[derive(Clone, Default)]
struct LogOutput(Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for LogOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn attribution_company_lifecycle_operator_signal_survives_restart_and_resolves() {
    let f = Fixture::new().await;
    let output = LogOutput::default();
    let writer = output.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let company = f.company().await;
    let claim = f.store.claim().await.unwrap().pop().unwrap();
    f.store
        .complete(&claim, DeliveryResult::Unavailable)
        .await
        .unwrap();
    let restarted = bridge_for(&f, "http://127.0.0.1:9");
    let pending = worker::probe(&restarted).await.unwrap();
    assert!(pending.undelivered && !pending.failed);
    sqlx::query("UPDATE attribution_lifecycle_outbox SET available_at = clock_timestamp() WHERE company_id = $1")
        .bind(company).execute(&f.store.pool).await.unwrap();
    let retry = f.store.claim().await.unwrap().pop().unwrap();
    f.store
        .complete(&retry, DeliveryResult::Delivered)
        .await
        .unwrap();
    assert_eq!(
        worker::probe(&restarted).await.unwrap(),
        LifecycleHealth {
            undelivered: false,
            failed: false
        }
    );
    let poison = f.company().await;
    sqlx::query("UPDATE attribution_lifecycle_outbox SET attempts = 11 WHERE company_id = $1")
        .bind(poison)
        .execute(&f.store.pool)
        .await
        .unwrap();
    let final_attempt = f.store.claim().await.unwrap().pop().unwrap();
    f.store
        .complete(&final_attempt, DeliveryResult::Unavailable)
        .await
        .unwrap();
    let another_process = bridge_for(&f, "http://127.0.0.1:9");
    assert!(worker::probe(&another_process).await.unwrap().failed);
    let log = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
    assert!(
        log.contains("attribution_lifecycle_health")
            && log.contains("undelivered=true")
            && log.contains("failed=true")
    );
    assert!(
        log.contains("lifecycle delivery healthy")
            && log.contains("undelivered=false failed=false")
    );
}

#[derive(Clone)]
struct Receiver {
    seen: mpsc::Sender<serde_json::Value>,
    hold: Arc<AtomicBool>,
    release: CancellationToken,
    credential: String,
}
async fn receive(
    State(state): State<Receiver>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> StatusCode {
    assert_eq!(headers["authorization"], state.credential);
    state.seen.send(payload).await.unwrap();
    if state.hold.load(Ordering::Acquire) {
        state.release.cancelled().await;
    }
    StatusCode::OK
}

#[tokio::test]
async fn attribution_company_lifecycle_delivery_shutdown_and_lost_ack_recover_durably() {
    let f = Fixture::new().await;
    let company = f.company().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let lifecycle = URL_SAFE_NO_PAD.encode([92u8; 32]);
    let (seen, mut received) = mpsc::channel(4);
    let receiver = Receiver {
        seen,
        hold: Arc::new(AtomicBool::new(true)),
        release: CancellationToken::new(),
        credential: format!("Bearer {lifecycle}"),
    };
    let server_stop = CancellationToken::new();
    let stop = server_stop.clone();
    let app = Router::new()
        .route("/internal/v1/company-lifecycle", post(receive))
        .with_state(receiver.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
    });
    let bridge = bridge_for(&f, &origin);
    let cancellation = CancellationToken::new();
    let task = tokio::spawn(worker::iteration(bridge.clone(), cancellation.clone()));
    let first = tokio::time::timeout(Duration::from_secs(3), received.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first["company_id"], company.to_string());
    cancellation.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("actual HTTP work must cancel and join")
        .unwrap();
    let attempts: i32 = sqlx::query_scalar("SELECT attempts FROM attribution_lifecycle_outbox WHERE company_id = $1 AND delivery_state = 'delivering'")
        .bind(company).fetch_one(&f.store.pool).await.unwrap();
    assert_eq!(
        attempts, 1,
        "unacknowledged delivery must remain recoverable"
    );
    receiver.hold.store(false, Ordering::Release);
    receiver.release.cancel();
    sqlx::query("UPDATE attribution_lifecycle_outbox SET lease_until = clock_timestamp() - interval '1 second' WHERE company_id = $1")
        .bind(company).execute(&f.store.pool).await.unwrap();
    worker::iteration(bridge.clone(), CancellationToken::new()).await;
    assert_eq!(
        received.recv().await.unwrap(),
        first,
        "retry must preserve the immutable lifecycle version"
    );
    let attempts: i32 = sqlx::query_scalar("SELECT attempts FROM attribution_lifecycle_outbox WHERE company_id = $1 AND delivery_state = 'delivered'")
        .bind(company).fetch_one(&f.store.pool).await.unwrap();
    assert_eq!(attempts, 2);
    worker::iteration(bridge.clone(), CancellationToken::new()).await;
    assert!(received.try_recv().is_err());
    f.company().await;
    let stopped = CancellationToken::new();
    stopped.cancel();
    worker::run(bridge, stopped).await;
    let untouched: i64 = sqlx::query_scalar("SELECT count(*) FROM attribution_lifecycle_outbox WHERE delivery_state = 'pending' AND attempts = 0")
        .fetch_one(&f.store.pool).await.unwrap();
    assert_eq!(
        untouched, 1,
        "shutdown between iterations admits no new work"
    );
    server_stop.cancel();
    server.await.unwrap().unwrap();
}
