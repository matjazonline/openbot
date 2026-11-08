use super::preparation_test_server::{Server, Stage};
use super::*;
use crate::application::services::mcp_client::*;
use chrono::Utc;
use tokio_util::sync::CancellationToken;
struct Allowed;
#[async_trait]
impl McpRequestAuthority for Allowed {
    async fn check(&self) -> McpPreparationResult<()> {
        Ok(())
    }
}
fn work(cancel: CancellationToken, millis: i64) -> McpPreparation {
    McpPreparation::new(
        Utc::now() + chrono::Duration::milliseconds(millis),
        cancel,
        Arc::new(Allowed),
    )
}
pub(crate) fn client(server: &Server) -> Arc<HttpMcpClient> {
    Arc::new(HttpMcpClient::new(
        EndpointPolicy::new([server.url.clone()]).unwrap(),
    ))
}

#[tokio::test]
async fn integration_gateway_native_mcp_cancellation_and_deadline_join_held_initialization() {
    for deadline in [false, true] {
        let stage = Stage::new();
        let server = Server::new(Some(stage.clone())).await;
        let client = client(&server);
        let cancel = CancellationToken::new();
        let preparation = work(cancel.clone(), if deadline { 500 } else { 20_000 });
        let endpoint = server.url.clone().try_into().unwrap();
        let provider = client.clone();
        let task = tokio::spawn(async move {
            provider
                .connect_prepared(
                    &endpoint,
                    SecretString::from("private-test-token"),
                    preparation,
                )
                .await
        });
        stage.observed().await;
        if !deadline {
            cancel.cancel();
        }
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        let Err(error) = result else {
            panic!("held initialization must end before any session is returned");
        };
        assert_eq!(
            error,
            if deadline {
                McpPreparationFailure::Deadline
            } else {
                McpPreparationFailure::Cancelled
            }
        );
        // Return before releasing the response proves actual pending body work was cancelled.
        stage.release();
        tokio::time::sleep(Duration::from_millis(50)).await;
        client.shutdown().await.unwrap();
        assert_eq!(server.counts().initialized, 0);
        assert_eq!(server.counts().catalogs, 0);
        assert_eq!(server.counts().calls, 0);
        assert!(client.workers.lock().await.is_empty());
        server.close().await;
    }
}

#[tokio::test]
async fn integration_gateway_native_mcp_abandonment_and_shutdown_join_initialization() {
    for abandon in [false, true] {
        let stage = Stage::new();
        let server = Server::new(Some(stage.clone())).await;
        let client = client(&server);
        let endpoint = server.url.clone().try_into().unwrap();
        let provider = client.clone();
        let task = tokio::spawn(async move {
            provider
                .connect(&endpoint, Some(SecretString::from("private-test-token")))
                .await
        });
        stage.observed().await;
        if abandon {
            task.abort();
        }
        tokio::time::timeout(Duration::from_secs(2), client.shutdown())
            .await
            .unwrap()
            .unwrap();
        assert!(task.await.is_err() || !abandon);
        stage.release();
        assert_eq!(server.counts().initialized, 0);
        assert_eq!(server.counts().calls, 0);
        assert!(client.workers.lock().await.is_empty());
        server.close().await;
    }
}

#[tokio::test]
async fn integration_gateway_native_mcp_competing_admission_cancel_releases_only_its_claim() {
    let server = Server::new(None).await;
    let client = client(&server);
    let held = client
        .capacity
        .clone()
        .acquire_many_owned(32)
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let first = work(cancel.clone(), 20_000);
    let second = work(CancellationToken::new(), 20_000);
    let a = client.clone();
    let endpoint: crate::entities::mcp::McpEndpoint = server.url.clone().try_into().unwrap();
    let first_endpoint = endpoint.clone();
    let left = tokio::spawn(async move {
        a.connect_prepared(&first_endpoint, SecretString::from("one"), first)
            .await
    });
    let b = client.clone();
    let right = tokio::spawn(async move {
        b.connect_prepared(&endpoint, SecretString::from("two"), second)
            .await
    });
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), left)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(McpPreparationFailure::Cancelled)));
    assert_eq!(server.counts().initialize, 0);
    drop(held);
    let session = tokio::time::timeout(Duration::from_secs(2), right)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    session.close().await.unwrap();
    client.shutdown().await.unwrap();
    assert_eq!(server.counts().initialize, 1);
    assert_eq!(server.counts().initialized, 1);
    assert_eq!(server.counts().deletes, 1);
    assert_eq!(client.capacity.available_permits(), 32);
    server.close().await;
}

pub(crate) async fn hold_admission(client: &HttpMcpClient) -> OwnedSemaphorePermit {
    client
        .capacity
        .clone()
        .acquire_many_owned(32)
        .await
        .unwrap()
}
