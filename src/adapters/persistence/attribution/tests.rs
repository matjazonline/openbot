use super::*;
use crate::{
    adapters::persistence::test_support::{OwnDatabase, insert_company_with_owner, own_database},
    application::{
        ports::account_deletion::HostAccountDeletion,
        session::{SessionPersistence, SessionRecord},
        use_cases::{company::CompanyPersistence, user::UserPersistence},
    },
    infra::attribution::hash,
};
struct Fixture {
    _database: OwnDatabase,
    store: PostgresPersistence,
    user: Uuid,
    actor: VerifiedSession,
}
impl Fixture {
    async fn new() -> Self {
        let database = own_database()
            .await
            .expect("attribution tests require PostgreSQL");
        let store = PostgresPersistence::new(database.pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let user = store
            .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
            .await
            .unwrap()
            .id;
        let record = SessionRecord {
            id: SessionId(Uuid::new_v4()),
            user_id: user,
            auth_revision: 0,
            issued_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::hours(2),
        };
        store.create_session(&record).await.unwrap();
        Self {
            _database: database,
            store,
            user,
            actor: VerifiedSession {
                user_id: user,
                session_id: record.id,
            },
        }
    }
    async fn company(&self) -> Uuid {
        let id = Uuid::new_v4();
        insert_company_with_owner(
            &self.store.pool,
            id,
            "Attribution",
            &id.simple().to_string(),
            self.user,
        )
        .await;
        id
    }
    async fn code(&self) -> RedeemCode {
        let (challenge, _) = oauth2::PkceCodeChallenge::new_random_sha256();
        let code = hash(oauth2::CsrfToken::new_random_len(32).secret());
        let binding = AuthorizationBinding {
            callback: Callback("https://attribution.example.test/auth/callback".into()),
            challenge: Challenge(challenge.as_str().into()),
            state: RequestState(oauth2::CsrfToken::new_random_len(32).secret().clone()),
        };
        self.store
            .issue_code(IssueCode {
                actor: self.actor,
                source: AuthorizationSource::Direct(binding.clone()),
                code: code.clone(),
            })
            .await
            .unwrap();
        RedeemCode {
            code,
            callback: binding.callback,
            challenge: binding.challenge,
            grant: hash(oauth2::CsrfToken::new_random_len(32).secret()),
        }
    }
    async fn grant(&self) -> SecretHash {
        let redemption = self.code().await;
        let grant = redemption.grant.clone();
        self.store.redeem(redemption).await.unwrap();
        grant
    }
}
#[tokio::test]
async fn attribution_sso_code_binding_competing_redeemers_and_parent_revocation() {
    let f = Fixture::new().await;
    let first = f.code().await;
    let wrong = RedeemCode {
        code: first.code.clone(),
        callback: first.callback.clone(),
        challenge: Challenge("wrong".into()),
        grant: hash("wrong-grant"),
    };
    assert!(matches!(
        f.store.redeem(wrong).await,
        Err(IdentityError::Unauthorized)
    ));
    let second = RedeemCode {
        code: first.code.clone(),
        callback: first.callback.clone(),
        challenge: first.challenge.clone(),
        grant: hash("other-grant"),
    };
    let first_grant = first.grant.clone();
    let second_grant = second.grant.clone();
    let (a, b) = tokio::join!(f.store.redeem(first), f.store.redeem(second));
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let grant = if a.is_ok() { first_grant } else { second_grant };
    assert_eq!(f.store.identity(&grant).await.unwrap().user_id, f.user);
    f.store.revoke_session(f.actor).await.unwrap();
    assert!(matches!(
        f.store.identity(&grant).await,
        Err(IdentityError::Unauthorized)
    ));
    assert!(matches!(
        f.store
            .issue_code(IssueCode {
                actor: f.actor,
                source: AuthorizationSource::Direct(AuthorizationBinding {
                    callback: Callback("https://attribution.example.test/auth/callback".into()),
                    challenge: Challenge("a".repeat(43)),
                    state: RequestState("b".repeat(43))
                }),
                code: hash("new-code")
            })
            .await,
        Err(IdentityError::Unauthorized)
    ));
}
#[tokio::test]
async fn attribution_company_access_current_membership_roles_and_security_revision() {
    let f = Fixture::new().await;
    let company = f.company().await;
    let grant = f.grant().await;
    assert_eq!(
        f.store.access(&grant, company).await.unwrap().permissions,
        vec!["read", "links:manage", "integrations:manage"]
    );
    assert!(matches!(
        f.store.access(&grant, Uuid::new_v4()).await,
        Err(IdentityError::NotFound)
    ));
    let page = f
        .store
        .companies(CompanyPageRequest {
            grant: grant.clone(),
            after: None,
            limit: 1,
        })
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].company_id, company);
    sqlx::query("UPDATE users SET password_hash = 'changed-hash' WHERE id = $1")
        .bind(f.user)
        .execute(&f.store.pool)
        .await
        .unwrap();
    assert!(matches!(
        f.store.identity(&grant).await,
        Err(IdentityError::Unauthorized)
    ));
}
#[tokio::test]
async fn attribution_company_lifecycle_owning_deletions_commit_and_rollback_atomically() {
    let f = Fixture::new().await;
    let direct = f.company().await;
    CompanyPersistence::delete(&f.store, direct).await.unwrap();
    let deleted = f.store.status(direct).await.unwrap();
    assert_eq!(deleted.state, CompanyState::Deleted);
    assert_eq!(deleted.version, 2);
    let owned = f.company().await;
    sqlx::raw_sql("CREATE FUNCTION attribution_test_reject_delete() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test rollback'; END $$; CREATE TRIGGER attribution_test_reject_delete BEFORE DELETE ON users FOR EACH ROW EXECUTE FUNCTION attribution_test_reject_delete();").execute(&f.store.pool).await.unwrap();
    assert!(f.store.delete_user_account(f.user).await.is_err());
    assert_eq!(
        f.store.status(owned).await.unwrap().state,
        CompanyState::Active
    );
    let notifications: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM attribution_lifecycle_outbox WHERE company_id = $1",
    )
    .bind(owned)
    .fetch_one(&f.store.pool)
    .await
    .unwrap();
    assert_eq!(notifications, 1);
    sqlx::raw_sql("DROP TRIGGER attribution_test_reject_delete ON users; DROP FUNCTION attribution_test_reject_delete();").execute(&f.store.pool).await.unwrap();
    f.store.delete_user_account(f.user).await.unwrap();
    assert_eq!(
        f.store.status(owned).await.unwrap().state,
        CompanyState::Deleted
    );
    let notifications:i64=sqlx::query_scalar("SELECT count(*) FROM attribution_lifecycle_outbox WHERE company_id = $1 AND state = 'deleted'").bind(owned).fetch_one(&f.store.pool).await.unwrap();
    assert_eq!(notifications, 1);
    let mut tx = f.store.pool.begin().await.unwrap();
    assert!(
        sqlx::query(
            "UPDATE attribution_company_lifecycles SET state = 'active' WHERE company_id = $1"
        )
        .bind(owned)
        .execute(&mut *tx)
        .await
        .is_err()
    );
    tx.rollback().await.unwrap();
}
#[tokio::test]
async fn attribution_company_lifecycle_outbox_competitors_fences_poison_and_exhaustion() {
    let f = Fixture::new().await;
    for _ in 0..4 {
        f.company().await;
    }
    let (a, b) = tokio::join!(f.store.claim(), f.store.claim());
    let mut all = a.unwrap();
    all.extend(b.unwrap());
    assert_eq!(all.len(), 4);
    let ids: std::collections::HashSet<_> = all.iter().map(|v| v.id).collect();
    assert_eq!(ids.len(), 4);
    for claim in &all {
        f.store
            .complete(claim, DeliveryResult::Unavailable)
            .await
            .unwrap();
    }
    assert!(f.store.claim().await.unwrap().is_empty());
    let old = &all[0];
    sqlx::query("UPDATE attribution_lifecycle_outbox SET available_at = clock_timestamp() - interval '1 second' WHERE id = $1").bind(old.id).execute(&f.store.pool).await.unwrap();
    let replacement = f.store.claim().await.unwrap().pop().unwrap();
    assert_eq!(old.id, replacement.id);
    assert_ne!(old.execution, replacement.execution);
    assert!(matches!(
        f.store.complete(old, DeliveryResult::Delivered).await,
        Err(IdentityError::Conflict)
    ));
    sqlx::query("UPDATE attribution_lifecycle_outbox SET attempts = 12, lease_until = clock_timestamp() - interval '1 second' WHERE id = $1").bind(replacement.id).execute(&f.store.pool).await.unwrap();
    assert!(f.store.claim().await.unwrap().is_empty());
    let row=sqlx::query("SELECT delivery_state, execution_id, last_failure FROM attribution_lifecycle_outbox WHERE id = $1").bind(old.id).fetch_one(&f.store.pool).await.unwrap();
    assert_eq!(row.get::<String, _>("delivery_state"), "failed");
    assert!(row.get::<Option<Uuid>, _>("execution_id").is_none());
    assert_eq!(row.get::<String, _>("last_failure"), "lease_expired");
}

#[path = "http_tests.rs"]
mod http_contract;

#[path = "worker_tests.rs"]
mod worker_tests;
