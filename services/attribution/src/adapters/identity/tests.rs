use super::{
    crypto::{self, Cipher},
    postgres::IdentityPostgres,
    tenants::lock_tenant,
};
use crate::{
    application::{freshness::Freshness, identity::*},
    test_support::{Database, identity_http},
};
use chrono::Utc;
use sqlx::Row;
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;
use uuid::Uuid;

struct Fixture {
    _database: Database,
    store: Arc<IdentityPostgres>,
}
impl Fixture {
    async fn new() -> Self {
        let database = Database::new().await;
        crate::adapters::postgres::migrate(&database.pool)
            .await
            .unwrap();
        let http = identity_http(database.pool.clone(), &database.config().public_origin);
        let store = Arc::new(IdentityPostgres {
            pool: database.pool.clone(),
            cipher: http.config.cipher.clone(),
        });
        Self {
            _database: database,
            store,
        }
    }
    fn status(&self, state: LifecycleState, version: i64) -> CompanyStatus {
        CompanyStatus {
            company_id: CompanyId(Uuid::new_v4()),
            version,
            state,
            changed_at: Utc::now(),
        }
    }
}
#[tokio::test]
async fn attribution_sso_login_binding_and_competing_consumers() {
    let f = Fixture::new().await;
    let tx = LoginTransaction {
        id: Uuid::new_v4(),
        state: LoginState(crypto::random()),
        browser: BrowserBinding(crypto::random()),
        pkce: PkceVerifier(crypto::random()),
        return_path: "/settings".into(),
    };
    f.store.begin_login(&tx).await.unwrap();
    assert!(matches!(
        f.store
            .consume_login(&tx.state, &BrowserBinding(crypto::random()))
            .await,
        Err(Failure::Unauthorized)
    ));
    let raw: Vec<u8> = sqlx::query_scalar(
        "SELECT encrypted_pkce FROM attribution_login_transactions WHERE id = $1",
    )
    .bind(tx.id)
    .fetch_one(&f.store.pool)
    .await
    .unwrap();
    assert!(
        !raw.windows(tx.pkce.0.len())
            .any(|s| s == tx.pkce.0.as_bytes())
    );
    assert!(f.store.cipher.open("session", tx.id, &raw).is_err());
    assert!(f.store.cipher.open("login", Uuid::new_v4(), &raw).is_err());
    let (a, b) = tokio::join!(
        f.store.consume_login(&tx.state, &tx.browser),
        f.store.consume_login(&tx.state, &tx.browser)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let successful = a.or(b).unwrap();
    assert_eq!(successful.pkce.0, tx.pkce.0);
    assert_eq!(successful.return_path, "/settings");
    assert!(f.store.consume_login(&tx.state, &tx.browser).await.is_err());
    f.store.begin_login(&tx).await.unwrap();
    sqlx::query("UPDATE attribution_login_transactions SET expires_at = clock_timestamp() - interval '1 second' WHERE id = $1")
        .bind(tx.id).execute(&f.store.pool).await.unwrap();
    assert!(matches!(
        f.store.consume_login(&tx.state, &tx.browser).await,
        Err(Failure::Unauthorized)
    ));
}
#[tokio::test]
async fn attribution_session_rotation_revocation_and_expiry_are_persisted() {
    let f = Fixture::new().await;
    let new = || NewSession {
        id: SessionId(Uuid::new_v4()),
        token: SessionToken(crypto::random()),
        user_id: UserId(Uuid::new_v4()),
        grant: IdentityGrant(crypto::random()),
        csrf: CsrfToken(crypto::random()),
        expires_at: Utc::now() + chrono::Duration::hours(1),
    };
    let first = new();
    f.store.create_session(&first, None).await.unwrap();
    let current = f.store.session(&first.token).await.unwrap();
    assert_eq!(current.grant.0, first.grant.0);
    assert_eq!(current.csrf.0, first.csrf.0);
    let second = new();
    f.store
        .create_session(&second, Some(&first.token))
        .await
        .unwrap();
    assert!(matches!(
        f.store.session(&first.token).await,
        Err(Failure::Unauthorized)
    ));
    sqlx::query("UPDATE attribution_sessions SET idle_expires_at = clock_timestamp() - interval '1 second' WHERE id = $1").bind(second.id.0).execute(&f.store.pool).await.unwrap();
    assert!(matches!(
        f.store.session(&second.token).await,
        Err(Failure::Unauthorized)
    ));
    let third = new();
    f.store.create_session(&third, None).await.unwrap();
    f.store.revoke_session(third.id).await.unwrap();
    assert!(matches!(
        f.store.session(&third.token).await,
        Err(Failure::Unauthorized)
    ));
    let fourth = new();
    f.store.create_session(&fourth, None).await.unwrap();
    sqlx::query("UPDATE attribution_sessions SET created_at = statement_timestamp() - interval '8 hours', absolute_expires_at = statement_timestamp() - interval '1 second', idle_expires_at = statement_timestamp() - interval '1 second' WHERE id = $1")
        .bind(fourth.id.0).execute(&f.store.pool).await.unwrap();
    assert!(matches!(
        f.store.session(&fourth.token).await,
        Err(Failure::Unauthorized)
    ));
}
#[tokio::test]
async fn attribution_company_lifecycle_competing_delivery_and_provision_never_revive_delete() {
    let f = Fixture::new().await;
    let active = f.status(LifecycleState::Active, 1);
    let observation = f.store.observation().await.unwrap();
    let mut deleted = active.clone();
    deleted.version = 2;
    deleted.state = LifecycleState::Deleted;
    let (a, b) = tokio::join!(
        f.store
            .observe(&active, Some("Example"), Some(&observation), None),
        f.store.observe(&deleted, None, None, None)
    );
    assert!(a.is_ok() || matches!(a, Err(Failure::Forbidden)));
    b.unwrap();
    let row = sqlx::query("SELECT state, version, display_name, purge_requested FROM attribution_tenants WHERE company_id = $1").bind(active.company_id.0).fetch_one(&f.store.pool).await.unwrap();
    assert_eq!(row.get::<String, _>("state"), "deleted");
    assert_eq!(row.get::<i64, _>("version"), 2);
    assert!(row.get::<Option<String>, _>("display_name").is_none());
    assert!(row.get::<bool, _>("purge_requested"));
    let mut later = active;
    later.version = 3;
    assert!(matches!(
        f.store.observe(&later, Some("Late"), None, None).await,
        Err(Failure::Forbidden)
    ));
    f.store.observe(&deleted, None, None, None).await.unwrap();
}
#[tokio::test]
async fn attribution_company_lifecycle_equal_version_competitors_conflict() {
    let f = Fixture::new().await;
    let original = f.status(LifecycleState::Active, 1);
    f.store.observe(&original, None, None, None).await.unwrap();
    let mut suspended = original.clone();
    suspended.version = 2;
    suspended.state = LifecycleState::Suspended;
    let mut active = original;
    active.version = 2;
    let (a, b) = tokio::join!(
        f.store.observe(&active, None, None, None),
        f.store.observe(&suspended, None, None, None)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(a, Err(Failure::Conflict)) || matches!(b, Err(Failure::Conflict)));
}
#[tokio::test]
async fn attribution_company_lifecycle_freshness_rejects_clock_rollback_and_cold_process() {
    let f = Fixture::new().await;
    let active = f.status(LifecycleState::Active, 1);
    let started = Instant::now();
    let observation = f.store.observation().await.unwrap();
    f.store
        .observe(&active, Some("Example"), Some(&observation), None)
        .await
        .unwrap();
    let freshness = Freshness::default();
    let ticket = freshness.record(&active, started).unwrap();
    f.store
        .check_tenant(active.company_id, &ticket)
        .await
        .unwrap();
    assert!(
        Freshness::default().ticket(active.company_id).is_none(),
        "fresh durable rows cannot reconstruct process-local age"
    );
    // The wall-clock predicate is made true as it would be following a rollback.
    sqlx::query("UPDATE attribution_tenants SET status_valid_until = clock_timestamp() + interval '1 day' WHERE company_id = $1").bind(active.company_id.0).execute(&f.store.pool).await.unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(301)).await;
    tokio::time::resume();
    assert!(matches!(
        f.store.check_tenant(active.company_id, &ticket).await,
        Err(Failure::Unavailable)
    ));
    let mut tx = f.store.pool.begin().await.unwrap();
    assert!(matches!(
        lock_tenant(&mut tx, active.company_id, &ticket).await,
        Err(Failure::Unavailable)
    ));
    tx.rollback().await.unwrap();
    assert!(
        freshness.record(&active, started).is_err(),
        "late response cannot renew an elapsed request budget"
    );
}
#[tokio::test]
async fn attribution_company_lifecycle_stale_observations_and_deliveries_cannot_refresh() {
    let f = Fixture::new().await;
    let active = f.status(LifecycleState::Active, 1);
    let mut older = f.store.observation().await.unwrap();
    let newer = f.store.observation().await.unwrap();
    f.store
        .observe(&active, Some("Current"), Some(&newer), None)
        .await
        .unwrap();
    older.started_at = Utc::now() + chrono::Duration::days(1);
    f.store
        .observe(&active, Some("Stale"), Some(&older), None)
        .await
        .unwrap();
    f.store.observe(&active, None, None, None).await.unwrap();
    let row = sqlx::query("SELECT display_name, observation_number, status_valid_until FROM attribution_tenants WHERE company_id = $1").bind(active.company_id.0).fetch_one(&f.store.pool).await.unwrap();
    assert_eq!(row.get::<String, _>("display_name"), "Current");
    assert_eq!(row.get::<i64, _>("observation_number"), newer.number);
    assert_eq!(
        row.get::<chrono::DateTime<Utc>, _>("status_valid_until")
            .timestamp_micros(),
        (newer.started_at + chrono::Duration::minutes(5)).timestamp_micros()
    );
}
#[tokio::test]
async fn attribution_company_lifecycle_reconciliation_fences_and_poison_backoff() {
    let f = Fixture::new().await;
    for _ in 0..4 {
        f.store
            .observe(&f.status(LifecycleState::Active, 1), None, None, None)
            .await
            .unwrap();
    }
    let (a, b) = tokio::join!(
        f.store.claim_reconciliation(),
        f.store.claim_reconciliation()
    );
    let mut all = a.unwrap();
    all.extend(b.unwrap());
    assert_eq!(all.len(), 4);
    let unique: std::collections::HashSet<_> = all.iter().map(|v| v.company_id).collect();
    assert_eq!(unique.len(), 4);
    for claim in &all {
        f.store.finish_reconciliation(claim, false).await.unwrap();
    }
    assert!(
        f.store.claim_reconciliation().await.unwrap().is_empty(),
        "poison batch must leave immediate claim set"
    );
    assert!(matches!(
        f.store.finish_reconciliation(&all[0], true).await,
        Err(Failure::Conflict)
    ));
}
#[test]
fn attribution_session_encryption_rotation_and_return_paths() {
    use base64::Engine;
    assert_eq!(
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(crypto::random())
            .unwrap()
            .len(),
        32
    );
    let key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([1u8; 32]);
    let next = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([2u8; 32]);
    let first = Cipher::from_json(
        &serde_json::json!({"active":1,"keys":[{"version":1,"key":key}]}).to_string(),
    )
    .unwrap();
    let second = Cipher::from_json(
        &serde_json::json!({"active":2,"keys":[{"version":1,"key":key},{"version":2,"key":next}]})
            .to_string(),
    )
    .unwrap();
    let id = Uuid::new_v4();
    let sealed = first.seal("session", id, b"secret").unwrap();
    assert_eq!(second.open("session", id, &sealed).unwrap(), b"secret");
    let mut corrupt = sealed;
    corrupt[30] ^= 1;
    assert!(second.open("session", id, &corrupt).is_err());
    for path in [
        "https://evil.test",
        "//evil.test",
        "/%2fevil",
        "/\\evil",
        "/\nevil",
    ] {
        assert!(crypto::local_return(path).is_err());
    }
    assert_eq!(
        crypto::local_return("/settings?company_id=test").unwrap(),
        "/settings?company_id=test"
    );
    assert!(verify_csrf(&CsrfToken("expected".into()), "wrong").is_err());
}

#[tokio::test]
async fn attribution_company_lifecycle_lost_reconciliation_lease_cannot_store_observation() {
    let f = Fixture::new().await;
    let active = f.status(LifecycleState::Active, 1);
    f.store.observe(&active, None, None, None).await.unwrap();
    let first = f.store.claim_reconciliation().await.unwrap().pop().unwrap();
    sqlx::query("UPDATE attribution_tenants SET reconcile_lease_until = clock_timestamp() - interval '1 second' WHERE company_id = $1")
        .bind(active.company_id.0).execute(&f.store.pool).await.unwrap();
    let current = f.store.claim_reconciliation().await.unwrap().pop().unwrap();
    assert_ne!(first.execution, current.execution);
    let observation = f.store.observation().await.unwrap();
    assert!(matches!(
        f.store
            .observe(&active, Some("stale"), Some(&observation), Some(&first))
            .await,
        Err(Failure::Conflict)
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM attribution_tenants WHERE company_id = $1 AND status_valid_until IS NOT NULL")
        .bind(active.company_id.0).fetch_one(&f.store.pool).await.unwrap();
    assert_eq!(count, 0);
    f.store
        .observe(&active, Some("current"), Some(&observation), Some(&current))
        .await
        .unwrap();
    f.store.finish_reconciliation(&current, true).await.unwrap();
}

#[tokio::test]
async fn attribution_company_lifecycle_waiting_writer_rechecks_expiry_and_suspension() {
    let f = Fixture::new().await;
    for suspend in [false, true] {
        let active = f.status(LifecycleState::Active, 1);
        let observation = f.store.observation().await.unwrap();
        f.store
            .observe(&active, Some("original"), Some(&observation), None)
            .await
            .unwrap();
        let ticket = Freshness::default()
            .record(&active, Instant::now())
            .unwrap();
        let mut holder = f.store.pool.begin().await.unwrap();
        sqlx::query("SELECT company_id FROM attribution_tenants WHERE company_id = $1 FOR UPDATE")
            .bind(active.company_id.0)
            .fetch_one(&mut *holder)
            .await
            .unwrap();
        if suspend {
            sqlx::query("UPDATE attribution_tenants SET state = 'suspended', version = 2, status_valid_until = NULL WHERE company_id = $1")
                .bind(active.company_id.0).execute(&mut *holder).await.unwrap();
        }
        let pool = f.store.pool.clone();
        let company = active.company_id;
        let (pid_tx, pid_rx) = tokio::sync::oneshot::channel();
        let writer = tokio::spawn(async move {
            let mut tx = pool.begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            pid_tx.send(pid).unwrap();
            lock_tenant(&mut tx, company, &ticket).await?;
            sqlx::query("UPDATE attribution_tenants SET display_name = 'unauthorized write' WHERE company_id = $1")
                .bind(company.0).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            Ok::<_, Failure>(())
        });
        let pid = pid_rx.await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let blocked: bool =
                    sqlx::query_scalar("SELECT cardinality(pg_blocking_pids($1)) > 0")
                        .bind(pid)
                        .fetch_one(&f.store.pool)
                        .await
                        .unwrap();
                if blocked {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("writer must actually wait on the lifecycle lock");
        if !suspend {
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(301)).await;
            tokio::time::resume();
        }
        holder.commit().await.unwrap();
        let result = writer.await.unwrap();
        assert!(matches!(
            (suspend, result),
            (true, Err(Failure::Forbidden)) | (false, Err(Failure::Unavailable))
        ));
        let name: String = sqlx::query_scalar(
            "SELECT display_name FROM attribution_tenants WHERE company_id = $1",
        )
        .bind(company.0)
        .fetch_one(&f.store.pool)
        .await
        .unwrap();
        assert_eq!(name, "original");
    }
}
