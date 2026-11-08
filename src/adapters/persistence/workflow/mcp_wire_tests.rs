use super::*;
use crate::{
    adapters::{
        integrations::workflow_dispatch::IntegrationActionDispatch,
        persistence::workflow::{action_dispatch::PostgresActionDispatch, mcp::McpActionAuthority},
        workflow_http::NoLocal,
    },
    application::integrations::wire::test_transport::{Fault, SerializationGateway},
};

fn dispatch(f: &Harness, fault: Fault) -> IntegrationActionDispatch<McpActionAuthority, NoLocal> {
    let p = f.setup.fixture.persistence.clone();
    let inner = execution_support::gateway(&f.vault, f.client.clone());
    let gateway = Arc::new(SerializationGateway::new(
        inner,
        f.account.owner.clone(),
        fault,
    ));
    IntegrationActionDispatch::new(
        p.clone(),
        PostgresActionDispatch::new(p, McpActionAuthority, NoLocal),
        gateway,
    )
}

#[tokio::test]
async fn integration_service_contract_mcp_real_host_receipts_survive_roundtrip_and_lost_replies() {
    for fault in [Fault::None, Fault::LostReply, Fault::InvalidReplyVersion] {
        let f = Box::pin(fixture(Delivery::Accepted)).await;
        let dispatch = dispatch(&f, fault);
        let cancel = CancellationToken::new();
        let result = Box::pin(dispatch.remote(&f.request, &UnreachableProvider, &cancel))
            .await
            .unwrap();
        let RemoteDispatchObservation::Committed(receipt) = result else {
            panic!("retained native committed receipt")
        };
        assert!(!receipt.result.to_string().contains(SECRET));
        assert_eq!(f.client.counts(), (1, 1, 1));
        assert_eq!(
            action_counts(&f.setup.fixture.persistence).await,
            counts(1, 1, 1)
        );
        assert!(matches!(
            Box::pin(dispatch.remote(&f.request, &UnreachableProvider, &cancel))
                .await
                .unwrap(),
            RemoteDispatchObservation::Committed(_)
        ));
        assert_eq!(
            f.client.counts(),
            (1, 1, 1),
            "no unsafe second send after lost wire reply"
        );
        f.client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn integration_service_contract_mcp_unknown_protocol_foreign_owner_and_local_cancel_send_nothing()
 {
    for (fault, expected) in [
        (Fault::UnknownVersion, PreDispatchFailure::Invalid),
        (Fault::ForeignOwner, PreDispatchFailure::Denied),
        (Fault::CancelBeforeDispatch, PreDispatchFailure::Cancelled),
    ] {
        let f = Box::pin(fixture(Delivery::Accepted)).await;
        let result = Box::pin(dispatch(&f, fault).remote(
            &f.request,
            &UnreachableProvider,
            &CancellationToken::new(),
        ))
        .await
        .unwrap();
        assert!(
            matches!(result,RemoteDispatchObservation::PreparationFailed(actual) if actual==expected)
        );
        assert_eq!(f.client.counts(), (0, 0, 0));
        assert_eq!(
            action_counts(&f.setup.fixture.persistence).await,
            counts(0, 0, 0)
        );
        f.client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn integration_service_contract_mcp_roundtrip_preserves_live_revocation_after_catalog_wait() {
    use crate::application::integrations::management::AccountRepository;
    let stage = Stage::new();
    let f = Box::pin(fixture(Delivery::FirstDiscoveryWait(stage.clone()))).await;
    let dispatch = dispatch(&f, Fault::None);
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(
        Box::pin(dispatch.remote(&f.request, &UnreachableProvider, &cancel)),
        async {
            stage.observed(1).await;
            f.vault
                .disconnect(
                    &f.account.owner,
                    f.account.id,
                    f.account.authorization_revision,
                )
                .await
                .unwrap();
            stage.release(1);
        }
    );
    assert!(matches!(
        result.unwrap(),
        RemoteDispatchObservation::PreparationFailed(PreDispatchFailure::Denied)
    ));
    assert_eq!(f.client.counts(), (1, 0, 1));
    assert_eq!(
        action_counts(&f.setup.fixture.persistence).await,
        counts(0, 0, 0)
    );
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn integration_service_contract_mcp_uncertain_effect_never_gains_a_retry_from_serialization()
{
    for fault in [Fault::None, Fault::LostReply] {
        let f = Box::pin(fixture(Delivery::Uncertain)).await;
        let dispatch = dispatch(&f, fault);
        let cancel = CancellationToken::new();
        assert!(
            Box::pin(dispatch.remote(&f.request, &UnreachableProvider, &cancel))
                .await
                .is_err()
        );
        assert!(matches!(
            Box::pin(dispatch.remote(&f.request, &UnreachableProvider, &cancel))
                .await
                .unwrap(),
            RemoteDispatchObservation::PossibleDispatchExists
        ));
        assert_eq!(f.client.counts(), (1, 1, 1));
        assert_eq!(
            action_counts(&f.setup.fixture.persistence).await,
            counts(1, 1, 0)
        );
        f.client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn integration_service_contract_mcp_stale_catalog_uses_the_original_live_host_authority() {
    let f = Box::pin(fixture(Delivery::Accepted)).await;
    let p = &f.setup.fixture.persistence;
    let (invocation, context) = p
        .prepare_integration_invocation(
            f.request.clone(),
            DispatchId::new(Uuid::new_v4()),
            chrono::Utc::now() + chrono::Duration::seconds(10),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let current = f.setup.current().await;
    p.update_mcp_connection(
        current.company_id,
        current.id,
        current.revision,
        crate::application::use_cases::mcp::McpConnectionWrite {
            slug: current.slug,
            endpoint: current.endpoint,
            enabled: current.enabled,
            auth: current.auth,
            discovered_tools: current.discovered_tools,
            tool_grants: current.tool_grants,
        },
    )
    .await
    .unwrap();
    let gateway = SerializationGateway::new(
        execution_support::gateway(&f.vault, f.client.clone()),
        f.account.owner.clone(),
        Fault::None,
    );
    let result = Box::pin(gateway.call_mcp(invocation, context))
        .await
        .unwrap();
    assert!(matches!(
        result,
        ExecutionOutcome::PreDispatch {
            failure: PreDispatchFailure::Denied
        }
    ));
    assert_eq!(f.client.counts(), (0, 0, 0));
    assert_eq!(action_counts(p).await, counts(0, 0, 0));
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn integration_service_contract_mcp_deadline_cancels_preparation_and_closes_without_dispatch()
{
    let stage = Stage::new();
    let f = Box::pin(fixture(Delivery::EveryDiscoveryWait(stage.clone()))).await;
    let p = &f.setup.fixture.persistence;
    let (invocation, context) = p
        .prepare_integration_invocation(
            f.request.clone(),
            DispatchId::new(Uuid::new_v4()),
            chrono::Utc::now() + chrono::Duration::milliseconds(500),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let gateway = SerializationGateway::new(
        execution_support::gateway(&f.vault, f.client.clone()),
        f.account.owner.clone(),
        Fault::None,
    );
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        Box::pin(gateway.call_mcp(invocation, context)),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(result, ExecutionOutcome::PreDispatch { .. }));
    assert_eq!(f.client.counts(), (1, 0, 1));
    assert_eq!(action_counts(p).await, counts(0, 0, 0));
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn integration_service_contract_owner_discovery_keeps_genuine_context_and_rejects_credential_echo()
 {
    use crate::application::use_cases::mcp::McpIntegrationDiscovery;
    for (fault, poison) in [
        (Fault::None, false),
        (Fault::UnknownVersion, false),
        (Fault::ForeignOwner, false),
        (Fault::None, true),
    ] {
        let f = Box::pin(fixture(if poison {
            Delivery::CredentialInCatalog
        } else {
            Delivery::Accepted
        }))
        .await;
        let gateway = Arc::new(SerializationGateway::new(
            execution_support::gateway(&f.vault, f.client.clone()),
            f.account.owner.clone(),
            fault,
        ));
        let discovery = crate::adapters::persistence::PostgresMcpDiscovery {
            vault: f.vault.clone(),
            gateway,
        };
        let current = f.setup.current().await;
        let result = Box::pin(discovery.refresh(
            f.setup.actor().user_id(),
            VerifiedOwner::from_verified_client(f.account.owner.clone()),
            &current,
        ))
        .await;
        let expected = matches!(fault, Fault::None) && !poison;
        assert_eq!(result.is_ok(), expected);
        let opens = usize::from(matches!(fault, Fault::None));
        assert_eq!(f.client.counts(), (opens, 0, opens));
        assert_eq!(
            f.setup.current().await.revision,
            current.revision + i64::from(expected)
        );
        assert_eq!(
            action_counts(&f.setup.fixture.persistence).await,
            counts(0, 0, 0)
        );
        f.client.shutdown().await.unwrap();
    }
}
