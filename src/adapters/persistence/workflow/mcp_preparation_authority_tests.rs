//! Actual persisted business/configuration authority with the production MCP client.
//! Only the pinned test endpoint is mapped to an owned loopback peer; no live TLS/provider claim.
use super::*;
use crate::entities::mcp::McpEndpoint;
use crate::{
    adapters::mcp::{
        HttpMcpClient,
        preparation_test_server::{Server, Stage as Boundary},
        preparation_tests::{client as native_client, hold_admission},
    },
    application::services::mcp_client::{
        McpPreparation, McpPreparationFailure, McpPreparationResult, McpRequestAuthority,
        McpSession,
    },
};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug)]
enum Wait {
    Admission,
    AfterDns,
    Initialize,
    Connected,
}
struct PausedAuthority {
    inner: McpPreparation,
    boundary: Arc<Boundary>,
    first: AtomicBool,
}
#[async_trait::async_trait]
impl McpRequestAuthority for PausedAuthority {
    async fn check(&self) -> McpPreparationResult<()> {
        if !self.first.swap(true, Ordering::SeqCst) {
            self.boundary.wait().await;
        }
        self.inner.check().await
    }
}
struct CountedClient {
    inner: Arc<HttpMcpClient>,
    endpoint: McpEndpoint,
    wait: Wait,
    boundary: Arc<Boundary>,
    arrived: Arc<Boundary>,
}
#[async_trait::async_trait]
impl McpClient for CountedClient {
    fn validate_endpoint(&self, endpoint: &McpEndpoint) -> AppResult<()> {
        if endpoint.as_str() == "https://mcp.example.test/rpc" {
            Ok(())
        } else {
            Err(AppError::Forbidden("Unregistered fixture endpoint".into()))
        }
    }
    async fn shutdown(&self) -> AppResult<()> {
        self.inner.shutdown().await
    }
    async fn connect(
        &self,
        _: &McpEndpoint,
        _: Option<SecretString>,
    ) -> AppResult<Box<dyn McpSession>> {
        Err(AppError::Forbidden(
            "Fixture requires guarded preparation".into(),
        ))
    }
    async fn connect_prepared(
        &self,
        endpoint: &McpEndpoint,
        token: SecretString,
        work: McpPreparation,
    ) -> McpPreparationResult<Box<dyn McpSession>> {
        self.validate_endpoint(endpoint)
            .map_err(|_| McpPreparationFailure::Denied)?;
        let prepared = if matches!(self.wait, Wait::AfterDns) {
            McpPreparation::new(
                work.deadline,
                work.cancellation.clone(),
                Arc::new(PausedAuthority {
                    inner: work.clone(),
                    boundary: self.boundary.clone(),
                    first: AtomicBool::new(false),
                }),
            )
        } else {
            work.clone()
        };
        self.arrived.arrive();
        let session = self
            .inner
            .connect_prepared(&self.endpoint, token, prepared)
            .await?;
        if matches!(self.wait, Wait::Connected) {
            let remaining = (work.deadline - chrono::Utc::now())
                .to_std()
                .map_err(|_| McpPreparationFailure::Deadline)?;
            let failure = tokio::select! {biased;
                _=work.cancellation.cancelled()=>Some(McpPreparationFailure::Cancelled),
                _=tokio::time::sleep(remaining)=>Some(McpPreparationFailure::Deadline),
                _=self.boundary.wait()=>None,
            };
            if let Some(failure) = failure {
                let _ = session.close().await;
                return Err(failure);
            }
        }
        Ok(session)
    }
}

#[tokio::test]
async fn vault_workflow_mcp_actual_authority_is_rechecked_after_each_preparation_wait() {
    // Both genuine purposes, two independent revocations, four wait-to-request edges.
    for configuration in [false, true] {
        for disable in [false, true] {
            for wait in [
                Wait::Admission,
                Wait::AfterDns,
                Wait::Initialize,
                Wait::Connected,
            ] {
                Box::pin(race(configuration, disable, wait)).await;
            }
        }
    }
}
async fn race(configuration: bool, disable: bool, wait: Wait) {
    use crate::application::{
        integrations::management::AccountRepository, use_cases::mcp::McpIntegrationDiscovery,
    };
    let f = Box::pin(fixture(Delivery::Accepted)).await;
    let p = &f.setup.fixture.persistence;
    let boundary = Boundary::new();
    let arrived = Boundary::new();
    let server = Server::new(if matches!(wait, Wait::Initialize) {
        Some(boundary.clone())
    } else {
        None
    })
    .await;
    let native = native_client(&server);
    let held = if matches!(wait, Wait::Admission) {
        Some(hold_admission(&native).await)
    } else {
        None
    };
    let client = Arc::new(CountedClient {
        inner: native,
        endpoint: server.url.clone().try_into().unwrap(),
        wait,
        boundary: boundary.clone(),
        arrived: arrived.clone(),
    });
    let gateway = super::execution_support::gateway(&f.vault, client.clone());
    let native =
        crate::adapters::persistence::workflow::action_dispatch::PostgresActionDispatch::new(
            p.clone(),
            crate::adapters::persistence::workflow::mcp::McpActionAuthority,
            crate::adapters::workflow_http::NoLocal,
        );
    let dispatch = crate::adapters::integrations::workflow_dispatch::IntegrationActionDispatch::new(
        p.clone(),
        native,
        gateway.clone(),
    );
    let discovery = crate::adapters::persistence::PostgresMcpDiscovery {
        vault: f.vault.clone(),
        gateway,
    };
    let current = f.setup.current().await;
    let execute = async {
        if configuration {
            assert!(
                Box::pin(discovery.refresh(
                    f.setup.actor().user_id(),
                    VerifiedOwner::from_verified_client(f.account.owner.clone()),
                    &current
                ))
                .await
                .is_err(),
                "{wait:?}"
            );
        } else {
            let result = Box::pin(dispatch.remote(
                &f.request,
                &UnreachableProvider,
                &CancellationToken::new(),
            ))
            .await
            .unwrap();
            assert!(
                matches!(
                    result,
                    RemoteDispatchObservation::PreparationFailed(PreDispatchFailure::Denied)
                ),
                "{wait:?}: expected typed preparation denial"
            );
        }
    };
    let revoke = async {
        if matches!(wait, Wait::Admission) {
            arrived.observed().await;
        } else {
            boundary.observed().await;
        }
        if disable {
            p.update_mcp_connection(
                current.company_id,
                current.id,
                current.revision,
                crate::application::use_cases::mcp::McpConnectionWrite {
                    slug: current.slug.clone(),
                    endpoint: current.endpoint.clone(),
                    enabled: false,
                    auth: current.auth.clone(),
                    discovered_tools: current.discovered_tools.clone(),
                    tool_grants: current.tool_grants.clone(),
                },
            )
            .await
            .unwrap();
        } else {
            f.vault
                .disconnect(
                    &f.account.owner,
                    f.account.id,
                    f.account.authorization_revision,
                )
                .await
                .unwrap();
        }
        drop(held);
        boundary.release();
    };
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tokio::join!(Box::pin(execute), Box::pin(revoke));
    })
    .await
    .expect("authority denial and actual cleanup complete");
    let counts = server.counts();
    assert_eq!(counts.catalogs, 0, "{wait:?}");
    assert_eq!(counts.calls, 0, "{wait:?}");
    let initialize = usize::from(matches!(wait, Wait::Initialize | Wait::Connected));
    assert_eq!(counts.initialize, initialize, "{wait:?}");
    assert_eq!(
        counts.initialized,
        usize::from(matches!(wait, Wait::Connected)),
        "{wait:?}"
    );
    assert_eq!(counts.deletes, initialize, "{wait:?}");
    assert_eq!(action_counts(p).await, super::counts(0, 0, 0));
    assert_eq!(
        f.setup.current().await.revision,
        current.revision + i64::from(disable)
    );
    client.shutdown().await.unwrap();
    server.close().await;
}
