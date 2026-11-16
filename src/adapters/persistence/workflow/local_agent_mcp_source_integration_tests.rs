//! The final credential gate retains actor, requester and a distinct real credential owner.
use super::Owner as SourceOwner;
use super::*;
use crate::adapters::integrations::owner::owner_for_user;
use crate::adapters::persistence::{
    credentials::CredentialCipher, integration_bindings::PostgresIntegrationBindings,
    integrations::PostgresVault,
};
use crate::application::integrations::{
    VerifiedOwner,
    management::{AccountRepository, AccountWrite},
};
use crate::application::use_cases::integration_binding::*;
use crate::application::use_cases::mcp::McpPersistence;
use crate::domain::integrations::*;
use secrecy::SecretString;
use tokio_util::sync::CancellationToken;

async fn integrated_source() -> (Source, PostgresVault, ConnectedAccount, Uuid) {
    let mut local = Box::pin(Setup::new()).await;
    local.fixture.persistence = PostgresPersistence::with_credential_cipher(
        local.fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    configure(&mut local, "https://mcp.example.test/rpc").await;
    let p = &local.fixture.persistence;
    let owner: Uuid = sqlx::query_scalar(
        "SELECT member.user_id FROM principals AS member_principal JOIN LATERAL membership_of(member_principal.user_id, member_principal.company_id) AS member ON true WHERE member_principal.company_id=$1 AND role='member' LIMIT 1",
    )
    .bind(local.scope.company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let vault = PostgresVault::new(p.clone());
    let account = vault
        .create(
            &owner_for_user(owner),
            AccountWrite {
                key: AccountKey::parse("native-distinct-owner").unwrap(),
                provider: ProviderId::parse("static-bearer").unwrap(),
                config: AuthConfigId::parse("static-bearer").unwrap(),
                method: AuthMethod::StaticSecret,
                schema: CredentialSchema::parse("static-v1").unwrap(),
                identity: None,
                metadata: AccountMetadata::new("Native distinct owner".into(), None).unwrap(),
                payload: SecretString::from("native-distinct-owner-fixture-secret"),
            },
        )
        .await
        .unwrap();
    let connection = p
        .list_mcp_connections(local.scope.company.as_uuid())
        .await
        .unwrap()
        .remove(0);
    let host = PostgresIntegrationBindings {
        persistence: p.clone(),
        vault: vault.clone(),
    };
    let proposal = host
        .propose(
            local.fixture.target.actor.user_id(),
            ProposeBinding {
                company: local.scope.company.as_uuid(),
                connection: connection.id,
                kind: BindingKind::Mcp,
                app: AppId::parse("static-bearer").unwrap(),
                expected_revision: connection.revision,
            },
        )
        .await
        .unwrap();
    host.confirm(
        VerifiedOwner::from_verified_client(account.owner.clone()),
        proposal.id,
        account.id,
        account.authorization_revision,
    )
    .await
    .unwrap();
    let source = Box::pin(start_source(local)).await;
    assert_ne!(owner, source.local.fixture.target.actor.user_id());
    assert_ne!(owner, source.requester);
    (source, vault, account, owner)
}

#[tokio::test]
async fn native_mcp_workflow_final_credential_guard_locks_distinct_source_and_credential_owners() {
    let (source, vault, account, owner) = Box::pin(integrated_source()).await;
    assert_eq!(account.owner, owner_for_user(owner));
    let mut a = Box::pin(action(source)).await;
    let request = Box::pin(approved(&mut a)).await;
    let writer = dispatch(&a);
    let RemoteReservationResult::Reserved(reservation) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("the real integration-backed native action must reserve");
    };
    let entry = writer.enter_remote(*reservation).await.unwrap();
    let p = &a.source.local.fixture.persistence;
    let (invocation, context) = p
        .prepare_integration_invocation(
            request,
            DispatchId::new(entry.marker),
            chrono::Utc::now() + chrono::Duration::seconds(20),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    for owner in [
        SourceOwner::Account(a.source.local.fixture.target.actor.user_id()),
        SourceOwner::Account(a.source.requester),
        SourceOwner::Account(owner),
        SourceOwner::Revision(a.source.revision),
    ] {
        let before = effects(&a.source.local).await;
        let competitor = competitor(&a.source, owner).await;
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            vault.final_dispatch_guard(&invocation, &context),
        )
        .await
        .expect("native final credential authority must not wait for any real account owner");
        assert!(
            matches!(result, Err(IntegrationError::Storage)),
            "{owner:?}: {result:?}"
        );
        assert_eq!(effects(&a.source.local).await, before);
        competitor.rollback().await.unwrap();
        vault
            .final_dispatch_guard(&invocation, &context)
            .await
            .unwrap();
    }
}
