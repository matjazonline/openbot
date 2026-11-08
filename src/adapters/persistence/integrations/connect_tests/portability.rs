use super::*;
use crate::adapters::persistence::integrations::renewal_tests::{
    server::{Reply, TokenServer},
    workspace::{self, FixtureConfiguration},
    workspace_authorization,
};
use crate::application::integrations::{grants::GrantWriter, private::CredentialCacheUpdate};
use crate::domain::integrations::authority::*;
use std::collections::BTreeMap;

struct Scope {
    f: Fixture,
    server: TokenServer,
    vault: PostgresVault,
    service: Arc<ConnectService>,
}
impl Scope {
    async fn new() -> Self {
        let f = Box::pin(Fixture::new()).await;
        let server = TokenServer::new().await;
        let mut registry = static_registry();
        for (app, config, client, capability) in [
            (
                "workspace-reader",
                "workspace-client-a",
                "strict-client",
                "resource.read",
            ),
            (
                "workspace-writer",
                "workspace-client-a",
                "strict-client",
                "resource.write",
            ),
            (
                "workspace-other-client",
                "workspace-client-b",
                "other-client",
                "resource.read",
            ),
        ] {
            let entry = workspace::registration_for(
                &server,
                FixtureConfiguration {
                    client: ExternalSubject::parse(client).unwrap(),
                    config: AuthConfigId::parse(config).unwrap(),
                    app: AppId::parse(app).unwrap(),
                    capabilities: vec![Capability::parse(capability).unwrap()],
                },
            );
            registry
                .register(workspace_authorization::with_authorization(entry, &server))
                .unwrap();
        }
        let vault = PostgresVault::with_registry(f.vault.persistence.clone(), Arc::new(registry));
        let service = f.service_on(vault.clone());
        Self {
            f,
            server,
            vault,
            service,
        }
    }
    fn reply(&self, workspace: &str, scopes: &str) {
        *self.server.state.reply.lock().unwrap() = Reply::Json(
            json!({"workspace":workspace,"scope":scopes,"access_token":"workspace-code-access-sentinel","refresh_token":"workspace-first-rotating-refresh","expires_in":3600,"token_type":"Bearer"}),
        );
    }
    async fn begin(&self, app: &str, reconnect: Option<ReconnectTarget>) -> ConnectStart {
        let app = AppId::parse(app).unwrap();
        let entry = self.vault.registry.get(&app).unwrap();
        self.service
            .begin_connect(
                &self.f.owner,
                BeginConnect {
                    app,
                    config: entry.config.id.clone(),
                    requested_capabilities: RequestedCapabilities::try_from(
                        entry.app.capabilities.clone(),
                    )
                    .unwrap(),
                    return_target: SafeReturnTarget::parse("/ui/profile/vault".into()).unwrap(),
                    reconnect,
                },
            )
            .await
            .unwrap()
    }
    fn callback(start: &ConnectStart) -> ConnectCallback {
        let url = url::Url::parse(start.authorization_url.as_str()).unwrap();
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .into_owned();
        ConnectCallback {
            provider: ProviderId::parse("workspace-fixture").unwrap(),
            state,
            code: Some(SecretString::from("workspace-authorization-code-sentinel")),
            error: None,
        }
    }
    async fn finish(&self, start: &ConnectStart) -> ConnectAttempt {
        self.service
            .finish_connect(
                &self.f.owner,
                Self::callback(start),
                work(CancellationToken::new()),
            )
            .await
            .unwrap()
    }
    async fn connect(&self, app: &str) -> ConnectedAccount {
        let start = self.begin(app, None).await;
        let done = self.finish(&start).await;
        assert_eq!(done.status, ConnectStatus::Connected);
        self.vault
            .get(self.f.owner.owner(), done.account.unwrap())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn integration_portability_custom_oauth_scope_pkce_and_installation_identity_need_no_email() {
    let f = Box::pin(Scope::new()).await;
    f.reply("installation-no-email", "workspace:inspect");
    let start = f.begin("workspace-reader", None).await;
    let url = url::Url::parse(start.authorization_url.as_str()).unwrap();
    let pairs = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
    assert_eq!(pairs["scope"], "workspace:inspect");
    assert_eq!(pairs["code_challenge_method"], "S256");
    assert_eq!(pairs["client_id"], "strict-client");
    let done = f.finish(&start).await;
    assert_eq!(done.status, ConnectStatus::Connected);
    let account = f
        .vault
        .get(f.f.owner.owner(), done.account.unwrap())
        .await
        .unwrap();
    assert_eq!(
        account.external_identity.as_ref().unwrap().subject.as_str(),
        "installation-no-email"
    );
    let raw = f
        .vault
        .read_usable(f.f.owner.owner(), account.id)
        .await
        .unwrap();
    let payload: Value = serde_json::from_str(raw.payload.expose_secret()).unwrap();
    assert!(payload.get("email").is_none());
    assert_eq!(payload["permissions"], json!(["resource.read"]));
    let observed = f.server.state.observation.lock().unwrap();
    let observed = observed.as_ref().unwrap();
    assert_eq!(observed.grant, "authorization_code");
    assert_eq!(
        observed.client_identity.as_ref().unwrap().as_str(),
        "strict-client"
    );
    assert_eq!(
        observed.code_digest,
        Some(Sha256::digest(b"workspace-authorization-code-sentinel").into())
    );
    assert!(observed.verifier_digest.is_some());
    assert!(
        static_registry()
            .get(&AppId::parse("workspace-reader").unwrap())
            .is_err()
    );
    assert!(
        !serde_json::to_string(&account)
            .unwrap()
            .contains("sentinel")
    );
}

#[tokio::test]
async fn integration_portability_workspace_accounts_and_oauth_clients_keep_separate_authorizations()
{
    let f = Box::pin(Scope::new()).await;
    f.reply("installation-one", "workspace:inspect");
    let a = f.connect("workspace-reader").await;
    let b = f.connect("workspace-other-client").await;
    f.reply("installation-two", "workspace:inspect");
    let c = f.connect("workspace-reader").await;
    assert_ne!(a.id, b.id);
    assert_ne!(a.id, c.id);
    assert_ne!(b.id, c.id);
    assert_ne!(
        a.external_identity.as_ref().unwrap().authorization_key,
        b.external_identity.as_ref().unwrap().authorization_key
    );
    assert_eq!(
        a.external_identity.as_ref().unwrap().subject,
        b.external_identity.as_ref().unwrap().subject
    );
    let raw = f.vault.read_usable(f.f.owner.owner(), a.id).await.unwrap();
    let wrong = f
        .vault
        .registry
        .get(&AppId::parse("workspace-other-client").unwrap())
        .unwrap();
    assert!(wrong.adapter.validate(&raw.payload).is_err());
    let mut foreign = f.f.owner.owner().clone();
    foreign.subject = OwnerSubject::parse(Uuid::new_v4().to_string()).unwrap();
    assert!(f.vault.get(&foreign, a.id).await.is_err());
    assert_eq!(f.server.calls(), 3);
}

#[tokio::test]
async fn integration_portability_scope_upgrade_requires_consent_and_retires_older_app_grants() {
    let f = Box::pin(Scope::new()).await;
    f.reply("shared-installation", "workspace:inspect");
    let account = f.connect("workspace-reader").await;
    let before = f
        .vault
        .read_usable(f.f.owner.owner(), account.id)
        .await
        .unwrap();
    let confirmation = OwnerConfirmation {
        owner: f.f.owner.clone(),
        account: account.id,
        expected_authorization: account.authorization_revision,
        host_scope: HostBinding::parse("company/portable-fixture").unwrap(),
        binding: HostBinding::parse("http/portable-fixture").unwrap(),
        app: AppId::parse("workspace-reader").unwrap(),
        destination: ApprovedDestination::parse("https://api.example.test").unwrap(),
        purpose: InvocationKind::DeterministicHttp,
        host_authority: HostAuthorityReference::parse("module-consent-fixture").unwrap(),
    };
    // These are module-storage consent facts. Real host enforcement is separately exercised
    // by the native HTTP/MCP gateway conformance cases, never inferred from these booleans.
    let grant = f
        .vault
        .create_confirmed(
            &confirmation,
            GrantAuthorityFacts {
                owner_is_member: true,
                administrator_can_edit: true,
                owner_confirmed: true,
            },
        )
        .await
        .unwrap();
    let writer = f
        .vault
        .registry
        .get(&AppId::parse("workspace-writer").unwrap())
        .unwrap();
    assert!(
        writer
            .adapter
            .validate_use(&before.payload, &writer.app)
            .is_err()
    );
    f.reply("shared-installation", "workspace:inspect workspace:mutate");
    let start = f
        .begin(
            "workspace-writer",
            Some(ReconnectTarget {
                account: account.id,
                authorization: account.authorization_revision,
            }),
        )
        .await;
    let done = f.finish(&start).await;
    assert_eq!(done.status, ConnectStatus::Connected);
    assert_eq!(done.account, Some(account.id));
    let after = f
        .vault
        .read_usable(f.f.owner.owner(), account.id)
        .await
        .unwrap();
    assert_eq!(
        after.account.authorization_revision.get(),
        account.authorization_revision.get() + 1
    );
    assert!(!f.vault.grant_metadata(grant.id).await.unwrap().enabled);
    assert!(
        writer
            .adapter
            .validate_use(&after.payload, &writer.app)
            .is_ok()
    );
    assert!(
        writer
            .adapter
            .validate_cache_update(CredentialCacheUpdate {
                previous: &before.payload,
                replacement: &after.payload
            })
            .is_err()
    );
}

#[tokio::test]
async fn integration_portability_missing_custom_scope_never_creates_an_account_or_replays_exchange()
{
    let f = Box::pin(Scope::new()).await;
    f.reply("missing-reader", "workspace:mutate");
    let start = f.begin("workspace-reader", None).await;
    assert_eq!(
        f.finish(&start).await.status,
        ConnectStatus::MissingPermissions
    );
    assert!(
        f.vault
            .list(f.f.owner.owner(), PageRequest::new(None, 1).unwrap())
            .await
            .unwrap()
            .accounts
            .is_empty()
    );
    assert!(matches!(
        f.service
            .finish_connect(
                &f.f.owner,
                Scope::callback(&start),
                work(CancellationToken::new())
            )
            .await,
        Err(IntegrationError::Denied)
    ));
    assert_eq!(f.server.calls(), 1);
}

#[tokio::test]
async fn integration_portability_installation_and_client_identity_encoding_has_no_separator_collision()
 {
    let server = TokenServer::new().await;
    let mut keys = Vec::new();
    for (workspace, client, app) in [
        ("installation/client", "one", "collision-one"),
        ("installation", "client/one", "collision-two"),
    ] {
        let entry = workspace::registration_for(
            &server,
            FixtureConfiguration {
                client: ExternalSubject::parse(client).unwrap(),
                config: AuthConfigId::parse(app).unwrap(),
                app: AppId::parse(app).unwrap(),
                capabilities: vec![Capability::parse("resource.read").unwrap()],
            },
        );
        let mut value: Value = serde_json::from_str(workspace::payload().expose_secret()).unwrap();
        value["workspace"] = json!(workspace);
        value["client"] = json!(client);
        let raw = SecretString::from(value.to_string());
        keys.push(
            entry
                .adapter
                .validate(&raw)
                .unwrap()
                .unwrap()
                .authorization_key,
        );
    }
    assert_ne!(keys[0], keys[1]);
    assert_eq!(server.calls(), 0);
}
