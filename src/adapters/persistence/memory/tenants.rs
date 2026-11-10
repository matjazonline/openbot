//! Per-company opt-in (U1, E2) and the company-delete hook (E9).
//!
//! Lock order, shared by every memory path: users → company → membership/principal → agent →
//! `memory_tenants` → `memory_namespaces` → `memory_agent_capture` → `memory_lifecycles`. Enable and disable take the
//! tenant row `FOR UPDATE` after the owner/admin authority locks; only disable bumps
//! `disable_epoch`.
use super::*;
use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::workflow::authority::authorize_company;
use crate::application::memory::{
    MemoryLifecycleSummary, MemoryNamespaceCounts, MemorySettingsStore, MemoryTenantState,
    MemoryTenantView,
};
use crate::application::workflow::{CompanyId, WorkflowActor};
use async_trait::async_trait;
use sqlx::PgConnection;

const MAX_REPORTED_PROBLEMS: i64 = 20;

/// The actor's account, then the company and their owner/admin membership (E2 order). Anyone
/// else gets the same concealed `NotFound` as a missing company.
pub(super) async fn lock_authority(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
) -> AppResult<()> {
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *db)
        .await?;
    sqlx::query("SELECT id FROM users WHERE id = $1 FOR SHARE")
        .bind(actor.user_id())
        .fetch_optional(&mut *db)
        .await?
        .ok_or_else(|| AppError::NotFound("Company not found".into()))?;
    authorize_company(db, company, actor).await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct TenantRow {
    state: String,
    revision: i64,
    provisioner_credential_id: Uuid,
}

async fn lock_tenant(db: &mut PgConnection, company: Uuid) -> AppResult<Option<TenantRow>> {
    Ok(sqlx::query_as::<_, TenantRow>(
        "SELECT tenant.state, tenant.revision, tenant.provisioner_credential_id \
         FROM memory_tenants AS tenant WHERE tenant.company_id = $1 FOR UPDATE",
    )
    .bind(company)
    .fetch_optional(&mut *db)
    .await?)
}

/// Re-drive (review p7b r1 M1): a failed `present`/`disabled` lifecycle of this company (or of
/// one namespace) is due again with a new generation and a fresh attempt budget. `absent` rows
/// are never touched here: deletions are re-driven only by an operator (docs/deploy.md).
pub(super) async fn redrive_failed_on(
    db: &mut PgConnection,
    company: Uuid,
    namespace: Option<Uuid>,
    mode: Redrive,
) -> AppResult<u64> {
    let automatic = mode == Redrive::Automatic;
    Ok(sqlx::query(
        "UPDATE memory_lifecycles AS lifecycle SET state = 'pending', \
           generation = lifecycle.generation + 1, attempts = 0, transient_retries = 0, \
           outcome_code = NULL, available_at = clock_timestamp(), updated_at = clock_timestamp() \
         WHERE lifecycle.company_id = $1 AND lifecycle.state = 'failed' \
           AND lifecycle.desired <> 'absent' \
           AND ($2::uuid IS NULL OR lifecycle.namespace_id = $2) \
           AND (NOT $3 OR (lifecycle.outcome_code <> ALL($4) \
             AND lifecycle.updated_at <= clock_timestamp() - $5 * interval '1 second'))",
    )
    .bind(company)
    .bind(namespace)
    .bind(automatic)
    .bind(DEFINITE_FAILURES)
    .bind(REDRIVE_COOLDOWN_SECS)
    .execute(&mut *db)
    .await?
    .rows_affected())
}

/// Who re-drives (review p7b r2 m1). An owner or admin re-drives every failed row; lazy
/// provisioning, which P7c calls per run, re-drives only failures a retry can fix, and only after
/// a cool-down, so a definitely refused scope never churns at run frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Redrive {
    Owner,
    Automatic,
}

/// Outcomes no automatic retry can fix. `namespace_conflict` instead gets a new namespace id.
pub(super) const DEFINITE_FAILURES: &[&str] = &[
    "namespace_conflict",
    "credential_list_truncated",
    "protocol_error",
    "invalid_lifecycle",
    "tenant_conflict",
    "tenant_unmanageable",
];
pub(super) const REDRIVE_COOLDOWN_SECS: i64 = 600;

fn stale() -> AppError {
    AppError::Conflict("Memory settings changed; reload and retry".into())
}

/// Desired `present` or `disabled` for the tenant. Never overrides `absent`: a deleted company's
/// tenant is not resurrected (the owner lock already makes that unreachable; this is the
/// database's own refusal).
async fn want_tenant(
    db: &mut PgConnection,
    company: Uuid,
    provisioner: Uuid,
    desired: &'static str,
) -> AppResult<()> {
    let written = sqlx::query(
        "INSERT INTO memory_lifecycles (id, company_id, resource, owner_kind, owner_id, \
           provisioner_credential_id, desired, observed, generation, fence, state, attempts, \
           available_at, updated_at) \
         VALUES ($1, $2, 'tenant', 'company', $2, $3, $4, 'none', 1, 0, 'pending', 0, \
           clock_timestamp(), clock_timestamp()) \
         ON CONFLICT (company_id) WHERE resource = 'tenant' DO UPDATE SET \
           desired = EXCLUDED.desired, \
           provisioner_credential_id = EXCLUDED.provisioner_credential_id, \
           generation = memory_lifecycles.generation + 1, \
           state = CASE WHEN memory_lifecycles.state = 'claimed' THEN 'claimed' ELSE 'pending' END, \
           attempts = 0, transient_retries = 0, \
           available_at = clock_timestamp(), outcome_code = NULL, updated_at = clock_timestamp() \
         WHERE memory_lifecycles.desired <> 'absent'",
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(provisioner)
    .bind(desired)
    .execute(&mut *db)
    .await?;
    if written.rows_affected() != 1 {
        return Err(AppError::Conflict(
            "This company's memory tenant is being deleted".into(),
        ));
    }
    Ok(())
}

/// Company deletion (E9): after the caller locked the company `FOR UPDATE`, the tenant's
/// lifecycle becomes `absent` (an upsert, so it holds even if it was never reconciled), and the
/// namespace lifecycles are settled as covered by the tenant delete. Locks the tenant and
/// namespace rows first, in the shared order, so a concurrent fenced completion never inverts it.
pub(crate) async fn retire_company_on(db: &mut PgConnection, company: Uuid) -> AppResult<()> {
    let Some(tenant) = lock_tenant(db, company).await? else {
        return Ok(());
    };
    sqlx::query(
        "SELECT namespace.id FROM memory_namespaces AS namespace \
         WHERE namespace.company_id = $1 ORDER BY namespace.id FOR UPDATE",
    )
    .bind(company)
    .fetch_all(&mut *db)
    .await?;
    sqlx::query(
        "INSERT INTO memory_lifecycles (id, company_id, resource, owner_kind, owner_id, \
           provisioner_credential_id, desired, observed, generation, fence, state, attempts, \
           available_at, updated_at) \
         VALUES ($1, $2, 'tenant', 'company', $2, $3, 'absent', 'none', 1, 0, 'pending', 0, \
           clock_timestamp(), clock_timestamp()) \
         ON CONFLICT (company_id) WHERE resource = 'tenant' DO UPDATE SET \
           desired = 'absent', generation = memory_lifecycles.generation + 1, \
           state = CASE WHEN memory_lifecycles.state = 'claimed' THEN 'claimed' ELSE 'pending' END, \
           attempts = 0, transient_retries = 0, \
           available_at = clock_timestamp(), outcome_code = NULL, updated_at = clock_timestamp()",
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(tenant.provisioner_credential_id)
    .execute(&mut *db)
    .await?;
    // The fence moves too, so an in-flight namespace claim can complete nothing.
    sqlx::query(
        "UPDATE memory_lifecycles AS lifecycle SET desired = 'absent', \
           generation = lifecycle.generation + 1, fence = lifecycle.fence + 1, state = 'settled', \
           lease_owner = NULL, lease_until = NULL, attempts = 0, transient_retries = 0, \
           outcome_code = 'tenant_removed', \
           updated_at = clock_timestamp() \
         WHERE lifecycle.company_id = $1 AND lifecycle.resource = 'namespace' \
           AND NOT (lifecycle.state = 'settled' AND lifecycle.desired = 'absent')",
    )
    .bind(company)
    .execute(&mut *db)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct TenantViewRow {
    state: String,
    revision: i64,
    provisioner_credential_id: Uuid,
    generation_cap: Option<i64>,
    changed_by: Uuid,
    changed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(sqlx::FromRow)]
struct SummaryRow {
    resource: String,
    desired: String,
    observed: String,
    state: String,
    attempts: i32,
    outcome_code: Option<String>,
}

impl From<SummaryRow> for MemoryLifecycleSummary {
    fn from(row: SummaryRow) -> Self {
        Self {
            resource: row.resource,
            desired: row.desired,
            observed: row.observed,
            state: row.state,
            attempts: row.attempts,
            outcome_code: row.outcome_code,
        }
    }
}

const SUMMARY_COLUMNS: &str = "lifecycle.resource, lifecycle.desired, lifecycle.observed, \
     lifecycle.state, lifecycle.attempts, lifecycle.outcome_code";

async fn tenant_view_on(
    db: &mut PgConnection,
    company: Uuid,
) -> AppResult<Option<MemoryTenantView>> {
    let Some(row) = sqlx::query_as::<_, TenantViewRow>(
        "SELECT tenant.state, tenant.revision, tenant.provisioner_credential_id, \
           tenant.generation_cap, tenant.changed_by, tenant.changed_at \
         FROM memory_tenants AS tenant WHERE tenant.company_id = $1",
    )
    .bind(company)
    .fetch_optional(&mut *db)
    .await?
    else {
        return Ok(None);
    };
    let tenant = sqlx::query_as::<_, SummaryRow>(&format!(
        "SELECT {SUMMARY_COLUMNS} FROM memory_lifecycles AS lifecycle \
         WHERE lifecycle.company_id = $1 AND lifecycle.resource = 'tenant'"
    ))
    .bind(company)
    .fetch_optional(&mut *db)
    .await?
    .map(Into::into);
    let counts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT namespace.state, count(*) FROM memory_namespaces AS namespace \
         WHERE namespace.company_id = $1 GROUP BY namespace.state",
    )
    .bind(company)
    .fetch_all(&mut *db)
    .await?;
    let mut namespaces = MemoryNamespaceCounts::default();
    for (state, count) in counts {
        match state.as_str() {
            "ready" => namespaces.ready = count,
            "provisioning" => namespaces.provisioning = count,
            "removing" => namespaces.removing = count,
            _ => return Err(AppError::Database("Invalid stored memory namespace".into())),
        }
    }
    let mut problems: Vec<MemoryLifecycleSummary> = sqlx::query_as::<_, SummaryRow>(&format!(
        "SELECT {SUMMARY_COLUMNS} FROM memory_lifecycles AS lifecycle \
         WHERE lifecycle.company_id = $1 AND lifecycle.resource = 'namespace' \
           AND lifecycle.state <> 'settled' AND lifecycle.outcome_code IS NOT NULL \
         ORDER BY lifecycle.updated_at DESC, lifecycle.id LIMIT $2"
    ))
    .bind(company)
    .bind(MAX_REPORTED_PROBLEMS + 1)
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
    let more_problems = problems.len() as i64 > MAX_REPORTED_PROBLEMS;
    problems.truncate(MAX_REPORTED_PROBLEMS as usize);
    let (capture, more_agents) = super::capture::capture_view_on(db, company).await?;
    Ok(Some(MemoryTenantView {
        state: tenant_state(&row.state)?,
        revision: row.revision,
        provisioner_credential_id: row.provisioner_credential_id,
        generation_cap: row.generation_cap,
        changed_by: row.changed_by,
        changed_at: row.changed_at,
        tenant,
        namespaces,
        problems,
        more_problems,
        capture,
        more_agents,
    }))
}

fn tenant_state(value: &str) -> AppResult<MemoryTenantState> {
    match value {
        "enabled" => Ok(MemoryTenantState::Enabled),
        "disabled" => Ok(MemoryTenantState::Disabled),
        _ => Err(AppError::Database("Invalid stored memory tenant".into())),
    }
}

/// Enable (U1) inside the caller's transaction, in the E2 lock order.
pub(super) async fn enable_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    expected_revision: i64,
    provisioner_credential_id: Uuid,
) -> AppResult<()> {
    let id = company.as_uuid();
    lock_authority(db, company, actor).await?;
    match lock_tenant(db, id).await? {
        None if expected_revision != 0 => return Err(stale()),
        None => {
            sqlx::query(
                "INSERT INTO memory_tenants (company_id, state, revision, disable_epoch, \
                       provisioner_credential_id, generation_cap, changed_by, changed_at) \
                     VALUES ($1, 'enabled', 1, 0, $2, NULL, $3, clock_timestamp())",
            )
            .bind(id)
            .bind(provisioner_credential_id)
            .bind(actor.user_id())
            .execute(&mut *db)
            .await?;
        }
        Some(tenant) if tenant.revision != expected_revision => return Err(stale()),
        Some(tenant) if tenant.provisioner_credential_id != provisioner_credential_id => {
            return Err(AppError::Conflict(
                "This company's memory is managed by another provisioner credential".into(),
            ));
        }
        // Enabling an enabled company is the owner's re-drive of failed provisioning.
        Some(tenant) if tenant.state == "enabled" => {
            redrive_failed_on(db, id, None, Redrive::Owner).await?;
            return Ok(());
        }
        Some(_) => {
            // Enabling revives nothing: the epoch is unchanged and data was retained.
            sqlx::query(
                "UPDATE memory_tenants AS tenant SET state = 'enabled', \
                       revision = tenant.revision + 1, changed_by = $2, \
                       changed_at = clock_timestamp() \
                     WHERE tenant.company_id = $1",
            )
            .bind(id)
            .bind(actor.user_id())
            .execute(&mut *db)
            .await?;
        }
    }
    want_tenant(db, id, provisioner_credential_id, "present").await?;
    redrive_failed_on(db, id, None, Redrive::Owner).await?;
    Ok(())
}

/// Disable (U1, E9) inside the caller's transaction: the tenant `FOR UPDATE`, then the epoch.
pub(super) async fn disable_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    expected_revision: i64,
) -> AppResult<()> {
    let id = company.as_uuid();
    lock_authority(db, company, actor).await?;
    let tenant = match lock_tenant(db, id).await? {
        None => return Err(AppError::BadRequest("Memory is not enabled".into())),
        Some(tenant) if tenant.revision != expected_revision => return Err(stale()),
        Some(tenant) if tenant.state == "disabled" => return Ok(()),
        Some(tenant) => tenant,
    };
    // Only disable bumps the epoch, so every intent created before it is refused at claim; the
    // open intents fail here, under the tenant lock every save freeze shares (E2, E9).
    sqlx::query(
        "UPDATE memory_tenants AS tenant SET state = 'disabled', \
               revision = tenant.revision + 1, disable_epoch = tenant.disable_epoch + 1, \
               changed_by = $2, changed_at = clock_timestamp() \
             WHERE tenant.company_id = $1",
    )
    .bind(id)
    .bind(actor.user_id())
    .execute(&mut *db)
    .await?;
    super::deliveries::fail_disabled_on(db, id).await?;
    want_tenant(db, id, tenant.provisioner_credential_id, "disabled").await
}

#[async_trait]
impl MemorySettingsStore for PostgresPersistence {
    async fn memory_tenant(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
    ) -> AppResult<Option<MemoryTenantView>> {
        let mut tx = self.pool().begin().await?;
        lock_authority(&mut tx, company, actor).await?;
        let view = tenant_view_on(&mut tx, company.as_uuid()).await?;
        tx.rollback().await?;
        Ok(view)
    }

    async fn enable_memory(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        expected_revision: i64,
        provisioner_credential_id: Uuid,
    ) -> AppResult<()> {
        let mut tx = self.pool().begin().await?;
        enable_on(
            &mut tx,
            company,
            actor,
            expected_revision,
            provisioner_credential_id,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn disable_memory(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        expected_revision: i64,
    ) -> AppResult<()> {
        let mut tx = self.pool().begin().await?;
        disable_on(&mut tx, company, actor, expected_revision).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn retry_memory_provisioning(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        expected_revision: i64,
    ) -> AppResult<u64> {
        let id = company.as_uuid();
        let mut tx = self.pool().begin().await?;
        lock_authority(&mut tx, company, actor).await?;
        match lock_tenant(&mut tx, id).await? {
            None => return Err(AppError::BadRequest("Memory is not enabled".into())),
            Some(tenant) if tenant.revision != expected_revision => return Err(stale()),
            Some(tenant) if tenant.state != "enabled" => {
                return Err(AppError::BadRequest("Memory is disabled".into()));
            }
            Some(_) => {}
        }
        let redriven = redrive_failed_on(&mut tx, id, None, Redrive::Owner).await?;
        tx.commit().await?;
        Ok(redriven)
    }

    async fn set_memory_capture(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        change: &crate::application::memory::MemoryCaptureChange,
    ) -> AppResult<()> {
        let mut tx = self.pool().begin().await?;
        super::capture::set_capture_on(&mut tx, company, actor, change).await?;
        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl crate::application::memory::MemoryPublicationGate for PostgresPersistence {
    async fn memory_enabled(&self, company: CompanyId) -> AppResult<bool> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM memory_tenants AS tenant \
             WHERE tenant.company_id = $1 AND tenant.state = 'enabled')",
        )
        .bind(company.as_uuid())
        .fetch_one(self.pool())
        .await?)
    }
}
