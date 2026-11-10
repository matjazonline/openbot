use super::*;
use crate::{
    adapters::mcp::{preparation_test_server::Server, preparation_tests::client},
    application::services::mcp_client::McpClient,
};
use secrecy::SecretString;

const SENTINEL: &str = "sdk-private-credential-echo-sentinel";

#[tokio::test]
async fn integration_service_contract_native_sdk_logs_cannot_bypass_production_filter() {
    for sse in [false, true] {
        let raw = Box::pin(captured(false, sse)).await;
        // Negative control: the real pinned SDK formats echoed peer information before
        // application response redaction. This fixture contains only a synthetic secret.
        assert!(
            raw.contains(SENTINEL),
            "the control must exercise the SDK diagnostic"
        );
        let protected = Box::pin(captured(true, sse)).await;
        assert!(!protected.contains(SENTINEL));
        assert!(protected.contains("integration.fixture_complete"));
    }
}

async fn captured(protected: bool, sse: bool) -> String {
    let output = Arc::new(Mutex::new(Vec::new()));
    let sink = output.clone();
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new("trace,rmcp::service=trace"))
        .with(tracing_subscriber::filter::filter_fn(move |metadata| {
            !protected || permitted_log_target(metadata)
        }))
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(move || Capture(sink.clone())),
        );
    let guard = tracing::subscriber::set_default(subscriber);
    let server = Server::with_server_info(SENTINEL, sse).await;
    let client = client(&server);
    let endpoint = server.url.clone().try_into().unwrap();
    let session = client
        .connect(&endpoint, Some(SecretString::from(SENTINEL)))
        .await
        .unwrap();
    assert!(session.discover().await.unwrap().is_empty());
    session.close().await.unwrap();
    client.shutdown().await.unwrap();
    assert_eq!(server.counts().initialize, 1);
    assert_eq!(server.counts().catalogs, 1);
    assert_eq!(server.counts().deletes, 1);
    server.close().await;
    tracing::info!(event = "integration.fixture_complete", outcome = "ok");
    drop(guard);
    String::from_utf8(output.lock().unwrap().clone()).unwrap()
}
