//! Resolution uses real deterministic workflow execution authority in these tests.
use super::*;
use crate::adapters::persistence::integrations::renewal_tests::server::{Reply, TokenServer};
use crate::application::integrations::{
    private::{CredentialStore, InjectionMethod},
    registry::CodeOwnedRegistry,
    renewal::*,
};
use crate::domain::integrations::*;
mod bounds;
mod cache;
mod commit;
mod failures;
mod google;
mod isolation;
mod management;
mod reconnect;
mod rotation;
mod strict;
struct RenewalSetup {
    setup: Setup,
    vault: PostgresVault,
    account: ConnectedAccount,
    server: TokenServer,
}
fn google_registry(server: &TokenServer) -> Arc<CodeOwnedRegistry> {
    let mut registry = CodeOwnedRegistry::default();
    for id in ["gmail", "gmail-read"] {
        let mut entry = server.google_entry();
        entry.app.id = AppId::parse(id).unwrap();
        entry.app.destinations.clear();
        if id == "gmail-read" {
            entry.app.capabilities = vec![Capability::parse("gmail.read").unwrap()];
        }
        registry.register(entry).unwrap();
    }
    Arc::new(registry)
}
fn google_payload(subject: &str, expires: chrono::DateTime<Utc>) -> SecretString {
    SecretString::from(json!({"subject":subject,"client":"google-client-fixture","mailbox":"vault@gmail.com","scopes":["openid","email","https://www.googleapis.com/auth/gmail.readonly","https://www.googleapis.com/auth/gmail.send"],"refresh_token":"refresh-sentinel","access_token":"old-access-sentinel","expires_at":expires,"generation":1,"refresh_retry_at":null}).to_string())
}
async fn managed(
    vault: &PostgresVault,
    user: Uuid,
    subject: &str,
    expires: chrono::DateTime<Utc>,
) -> ConnectedAccount {
    let entry = vault.registry.get(&AppId::parse("gmail").unwrap()).unwrap();
    let payload = google_payload(subject, expires);
    vault
        .create(
            &owner_for_user(user),
            AccountWrite {
                key: AccountKey::parse(subject).unwrap(),
                provider: entry.provider.id.clone(),
                config: entry.config.id.clone(),
                method: entry.provider.method,
                schema: entry.provider.schema.clone(),
                identity: entry.adapter.validate(&payload).unwrap(),
                metadata: AccountMetadata::new(subject.into(), None).unwrap(),
                payload,
            },
        )
        .await
        .unwrap()
}
async fn attach_app(
    setup: &Setup,
    vault: &PostgresVault,
    account: &ConnectedAccount,
    app: AppId,
) -> UseGrant {
    let p = &setup.fixture.persistence;
    let connection = p
        .list_http(setup.fixture.target.company, setup.fixture.target.actor)
        .await
        .unwrap()
        .into_iter()
        .find(|c| c.id == setup.connection.id)
        .unwrap();
    let host = PostgresIntegrationBindings {
        persistence: p.clone(),
        vault: vault.clone(),
    };
    let proposal = host
        .propose(
            setup.fixture.target.actor.user_id(),
            ProposeBinding {
                company: setup.fixture.target.company.as_uuid(),
                connection: connection.id,
                kind: BindingKind::Http,
                app,
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
    .unwrap()
}
impl RenewalSetup {
    async fn new(expires: chrono::DateTime<Utc>) -> Self {
        let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
        let server = TokenServer::new().await;
        let vault = PostgresVault::with_registry(
            setup.fixture.persistence.clone(),
            google_registry(&server),
        );
        let account = managed(
            &vault,
            setup.fixture.target.actor.user_id(),
            "google-private",
            expires,
        )
        .await;
        attach_app(&setup, &vault, &account, AppId::parse("gmail").unwrap()).await;
        Self {
            setup,
            vault,
            account,
            server,
        }
    }
    async fn resolve(&self) -> ResolutionResult<ResolvedCredential> {
        let (i, c) = prepared(&self.setup).await;
        self.vault.resolve(&i, &c).await
    }
    async fn credential(&self) -> crate::application::integrations::private::CredentialRecord {
        self.vault
            .read_usable(&self.account.owner, self.account.id)
            .await
            .unwrap()
    }
    async fn separate_service(&self) -> ServiceInstance {
        independent_service(&self.vault).await
    }
    async fn wait_blocked(&self, application: &str) {
        wait_blocked(&self.vault, application).await
    }
}

struct ServiceInstance {
    vault: PostgresVault,
    application: String,
}

async fn independent_service(original: &PostgresVault) -> ServiceInstance {
    let application = format!("vg05-{}", Uuid::new_v4().simple());
    let options = original
        .persistence
        .pool()
        .connect_options()
        .as_ref()
        .clone()
        .application_name(&application);
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    let vault = PostgresVault::with_registry(
        PostgresPersistence::with_credential_cipher(pool, CredentialCipher::for_test()),
        original.registry.clone(),
    );
    ServiceInstance { vault, application }
}
async fn wait_blocked(vault: &PostgresVault, application: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(5),async{
        loop {
            let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND application_name=$1 AND cardinality(pg_blocking_pids(pid))>0)")
                .bind(application).fetch_one(vault.persistence.pool()).await.unwrap();
            if blocked{break;}tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
}
