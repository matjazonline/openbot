//! Opt-in, lazy provisioning and the owner-row lock protocol (E2, E9), against PostgreSQL.
//!
//! Every test takes `own_database`: lifecycle rows have no company FK and are claimed by a
//! table-wide sweep, so a shared database would leave claimable residue for other tests.
use super::namespaces::ensure_on;
use super::tenants::{disable_on, enable_on};
use super::*;
use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::credentials::CredentialCipher;
use crate::adapters::persistence::test_support::{OwnDatabase, own_database};
use crate::application::memory::*;
use crate::application::workflow::{CompanyId, WorkflowActor};
use crate::entities::company_member::CompanyAccessRole;
use crate::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
use crate::use_cases::company_invite::CompanyInvitePersistence;
use crate::use_cases::user::UserPersistence;
use std::time::Duration;

pub(super) const PROVISIONER: Uuid = Uuid::from_u128(0x0190f1a2_b3c4_4d5e_8f9a_0b1c2d3e4f50);

pub(super) struct Fixture {
    pub database: OwnDatabase,
    pub persistence: PostgresPersistence,
    pub company: Uuid,
    pub owner: Uuid,
    pub admin: Uuid,
    pub member: Uuid,
    pub agent: Uuid,
}

async fn user(persistence: &PostgresPersistence, role: &str) -> (Uuid, String) {
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("memory-{role}-{suffix}@example.test");
    let user = persistence
        .create_user(&format!("memory_{role}_{suffix}"), &email, "hash")
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(user.id)
        .execute(persistence.pool())
        .await
        .unwrap();
    (user.id, email)
}

async fn join(persistence: &PostgresPersistence, company: Uuid, role: CompanyAccessRole) -> Uuid {
    let (id, email) = user(persistence, "member").await;
    let invite = persistence
        .create_invite(
            crate::adapters::persistence::test_support::invite_command(
                persistence,
                company,
                &email,
                role,
            )
            .await,
        )
        .await
        .unwrap();
    persistence
        .accept_pending_invite(invite.id, id, &email)
        .await
        .unwrap()
        .unwrap();
    id
}

pub(super) async fn agent(persistence: &PostgresPersistence, company: Uuid) -> Uuid {
    AgentPersistence::create(
        persistence,
        company,
        AgentWrite {
            name: "Memory agent".into(),
            slug: format!("memory-agent-{}", Uuid::new_v4().simple()),
            created_by: Some(crate::entities::creation::CreationProvenance::system()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

pub(super) async fn company(persistence: &PostgresPersistence, owner: Uuid) -> Uuid {
    CompanyPersistence::create(
        persistence,
        owner,
        CompanyWrite {
            name: "Memory".into(),
            slug: format!("memory-{}", Uuid::new_v4().simple()),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap()
    .id
}

impl Fixture {
    pub(super) async fn new() -> Option<Self> {
        let database = own_database().await?;
        let persistence = PostgresPersistence::with_credential_cipher(
            database.pool.clone(),
            CredentialCipher::for_test_with_keys(&[(1, [7_u8; 32])], 1),
        );
        let (owner, _) = user(&persistence, "owner").await;
        let company = company(&persistence, owner).await;
        let admin = join(&persistence, company, CompanyAccessRole::Admin).await;
        let member = join(&persistence, company, CompanyAccessRole::Member).await;
        let agent = agent(&persistence, company).await;
        Some(Self {
            database,
            persistence,
            company,
            owner,
            admin,
            member,
            agent,
        })
    }

    pub(super) fn pool(&self) -> &sqlx::PgPool {
        &self.database.pool
    }

    pub(super) fn scope(&self) -> CompanyId {
        CompanyId::new(self.company)
    }

    pub(super) fn actor(user: Uuid) -> WorkflowActor {
        WorkflowActor::authenticated(user).unwrap()
    }

    pub(super) async fn revision(&self) -> i64 {
        sqlx::query_scalar("SELECT revision FROM memory_tenants WHERE company_id = $1")
            .bind(self.company)
            .fetch_optional(self.pool())
            .await
            .unwrap()
            .unwrap_or(0)
    }

    pub(super) async fn enable(&self) -> AppResult<()> {
        let revision = self.revision().await;
        self.persistence
            .enable_memory(self.scope(), Self::actor(self.owner), revision, PROVISIONER)
            .await
    }

    pub(super) async fn disable(&self) -> AppResult<()> {
        let revision = self.revision().await;
        self.persistence
            .disable_memory(self.scope(), Self::actor(self.owner), revision)
            .await
    }

    pub(super) async fn ensure(&self, owner: NamespaceOwner) -> NamespaceReadiness {
        self.persistence
            .ensure_namespace(self.scope(), owner)
            .await
            .unwrap()
    }

    /// (desired, state, generation) of the tenant lifecycle or one namespace's lifecycle.
    pub(super) async fn lifecycle(&self, namespace: Option<Uuid>) -> Option<(String, String, i64)> {
        sqlx::query_as(
            "SELECT desired, state, generation FROM memory_lifecycles \
             WHERE company_id = $1 AND namespace_id IS NOT DISTINCT FROM $2",
        )
        .bind(self.company)
        .bind(namespace)
        .fetch_optional(self.pool())
        .await
        .unwrap()
    }

    /// The namespaces of one scope: (id, state).
    pub(super) async fn namespaces(&self, kind: &str) -> Vec<(Uuid, String)> {
        sqlx::query_as(
            "SELECT id, state FROM memory_namespaces WHERE company_id = $1 AND scope_kind = $2 \
             ORDER BY created_at, id",
        )
        .bind(self.company)
        .bind(kind)
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    pub(super) async fn tenant(&self) -> Option<(String, i64, i64)> {
        sqlx::query_as(
            "SELECT state, revision, disable_epoch FROM memory_tenants WHERE company_id = $1",
        )
        .bind(self.company)
        .fetch_optional(self.pool())
        .await
        .unwrap()
    }
}

pub(super) async fn backend_pid(tx: &mut sqlx::PgConnection) -> i32 {
    sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(tx)
        .await
        .unwrap()
}

/// Wait until some backend is blocked by `blocker`'s locks (a real lock wait, not a sleep).
pub(super) async fn blocked_by(pool: &sqlx::PgPool, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity \
                 WHERE datname = current_database() AND $1 = ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(pool)
            .await
            .unwrap();
            if waiting.is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the competing transaction must wait on the held lock");
}

#[tokio::test]
async fn memory_scope_owner_or_admin_opt_in_with_revision_and_disable_epoch() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    // A plain member, an outsider and a foreign company's owner are all "not found".
    let (outsider, _) = user(&f.persistence, "outsider").await;
    let foreign_company = company(&f.persistence, outsider).await;
    for actor in [f.member, outsider] {
        let refused = f
            .persistence
            .enable_memory(f.scope(), Fixture::actor(actor), 0, PROVISIONER)
            .await;
        assert!(matches!(refused, Err(AppError::NotFound(_))), "{refused:?}");
    }
    assert!(f.tenant().await.is_none());
    // The unconfigured process refuses enable before any write.
    let unconfigured = MemorySettings::new(&f.persistence, None);
    assert!(matches!(
        unconfigured
            .enable(f.scope(), Fixture::actor(f.owner), 0)
            .await,
        Err(AppError::BadRequest(_))
    ));

    // An admin enables; the first revision is 1 and the tenant lifecycle is pending `present`.
    f.persistence
        .enable_memory(f.scope(), Fixture::actor(f.admin), 0, PROVISIONER)
        .await
        .unwrap();
    assert_eq!(f.tenant().await, Some(("enabled".into(), 1, 0)));
    assert_eq!(
        f.lifecycle(None).await,
        Some(("present".into(), "pending".into(), 1))
    );
    // A stale form, and another provisioner credential, are refused.
    assert!(matches!(
        f.persistence
            .enable_memory(f.scope(), Fixture::actor(f.owner), 0, PROVISIONER)
            .await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        f.persistence
            .enable_memory(f.scope(), Fixture::actor(f.owner), 1, Uuid::new_v4())
            .await,
        Err(AppError::Conflict(_))
    ));

    // Only disable bumps the epoch; enable again changes the revision only.
    f.disable().await.unwrap();
    assert_eq!(f.tenant().await, Some(("disabled".into(), 2, 1)));
    assert_eq!(
        f.lifecycle(None).await,
        Some(("disabled".into(), "pending".into(), 2))
    );
    f.enable().await.unwrap();
    assert_eq!(f.tenant().await, Some(("enabled".into(), 3, 1)));
    assert_eq!(
        f.lifecycle(None).await,
        Some(("present".into(), "pending".into(), 3))
    );
    let changed_by: Uuid =
        sqlx::query_scalar("SELECT changed_by FROM memory_tenants WHERE company_id = $1")
            .bind(f.company)
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(changed_by, f.owner, "each change records its actor");

    // The settings view is owner/admin only, and names the lifecycle.
    let view = MemorySettings::new(&f.persistence, Some(PROVISIONER))
        .view(f.scope(), Fixture::actor(f.admin))
        .await
        .unwrap();
    let tenant = view.tenant.unwrap();
    assert_eq!(tenant.state, MemoryTenantState::Enabled);
    assert_eq!(tenant.tenant.unwrap().desired, "present");
    assert!(matches!(
        f.persistence
            .memory_tenant(f.scope(), Fixture::actor(f.member))
            .await,
        Err(AppError::NotFound(_))
    ));
    // Cross-company: the foreign owner cannot read or change this company, nor this owner theirs.
    assert!(matches!(
        f.persistence
            .memory_tenant(f.scope(), Fixture::actor(outsider))
            .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        f.persistence
            .enable_memory(
                CompanyId::new(foreign_company),
                Fixture::actor(f.owner),
                0,
                PROVISIONER
            )
            .await,
        Err(AppError::NotFound(_))
    ));
}

#[tokio::test]
async fn memory_scope_lazy_provisioning_maps_scopes_and_refuses_missing_identities() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let agent = NamespaceOwner::Agent(f.agent);
    assert_eq!(f.ensure(agent).await, NamespaceReadiness::NotEnabled);
    f.enable().await.unwrap();
    for owner in [
        NamespaceOwner::Company,
        agent,
        NamespaceOwner::User(f.member),
    ] {
        assert_eq!(f.ensure(owner).await, NamespaceReadiness::NotReady);
        // Idempotent: one namespace and one lifecycle per scope.
        assert_eq!(f.ensure(owner).await, NamespaceReadiness::NotReady);
    }
    for kind in ["company", "agent", "user"] {
        let namespaces = f.namespaces(kind).await;
        assert_eq!(namespaces.len(), 1, "{kind}");
        assert_eq!(
            f.lifecycle(Some(namespaces[0].0)).await,
            Some(("present".into(), "pending".into(), 1))
        );
    }
    // Missing or foreign identities never widen the scope: they are refused outright.
    let (outsider, _) = user(&f.persistence, "outsider").await;
    let other_company = company(&f.persistence, outsider).await;
    let foreign_agent = super::tests::agent(&f.persistence, other_company).await;
    for owner in [
        NamespaceOwner::Agent(foreign_agent),
        NamespaceOwner::Agent(Uuid::new_v4()),
        NamespaceOwner::User(outsider),
        NamespaceOwner::User(Uuid::new_v4()),
    ] {
        assert_eq!(
            f.ensure(owner).await,
            NamespaceReadiness::OwnerUnavailable,
            "{owner:?}"
        );
    }
    assert!(matches!(
        f.persistence
            .ensure_namespace(CompanyId::new(Uuid::new_v4()), NamespaceOwner::Company)
            .await,
        Ok(NamespaceReadiness::OwnerUnavailable)
    ));
    // Ready once the credential is stored; never while the tenant is disabled.
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    sqlx::query(
        "UPDATE memory_namespaces SET state = 'ready', credential_id = $2, \
         credential_envelope = 'enc:v2:fixture' WHERE id = $1",
    )
    .bind(namespace)
    .bind(Uuid::new_v4())
    .execute(f.pool())
    .await
    .unwrap();
    assert_eq!(
        f.ensure(agent).await,
        NamespaceReadiness::Ready(NamespaceExternalId(namespace))
    );
    f.disable().await.unwrap();
    assert_eq!(f.ensure(agent).await, NamespaceReadiness::NotEnabled);
}

#[tokio::test]
async fn memory_scope_owner_removal_retires_namespaces_and_upserts_absent_lifecycles() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    f.ensure(NamespaceOwner::User(f.member)).await;
    f.ensure(NamespaceOwner::Company).await;
    let (agent_namespace, _) = f.namespaces("agent").await[0].clone();
    let (user_namespace, _) = f.namespaces("user").await[0].clone();

    AgentPersistence::delete(&f.persistence, f.agent)
        .await
        .unwrap();
    assert_eq!(
        f.namespaces("agent").await,
        [(agent_namespace, "removing".into())]
    );
    assert_eq!(
        f.lifecycle(Some(agent_namespace)).await,
        Some(("absent".into(), "pending".into(), 2))
    );
    CompanyInvitePersistence::remove_member(&f.persistence, f.company, f.member)
        .await
        .unwrap();
    assert_eq!(
        f.namespaces("user").await,
        [(user_namespace, "removing".into())]
    );
    assert_eq!(
        f.lifecycle(Some(user_namespace)).await,
        Some(("absent".into(), "pending".into(), 2))
    );
    // A removed owner cannot be provisioned again.
    assert_eq!(
        f.ensure(NamespaceOwner::User(f.member)).await,
        NamespaceReadiness::OwnerUnavailable
    );

    // Company delete: the tenant lifecycle becomes absent and outlives the company; namespace
    // lifecycles are covered by the tenant delete; the client rows cascade away.
    CompanyPersistence::delete(&f.persistence, f.company)
        .await
        .unwrap();
    assert_eq!(
        f.lifecycle(None).await,
        Some(("absent".into(), "pending".into(), 2))
    );
    let covered: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT desired, state, outcome_code FROM memory_lifecycles \
         WHERE company_id = $1 AND resource = 'namespace'",
    )
    .bind(f.company)
    .fetch_all(f.pool())
    .await
    .unwrap();
    assert_eq!(covered.len(), 3);
    assert!(covered.iter().all(|row| row
        == &(
            "absent".to_owned(),
            "settled".to_owned(),
            Some("tenant_removed".to_owned())
        )));
    assert!(f.tenant().await.is_none());
    assert!(f.namespaces("company").await.is_empty());
}

#[tokio::test]
async fn memory_scope_account_deletion_retires_owned_tenants_and_member_namespaces() {
    use crate::application::ports::account_deletion::HostAccountDeletion;
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::User(f.member)).await;
    let (namespace, _) = f.namespaces("user").await[0].clone();
    f.persistence.delete_user_account(f.member).await.unwrap();
    assert_eq!(
        f.lifecycle(Some(namespace)).await,
        Some(("absent".into(), "pending".into(), 2))
    );
    f.persistence.delete_user_account(f.owner).await.unwrap();
    assert_eq!(
        f.lifecycle(None).await,
        Some(("absent".into(), "pending".into(), 2))
    );
}

#[tokio::test]
async fn memory_scope_lifecycle_resurrection_is_refused() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    // A tenant whose lifecycle is already `absent` is never turned back to present or disabled.
    sqlx::query("UPDATE memory_lifecycles SET desired = 'absent' WHERE company_id = $1")
        .bind(f.company)
        .execute(f.pool())
        .await
        .unwrap();
    assert!(matches!(f.disable().await, Err(AppError::Conflict(_))));
    assert_eq!(
        f.tenant().await,
        Some(("enabled".into(), 1, 0)),
        "rolled back"
    );
    // After company deletion nothing can enable or provision it again.
    CompanyPersistence::delete(&f.persistence, f.company)
        .await
        .unwrap();
    assert!(matches!(f.enable().await, Err(AppError::NotFound(_))));
    assert_eq!(
        f.ensure(NamespaceOwner::Company).await,
        NamespaceReadiness::OwnerUnavailable
    );
    assert!(f.tenant().await.is_none());
}

#[tokio::test]
async fn memory_scope_lifecycle_lease_checks_refuse_incoherent_rows() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let mut tx = f.pool().begin().await.unwrap();
    let insert = |state: &'static str,
                  owner: Option<Uuid>,
                  until: bool,
                  owner_kind: &'static str| {
        format!(
            "INSERT INTO memory_lifecycles (id, company_id, resource, namespace_id, owner_kind, \
             owner_id, provisioner_credential_id, desired, observed, generation, fence, state, \
             lease_owner, lease_until, attempts, available_at, updated_at) VALUES (gen_random_uuid(), \
             '{company}', 'namespace', gen_random_uuid(), '{owner_kind}', '{owner_id}', gen_random_uuid(), \
             'present', 'none', 1, 0, '{state}', {lease}, {until}, 0, now(), now())",
            company = f.company,
            owner_id = if owner_kind == "company" {
                Uuid::new_v4()
            } else {
                f.agent
            },
            lease = owner.map_or("NULL".into(), |owner| format!("'{owner}'")),
            until = if until { "now()" } else { "NULL" },
        )
    };
    for (statement, why) in [
        (
            insert("pending", Some(Uuid::new_v4()), false, "agent"),
            "pending with a lease owner",
        ),
        (
            insert("pending", Some(Uuid::new_v4()), true, "agent"),
            "pending with a lease",
        ),
        (
            insert("claimed", None, true, "agent"),
            "claimed without an owner",
        ),
        (
            insert("claimed", Some(Uuid::new_v4()), false, "agent"),
            "claimed without an expiry",
        ),
        (
            insert("pending", None, false, "company"),
            "company owner that is not the company",
        ),
    ] {
        let mut savepoint = sqlx::Connection::begin(&mut *tx).await.unwrap();
        let refused = sqlx::query(&statement).execute(&mut *savepoint).await;
        assert!(refused.is_err(), "{why} was accepted");
        savepoint.rollback().await.unwrap();
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn memory_scope_competing_company_delete_and_enable_serialize_in_both_orders() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    // Enable first: delete waits on the company lock, then retires the tenant it committed.
    let mut enable = f.pool().begin().await.unwrap();
    let pid = backend_pid(&mut enable).await;
    enable_on(
        &mut enable,
        f.scope(),
        Fixture::actor(f.owner),
        0,
        PROVISIONER,
    )
    .await
    .unwrap();
    let persistence = f.persistence.clone();
    let company = f.company;
    let delete =
        tokio::spawn(async move { CompanyPersistence::delete(&persistence, company).await });
    blocked_by(f.pool(), pid).await;
    enable.commit().await.unwrap();
    delete.await.unwrap().unwrap();
    assert_eq!(
        f.lifecycle(None).await,
        Some(("absent".into(), "pending".into(), 2))
    );

    // Delete first: enable waits behind the company lock, then finds no company.
    let Some(g) = Fixture::new().await else {
        return;
    };
    let mut delete = g.pool().begin().await.unwrap();
    let pid = backend_pid(&mut delete).await;
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR UPDATE")
        .bind(g.company)
        .execute(&mut *delete)
        .await
        .unwrap();
    let persistence = g.persistence.clone();
    let (scope, owner) = (g.scope(), Fixture::actor(g.owner));
    let enable = tokio::spawn(async move {
        persistence
            .enable_memory(scope, owner, 0, PROVISIONER)
            .await
    });
    blocked_by(g.pool(), pid).await;
    retire_company_on(&mut delete, g.company).await.unwrap();
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(g.company)
        .execute(&mut *delete)
        .await
        .unwrap();
    delete.commit().await.unwrap();
    assert!(matches!(enable.await.unwrap(), Err(AppError::NotFound(_))));
    assert!(
        g.lifecycle(None).await.is_none(),
        "no tenant was ever recorded"
    );
}

#[tokio::test]
async fn memory_scope_competing_agent_delete_and_lazy_provisioning_serialize_in_both_orders() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    // Provisioning first: the delete waits on the agent row, then retires the new namespace.
    let mut provision = f.pool().begin().await.unwrap();
    let pid = backend_pid(&mut provision).await;
    assert_eq!(
        ensure_on(&mut provision, f.company, NamespaceOwner::Agent(f.agent))
            .await
            .unwrap(),
        NamespaceReadiness::NotReady
    );
    let persistence = f.persistence.clone();
    let agent = f.agent;
    let delete = tokio::spawn(async move { AgentPersistence::delete(&persistence, agent).await });
    blocked_by(f.pool(), pid).await;
    provision.commit().await.unwrap();
    delete.await.unwrap().unwrap();
    let namespaces = f.namespaces("agent").await;
    assert_eq!(namespaces.len(), 1);
    assert_eq!(namespaces[0].1, "removing");
    assert_eq!(
        f.lifecycle(Some(namespaces[0].0)).await,
        Some(("absent".into(), "pending".into(), 2))
    );

    // Delete first: provisioning waits on the agent lock, then finds no agent.
    let second = super::tests::agent(&f.persistence, f.company).await;
    let mut delete = f.pool().begin().await.unwrap();
    let pid = backend_pid(&mut delete).await;
    sqlx::query("SELECT id FROM agents WHERE id = $1 FOR UPDATE")
        .bind(second)
        .execute(&mut *delete)
        .await
        .unwrap();
    let persistence = f.persistence.clone();
    let scope = f.scope();
    let provision = tokio::spawn(async move {
        persistence
            .ensure_namespace(scope, NamespaceOwner::Agent(second))
            .await
    });
    blocked_by(f.pool(), pid).await;
    retire_namespaces_on(&mut delete, f.company, NamespaceOwner::Agent(second))
        .await
        .unwrap();
    sqlx::query("UPDATE principals SET kind = 'external', agent_id = NULL WHERE agent_id = $1")
        .bind(second)
        .execute(&mut *delete)
        .await
        .unwrap();
    sqlx::query("DELETE FROM agents WHERE id = $1")
        .bind(second)
        .execute(&mut *delete)
        .await
        .unwrap();
    delete.commit().await.unwrap();
    assert_eq!(
        provision.await.unwrap().unwrap(),
        NamespaceReadiness::OwnerUnavailable
    );
    assert_eq!(
        f.namespaces("agent").await.len(),
        1,
        "nothing for the deleted agent"
    );
}

#[tokio::test]
async fn memory_scope_competing_member_removal_and_provisioning_serialize_in_both_orders() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    // Provisioning first: removal waits on the company lock, then retires the namespace.
    let mut provision = f.pool().begin().await.unwrap();
    let pid = backend_pid(&mut provision).await;
    ensure_on(&mut provision, f.company, NamespaceOwner::User(f.member))
        .await
        .unwrap();
    let persistence = f.persistence.clone();
    let (company, member) = (f.company, f.member);
    let removal = tokio::spawn(async move {
        CompanyInvitePersistence::remove_member(&persistence, company, member).await
    });
    blocked_by(f.pool(), pid).await;
    provision.commit().await.unwrap();
    removal.await.unwrap().unwrap();
    let namespaces = f.namespaces("user").await;
    assert_eq!(namespaces.len(), 1);
    assert_eq!(namespaces[0].1, "removing");

    // Removal first: provisioning waits behind the membership's company lock, then finds no
    // member. The removal here is the real one, held open by a lock it must take first.
    let admin = f.admin;
    let mut hold = f.pool().begin().await.unwrap();
    let pid = backend_pid(&mut hold).await;
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *hold)
        .await
        .unwrap();
    let persistence = f.persistence.clone();
    let removal = tokio::spawn(async move {
        CompanyInvitePersistence::remove_member(&persistence, company, admin).await
    });
    blocked_by(f.pool(), pid).await;
    let persistence = f.persistence.clone();
    let scope = f.scope();
    let provision = tokio::spawn(async move {
        persistence
            .ensure_namespace(scope, NamespaceOwner::User(admin))
            .await
    });
    // Both wait on the held company lock; the removal queued first, so it commits first.
    crate::adapters::persistence::test_support::wait_until_backends_are_blocked(f.pool(), 2).await;
    hold.commit().await.unwrap();
    removal.await.unwrap().unwrap();
    assert_eq!(
        provision.await.unwrap().unwrap(),
        NamespaceReadiness::OwnerUnavailable
    );
    assert_eq!(
        f.namespaces("user").await.len(),
        1,
        "nothing for the removed admin"
    );
}

#[tokio::test]
async fn memory_scope_competing_disable_enable_and_provisioning_serialize() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    // Disable holds the tenant: a concurrent enable from the same form revision is stale, and a
    // lazy provisioning waits and then sees the tenant disabled.
    let mut disable = f.pool().begin().await.unwrap();
    let pid = backend_pid(&mut disable).await;
    disable_on(&mut disable, f.scope(), Fixture::actor(f.owner), 1)
        .await
        .unwrap();
    let persistence = f.persistence.clone();
    let (scope, admin) = (f.scope(), Fixture::actor(f.admin));
    let enable = tokio::spawn(async move {
        persistence
            .enable_memory(scope, admin, 1, PROVISIONER)
            .await
    });
    let persistence = f.persistence.clone();
    let agent = f.agent;
    let provision = tokio::spawn(async move {
        persistence
            .ensure_namespace(scope, NamespaceOwner::Agent(agent))
            .await
    });
    crate::adapters::persistence::test_support::wait_until_backends_are_blocked(f.pool(), 2).await;
    blocked_by(f.pool(), pid).await;
    disable.commit().await.unwrap();
    assert!(matches!(enable.await.unwrap(), Err(AppError::Conflict(_))));
    assert_eq!(
        provision.await.unwrap().unwrap(),
        NamespaceReadiness::NotEnabled
    );
    assert_eq!(f.tenant().await, Some(("disabled".into(), 2, 1)));

    // Enable holds the tenant: a concurrent disable from the stale revision is refused.
    let mut enable = f.pool().begin().await.unwrap();
    let pid = backend_pid(&mut enable).await;
    enable_on(
        &mut enable,
        f.scope(),
        Fixture::actor(f.admin),
        2,
        PROVISIONER,
    )
    .await
    .unwrap();
    let persistence = f.persistence.clone();
    let owner = Fixture::actor(f.owner);
    let disable = tokio::spawn(async move { persistence.disable_memory(scope, owner, 2).await });
    blocked_by(f.pool(), pid).await;
    enable.commit().await.unwrap();
    assert!(matches!(disable.await.unwrap(), Err(AppError::Conflict(_))));
    assert_eq!(f.tenant().await, Some(("enabled".into(), 3, 1)));
}

/// Company delete's opening: the company `FOR UPDATE`, then the memory rows (tenant, namespaces,
/// lifecycles), as `delete_company_with_cleanup` takes them. The caller finishes with the delete.
async fn hold_company_delete(f: &Fixture) -> (sqlx::Transaction<'static, sqlx::Postgres>, i32) {
    let mut delete = f.pool().begin().await.unwrap();
    let pid = backend_pid(&mut delete).await;
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR UPDATE")
        .bind(f.company)
        .execute(&mut *delete)
        .await
        .unwrap();
    retire_company_on(&mut delete, f.company).await.unwrap();
    (delete, pid)
}

async fn finish_company_delete(
    f: &Fixture,
    mut delete: sqlx::Transaction<'static, sqlx::Postgres>,
) {
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(f.company)
        .execute(&mut *delete)
        .await
        .unwrap();
    delete.commit().await.unwrap();
}

/// Review p7b r1 M4: agent delete takes the company before the agent, so it serializes with
/// company delete in both orders. Under the old order (agent, then namespaces) the second order
/// deadlocks: company delete holds the namespaces and cascades into the held agent.
#[tokio::test]
async fn memory_scope_competing_agent_delete_and_company_delete_never_deadlock() {
    use crate::adapters::persistence::agent::{finish_agent_delete_on, lock_agent_delete_on};
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    let second = agent(&f.persistence, f.company).await;
    f.ensure(NamespaceOwner::Agent(second)).await;

    // Agent delete first, holding its locks; the company delete waits behind them.
    let mut agent_delete = f.pool().begin().await.unwrap();
    let pid = backend_pid(&mut agent_delete).await;
    let locks = lock_agent_delete_on(&mut agent_delete, f.agent)
        .await
        .unwrap()
        .unwrap();
    let persistence = f.persistence.clone();
    let company_id = f.company;
    let company_delete =
        tokio::spawn(async move { CompanyPersistence::delete(&persistence, company_id).await });
    blocked_by(f.pool(), pid).await;
    finish_agent_delete_on(&mut agent_delete, locks)
        .await
        .unwrap();
    agent_delete.commit().await.unwrap();
    company_delete.await.unwrap().unwrap();
    assert_eq!(
        f.lifecycle(None).await,
        Some(("absent".into(), "pending".into(), 2))
    );

    // Company delete first: the agent delete waits on the company, then finds nothing.
    let Some(g) = Fixture::new().await else {
        return;
    };
    g.enable().await.unwrap();
    g.ensure(NamespaceOwner::Agent(g.agent)).await;
    let (delete, pid) = hold_company_delete(&g).await;
    let persistence = g.persistence.clone();
    let agent_id = g.agent;
    let agent_delete =
        tokio::spawn(async move { AgentPersistence::delete(&persistence, agent_id).await });
    blocked_by(g.pool(), pid).await;
    finish_company_delete(&g, delete).await;
    agent_delete.await.unwrap().unwrap();
}

/// Review p7b r1 M4: account deletion locks every company it touches (member companies
/// `FOR KEY SHARE`) before its memberships, so it serializes with company delete in both orders.
#[tokio::test]
async fn memory_scope_competing_account_deletion_and_company_delete_never_deadlock() {
    use crate::adapters::persistence::user_lifecycle::{
        finish_account_deletion_on, lock_account_on,
    };
    use crate::application::ports::account_deletion::HostAccountDeletion;
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::User(f.member)).await;

    // Account deletion first; the company delete waits behind its member-company lock.
    let mut account = f.pool().begin().await.unwrap();
    let pid = backend_pid(&mut account).await;
    let locks = lock_account_on(&mut account, f.member).await.unwrap();
    let persistence = f.persistence.clone();
    let company_id = f.company;
    let company_delete =
        tokio::spawn(async move { CompanyPersistence::delete(&persistence, company_id).await });
    blocked_by(f.pool(), pid).await;
    finish_account_deletion_on(&mut account, f.member, locks)
        .await
        .unwrap();
    account.commit().await.unwrap();
    company_delete.await.unwrap().unwrap();
    assert_eq!(
        f.lifecycle(None).await,
        Some(("absent".into(), "pending".into(), 2))
    );

    // Company delete first: the account deletion waits on the company, then deletes the rest.
    let Some(g) = Fixture::new().await else {
        return;
    };
    g.enable().await.unwrap();
    g.ensure(NamespaceOwner::User(g.member)).await;
    let (delete, pid) = hold_company_delete(&g).await;
    let persistence = g.persistence.clone();
    let member = g.member;
    let account = tokio::spawn(async move { persistence.delete_user_account(member).await });
    blocked_by(g.pool(), pid).await;
    finish_company_delete(&g, delete).await;
    account.await.unwrap().unwrap();
}

/// Review p7b r1 n3: a returning member gets a new namespace row and id (E9).
#[tokio::test]
async fn memory_scope_returning_member_gets_a_new_namespace_id() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::User(f.member)).await;
    let (old, _) = f.namespaces("user").await[0].clone();
    CompanyInvitePersistence::remove_member(&f.persistence, f.company, f.member)
        .await
        .unwrap();
    let email: String = sqlx::query_scalar("SELECT email::text FROM users WHERE id = $1")
        .bind(f.member)
        .fetch_one(f.pool())
        .await
        .unwrap();
    let invite = f
        .persistence
        .create_invite(
            crate::adapters::persistence::test_support::invite_command(
                &f.persistence,
                f.company,
                &email,
                CompanyAccessRole::Member,
            )
            .await,
        )
        .await
        .unwrap();
    f.persistence
        .accept_pending_invite(invite.id, f.member, &email)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        f.ensure(NamespaceOwner::User(f.member)).await,
        NamespaceReadiness::NotReady
    );
    let namespaces = f.namespaces("user").await;
    assert_eq!(namespaces.len(), 2);
    assert_eq!(namespaces[0], (old, "removing".into()));
    assert_ne!(namespaces[1].0, old, "a new namespace id");
    assert_eq!(namespaces[1].1, "provisioning");
}

/// Review p7b r2 m1: lazy provisioning re-drives only retryable failures, and only after a
/// cool-down; a tombstoned (conflicting) namespace is replaced by a new id instead.
#[tokio::test]
async fn memory_scope_lazy_redrive_skips_definite_failures_and_replaces_a_conflict() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    let fail = |outcome: &'static str, age: &'static str| {
        format!(
            "UPDATE memory_lifecycles SET state = 'failed', attempts = 8, outcome_code = '{outcome}', \
             updated_at = clock_timestamp() - interval '{age}' WHERE namespace_id = $1"
        )
    };
    let state = |f: &'_ Fixture, id: Uuid| {
        let pool = f.pool().clone();
        async move {
            sqlx::query_scalar::<_, String>(
                "SELECT state FROM memory_lifecycles WHERE namespace_id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    for (outcome, age) in [
        ("protocol_error", "1 hour"),
        ("credential_list_truncated", "1 hour"),
        ("lease_expired", "1 second"),
    ] {
        sqlx::query(&fail(outcome, age))
            .bind(namespace)
            .execute(f.pool())
            .await
            .unwrap();
        assert_eq!(
            f.ensure(NamespaceOwner::Agent(f.agent)).await,
            NamespaceReadiness::NotReady
        );
        assert_eq!(
            state(&f, namespace).await,
            "failed",
            "{outcome} after {age} is not churned"
        );
    }
    // A fresh tombstone waits out the cool-down (r3 m1); an older one is replaced.
    for age in ["1 second", "1 hour"] {
        sqlx::query(&fail("namespace_conflict", age))
            .bind(namespace)
            .execute(f.pool())
            .await
            .unwrap();
        assert_eq!(
            f.ensure(NamespaceOwner::Agent(f.agent)).await,
            NamespaceReadiness::NotReady
        );
        if age == "1 second" {
            assert_eq!(f.namespaces("agent").await.len(), 1, "not replaced yet");
        }
    }
    let namespaces = f.namespaces("agent").await;
    assert_eq!(namespaces.len(), 2);
    assert_eq!(namespaces[0], (namespace, "removing".into()));
    assert_ne!(
        namespaces[1].0, namespace,
        "the scope gets a new namespace id"
    );
    assert_eq!(
        f.lifecycle(Some(namespace))
            .await
            .map(|(desired, _, _)| desired),
        Some("absent".into())
    );
    assert_eq!(state(&f, namespaces[1].0).await, "pending");
}
