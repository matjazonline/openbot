use super::*;
use crate::{
    adapters::http::HttpSettings, application::test_store::static_store, infra::config::Config,
};
use std::{collections::HashMap, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn config(pairs: &[(&str, &str)]) -> Config {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Config::from_lookup(|key| map.get(key).cloned()).unwrap()
}

async fn get(address: std::net::SocketAddr, path: &str) -> Option<u16> {
    let mut stream = tokio::net::TcpStream::connect(address).await.ok()?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).await.ok()?;
    response.split_whitespace().nth(1)?.parse().ok()
}

#[tokio::test]
async fn memory_startup_unreachable_database_fails_explicitly_without_leaking_the_url() {
    let config = config(&[
        (
            "MEMORY_DATABASE_URL",
            "postgres://user:secret-password@127.0.0.1:1/memory",
        ),
        ("MEMORY_DB_CONNECT_TIMEOUT_MS", "2000"),
        ("MEMORY_DB_ACQUIRE_TIMEOUT_MS", "1000"),
    ]);
    let started = std::time::Instant::now();
    let error = serve(config, CancellationToken::new())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("cannot connect to the memory database"),
        "{error}"
    );
    assert!(!error.contains("secret-password"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn memory_startup_readiness_reflects_the_store_and_shutdown() {
    let (mut store, _) = static_store();
    store.available = false;
    let app = App::new(
        Arc::new(store),
        crate::application::test_embedder::fake_pool(),
    );
    let config = config(&[
        ("MEMORY_DATABASE_URL", "postgres://u@localhost/m"),
        ("MEMORY_SHUTDOWN_GRACE_MS", "2000"),
    ]);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let server = tokio::spawn({
        let shutdown = shutdown.clone();
        async move { serve_app(app, &config, listener, shutdown).await }
    });
    assert_eq!(get(address, "/healthz").await, Some(200));
    assert_eq!(
        get(address, "/readyz").await,
        Some(503),
        "an unavailable store is not ready"
    );
    shutdown.cancel();
    let stopped = tokio::time::timeout(Duration::from_secs(5), server).await;
    assert!(
        stopped
            .expect("graceful shutdown completes within the grace period")
            .unwrap()
            .is_ok()
    );
    assert_eq!(
        get(address, "/healthz").await,
        None,
        "the listener is closed after shutdown"
    );
}

#[tokio::test]
async fn memory_startup_ready_when_store_reachable_and_cancellation_is_bounded() {
    let (store, _) = static_store();
    let app = App::new(
        Arc::new(store),
        crate::application::test_embedder::fake_pool(),
    );
    let mut config = config(&[("MEMORY_DATABASE_URL", "postgres://u@localhost/m")]);
    config.shutdown_grace = Duration::from_millis(200);
    config.http = HttpSettings {
        max_in_flight: 4,
        ..config.http.clone()
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let server = tokio::spawn({
        let shutdown = shutdown.clone();
        async move { serve_app(app, &config, listener, shutdown).await }
    });
    assert_eq!(get(address, "/readyz").await, Some(200));
    // An idle keep-alive connection must not hold shutdown beyond the grace period.
    let _idle = tokio::net::TcpStream::connect(address).await.unwrap();
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(3), server)
        .await
        .expect("shutdown is bounded by the grace period")
        .unwrap()
        .unwrap();
}

/// A missing or modified model fails startup explicitly; there is no keyword-only mode.
#[tokio::test]
async fn memory_startup_missing_model_fails_explicitly() {
    let empty = std::env::temp_dir().join(format!("memory-no-model-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&empty).unwrap();
    let config = config(&[
        ("MEMORY_DATABASE_URL", "postgres://u@localhost/m"),
        ("MEMORY_MODEL_DIR", empty.to_str().unwrap()),
    ]);
    let error = format!("{:#}", load_embeddings(&config).await.err().unwrap());
    assert!(error.contains("embedding model is unavailable"), "{error}");
    assert!(error.contains("prepare-model.py"), "{error}");
    std::fs::remove_dir_all(empty).unwrap();
    assert!(
        Config::from_lookup(|key| match key {
            "MEMORY_DATABASE_URL" => Some("postgres://u@localhost/m".into()),
            "MEMORY_MODEL_DIR" => Some(" ".into()),
            _ => None,
        })
        .is_err()
    );
}
