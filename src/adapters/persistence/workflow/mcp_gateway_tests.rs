use super::*;
#[path = "mcp_gateway_support.rs"]
mod execution_support;
use crate::application::{
    integrations::{private::CredentialStore, *},
    services::mcp_client::McpClient,
};
use execution_support::*;
#[path = "mcp_preparation_authority_tests.rs"]
mod preparation_authority_tests;
#[path = "mcp_projection_tests.rs"]
mod projection_tests;
#[path = "mcp_wire_tests.rs"]
mod wire_tests;

#[tokio::test]
async fn vault_workflow_mcp_private_session_uses_native_receipt_and_redacts_active_credential() {
    let f = Box::pin(fixture(Delivery::Accepted)).await;
    let cancel = CancellationToken::new();
    let result = Box::pin(f.dispatch.remote(&f.request, &UnreachableProvider, &cancel))
        .await
        .unwrap();
    let RemoteDispatchObservation::Committed(receipt) = result else {
        panic!("gateway committed through native receipt handling");
    };
    assert!(!receipt.result.to_string().contains(SECRET));
    assert_eq!(f.client.counts(), (1, 1, 1));
    assert!(matches!(
        Box::pin(f.dispatch.remote(&f.request, &UnreachableProvider, &cancel))
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(
        f.client.counts(),
        (1, 1, 1),
        "receipt reuse opens no session"
    );
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_workflow_mcp_generation_change_during_catalog_closes_and_rebuilds_before_entry() {
    let stage = Stage::new();
    let (f, _server) = Box::pin(managed_fixture(Delivery::FirstDiscoveryWait(stage.clone()))).await;
    let p = &f.setup.fixture.persistence;
    let (invocation, context) = p
        .prepare_integration_invocation(
            f.request.clone(),
            DispatchId::new(Uuid::new_v4()),
            chrono::Utc::now() + chrono::Duration::seconds(25),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(
        Box::pin(f.dispatch.remote(&f.request, &UnreachableProvider, &cancel)),
        async {
            stage.observed(1).await;
            crate::adapters::persistence::integrations::resolve::testsupport::cache_refresh(
                &f.vault,
                &invocation,
                &context,
                google_payload(2, "new-mcp-access-sentinel"),
            )
            .await
            .unwrap();
            stage.release(1);
        }
    );
    let RemoteDispatchObservation::Committed(receipt) = result.unwrap() else {
        panic!("generation rebuilt before business dispatch");
    };
    assert!(!receipt.result.to_string().contains("mcp-access-sentinel"));
    assert_eq!(f.client.counts(), (2, 1, 2));
    let current = f
        .vault
        .read_usable(&f.account.owner, f.account.id)
        .await
        .unwrap();
    assert_eq!(
        current.versions.authorization,
        f.account.authorization_revision
    );
    assert_eq!(current.versions.generation.get(), 2);
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_workflow_mcp_uncertain_tool_call_never_reopens_or_replays() {
    let f = Box::pin(fixture(Delivery::Uncertain)).await;
    let cancel = CancellationToken::new();
    assert!(
        Box::pin(f.dispatch.remote(&f.request, &UnreachableProvider, &cancel))
            .await
            .is_err()
    );
    assert!(matches!(
        Box::pin(f.dispatch.remote(&f.request, &UnreachableProvider, &cancel))
            .await
            .unwrap(),
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    assert_eq!(f.client.counts(), (1, 1, 1));
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_workflow_mcp_cancellation_drops_actual_call_then_awaits_session_close() {
    let stage = Stage::new();
    let f = Box::pin(fixture(Delivery::CallWait(stage.clone()))).await;
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(
        Box::pin(f.dispatch.remote(&f.request, &UnreachableProvider, &cancel)),
        async {
            stage.observed(1).await;
            cancel.cancel();
        }
    );
    assert!(matches!(
        result.unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    assert_eq!(f.client.counts(), (1, 1, 1));
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_workflow_mcp_credential_in_catalog_is_rejected_before_any_business_marker() {
    let f = Box::pin(fixture(Delivery::CredentialInCatalog)).await;
    assert!(matches!(
        Box::pin(
            f.dispatch
                .remote(&f.request, &UnreachableProvider, &CancellationToken::new())
        )
        .await
        .unwrap(),
        RemoteDispatchObservation::PreparationFailed(_)
    ));
    assert_eq!(f.client.counts(), (1, 0, 1));
    assert_eq!(
        action_counts(&f.setup.fixture.persistence).await,
        counts(0, 0, 0)
    );
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_workflow_mcp_owner_configuration_discovers_without_a_business_invocation() {
    use crate::application::use_cases::mcp::McpIntegrationDiscovery;
    let f = Box::pin(fixture(Delivery::Accepted)).await;
    let discovery = crate::adapters::persistence::PostgresMcpDiscovery {
        vault: f.vault.clone(),
        gateway: gateway(&f.vault, f.client.clone()),
    };
    let current = f.setup.current().await;
    let updated = Box::pin(discovery.refresh(
        f.setup.actor().user_id(),
        VerifiedOwner::from_verified_client(f.account.owner.clone()),
        &current,
    ))
    .await
    .unwrap();
    assert_eq!(updated.revision, current.revision + 1);
    assert_eq!(updated.discovered_tools.len(), 1);
    assert_eq!(f.client.counts(), (1, 0, 1));
    assert_eq!(
        action_counts(&f.setup.fixture.persistence).await,
        counts(0, 0, 0)
    );
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_workflow_mcp_competing_catalog_refreshes_commit_one_current_revision() {
    use crate::application::use_cases::mcp::McpIntegrationDiscovery;
    let stage = Stage::new();
    let f = Box::pin(fixture(Delivery::EveryDiscoveryWait(stage.clone()))).await;
    let discovery = crate::adapters::persistence::PostgresMcpDiscovery {
        vault: f.vault.clone(),
        gateway: gateway(&f.vault, f.client.clone()),
    };
    let current = f.setup.current().await;
    let owner = VerifiedOwner::from_verified_client(f.account.owner.clone());
    let (a, b, ()) = tokio::join!(
        Box::pin(discovery.refresh(f.setup.actor().user_id(), owner.clone(), &current)),
        Box::pin(discovery.refresh(f.setup.actor().user_id(), owner, &current)),
        async {
            stage.observed(2).await;
            stage.release(2);
        }
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(f.setup.current().await.revision, current.revision + 1);
    assert_eq!(f.client.counts(), (2, 0, 2));
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_workflow_mcp_owner_disconnect_during_catalog_prevents_configuration_save() {
    use crate::application::{
        integrations::management::AccountRepository, use_cases::mcp::McpIntegrationDiscovery,
    };
    let stage = Stage::new();
    let f = Box::pin(fixture(Delivery::EveryDiscoveryWait(stage.clone()))).await;
    let discovery = crate::adapters::persistence::PostgresMcpDiscovery {
        vault: f.vault.clone(),
        gateway: gateway(&f.vault, f.client.clone()),
    };
    let current = f.setup.current().await;
    let (result, ()) = tokio::join!(
        Box::pin(discovery.refresh(
            f.setup.actor().user_id(),
            VerifiedOwner::from_verified_client(f.account.owner.clone()),
            &current
        )),
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
    assert!(result.is_err());
    assert_eq!(f.setup.current().await.revision, current.revision);
    assert_eq!(f.client.counts(), (1, 0, 1));
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_workflow_mcp_browser_expiring_during_catalog_prevents_configuration_save() {
    use crate::{
        application::{
            integrations::connect::ConnectBrowser, use_cases::mcp::McpIntegrationDiscovery,
        },
        domain::integrations::{BrowserBinding, ConnectSessionReference},
    };
    use sha2::{Digest, Sha256};
    let stage = Stage::new();
    let f = Box::pin(fixture(Delivery::EveryDiscoveryWait(stage.clone()))).await;
    let p = &f.setup.fixture.persistence;
    let sessions = crate::adapters::http::session::SessionAuthority::new(
        &crate::infra::config::AppConfig::for_test(),
        Arc::new(p.clone()),
    );
    let cookie = sessions.cookie_for_test(f.setup.actor().user_id()).await;
    let session = sessions.verify(cookie.value()).await.unwrap().unwrap();
    let reference = session.session_id.0.to_string();
    let owner = VerifiedOwner::for_browser(
        f.account.owner.clone(),
        ConnectBrowser {
            session: ConnectSessionReference::parse(reference.clone()).unwrap(),
            binding: BrowserBinding::parse(format!(
                "{:x}",
                Sha256::digest(format!("openbot/vault/{reference}").as_bytes())
            ))
            .unwrap(),
        },
    );
    let discovery = crate::adapters::persistence::PostgresMcpDiscovery {
        vault: f.vault.clone(),
        gateway: gateway(&f.vault, f.client.clone()),
    };
    let current = f.setup.current().await;
    let (result, ()) = tokio::join!(
        Box::pin(discovery.refresh(f.setup.actor().user_id(), owner, &current)),
        async {
            stage.observed(1).await;
            sqlx::query("UPDATE user_sessions SET issued_at=clock_timestamp()-interval '2 seconds',expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
            .bind(session.session_id.0).execute(p.pool()).await.unwrap();
            stage.release(1);
        }
    );
    assert!(result.is_err());
    assert_eq!(f.setup.current().await.revision, current.revision);
    assert_eq!(f.client.counts(), (1, 0, 1));
    f.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_workflow_mcp_another_administrator_cannot_discover_the_owners_account() {
    use crate::{
        adapters::integrations::owner::owner_for_user,
        application::use_cases::mcp::McpIntegrationDiscovery, use_cases::user::UserPersistence,
    };
    let f = Box::pin(fixture(Delivery::Accepted)).await;
    let p = &f.setup.fixture.persistence;
    let suffix = Uuid::new_v4().simple().to_string();
    let user = p
        .create_user(
            &format!("mcp-other-{suffix}"),
            &format!("{suffix}@example.test"),
            "hash",
        )
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role) VALUES(gen_random_uuid(),$1,$2,'admin')")
        .bind(f.setup.company().as_uuid())
        .bind(user.id)
        .execute(p.pool())
        .await
        .unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    crate::adapters::persistence::participant::create_person_principal_on(
        &mut tx,
        f.setup.company().as_uuid(),
        user.id,
        "other-administrator",
        "other-administrator@example.test",
    )
    .await
    .unwrap();
    crate::adapters::persistence::workflow::authority::authorize_company(
        &mut tx,
        f.setup.company(),
        crate::application::workflow::WorkflowActor::authenticated(user.id).unwrap(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let discovery = crate::adapters::persistence::PostgresMcpDiscovery {
        vault: f.vault.clone(),
        gateway: gateway(&f.vault, f.client.clone()),
    };
    assert!(
        Box::pin(discovery.refresh(
            user.id,
            VerifiedOwner::from_verified_client(owner_for_user(user.id)),
            &f.setup.current().await
        ))
        .await
        .is_err()
    );
    assert_eq!(f.client.counts(), (0, 0, 0));
    f.client.shutdown().await.unwrap();
}
