use super::*;
use crate::{
    adapters::{
        integrations::owner::{company_binding, owner_for_user},
        persistence::{
            integration_bindings::PostgresIntegrationBindings, integrations::PostgresVault,
        },
    },
    application::{
        integrations::{
            grants::GrantWriter,
            lifecycle::TeardownDrain,
            management::{AccountRepository, AccountWrite},
            *,
        },
        use_cases::integration_binding::*,
    },
    domain::integrations::{self as integration, authority::*},
    entities::mcp::{McpAuth, McpEndpoint},
    use_cases::{
        company_invite::CompanyInvitePersistence,
        mcp::{McpConnectionWrite, McpCredentialPersistence, McpPersistence},
    },
};
use chrono::{Duration as ChronoDuration, Utc};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;

fn owner(user: Uuid) -> VerifiedOwner {
    VerifiedOwner::from_verified_client(owner_for_user(user))
}
fn write(key: &str, secret: &str) -> AccountWrite {
    AccountWrite {
        key: integration::AccountKey::parse(key).unwrap(),
        provider: integration::ProviderId::parse("static-bearer").unwrap(),
        config: integration::AuthConfigId::parse("static-bearer").unwrap(),
        method: integration::AuthMethod::StaticSecret,
        schema: integration::CredentialSchema::parse("static-v1").unwrap(),
        identity: None,
        metadata: integration::AccountMetadata::new(key.into(), None).unwrap(),
        payload: SecretString::from(secret.to_owned()),
    }
}
pub(super) async fn account(
    setup: &Setup,
    user: Uuid,
    key: &str,
    secret: &str,
) -> (PostgresVault, integration::ConnectedAccount) {
    let vault = PostgresVault::new(setup.fixture.persistence.clone());
    let account = vault
        .create(&owner_for_user(user), write(key, secret))
        .await
        .unwrap();
    (vault, account)
}
pub(super) async fn attach(
    setup: &Setup,
    vault: &PostgresVault,
    account: &integration::ConnectedAccount,
) -> UseGrant {
    Box::pin(attach_source(
        setup,
        vault,
        account,
        integration::AppId::parse("static-bearer").unwrap(),
    ))
    .await
}
pub(super) async fn attach_source(
    setup: &Setup,
    vault: &PostgresVault,
    account: &integration::ConnectedAccount,
    app: integration::AppId,
) -> UseGrant {
    let p = &setup.fixture.persistence;
    let host = PostgresIntegrationBindings {
        persistence: p.clone(),
        vault: vault.clone(),
    };
    let current = p
        .list_http(setup.fixture.target.company, setup.fixture.target.actor)
        .await
        .unwrap()
        .into_iter()
        .find(|c| c.id == setup.connection.id)
        .unwrap();
    let proposal = host
        .propose(
            setup.fixture.target.actor.user_id(),
            ProposeBinding {
                company: setup.fixture.target.company.as_uuid(),
                connection: current.id,
                kind: BindingKind::Http,
                app,
                expected_revision: current.revision,
            },
        )
        .await
        .unwrap();
    let user = Uuid::parse_str(account.owner.subject.as_str()).unwrap();
    host.confirm(
        owner(user),
        proposal.id,
        account.id,
        account.authorization_revision,
    )
    .await
    .unwrap()
}
async fn prepared(setup: &Setup) -> (AuthorizedInvocation, ExecutionContext) {
    let p = &setup.fixture.persistence;
    let request = p
        .http_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    p.prepare_integration_invocation(
        ActionDispatchRequest {
            subject: intent.approval_subject(),
            fence: setup.claim.fence,
            lease: lease(),
        },
        integration::DispatchId::new(Uuid::new_v4()),
        Utc::now() + ChronoDuration::seconds(20),
        CancellationToken::new(),
    )
    .await
    .unwrap()
}
async fn member(setup: &Setup) -> Uuid {
    let user = Uuid::new_v4();
    let p = &setup.fixture.persistence;
    sqlx::query("INSERT INTO users(id,username,email,password_hash) VALUES($1,$2,$3,'hash')")
        .bind(user)
        .bind(format!("binding-{}", user.simple()))
        .bind(format!("{}@example.test", user.simple()))
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES($1,$2,$3,'member',0)")
        .bind(Uuid::new_v4()).bind(setup.fixture.target.company.as_uuid()).bind(user).execute(p.pool()).await.unwrap();
    restore_principal(
        &setup.fixture.persistence,
        setup.fixture.target.company.as_uuid(),
        user,
    )
    .await;
    user
}
async fn restore_principal(p: &PostgresPersistence, company: Uuid, user: Uuid) {
    let (username, email): (String, String) =
        sqlx::query_as("SELECT username::text,email::text FROM users WHERE id=$1")
            .bind(user)
            .fetch_one(p.pool())
            .await
            .unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    crate::adapters::persistence::participant::create_person_principal_on(
        &mut tx, company, user, &username, &email,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

fn consent(account: &integration::ConnectedAccount, number: usize) -> OwnerConfirmation {
    OwnerConfirmation {
        owner: VerifiedOwner::from_verified_client(account.owner.clone()),
        account: account.id,
        host_scope: integration::HostBinding::parse("cap-scope").unwrap(),
        binding: integration::HostBinding::parse(format!("cap/{number}")).unwrap(),
        app: integration::AppId::parse("static-bearer").unwrap(),
        destination: ApprovedDestination::parse("https://api.example.test/").unwrap(),
        purpose: InvocationKind::DeterministicHttp,
        expected_authorization: account.authorization_revision,
        host_authority: integration::HostAuthorityReference::parse("host-test").unwrap(),
    }
}

fn confirmed() -> GrantAuthorityFacts {
    GrantAuthorityFacts {
        owner_is_member: true,
        administrator_can_edit: true,
        owner_confirmed: true,
    }
}

fn changed_policy(vault: &PostgresVault, change: usize) -> PostgresVault {
    use crate::application::integrations::registry::*;
    let mut registry = CodeOwnedRegistry::default();
    let adapter = if change == 2 {
        StaticCredentialAdapter::api_key_header("X-Changed-Key").unwrap()
    } else {
        StaticCredentialAdapter::bearer()
    };
    registry
        .register(Registration {
            authorization: None,
            renewal: crate::application::integrations::renewal::RenewalStrategy::NoRenewal,
            provider: AuthProvider {
                id: integration::ProviderId::parse("static-bearer").unwrap(),
                method: integration::AuthMethod::StaticSecret,
                schema: integration::CredentialSchema::parse("static-v1").unwrap(),
            },
            config: AuthConfig {
                id: integration::AuthConfigId::parse("static-bearer").unwrap(),
                provider: integration::ProviderId::parse("static-bearer").unwrap(),
                client_identity: integration::ExternalSubject::parse("openbot-static").unwrap(),
                client_secret_reference: None,
            },
            app: AppDefinition {
                    transport: crate::application::integrations::destinations::AppTransportPolicy::OwnerApproved,
                id: integration::AppId::parse("static-bearer").unwrap(),
                provider: integration::ProviderId::parse("static-bearer").unwrap(),
                config: integration::AuthConfigId::parse("static-bearer").unwrap(),
                destinations: if change == 1 {
                    vec![
                        ApprovedDestination::parse("https://api.example.test/").unwrap(),
                        ApprovedDestination::parse("https://other.example.test/").unwrap(),
                    ]
                } else {
                    vec![]
                },
                capabilities: if change == 0 {
                    vec![integration::Capability::parse("new-capability").unwrap()]
                } else {
                    vec![]
                },
            },
            adapter: Arc::new(adapter),
        })
        .unwrap();
    PostgresVault {
        persistence: vault.persistence.clone(),
        registry: Arc::new(registry),
    }
}

mod authority;
mod bindings;
mod policies;
mod temporal;

mod membership_race;

mod renewal;

mod execution;
