//! DB-backed `memory_authority_` cases (01B, D3, D13). Require MEMORY_TEST_DATABASE_URL.
use super::test_support::{TestDb, test_db};
use crate::{
    application::{
        operations::{Surface, by_id},
        service::{App, Principal},
    },
    domain::{
        admin_policy::AdminScope,
        credential::{self, TokenKind},
        error::ErrorCode,
    },
};
use chrono::Utc;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

struct Fixture {
    db: TestDb,
    app: App,
    operator: Principal,
}

async fn fixture() -> Fixture {
    let db = test_db().await;
    let app = App::new(
        Arc::new(db.store.clone()),
        crate::application::test_embedder::fake_pool(),
    );
    let issued = credential::issue(TokenKind::Admin).unwrap();
    app.store()
        .bootstrap_operator(issued.id, "operator", issued.verifier)
        .await
        .unwrap();
    let operator = app
        .authenticate(Some(&issued.token), Surface::Admin, Utc::now())
        .await
        .unwrap();
    Fixture { db, app, operator }
}

impl Fixture {
    async fn run(&self, principal: &Principal, id: &str, input: Value) -> Result<Value, ErrorCode> {
        self.run_as(principal, id, input, Uuid::now_v7()).await
    }
    async fn run_as(
        &self,
        principal: &Principal,
        id: &str,
        input: Value,
        request_id: Uuid,
    ) -> Result<Value, ErrorCode> {
        self.app
            .execute(by_id(id).unwrap(), principal, input, Utc::now(), request_id)
            .await
            .map_err(|error| error.code)
    }
    async fn admin_token(&self, scope: AdminScope) -> (String, Principal) {
        let issued = self
            .run(
                &self.operator,
                "admin.admin_credentials.issue",
                json!({"scope": scope, "label": "admin", "expires_at": null}),
            )
            .await
            .unwrap();
        let token = issued["token"].as_str().unwrap().to_owned();
        let principal = self
            .app
            .authenticate(Some(&token), Surface::Admin, Utc::now())
            .await
            .unwrap();
        (token, principal)
    }
    async fn tenant(&self, admin: &Principal, tenant: &str, namespaces: &[&str]) {
        self.run(
            admin,
            "admin.tenants.create",
            json!({"external_id": tenant}),
        )
        .await
        .unwrap();
        for namespace in namespaces {
            self.run(
                admin,
                "admin.namespaces.create",
                json!({"tenant": tenant, "namespace": namespace}),
            )
            .await
            .unwrap();
        }
    }
    async fn client(
        &self,
        admin: &Principal,
        tenant: &str,
        grants: Value,
    ) -> Result<String, ErrorCode> {
        let issued = self
            .run(
                admin,
                "admin.credentials.issue",
                json!({"tenant": tenant, "label": "client", "expires_at": null, "grants": grants}),
            )
            .await?;
        Ok(issued["token"].as_str().unwrap().to_owned())
    }
    async fn login(&self, token: &str) -> Result<Principal, ErrorCode> {
        self.app
            .authenticate(Some(token), Surface::Client, Utc::now())
            .await
            .map_err(|error| error.code)
    }
    async fn queue_operation(
        &self,
        tenant: &str,
        namespace: &str,
        token: &str,
        state: &str,
    ) -> Uuid {
        let id = Uuid::now_v7();
        let credential = credential::parse(token).unwrap().id;
        sqlx::query(
            "INSERT INTO generation_operations (tenant_id, namespace_id, id, credential_id, \
             idempotency_key, state, producing_ceiling, watermark_kind, watermark_level, input_level, \
             allowance_tokens, max_output_tokens, expires_at, lease_until, fence, request) \
             SELECT t.id, n.id, $3, $4, $5, $6, 6, 'ceiling_fallback', 6, 6, 1000, 100, \
                    now() + interval '1 day', \
                    CASE WHEN $6 IN ('claimed','dispatched') THEN now() + interval '90 seconds' END, 1, \
                    jsonb_build_object('v', 1, 'events', jsonb_build_array()) \
               FROM tenants t JOIN namespaces n ON n.tenant_id = t.id \
              WHERE t.external_id = $1 AND n.external_id = $2",
        )
        .bind(tenant)
        .bind(namespace)
        .bind(id)
        .bind(credential)
        .bind(id.to_string())
        .bind(state)
        .execute(self.db.pool())
        .await
        .unwrap();
        id
    }
    async fn operation(&self, id: Uuid) -> (String, Option<String>, bool) {
        let row: (String, Option<String>, bool) = sqlx::query_as(
            "SELECT state, outcome_code, cancel_requested_at IS NOT NULL FROM generation_operations WHERE id = $1",
        )
        .bind(id)
        .fetch_one(self.db.pool())
        .await
        .unwrap();
        row
    }
}

/// A usable grant: write operations only when a write bound is given (m5).
fn grant(namespace: &str, read: i64, write: Option<i64>) -> Value {
    let operations = if write.is_some() {
        json!(["search", "save", "fetch", "generate"])
    } else {
        json!(["search", "fetch"])
    };
    json!({"namespace": namespace, "read_max": read, "write_max": write, "operations": operations})
}

#[tokio::test]
async fn memory_authority_credentials_verify_expire_and_revoke() {
    let f = fixture().await;
    let op = f.operator.clone();
    f.tenant(&op, "tenant-a", &["ns-a"]).await;
    let token = f
        .client(&op, "tenant-a", json!([grant("ns-a", 6, Some(6))]))
        .await
        .unwrap();
    let Principal::Client(client) = f.login(&token).await.unwrap() else {
        panic!("client principal")
    };
    assert_eq!(client.tenant, "tenant-a");
    assert_eq!(client.grants.len(), 1);
    // A wrong secret for a real id and an unknown id are the same 401.
    let mut wrong = token.clone();
    wrong.replace_range(wrong.len() - 4.., "AAAA");
    assert_eq!(
        f.login(&wrong).await.unwrap_err(),
        ErrorCode::Unauthenticated
    );
    let unknown = credential::issue(TokenKind::Client).unwrap().token;
    assert_eq!(
        f.login(&unknown).await.unwrap_err(),
        ErrorCode::Unauthenticated
    );
    // Client tokens never authenticate the admin surface, and vice versa.
    assert!(
        f.app
            .authenticate(Some(&token), Surface::Admin, Utc::now())
            .await
            .is_err()
    );
    // Expiry is enforced at authentication time.
    let expiring = f
        .run(&op, "admin.credentials.issue", json!({"tenant": "tenant-a", "label": "short",
             "expires_at": (Utc::now() + chrono::Duration::seconds(60)), "grants": [grant("ns-a", 1, None)]}))
        .await
        .unwrap();
    let expiring = expiring["token"].as_str().unwrap();
    assert!(f.login(expiring).await.is_ok());
    let later = Utc::now() + chrono::Duration::seconds(120);
    assert_eq!(
        f.app
            .authenticate(Some(expiring), Surface::Client, later)
            .await
            .unwrap_err()
            .code,
        ErrorCode::CredentialExpired
    );
    // Revocation cancels queued work the credential authorized and stops authentication.
    let queued = f
        .queue_operation("tenant-a", "ns-a", &token, "accepted")
        .await;
    let dispatched = f
        .queue_operation("tenant-a", "ns-a", &token, "dispatched")
        .await;
    let id = credential::parse(&token).unwrap().id;
    let revoked = f
        .run(
            &op,
            "admin.credentials.revoke",
            json!({"tenant": "tenant-a", "credential_id": id}),
        )
        .await
        .unwrap();
    assert!(revoked["revoked_at"].is_string());
    assert_eq!(
        f.login(&token).await.unwrap_err(),
        ErrorCode::CredentialRevoked
    );
    assert_eq!(
        f.operation(queued).await,
        ("cancelled".into(), Some("credential_revoked".into()), false)
    );
    assert_eq!(f.operation(dispatched).await.0, "dispatched");
    assert!(
        f.operation(dispatched).await.2,
        "a dispatched operation is flagged, not freed"
    );
}

#[tokio::test]
async fn memory_authority_tenants_are_isolated_and_foreign_ids_reveal_nothing() {
    let f = fixture().await;
    let op = f.operator.clone();
    f.tenant(&op, "tenant-a", &["shared-name"]).await;
    f.tenant(&op, "tenant-b", &["shared-name"]).await;
    let a = f
        .client(&op, "tenant-a", json!([grant("shared-name", 9, Some(9))]))
        .await
        .unwrap();
    let b = f
        .client(&op, "tenant-b", json!([grant("shared-name", 9, Some(9))]))
        .await
        .unwrap();
    let (Principal::Client(a), Principal::Client(b)) =
        (f.login(&a).await.unwrap(), f.login(&b).await.unwrap())
    else {
        panic!("clients")
    };
    assert_ne!(a.tenant_id, b.tenant_id);
    assert_ne!(a.grants[0].namespace_id, b.grants[0].namespace_id);
    // Granting a namespace of another tenant is impossible: names resolve within the tenant.
    assert_eq!(
        f.client(&op, "tenant-a", json!([grant("only-in-b", 1, None)]))
            .await
            .unwrap_err(),
        ErrorCode::InvalidRequest
    );
    // A foreign credential id is indistinguishable from a missing one.
    assert_eq!(
        f.run(
            &op,
            "admin.credentials.revoke",
            json!({"tenant": "tenant-a", "credential_id": b.credential_id})
        )
        .await
        .unwrap_err(),
        ErrorCode::NotFound
    );
    // The credential endpoint lists only the caller's own grants.
    let view = f
        .run(&Principal::Client(a.clone()), "credential.get", json!({}))
        .await
        .unwrap();
    assert_eq!(view["tenant"], "tenant-a");
    assert_eq!(view["grants"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn memory_authority_provisioner_is_bounded_to_tenants_it_created() {
    let f = fixture().await;
    let op = f.operator.clone();
    let (_, prov) = f.admin_token(AdminScope::Provisioner).await;
    let (_, other) = f.admin_token(AdminScope::Provisioner).await;
    f.tenant(&op, "operator-tenant", &["ns"]).await;
    f.tenant(&prov, "prov-tenant", &["ns"]).await;
    // Outside its own tenants everything is not-found, including credential minting.
    for (id, input) in [
        (
            "admin.tenants.disable",
            json!({"tenant": "operator-tenant"}),
        ),
        (
            "admin.namespaces.create",
            json!({"tenant": "operator-tenant", "namespace": "x"}),
        ),
        (
            "admin.credentials.list",
            json!({"tenant": "operator-tenant"}),
        ),
    ] {
        assert_eq!(
            f.run(&prov, id, input).await.unwrap_err(),
            ErrorCode::NotFound,
            "{id}"
        );
    }
    assert_eq!(
        f.client(&prov, "operator-tenant", json!([grant("ns", 9, Some(9))]))
            .await
            .unwrap_err(),
        ErrorCode::NotFound
    );
    assert_eq!(
        f.run(
            &other,
            "admin.tenants.disable",
            json!({"tenant": "prov-tenant"})
        )
        .await
        .unwrap_err(),
        ErrorCode::NotFound
    );
    // Within its tenants it can mint a client credential: transitive data authority.
    assert!(
        f.client(&prov, "prov-tenant", json!([grant("ns", 9, Some(9))]))
            .await
            .is_ok()
    );
    // Operator-only actions.
    assert_eq!(
        f.run(
            &prov,
            "admin.tenants.settings",
            json!({"tenant": "prov-tenant", "generation_daily_token_cap": 5})
        )
        .await
        .unwrap_err(),
        ErrorCode::Forbidden
    );
    assert_eq!(
        f.run(
            &prov,
            "admin.admin_credentials.issue",
            json!({"scope": "operator", "label": "x", "expires_at": null})
        )
        .await
        .unwrap_err(),
        ErrorCode::Forbidden
    );
    // Client tokens cannot use the admin surface at all.
    let client = f
        .client(&prov, "prov-tenant", json!([grant("ns", 1, None)]))
        .await
        .unwrap();
    assert!(
        f.app
            .authenticate(Some(&client), Surface::Admin, Utc::now())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn memory_authority_operator_disable_is_sticky_against_provisioner_disable_then_enable() {
    let f = fixture().await;
    let op = f.operator.clone();
    let (_, prov) = f.admin_token(AdminScope::Provisioner).await;
    f.tenant(&prov, "t", &["ns"]).await;
    let token = f
        .client(&prov, "t", json!([grant("ns", 6, Some(6))]))
        .await
        .unwrap();
    let view = f
        .run(&op, "admin.tenants.disable", json!({"tenant": "t"}))
        .await
        .unwrap();
    assert_eq!(view["disabled_by_scope"], "operator");
    assert_eq!(
        f.login(&token).await.unwrap_err(),
        ErrorCode::TenantDisabled
    );
    // R1: a provisioner re-disable is a no-op that keeps the operator as disabler…
    let view = f
        .run(&prov, "admin.tenants.disable", json!({"tenant": "t"}))
        .await
        .unwrap();
    assert_eq!(view["disabled_by_scope"], "operator");
    // …so its enable is still refused.
    assert_eq!(
        f.run(&prov, "admin.tenants.enable", json!({"tenant": "t"}))
            .await
            .unwrap_err(),
        ErrorCode::Forbidden
    );
    // The operator may enable; a provisioner-only disable may be undone by the provisioner.
    f.run(&op, "admin.tenants.enable", json!({"tenant": "t"}))
        .await
        .unwrap();
    f.run(&prov, "admin.tenants.disable", json!({"tenant": "t"}))
        .await
        .unwrap();
    let view = f
        .run(&prov, "admin.tenants.enable", json!({"tenant": "t"}))
        .await
        .unwrap();
    assert_eq!(view["state"], "active");
    assert!(f.login(&token).await.is_ok());
}

#[tokio::test]
async fn memory_authority_disable_cancels_queued_work_and_enable_never_revives_it() {
    let f = fixture().await;
    let op = f.operator.clone();
    f.tenant(&op, "t", &["ns"]).await;
    let token = f
        .client(&op, "t", json!([grant("ns", 6, Some(6))]))
        .await
        .unwrap();
    let accepted = f.queue_operation("t", "ns", &token, "accepted").await;
    let view = f
        .run(&op, "admin.tenants.disable", json!({"tenant": "t"}))
        .await
        .unwrap();
    assert_eq!(view["cancelled_operations"], 1);
    f.run(&op, "admin.tenants.enable", json!({"tenant": "t"}))
        .await
        .unwrap();
    assert_eq!(f.operation(accepted).await.0, "cancelled");
    // Delete is permanent and the tenant id can never be recreated.
    f.run(&op, "admin.tenants.delete", json!({"tenant": "t"}))
        .await
        .unwrap();
    assert_eq!(
        f.login(&token).await.unwrap_err(),
        ErrorCode::CredentialRevoked
    );
    assert_eq!(
        f.run(&op, "admin.tenants.create", json!({"external_id": "t"}))
            .await
            .unwrap_err(),
        ErrorCode::Conflict
    );
    assert_eq!(
        f.run(&op, "admin.tenants.enable", json!({"tenant": "t"}))
            .await
            .unwrap_err(),
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn memory_authority_disable_races_competing_claimers_without_revival() {
    let f = fixture().await;
    let op = f.operator.clone();
    f.tenant(&op, "t", &["ns"]).await;
    let token = f
        .client(&op, "t", json!([grant("ns", 6, Some(6))]))
        .await
        .unwrap();
    let mut ops = Vec::new();
    for _ in 0..20 {
        ops.push(f.queue_operation("t", "ns", &token, "accepted").await);
    }
    // Competing claimers follow the D13 order (tenant FOR SHARE SKIP LOCKED, then the
    // operation) while the disable runs; afterwards no accepted or claimed row may remain.
    let pool = f.db.pool().clone();
    let claimers: Vec<_> = (0..4)
        .map(|_| {
            let pool = pool.clone();
            tokio::spawn(async move {
                for _ in 0..10 {
                    let mut tx = pool.begin().await.unwrap();
                    let tenant: Option<Uuid> = sqlx::query_scalar(
                        "SELECT id FROM tenants WHERE external_id = 't' AND state = 'active' FOR SHARE SKIP LOCKED",
                    )
                    .fetch_optional(&mut *tx)
                    .await
                    .unwrap();
                    if let Some(tenant) = tenant {
                        let _ = sqlx::query(
                            "UPDATE generation_operations SET state = 'claimed', lease_until = now() + interval '90 seconds' \
                             WHERE (tenant_id, id) IN (SELECT tenant_id, id FROM generation_operations \
                             WHERE tenant_id = $1 AND state = 'accepted' ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED) \
                             AND NOT EXISTS (SELECT 1 FROM generation_operations WHERE tenant_id = $1 AND state IN ('claimed','dispatched'))",
                        )
                        .bind(tenant)
                        .execute(&mut *tx)
                        .await;
                    }
                    let _ = tx.commit().await;
                    tokio::task::yield_now().await;
                }
            })
        })
        .collect();
    let disabled = f
        .run(&op, "admin.tenants.disable", json!({"tenant": "t"}))
        .await;
    for claimer in claimers {
        claimer.await.unwrap();
    }
    assert!(disabled.is_ok());
    // A claim that committed before the disable was cancelled by it; one attempted after it
    // was skipped because the tenant is no longer active.
    f.run(&op, "admin.tenants.enable", json!({"tenant": "t"}))
        .await
        .unwrap();
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM generation_operations WHERE state IN ('accepted','claimed')",
    )
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(
        live, 0,
        "no queued inference survives a disable/enable cycle"
    );
}

#[tokio::test]
async fn memory_authority_namespace_delete_races_grant_issue_and_leaves_no_grant() {
    let f = Arc::new(fixture().await);
    let op = f.operator.clone();
    f.tenant(&op, "t", &[]).await;
    for round in 0..6 {
        let namespace = format!("ns-{round}");
        f.run(
            &op,
            "admin.namespaces.create",
            json!({"tenant": "t", "namespace": namespace}),
        )
        .await
        .unwrap();
        let issue = {
            let f = f.clone();
            let op = op.clone();
            let namespace = namespace.clone();
            tokio::spawn(async move {
                f.client(&op, "t", json!([grant(&namespace, 3, None)]))
                    .await
            })
        };
        let delete = {
            let f = f.clone();
            let op = op.clone();
            let namespace = namespace.clone();
            tokio::spawn(async move {
                f.run(
                    &op,
                    "admin.namespaces.delete",
                    json!({"tenant": "t", "namespace": namespace}),
                )
                .await
            })
        };
        let (issued, deleted) = (issue.await.unwrap(), delete.await.unwrap());
        assert!(deleted.is_ok());
        // Either order is legal; afterwards no grant may reference the deleted namespace.
        if let Ok(token) = issued {
            let Principal::Client(client) = f.login(&token).await.unwrap() else {
                panic!()
            };
            assert!(
                client.grants.is_empty(),
                "GET /v1/credential never lists a deleted namespace"
            );
        }
        let grants: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM credential_grants g JOIN namespaces n \
             ON (n.tenant_id, n.id) = (g.tenant_id, g.namespace_id) WHERE n.state = 'deleted'",
        )
        .fetch_one(f.db.pool())
        .await
        .unwrap();
        assert_eq!(grants, 0);
        // The tombstone blocks recreation, visibly only to admin create.
        assert_eq!(
            f.run(
                &op,
                "admin.namespaces.create",
                json!({"tenant": "t", "namespace": namespace})
            )
            .await
            .unwrap_err(),
            ErrorCode::NamespaceDeleted
        );
    }
}

#[tokio::test]
async fn memory_authority_bootstrap_is_single_and_audited() {
    let f = fixture().await;
    let again = credential::issue(TokenKind::Admin).unwrap();
    assert_eq!(
        f.app
            .store()
            .bootstrap_operator(again.id, "second", again.verifier)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    let audited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = 'admin.bootstrap'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(audited, 1);
    assert!(
        sqlx::query("UPDATE audit_events SET outcome = 'x'")
            .execute(f.db.pool())
            .await
            .is_err()
    );
}

fn admin_id(principal: &Principal) -> Uuid {
    match principal {
        Principal::Admin(actor) => actor.id,
        Principal::Client(_) => panic!("admin principal expected"),
    }
}

async fn namespaces_of(f: &Fixture, tenant: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM namespaces n JOIN tenants t ON t.id = n.tenant_id \
         WHERE t.external_id = $1",
    )
    .bind(tenant)
    .fetch_one(f.db.pool())
    .await
    .unwrap()
}

/// m1: authentication reads the admin credential outside the write transaction; every admin
/// write re-checks it under `FOR SHARE`, so a revocation or expiry is never outrun.
#[tokio::test]
async fn memory_authority_admin_writes_recheck_their_credential_against_revocation() {
    let f = Arc::new(fixture().await);
    let op = f.operator.clone();
    let (_, prov) = f.admin_token(AdminScope::Provisioner).await;
    let prov_id = admin_id(&prov);
    f.tenant(&prov, "t", &[]).await;

    // A revocation committed after authentication refuses the stale principal.
    let (_, stale) = f.admin_token(AdminScope::Provisioner).await;
    f.run(
        &op,
        "admin.admin_credentials.revoke",
        json!({"credential_id": admin_id(&stale)}),
    )
    .await
    .unwrap();
    assert_eq!(
        f.run(
            &stale,
            "admin.tenants.create",
            json!({"external_id": "late"})
        )
        .await
        .unwrap_err(),
        ErrorCode::CredentialRevoked
    );
    // So does an expiry that passed after authentication.
    let (_, expiring) = f.admin_token(AdminScope::Provisioner).await;
    f.tenant(&expiring, "t-exp", &[]).await;
    sqlx::query(
        "UPDATE admin_credentials SET expires_at = now() - interval '1 second' WHERE id = $1",
    )
    .bind(admin_id(&expiring))
    .execute(f.db.pool())
    .await
    .unwrap();
    assert_eq!(
        f.run(
            &expiring,
            "admin.namespaces.create",
            json!({"tenant": "t-exp", "namespace": "n"})
        )
        .await
        .unwrap_err(),
        ErrorCode::CredentialExpired
    );

    // Deterministic interleaving: a revoke holds the credential row (uncommitted) while an admin
    // write arrives. The write waits on its FOR SHARE re-check and, once the revoke commits, is
    // refused; it can never commit after the revocation.
    let mut revoke = f.db.pool().begin().await.unwrap();
    sqlx::query("UPDATE admin_credentials SET revoked_at = now() WHERE id = $1")
        .bind(prov_id)
        .execute(&mut *revoke)
        .await
        .unwrap();
    let write = {
        let f = f.clone();
        let prov = prov.clone();
        tokio::spawn(async move {
            f.run(
                &prov,
                "admin.namespaces.create",
                json!({"tenant": "t", "namespace": "blocked"}),
            )
            .await
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!write.is_finished(), "the write must wait for the revoke");
    revoke.commit().await.unwrap();
    assert_eq!(
        write.await.unwrap().unwrap_err(),
        ErrorCode::CredentialRevoked
    );
    assert_eq!(namespaces_of(&f, "t").await, 0);
}

/// m1 with competing claimants: many admin writes race one revocation. Each write either
/// committed before the revoke (and is counted and audited) or is refused as revoked; nothing
/// else happens and nothing commits unaccounted.
#[tokio::test]
async fn memory_authority_admin_revocation_races_competing_admin_writes() {
    let f = Arc::new(fixture().await);
    let op = f.operator.clone();
    let (_, prov) = f.admin_token(AdminScope::Provisioner).await;
    let prov_id = admin_id(&prov);
    f.tenant(&prov, "t", &[]).await;
    let writers: Vec<_> = (0..6)
        .map(|worker| {
            let f = f.clone();
            let prov = prov.clone();
            tokio::spawn(async move {
                let mut results = Vec::new();
                for round in 0..8 {
                    let namespace = format!("w{worker}-{round}");
                    results.push(
                        f.run(
                            &prov,
                            "admin.namespaces.create",
                            json!({"tenant": "t", "namespace": namespace}),
                        )
                        .await
                        .map(|_| ()),
                    );
                    tokio::task::yield_now().await;
                }
                results
            })
        })
        .collect();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    f.run(
        &op,
        "admin.admin_credentials.revoke",
        json!({"credential_id": prov_id}),
    )
    .await
    .unwrap();
    let mut committed = 0;
    for writer in writers {
        for result in writer.await.unwrap() {
            match result {
                Ok(()) => committed += 1,
                Err(code) => assert_eq!(code, ErrorCode::CredentialRevoked),
            }
        }
    }
    assert_eq!(namespaces_of(&f, "t").await, committed);
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE actor_id = $1 AND action = 'namespace.create'",
    )
    .bind(prov_id)
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(audited, committed);
    // After the revoke returned, the stale principal can do nothing.
    assert_eq!(
        f.run(
            &prov,
            "admin.namespaces.create",
            json!({"tenant": "t", "namespace": "after"})
        )
        .await
        .unwrap_err(),
        ErrorCode::CredentialRevoked
    );
}

/// 06B.4 with competing claimants: many client saves, all made with a principal authenticated
/// before the revoke, race the revocation of that client credential. Each save either committed
/// before the revoke (its observation exists and is audited) or is refused as revoked; nothing
/// commits unaccounted, and after the revoke returns the stale principal can save nothing. The
/// revoke starts once every writer has committed one save, and the writers keep saving until
/// refused, so both outcomes occur in every run (review B3 F3a).
#[tokio::test]
async fn memory_authority_client_revocation_races_competing_saves() {
    let f = Arc::new(fixture().await);
    let op = f.operator.clone();
    f.tenant(&op, "t", &["ns"]).await;
    let token = f
        .client(&op, "t", json!([grant("ns", 6, Some(6))]))
        .await
        .unwrap();
    let id = credential::parse(&token).unwrap().id;
    let principal = f.login(&token).await.unwrap();
    let save = |key: String| {
        json!({"idempotency_key": key,
               "expires_at": (Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
               "namespace": "ns", "session_id": null,
               "context": {"producing_ceiling": 3, "consumed": {"kind": "exact", "level": 3},
                           "sources": [], "external_sources": [], "expected_access_level": null},
               "items": [{"kind": "fact", "title": "t", "body": "b",
                          "occurred_at": "2026-11-08T10:00:00Z", "expires_at": null}]})
    };
    const WRITERS: usize = 6;
    let started = Arc::new(tokio::sync::Barrier::new(WRITERS + 1));
    let writers: Vec<_> = (0..WRITERS)
        .map(|worker| {
            let f = f.clone();
            let principal = principal.clone();
            let started = started.clone();
            tokio::spawn(async move {
                let mut results = Vec::new();
                for round in 0..500 {
                    let result = f
                        .run(
                            &principal,
                            "observations.save",
                            save(format!("w{worker}.{round}")),
                        )
                        .await
                        .map(|_| ());
                    let refused = result.is_err();
                    results.push(result);
                    if round == 0 {
                        started.wait().await;
                    }
                    if refused {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
                results
            })
        })
        .collect();
    started.wait().await;
    f.run(
        &op,
        "admin.credentials.revoke",
        json!({"tenant": "t", "credential_id": id}),
    )
    .await
    .unwrap();
    let (mut committed, mut refused) = (0, 0);
    for writer in writers {
        for result in writer.await.unwrap() {
            match result {
                Ok(()) => committed += 1,
                Err(code) => {
                    assert_eq!(code, ErrorCode::CredentialRevoked);
                    refused += 1;
                }
            }
        }
    }
    assert!(
        committed >= WRITERS as i64,
        "every writer committed before the revoke"
    );
    assert_eq!(refused, WRITERS, "every writer was then refused");
    let saved: i64 = sqlx::query_scalar("SELECT count(*) FROM observations")
        .fetch_one(f.db.pool())
        .await
        .unwrap();
    assert_eq!(saved, committed);
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE actor_id = $1 AND action = 'observation.save'",
    )
    .bind(id)
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(audited, committed);
    assert_eq!(
        f.run(&principal, "observations.save", save("after".into()))
            .await
            .unwrap_err(),
        ErrorCode::CredentialRevoked
    );
}

/// m2: only a usable operator (neither revoked nor expired) blocks bootstrap.
#[tokio::test]
async fn memory_authority_bootstrap_recovers_when_no_usable_operator_remains() {
    let f = fixture().await;
    sqlx::query("UPDATE admin_credentials SET expires_at = now() - interval '1 second'")
        .execute(f.db.pool())
        .await
        .unwrap();
    let second = credential::issue(TokenKind::Admin).unwrap();
    f.app
        .store()
        .bootstrap_operator(second.id, "second", second.verifier)
        .await
        .expect("an expired operator no longer blocks bootstrap");
    let third = credential::issue(TokenKind::Admin).unwrap();
    assert_eq!(
        f.app
            .store()
            .bootstrap_operator(third.id, "third", third.verifier)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict,
        "the new usable operator blocks a further bootstrap"
    );
    sqlx::query("UPDATE admin_credentials SET revoked_at = now() WHERE id = $1")
        .bind(second.id)
        .execute(f.db.pool())
        .await
        .unwrap();
    f.app
        .store()
        .bootstrap_operator(third.id, "third", third.verifier)
        .await
        .expect("a revoked operator no longer blocks bootstrap");
}

async fn reserve(f: &Fixture, operation: Uuid, tokens: i64) {
    sqlx::query(
        "UPDATE generation_operations SET reserved_tokens = $2, spend_day = DATE '2030-01-01' \
         WHERE id = $1",
    )
    .bind(operation)
    .bind(tokens)
    .execute(f.db.pool())
    .await
    .unwrap();
}

async fn day_reserved(f: &Fixture) -> i64 {
    sqlx::query_scalar("SELECT reserved FROM tenant_spend_days WHERE day = DATE '2030-01-01'")
        .fetch_one(f.db.pool())
        .await
        .unwrap()
}

/// m3: cancellation hands a reservation back exactly once and leaves `reserved_tokens = 0` on
/// the cancelled row; a dispatched operation keeps its reservation until it is settled.
#[tokio::test]
async fn memory_authority_cancellation_releases_each_reservation_once() {
    let f = fixture().await;
    let op = f.operator.clone();
    f.tenant(&op, "t", &["ns"]).await;
    let token = f
        .client(&op, "t", json!([grant("ns", 6, Some(6))]))
        .await
        .unwrap();
    let claimed = f.queue_operation("t", "ns", &token, "claimed").await;
    let accepted = f.queue_operation("t", "ns", &token, "accepted").await;
    reserve(&f, claimed, 300).await;
    reserve(&f, accepted, 0).await;
    sqlx::query(
        "INSERT INTO tenant_spend_days (tenant_id, day, reserved) \
         SELECT id, DATE '2030-01-01', 1000 FROM tenants WHERE external_id = 't'",
    )
    .execute(f.db.pool())
    .await
    .unwrap();
    let view = f
        .run(&op, "admin.tenants.disable", json!({"tenant": "t"}))
        .await
        .unwrap();
    assert_eq!(view["cancelled_operations"], 2);
    assert_eq!(day_reserved(&f).await, 700);
    let zeroed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM generation_operations WHERE state = 'cancelled' AND reserved_tokens = 0",
    )
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(zeroed, 2);
    // Later state changes over the same rows release nothing again.
    f.run(&op, "admin.tenants.enable", json!({"tenant": "t"}))
        .await
        .unwrap();
    let id = credential::parse(&token).unwrap().id;
    f.run(
        &op,
        "admin.credentials.revoke",
        json!({"tenant": "t", "credential_id": id}),
    )
    .await
    .unwrap();
    f.run(&op, "admin.tenants.delete", json!({"tenant": "t"}))
        .await
        .unwrap();
    assert_eq!(day_reserved(&f).await, 700);
}

/// m3 with competing claimants: a namespace delete and a credential revoke cover the same
/// operations and both hold only tenant FOR SHARE. Operation rows are locked in id order, so
/// both complete, every operation is cancelled exactly once and its reservation is released
/// exactly once.
#[tokio::test]
async fn memory_authority_overlapping_cancellations_complete_and_release_once() {
    let f = Arc::new(fixture().await);
    let op = f.operator.clone();
    f.tenant(&op, "t", &[]).await;
    sqlx::query(
        "INSERT INTO tenant_spend_days (tenant_id, day, reserved) \
         SELECT id, DATE '2030-01-01', 100000 FROM tenants WHERE external_id = 't'",
    )
    .execute(f.db.pool())
    .await
    .unwrap();
    let mut released = 0;
    for round in 0..5 {
        let namespace = format!("ns-{round}");
        f.run(
            &op,
            "admin.namespaces.create",
            json!({"tenant": "t", "namespace": namespace}),
        )
        .await
        .unwrap();
        let token = f
            .client(&op, "t", json!([grant(&namespace, 6, Some(6))]))
            .await
            .unwrap();
        for _ in 0..12 {
            let id = f.queue_operation("t", &namespace, &token, "accepted").await;
            reserve(&f, id, 10).await;
            released += 10;
        }
        let credential = credential::parse(&token).unwrap().id;
        let delete = {
            let (f, op, namespace) = (f.clone(), op.clone(), namespace.clone());
            tokio::spawn(async move {
                f.run(
                    &op,
                    "admin.namespaces.delete",
                    json!({"tenant": "t", "namespace": namespace}),
                )
                .await
            })
        };
        let revoke = {
            let (f, op) = (f.clone(), op.clone());
            tokio::spawn(async move {
                f.run(
                    &op,
                    "admin.credentials.revoke",
                    json!({"tenant": "t", "credential_id": credential}),
                )
                .await
            })
        };
        assert!(delete.await.unwrap().is_ok());
        assert!(revoke.await.unwrap().is_ok());
    }
    let (live, reserved): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE state <> 'cancelled'), coalesce(sum(reserved_tokens), 0)::bigint \
           FROM generation_operations",
    )
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!((live, reserved), (0, 0));
    assert_eq!(day_reserved(&f).await, 100000 - released);
}

/// m7/m12: tenant and namespace creation are idempotent by external id; an unusable tenant id
/// (tombstoned, or managed by another admin) gets one uniform conflict.
#[tokio::test]
async fn memory_authority_admin_creates_are_idempotent_by_external_id() {
    let f = fixture().await;
    let op = f.operator.clone();
    let (_, prov) = f.admin_token(AdminScope::Provisioner).await;
    let first = f
        .run(&prov, "admin.tenants.create", json!({"external_id": "t"}))
        .await
        .unwrap();
    let again = f
        .run(&prov, "admin.tenants.create", json!({"external_id": "t"}))
        .await
        .unwrap();
    assert_eq!(first, again);
    for _ in 0..2 {
        let view = f
            .run(
                &prov,
                "admin.namespaces.create",
                json!({"tenant": "t", "namespace": "ns"}),
            )
            .await
            .unwrap();
        assert_eq!(view["state"], "active");
    }
    assert_eq!(namespaces_of(&f, "t").await, 1);
    f.tenant(&op, "operator-owned", &[]).await;
    f.tenant(&op, "gone", &[]).await;
    f.run(&op, "admin.tenants.delete", json!({"tenant": "gone"}))
        .await
        .unwrap();
    for (principal, tenant) in [(&prov, "operator-owned"), (&prov, "gone"), (&op, "gone")] {
        assert_eq!(
            f.run(
                principal,
                "admin.tenants.create",
                json!({"external_id": tenant})
            )
            .await
            .unwrap_err(),
            ErrorCode::Conflict,
            "{tenant}"
        );
    }
}

/// m11: success paths of the remaining admin operations, and the audit request id.
#[tokio::test]
async fn memory_authority_settings_listing_admin_revocation_and_repeated_delete() {
    let f = fixture().await;
    let op = f.operator.clone();
    f.tenant(&op, "t", &["ns", "other"]).await;
    // Operator spend-cap setting.
    let request_id = Uuid::now_v7();
    let view = f
        .run_as(
            &op,
            "admin.tenants.settings",
            json!({"tenant": "t", "generation_daily_token_cap": 50000}),
            request_id,
        )
        .await
        .unwrap();
    assert_eq!(view["generation_daily_token_cap"], 50000);
    let audited: Option<Uuid> =
        sqlx::query_scalar("SELECT request_id FROM audit_events WHERE action = 'tenant.settings'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(
        audited,
        Some(request_id),
        "admin audit rows carry the request id"
    );
    // Listing shows every credential with its grants and revocation state.
    let kept = f
        .client(
            &op,
            "t",
            json!([grant("ns", 6, Some(6)), grant("other", 2, None)]),
        )
        .await
        .unwrap();
    let dropped = f
        .client(&op, "t", json!([grant("ns", 1, None)]))
        .await
        .unwrap();
    let dropped_id = credential::parse(&dropped).unwrap().id;
    f.run(
        &op,
        "admin.credentials.revoke",
        json!({"tenant": "t", "credential_id": dropped_id}),
    )
    .await
    .unwrap();
    let list = f
        .run(&op, "admin.credentials.list", json!({"tenant": "t"}))
        .await
        .unwrap();
    let credentials = list["credentials"].as_array().unwrap();
    assert_eq!(credentials.len(), 2);
    let kept_view = credentials
        .iter()
        .find(|c| c["credential_id"] == json!(credential::parse(&kept).unwrap().id))
        .unwrap();
    assert!(kept_view["revoked_at"].is_null());
    assert_eq!(kept_view["grants"].as_array().unwrap().len(), 2);
    let dropped_view = credentials
        .iter()
        .find(|c| c["credential_id"] == json!(dropped_id))
        .unwrap();
    assert!(dropped_view["revoked_at"].is_string());
    // Revoking an admin credential stops its authentication; repeating it is harmless.
    let (prov_token, prov) = f.admin_token(AdminScope::Provisioner).await;
    for _ in 0..2 {
        let view = f
            .run(
                &op,
                "admin.admin_credentials.revoke",
                json!({"credential_id": admin_id(&prov)}),
            )
            .await
            .unwrap();
        assert_eq!(view["revoked"], true);
        assert_eq!(view["scope"], "provisioner");
    }
    assert_eq!(
        f.app
            .authenticate(Some(&prov_token), Surface::Admin, Utc::now())
            .await
            .unwrap_err()
            .code,
        ErrorCode::CredentialRevoked
    );
    assert_eq!(
        f.run(
            &op,
            "admin.admin_credentials.revoke",
            json!({"credential_id": Uuid::now_v7()})
        )
        .await
        .unwrap_err(),
        ErrorCode::NotFound
    );
    // Repeated namespace delete answers the tombstone again without a second audit row; an
    // unknown namespace is not found.
    for _ in 0..2 {
        let view = f
            .run(
                &op,
                "admin.namespaces.delete",
                json!({"tenant": "t", "namespace": "other"}),
            )
            .await
            .unwrap();
        assert_eq!(view["state"], "deleted");
    }
    let deletes: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = 'namespace.delete'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(deletes, 1);
    assert_eq!(
        f.run(
            &op,
            "admin.namespaces.delete",
            json!({"tenant": "t", "namespace": "never"})
        )
        .await
        .unwrap_err(),
        ErrorCode::NotFound
    );
    // The surviving credential lost its grant on the deleted namespace.
    let Principal::Client(client) = f.login(&kept).await.unwrap() else {
        panic!("client principal")
    };
    assert_eq!(client.grants.len(), 1);
    assert_eq!(client.grants[0].namespace, "ns");
}
