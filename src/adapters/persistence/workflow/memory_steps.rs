//! `memory.load` and `memory.save` against the claimed run (phase 05 E3, E4, E6, E8). Each
//! write keeps the existing lock order: the authority prelock and run lock (`http_runtime::facts`),
//! the fence (`lease::lock_fence`), then `memory_tenants` and `memory_namespaces` FOR SHARE, then
//! the intent. The service is never called inside one of these transactions.
use super::http::runtime;
use super::persistence_bound::{Attempt, End, WriteBound};
use super::*;
use crate::adapters::persistence::credentials::CredentialContext;
use crate::adapters::persistence::memory::deliveries::{self, SaveEnqueue, SaveIntent, SaveScope};
use crate::application::memory::{
    ClientCredential, MemoryDetail, MemoryNamespaces, MemorySaveReceipt, NamespaceExternalId,
    NamespaceOwner, TenantExternalId,
};
use crate::application::workflow::{
    actions::ActionScope,
    activation::ActivationRequest,
    lease::{ClaimedWorkflow, LeasePolicy},
    memory::{LOAD, LoadAuthority, LoadAuthorization, MemoryStepScope, SAVE, SavePark},
    polling::WorkflowStepKind,
};
use crate::entities::access_level::AccessLevel;
use serde_json::{Value, json};

fn bound(name: &'static str, claim: &ClaimedWorkflow) -> AppResult<WriteBound> {
    let policy = LeasePolicy::new(std::time::Duration::from_secs(6))?;
    Ok(WriteBound::within(
        name,
        policy,
        claim.window.expires.min(claim.window.run_deadline),
    ))
}

fn ownership_lost() -> AppError {
    AppError::Execution(crate::app_error::ExecutionFailure::OwnershipLost)
}

fn action(claim: &ClaimedWorkflow) -> ActionScope {
    let scope = claim.fence.scope;
    ActionScope {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
    }
}

/// The step's literal scope, its agent resolved against the frozen bundle (E4).
fn step_scope(bundle: &PublishedBundle, inputs: &Value) -> AppResult<MemoryStepScope> {
    match inputs["scope"]["kind"].as_str() {
        Some("company") => Ok(MemoryStepScope::Company),
        Some("agent") => {
            let key = inputs["scope"]["agent"].as_str().ok_or_else(invalid)?;
            bundle
                .snapshots()
                .agents
                .iter()
                .find(|agent| agent.key.as_str() == key)
                .map(|agent| MemoryStepScope::Agent(agent.agent_id.as_uuid()))
                .ok_or_else(|| {
                    AppError::BadRequest("Memory scope agent is not in this bundle".into())
                })
        }
        _ => Err(invalid()),
    }
}

fn owner_columns(owner: NamespaceOwner) -> (&'static str, Option<Uuid>, Option<Uuid>) {
    match owner {
        NamespaceOwner::Company => ("company", None, None),
        NamespaceOwner::Agent(agent) => ("agent", Some(agent), None),
        NamespaceOwner::User(user) => ("user", None, Some(user)),
    }
}

/// Opened under the fence: the tenant must be enabled and the scope's namespace ready.
pub(super) enum Namespace {
    Ready(NamespaceExternalId, ClientCredential),
    NotReady,
    NotEnabled,
}

/// The ready namespace of `owner` with its opened client credential, under the tenant and
/// namespace share locks (E6 order: tenant before namespace).
pub(super) async fn namespace_on(
    persistence: &PostgresPersistence,
    db: &mut PgConnection,
    company: CompanyId,
    owner: NamespaceOwner,
) -> AppResult<Namespace> {
    let enabled: Option<bool> = sqlx::query_scalar(
        "SELECT tenant.state = 'enabled' FROM memory_tenants AS tenant \
         WHERE tenant.company_id=$1 FOR SHARE",
    )
    .bind(company.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    if enabled != Some(true) {
        return Ok(Namespace::NotEnabled);
    }
    let (kind, agent, user) = owner_columns(owner);
    let ready: Option<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT namespace.id, namespace.credential_id, namespace.credential_envelope \
         FROM memory_namespaces AS namespace \
         WHERE namespace.company_id=$1 AND namespace.scope_kind=$2 \
           AND namespace.agent_id IS NOT DISTINCT FROM $3 \
           AND namespace.user_id IS NOT DISTINCT FROM $4 \
           AND namespace.state='ready' FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(kind)
    .bind(agent)
    .bind(user)
    .fetch_optional(&mut *db)
    .await?;
    let Some((namespace, credential_id, envelope)) = ready else {
        return Ok(Namespace::NotReady);
    };
    let token = persistence.credential_cipher()?.open_envelope(
        &CredentialContext::memory_client_credential(company.as_uuid(), namespace),
        &envelope,
    )?;
    Ok(Namespace::Ready(
        NamespaceExternalId(namespace),
        ClientCredential {
            tenant: TenantExternalId(company.as_uuid()),
            id: credential_id,
            token,
        },
    ))
}

#[derive(sqlx::FromRow)]
struct RunLevels {
    knowledge_ceiling: i16,
    content_level: i16,
}

/// The read ceiling (E3): `min(admitted ceiling, current destination, grant read_max 9)`, with a
/// destination below what the run already consumed refused before any request.
async fn read_ceiling_on(db: &mut PgConnection, claim: &ClaimedWorkflow) -> AppResult<AccessLevel> {
    read_ceiling_for(db, claim.fence).await
}

/// `read_ceiling_on` for a fenced attempt; the memory tools use it as the steps do (E3).
pub(super) async fn read_ceiling_for(
    db: &mut PgConnection,
    fence: crate::application::workflow::lease::WorkflowFence,
) -> AppResult<AccessLevel> {
    let scope = fence.scope;
    let levels = sqlx::query_as::<_, RunLevels>(
        "SELECT knowledge_ceiling::smallint AS knowledge_ceiling, content_level::smallint AS content_level \
         FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let levels = knowledge::stored_levels(levels.knowledge_ceiling, levels.content_level)?;
    let destination = knowledge::destination_level_on(db, scope.company, scope.run).await?;
    if destination < levels.content() {
        return Err(AppError::KnowledgeRelease {
            content: levels.content(),
            destination,
        });
    }
    Ok(levels.ceiling().min(destination).min(AccessLevel::MAX))
}

async fn authorize_on(
    persistence: &PostgresPersistence,
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    bound: &WriteBound,
) -> AppResult<LoadAuthorization> {
    let facts = runtime::facts(tx, action(claim)).await?;
    if !lease::lock_fence(tx, claim.fence, bound.server_timeout()).await? {
        return Err(ownership_lost());
    }
    live(tx, claim, bound).await?;
    let inputs = runtime::frozen_inputs(tx, &facts, claim, &WorkflowStepKind(LOAD.into())).await?;
    let scope = step_scope(&facts.bundle, &inputs)?;
    let read_ceiling = read_ceiling_on(tx, claim).await?;
    let query = inputs["query"].as_str().ok_or_else(invalid)?.to_owned();
    let limit = inputs["limit"]
        .as_u64()
        .and_then(|limit| u32::try_from(limit).ok())
        .ok_or_else(invalid)?;
    Ok(
        match namespace_on(persistence, tx, claim.fence.scope.company, scope.owner()).await? {
            Namespace::NotEnabled => LoadAuthorization::NotEnabled,
            Namespace::NotReady => LoadAuthorization::NotReady(scope.owner()),
            Namespace::Ready(namespace, credential) => {
                LoadAuthorization::Ready(Box::new(LoadAuthority {
                    scope,
                    namespace,
                    credential,
                    read_ceiling,
                    query,
                    limit,
                }))
            }
        },
    )
}

async fn observe_on(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    used: AccessLevel,
    exposed: AccessLevel,
    items: &[MemoryDetail],
    bound: &WriteBound,
) -> AppResult<()> {
    let facts = runtime::facts(tx, action(claim)).await?;
    if !lease::lock_fence(tx, claim.fence, bound.server_timeout()).await? {
        return Err(ownership_lost());
    }
    live(tx, claim, bound).await?;
    runtime::frozen_inputs(tx, &facts, claim, &WorkflowStepKind(LOAD.into())).await?;
    // The answer must stay within the ceiling the request was authorized at (E6).
    if exposed > used
        || items
            .iter()
            .any(|item| i16::from(item.access_level) > used.as_i16())
    {
        return Err(AppError::KnowledgeRelease {
            content: exposed.max(used),
            destination: used,
        });
    }
    let scope = claim.fence.scope;
    // The answer's own label and every returned item's (review F11), the same level the step
    // output reports.
    let carried =
        crate::application::workflow::memory::carried_level(exposed, items).ok_or_else(invalid)?;
    knowledge::raise_content_level_on(tx, scope.company, scope.run, carried).await?;
    let refs: Vec<Value> = items
        .iter()
        .map(|item| json!({"id": item.observation_id, "revision": item.revision, "content_sha256": item.content_sha256}))
        .collect();
    sqlx::query(
        "INSERT INTO workflow_memory_consumptions (company_id,run_id,execution_id,job_id, \
           attempt_number,invocation_id,read_ceiling,exposed_level,items) \
         VALUES ($1,$2,$3,$4,$5,'00000000-0000-0000-0000-000000000000',$6::smallint,$7::smallint,$8) \
         ON CONFLICT (company_id,run_id,execution_id,job_id,attempt_number,invocation_id) \
         DO UPDATE SET read_ceiling=EXCLUDED.read_ceiling, exposed_level=EXCLUDED.exposed_level, \
           items=EXCLUDED.items",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .bind(claim.fence.attempt.0)
    .bind(used.as_i16())
    .bind(carried.as_i16())
    .bind(Value::Array(refs))
    .execute(&mut **tx)
    .await?;
    live(tx, claim, bound).await
}

async fn park_on(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    bound: &WriteBound,
) -> AppResult<End<SavePark>> {
    let facts = runtime::facts(tx, action(claim)).await?;
    if !lease::lock_fence(tx, claim.fence, bound.server_timeout()).await? {
        return Err(ownership_lost());
    }
    live(tx, claim, bound).await?;
    let inputs = runtime::frozen_inputs(tx, &facts, claim, &WorkflowStepKind(SAVE.into())).await?;
    let step = step_scope(&facts.bundle, &inputs)?;
    let facts_text: Vec<String> = inputs["facts"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|fact| fact.as_str().map(str::to_owned).ok_or_else(invalid))
        .collect::<AppResult<_>>()?;
    let scope = claim.fence.scope;
    let wait = Uuid::new_v4();
    // One logical save per execution: its key survives every retry of this step (E4).
    let key = format!("ob.s.{}", scope.execution.as_uuid().simple());
    let intent = SaveIntent {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
        invocation: None,
        wait: Some(wait),
        scope: match step {
            MemoryStepScope::Company => SaveScope::Company,
            MemoryStepScope::Agent(agent) => SaveScope::Agent(agent),
        },
        key: &key,
        facts: &facts_text,
        external_sources: Vec::new(),
    };
    match deliveries::enqueue_save_on(tx, &intent).await? {
        SaveEnqueue::Enqueued { replayed: true, .. } => Err(AppError::Conflict(
            "memory.save intent already parked for this execution".into(),
        )),
        SaveEnqueue::Enqueued {
            replayed: false, ..
        } => {
            park_effect_on(tx, claim, wait).await?;
            live_after_park(tx, claim).await?;
            Ok(End::Commit(SavePark::Parked(WaitId::new(wait))))
        }
        SaveEnqueue::NotEnabled => Ok(End::Rollback(SavePark::NotEnabled)),
        SaveEnqueue::NotReady => Ok(End::Rollback(SavePark::NotReady(step.owner()))),
        SaveEnqueue::TooLarge => Ok(End::Rollback(SavePark::TooLarge)),
    }
}

/// The effect wait and the fenced close of this attempt and job (`memory_parked`).
async fn park_effect_on(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    wait: Uuid,
) -> AppResult<()> {
    let fence = claim.fence;
    let scope = fence.scope;
    sqlx::query("INSERT INTO workflow_waits (company_id,run_id,execution_id,id,reason,deadline,created_at) SELECT company_id,id,$3,$4,'effect',deadline,clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait)
        .execute(&mut **tx).await?;
    let closed = sqlx::query("UPDATE task_attempts SET status='completed',stop_reason='memory_parked',finished_at=clock_timestamp() WHERE task_id=$1 AND attempt_number=$2 AND worker_id=$3 AND execution_generation=$4 AND status='processing'")
        .bind(scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0)
        .execute(&mut **tx).await?;
    sqlx::query("UPDATE workflow_runs SET state='waiting',waiting_reason='effect' WHERE company_id=$1 AND id=$2 AND state IN ('queued','running')")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut **tx).await?;
    let job = sqlx::query("UPDATE background_tasks SET status='completed',updated_at=clock_timestamp(),worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND status='processing' AND worker_id=$4 AND execution_generation=$5 AND attempt_ordinal=$6 AND lock_expires_at>clock_timestamp()")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid())
        .bind(fence.worker.0).bind(fence.generation.0).bind(fence.attempt.0)
        .execute(&mut **tx).await?;
    if closed.rows_affected() != 1 || job.rows_affected() != 1 {
        return Err(super::persistence_bound::refused("memory park"));
    }
    waits::audit_wait(tx, scope, wait, "wait_parked").await
}

/// After the park the job is no longer processing; only the run deadline can still end it.
async fn live_after_park(tx: &mut PgConnection, claim: &ClaimedWorkflow) -> AppResult<()> {
    let open: bool = sqlx::query_scalar(
        "SELECT deadline > clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(claim.fence.scope.company.as_uuid())
    .bind(claim.fence.scope.run.as_uuid())
    .fetch_one(tx)
    .await?;
    if !open {
        return Err(ownership_lost());
    }
    Ok(())
}

async fn live(tx: &mut PgConnection, claim: &ClaimedWorkflow, bound: &WriteBound) -> AppResult<()> {
    if lease::live_window(tx, claim.fence, bound.started())
        .await?
        .is_none()
    {
        return Err(ownership_lost());
    }
    Ok(())
}

#[async_trait]
impl crate::application::workflow::memory::MemoryStepStore for PostgresPersistence {
    async fn authorize_memory_load(&self, claim: &ClaimedWorkflow) -> AppResult<LoadAuthorization> {
        let bound = bound("memory.authorize", claim)?;
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                // A read: nothing it locked or checked is kept.
                let result = authorize_on(self, &mut tx, claim, &bound)
                    .await
                    .map(End::Rollback);
                bound.end(tx, result).await
            }))
            .await;
        match attempt {
            Attempt::Done(authority) => Ok(authority),
            Attempt::Failed(error) => Err(error),
            Attempt::Ambiguous => Err(bound.unresolved()),
        }
    }

    async fn observe_memory_load(
        &self,
        claim: &ClaimedWorkflow,
        read_ceiling: AccessLevel,
        exposed: AccessLevel,
        items: &[MemoryDetail],
    ) -> AppResult<()> {
        let bound = bound("memory.consume", claim)?;
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = observe_on(&mut tx, claim, read_ceiling, exposed, items, &bound)
                    .await
                    .map(End::Commit);
                bound.end(tx, result).await
            }))
            .await;
        match attempt {
            Attempt::Done(()) => Ok(()),
            Attempt::Failed(error) => Err(error),
            // A possibly committed rise grants no output (E6: an ambiguous commit grants nothing).
            Attempt::Ambiguous => Err(bound.unresolved()),
        }
    }

    async fn park_memory_save(&self, claim: &ClaimedWorkflow) -> AppResult<SavePark> {
        let bound = bound("memory.save", claim)?;
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = park_on(&mut tx, claim, &bound).await;
                bound.end(tx, result).await
            }))
            .await;
        match attempt {
            Attempt::Done(park) => Ok(park),
            Attempt::Failed(error) => Err(error),
            // The parked intent is the effect's only path; an unknown commit is recovered by the
            // supervisor's suspension proof or the retry's replay refusal.
            Attempt::Ambiguous => Err(bound.unresolved()),
        }
    }

    async fn provision_memory(
        &self,
        claim: &ClaimedWorkflow,
        owner: NamespaceOwner,
    ) -> AppResult<()> {
        // Its own short transaction, outside the run's (P7b obligation).
        self.ensure_namespace(claim.fence.scope.company, owner)
            .await
            .map(|_| ())
    }
}

/// What a memory effect wait sees (E8): nothing yet, the delivered receipt as the step output,
/// or the terminal outcome. `None` when this wait has no memory intent.
pub(super) enum MemoryResume {
    Pending,
    Delivered(Value),
    Failed(String),
}

pub(super) async fn effect_resume_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
) -> AppResult<Option<MemoryResume>> {
    let intent: Option<(String, Option<Value>, Option<String>)> = sqlx::query_as(
        "SELECT intent.state, intent.result, intent.outcome_code FROM memory_deliveries AS intent \
         WHERE intent.company_id=$1 AND intent.execution_id=$2 AND intent.wait_id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(wait)
    .fetch_optional(&mut *db)
    .await?;
    let Some((state, result, outcome)) = intent else {
        return Ok(None);
    };
    Ok(Some(match state.as_str() {
        "pending" | "claimed" => MemoryResume::Pending,
        "delivered" => {
            let receipt: MemorySaveReceipt =
                serde_json::from_value(result.ok_or_else(invalid)?).map_err(|_| invalid())?;
            MemoryResume::Delivered(crate::application::workflow::memory::save_output(&receipt))
        }
        _ => MemoryResume::Failed(outcome.unwrap_or_else(|| state.clone())),
    }))
}

/// Supervisor proof that this fence was closed by the memory park and owns `wait`.
pub(super) async fn confirm_suspension(
    persistence: &PostgresPersistence,
    fence: crate::application::workflow::lease::WorkflowFence,
    wait: Uuid,
) -> AppResult<bool> {
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM task_attempts AS attempt \
               JOIN memory_deliveries AS intent ON intent.company_id=$1 AND intent.execution_id=$2 AND intent.wait_id=$3 \
              WHERE attempt.task_id=$4 AND attempt.attempt_number=$5 AND attempt.worker_id=$6 \
                AND attempt.execution_generation=$7 AND attempt.status='completed' \
                AND attempt.stop_reason='memory_parked')",
        )
        .bind(fence.scope.company.as_uuid())
        .bind(fence.scope.execution.as_uuid())
        .bind(wait)
        .bind(fence.scope.job.0)
        .bind(fence.attempt.0)
        .bind(fence.worker.0)
        .bind(fence.generation.0)
        .fetch_one(persistence.pool()),
    )
    .await
    .map_err(|_| lease::timed_out())?
    .map_err(Into::into)
}
