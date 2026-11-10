//! Lazy namespace provisioning and the owner-removal hook (E2, E9), in the shared lock order
//! (users → company → membership → agent → tenant → namespace → lifecycle).
use super::*;
use crate::adapters::persistence::PostgresPersistence;
use crate::application::memory::{
    MemoryNamespaces, NamespaceExternalId, NamespaceOwner, NamespaceReadiness,
};
use crate::application::workflow::CompanyId;
use async_trait::async_trait;
use sqlx::PgConnection;

/// The stored shape of an owner: scope kind, agent id, user id, and the lifecycle owner id.
struct OwnerColumns {
    kind: &'static str,
    agent: Option<Uuid>,
    user: Option<Uuid>,
    owner_id: Uuid,
}

fn columns(company: Uuid, owner: NamespaceOwner) -> OwnerColumns {
    match owner {
        NamespaceOwner::Company => OwnerColumns {
            kind: "company",
            agent: None,
            user: None,
            owner_id: company,
        },
        NamespaceOwner::Agent(agent) => OwnerColumns {
            kind: "agent",
            agent: Some(agent),
            user: None,
            owner_id: agent,
        },
        NamespaceOwner::User(user) => OwnerColumns {
            kind: "user",
            agent: None,
            user: Some(user),
            owner_id: user,
        },
    }
}

/// The owner rows `FOR SHARE`, in order; `false` when any is missing or in another company.
/// Deleting the owner takes `FOR UPDATE` on the same row, so it serializes with this.
async fn lock_owner(
    db: &mut PgConnection,
    company: Uuid,
    owner: NamespaceOwner,
) -> AppResult<bool> {
    if let NamespaceOwner::User(user) = owner {
        let account: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM users WHERE id = $1 FOR SHARE")
                .bind(user)
                .fetch_optional(&mut *db)
                .await?;
        if account.is_none() {
            return Ok(false);
        }
    }
    let live: Option<Uuid> = sqlx::query_scalar("SELECT id FROM companies WHERE id = $1 FOR SHARE")
        .bind(company)
        .fetch_optional(&mut *db)
        .await?;
    if live.is_none() {
        return Ok(false);
    }
    let found: Option<Uuid> = match owner {
        NamespaceOwner::Company => Some(company),
        NamespaceOwner::User(user) => {
            sqlx::query_scalar(
                "SELECT member.id FROM company_members AS member \
                 WHERE member.company_id = $1 AND member.user_id = $2 FOR SHARE",
            )
            .bind(company)
            .bind(user)
            .fetch_optional(&mut *db)
            .await?
        }
        NamespaceOwner::Agent(agent) => {
            sqlx::query_scalar(
                "SELECT agent.id FROM agents AS agent \
                 WHERE agent.id = $1 AND agent.company_id = $2 FOR SHARE",
            )
            .bind(agent)
            .bind(company)
            .fetch_optional(&mut *db)
            .await?
        }
    };
    Ok(found.is_some())
}

pub(super) async fn ensure_on(
    db: &mut PgConnection,
    company: Uuid,
    owner: NamespaceOwner,
) -> AppResult<NamespaceReadiness> {
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *db)
        .await?;
    if !lock_owner(db, company, owner).await? {
        return Ok(NamespaceReadiness::OwnerUnavailable);
    }
    let tenant: Option<(String, Uuid)> = sqlx::query_as(
        "SELECT tenant.state, tenant.provisioner_credential_id FROM memory_tenants AS tenant \
         WHERE tenant.company_id = $1 FOR SHARE",
    )
    .bind(company)
    .fetch_optional(&mut *db)
    .await?;
    let Some((_, provisioner)) = tenant.filter(|(state, _)| state == "enabled") else {
        return Ok(NamespaceReadiness::NotEnabled);
    };
    let scope = owner;
    let owner = columns(company, owner);
    let existing: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT namespace.id, namespace.state FROM memory_namespaces AS namespace \
         WHERE namespace.company_id = $1 AND namespace.scope_kind = $2 \
           AND namespace.agent_id IS NOT DISTINCT FROM $3 \
           AND namespace.user_id IS NOT DISTINCT FROM $4 AND namespace.state <> 'removing'",
    )
    .bind(company)
    .bind(owner.kind)
    .bind(owner.agent)
    .bind(owner.user)
    .fetch_optional(&mut *db)
    .await?;
    match existing {
        Some((id, state)) if state == "ready" => {
            return Ok(NamespaceReadiness::Ready(NamespaceExternalId(id)));
        }
        Some((id, _)) => {
            let conflicted: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM memory_lifecycles AS lifecycle \
                   WHERE lifecycle.company_id = $1 AND lifecycle.namespace_id = $2 \
                     AND lifecycle.state = 'failed' AND lifecycle.outcome_code = 'namespace_conflict' \
                     AND lifecycle.updated_at <= clock_timestamp() - $3 * interval '1 second')",
            )
            .bind(company)
            .bind(id)
            .bind(super::tenants::REDRIVE_COOLDOWN_SECS)
            .fetch_one(&mut *db)
            .await?;
            if !conflicted {
                // A retryable failure is due again after a cool-down (r1 M1, r2 m1); a definite
                // one waits for the owner's re-drive.
                super::tenants::redrive_failed_on(
                    db,
                    company,
                    Some(id),
                    super::tenants::Redrive::Automatic,
                )
                .await?;
                return Ok(NamespaceReadiness::NotReady);
            }
            // Our id is tombstoned remotely and can never be created: after the same cool-down,
            // retire it and provision the scope under a new namespace id (r2 m1, r3 m1).
            retire_namespaces_on(db, company, scope).await?;
        }
        None => {}
    }
    // A concurrent provisioner of the same scope waits on the unique live index and then
    // inserts nothing: one namespace and one lifecycle per scope.
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO memory_namespaces (company_id, id, scope_kind, agent_id, user_id, state) \
         VALUES ($1, $2, $3, $4, $5, 'provisioning') ON CONFLICT DO NOTHING RETURNING id",
    )
    .bind(company)
    .bind(Uuid::new_v4())
    .bind(owner.kind)
    .bind(owner.agent)
    .bind(owner.user)
    .fetch_optional(&mut *db)
    .await?;
    let Some(namespace) = inserted else {
        return Ok(NamespaceReadiness::NotReady);
    };
    sqlx::query(
        "INSERT INTO memory_lifecycles (id, company_id, resource, namespace_id, owner_kind, \
           owner_id, provisioner_credential_id, desired, observed, generation, fence, state, \
           attempts, available_at, updated_at) \
         VALUES ($1, $2, 'namespace', $3, $4, $5, $6, 'present', 'none', 1, 0, 'pending', 0, \
           clock_timestamp(), clock_timestamp())",
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(namespace)
    .bind(owner.kind)
    .bind(owner.owner_id)
    .bind(provisioner)
    .execute(&mut *db)
    .await?;
    Ok(NamespaceReadiness::NotReady)
}

#[async_trait]
impl MemoryNamespaces for PostgresPersistence {
    /// Its own short transaction: the caller's run continues with `not_ready` meanwhile.
    async fn ensure_namespace(
        &self,
        company: CompanyId,
        owner: NamespaceOwner,
    ) -> AppResult<NamespaceReadiness> {
        let mut tx = self.pool().begin().await?;
        let readiness = ensure_on(&mut tx, company.as_uuid(), owner).await?;
        tx.commit().await?;
        Ok(readiness)
    }
}

/// Agent delete, member removal and account deletion (E9), inside their transaction and after
/// their owner-row lock: the owner's live namespaces become `removing` (their credentials are
/// dropped here and revoked remotely) and each lifecycle is upserted to `absent`, which also
/// supersedes an in-flight provisioning claim. A returning owner gets a new namespace and id.
pub(crate) async fn retire_namespaces_on(
    db: &mut PgConnection,
    company: Uuid,
    owner: NamespaceOwner,
) -> AppResult<u64> {
    let owner = columns(company, owner);
    let retired = sqlx::query(
        "WITH retired AS ( \
           UPDATE memory_namespaces AS namespace SET state = 'removing', credential_id = NULL, \
             credential_envelope = NULL \
           WHERE namespace.company_id = $1 AND namespace.scope_kind = $2 \
             AND namespace.agent_id IS NOT DISTINCT FROM $3 \
             AND namespace.user_id IS NOT DISTINCT FROM $4 AND namespace.state <> 'removing' \
           RETURNING namespace.company_id, namespace.id) \
         INSERT INTO memory_lifecycles (id, company_id, resource, namespace_id, owner_kind, \
           owner_id, provisioner_credential_id, desired, observed, generation, fence, state, \
           attempts, available_at, updated_at) \
         SELECT gen_random_uuid(), retired.company_id, 'namespace', retired.id, $2, $5, \
           tenant.provisioner_credential_id, 'absent', 'none', 1, 0, 'pending', 0, \
           clock_timestamp(), clock_timestamp() \
         FROM retired JOIN memory_tenants AS tenant ON tenant.company_id = retired.company_id \
         ON CONFLICT (company_id, namespace_id) WHERE resource = 'namespace' DO UPDATE SET \
           desired = 'absent', generation = memory_lifecycles.generation + 1, \
           state = CASE WHEN memory_lifecycles.state = 'claimed' THEN 'claimed' ELSE 'pending' END, \
           attempts = 0, transient_retries = 0, \
           available_at = clock_timestamp(), outcome_code = NULL, updated_at = clock_timestamp()",
    )
    .bind(company)
    .bind(owner.kind)
    .bind(owner.agent)
    .bind(owner.user)
    .bind(owner.owner_id)
    .execute(&mut *db)
    .await?;
    // After the namespace update, which waited out any save freezing under its FOR SHARE, so
    // that save's intent is visible here and is cancelled too (E9).
    let removing: Vec<Uuid> = sqlx::query_scalar(
        "SELECT namespace.id FROM memory_namespaces AS namespace \
         WHERE namespace.company_id = $1 AND namespace.scope_kind = $2 \
           AND namespace.agent_id IS NOT DISTINCT FROM $3 \
           AND namespace.user_id IS NOT DISTINCT FROM $4 AND namespace.state = 'removing'",
    )
    .bind(company)
    .bind(owner.kind)
    .bind(owner.agent)
    .bind(owner.user)
    .fetch_all(&mut *db)
    .await?;
    super::deliveries::cancel_removed_on(db, company, &removing).await?;
    Ok(retired.rows_affected())
}
