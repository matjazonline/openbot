use super::*;
use crate::application::integrations::grants::BindingAuthority;
struct PausedWriter {
    vault: PostgresVault,
    ready: Arc<tokio::sync::Notify>,
    resume: Arc<tokio::sync::Notify>,
}
#[async_trait]
impl GrantWriter for PausedWriter {
    async fn create_confirmed(
        &self,
        c: &OwnerConfirmation,
        facts: GrantAuthorityFacts,
    ) -> integration::IntegrationResult<UseGrant> {
        self.ready.notify_one();
        self.resume.notified().await;
        self.vault.create_confirmed(c, facts).await
    }
    async fn revoke(
        &self,
        owner: &integration::AccountOwner,
        id: integration::UseGrantId,
        expected: integration::GrantRevision,
    ) -> integration::IntegrationResult<()> {
        self.vault.revoke(owner, id, expected).await
    }
}
#[tokio::test]
async fn vault_binding_membership_revocation_covers_consent_created_while_delete_waits() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let user = member(&setup).await;
    let (vault, account) = account(&setup, user, "membership-race", "secret").await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company.as_uuid();
    let host = PostgresIntegrationBindings {
        persistence: p.clone(),
        vault: vault.clone(),
    };
    let proposal = host
        .propose(
            setup.fixture.target.actor.user_id(),
            ProposeBinding {
                company,
                connection: setup.connection.id,
                kind: BindingKind::Http,
                app: integration::AppId::parse("static-bearer").unwrap(),
                expected_revision: setup.connection.revision,
            },
        )
        .await
        .unwrap();
    let c = OwnerConfirmation {
        owner: owner(user),
        account: account.id,
        host_scope: company_binding(company),
        binding: crate::adapters::persistence::integration_bindings::binding(
            company,
            setup.connection.id,
            BindingKind::Http,
        ),
        app: proposal.app,
        destination: proposal.destination,
        purpose: InvocationKind::DeterministicHttp,
        expected_authorization: account.authorization_revision,
        host_authority: integration::HostAuthorityReference::parse(proposal.id.to_string())
            .unwrap(),
    };
    let ready = Arc::new(tokio::sync::Notify::new());
    let resume = Arc::new(tokio::sync::Notify::new());
    let writer = PausedWriter {
        vault: vault.clone(),
        ready: ready.clone(),
        resume: resume.clone(),
    };
    let consent = tokio::spawn(async move { BindingAuthority::confirm(&host, c, &writer).await });
    ready.notified().await;
    let remover = p.clone();
    let removal = tokio::spawn(async move { remover.remove_member(company, user).await });
    let observed=tokio::time::timeout(Duration::from_secs(3),async {
        loop {
            let start:Option<chrono::DateTime<Utc>>=sqlx::query_scalar("SELECT xact_start FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query=$1 LIMIT 1")
                .bind("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE").fetch_optional(p.pool()).await.unwrap();
            if let Some(start)=start {break start;}
            tokio::task::yield_now().await;
        }
    }).await;
    resume.notify_one();
    let grant = consent.await.unwrap().unwrap();
    removal.await.unwrap().unwrap();
    let transaction_start =
        observed.expect("real member removal must wait for the consent's host lock");
    let (confirmed,cutoff):(chrono::DateTime<Utc>,chrono::DateTime<Utc>)=sqlx::query_as("SELECT grant_row.confirmed_at,intent.created_at FROM integration_use_grants AS grant_row JOIN integration_teardown_intents AS intent ON intent.owner_namespace=grant_row.owner_namespace AND intent.owner_subject=grant_row.owner_subject AND intent.host_binding=grant_row.host_scope AND intent.kind='membership' WHERE grant_row.id=$1")
        .bind(grant.id.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(
        confirmed > transaction_start,
        "the fixture must place consent after the waiting transaction began"
    );
    assert!(
        cutoff >= confirmed,
        "a later committed revocation must retire this consent"
    );
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES($1,$2,$3,'member',0)").bind(Uuid::new_v4()).bind(company).bind(user).execute(p.pool()).await.unwrap();
    restore_principal(p, company, user).await;
    vault.drain(16, &CancellationToken::new()).await.unwrap();
    assert!(!vault.grant_metadata(grant.id).await.unwrap().enabled);
    assert!(
        p.http_step_request(&setup.claim, &setup.kind)
            .await
            .is_err()
    );
}
