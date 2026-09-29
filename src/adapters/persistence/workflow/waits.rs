use super::*;
use crate::application::workflow::{activation::*, completion::CommitDisposition, waits::*};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(sqlx::FromRow)]
pub(super) struct WaitRun {
    pub binding_id: Uuid,
    pub state: String,
    pub deadline: DateTime<Utc>,
    pub max_steps: i32,
}
#[derive(sqlx::FromRow)]
pub(super) struct WaitRow {
    pub id: Uuid,
    pub deadline: DateTime<Utc>,
    pub reason: String,
    pub state: String,
    pub event_name: Option<String>,
    pub correlation: Option<String>,
    pub failure_code: Option<String>,
}
#[async_trait]
impl WorkflowWaits for PostgresPersistence {
    async fn park_wait(&self, scope: ActivationRequest) -> AppResult<Option<ParkedWorkflow>> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            let result = park_on(&mut tx, scope).await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
    async fn record_signal(&self, signal: WorkflowSignal) -> AppResult<bool> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            let result = signal_on(&mut tx, signal).await?;
            if result {
                tx.commit().await?;
            } else {
                tx.rollback().await?;
            }
            Ok(result)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
    async fn resume_wait(&self, scope: ActivationRequest) -> AppResult<WaitProgress> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            let result = wait_commit::resume_on(&mut tx, scope).await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
    async fn sweep_waits(&self, limit: u16) -> AppResult<u16> {
        if !(1..=128).contains(&limit) {
            return Err(AppError::BadRequest("Invalid wait sweep limit".into()));
        }
        let rows = tokio::time::timeout(Duration::from_secs(2), select_due(&self.pool, limit))
            .await
            .map_err(|_| lease::timed_out())??;
        let mut settled = 0;
        for candidate in rows {
            let scope = ActivationRequest {
                company: CompanyId::new(candidate.company_id),
                run: RunId::new(candidate.run_id),
                execution: ExecutionId::new(candidate.execution_id),
                job: WorkflowJobId(candidate.job_id),
            };
            let changed = match self.resume_wait(scope).await? {
                WaitProgress::Completed(saved) => saved.disposition == CommitDisposition::Committed,
                WaitProgress::Expired(disposition) | WaitProgress::Failed { disposition, .. } => {
                    disposition == CommitDisposition::Committed
                }
                _ => false,
            };
            if changed {
                settled += 1;
            }
        }
        Ok(settled)
    }
}

pub(super) async fn lock(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Option<WaitRun>> {
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)").execute(&mut *db).await?;
    let run=sqlx::query_as::<_,WaitRun>("SELECT binding_id,state,deadline,max_steps FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_optional(&mut *db).await?;
    if run.is_none() {
        return Ok(None);
    }
    let execution: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    if execution.is_none() {
        return Ok(None);
    }
    let payload:Option<Value>=sqlx::query_scalar("SELECT payload FROM background_tasks WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status IN ('pending','completed') AND worker_id IS NULL AND execution_generation IS NULL AND lock_expires_at IS NULL FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).fetch_optional(&mut *db).await?;
    match payload {
        Some(payload) => {
            if decode_job_payload(payload)? == scope.execution {
                Ok(run)
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}
pub(super) async fn row(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Option<WaitRow>> {
    Ok(sqlx::query_as::<_,WaitRow>("SELECT id,deadline,reason,state,event_name,correlation,failure_code FROM workflow_waits WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(db).await?)
}
pub(super) async fn now(db: &mut PgConnection) -> AppResult<DateTime<Utc>> {
    Ok(sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(db)
        .await?)
}
pub(super) async fn audit(
    db: &mut PgConnection,
    scope: ActivationRequest,
    kind: &str,
) -> AppResult<()> {
    sqlx::query("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id) SELECT run.company_id,run.id,(SELECT COALESCE(MAX(event.sequence),0)+1 FROM workflow_run_events AS event WHERE event.company_id=$1 AND event.run_id=$2),$4,run.actor_id,$3 FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(kind).execute(db).await?;
    Ok(())
}
async fn park_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Option<ParkedWorkflow>> {
    let Some(run) = lock(db, scope).await? else {
        return Ok(None);
    };
    if let Some(wait) = row(db, scope).await? {
        return Ok(Some(ParkedWorkflow {
            wait: WaitId::new(wait.id),
            deadline: wait.deadline,
        }));
    }
    let now = now(db).await?;
    if !matches!(run.state.as_str(), "queued" | "running") || run.deadline <= now {
        return Ok(None);
    }
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    if !is_wait(db, scope, binding.bundle()).await? {
        return Ok(None);
    }
    let Some(activation) = pending_recovery::activate(db, scope).await? else {
        return Ok(None);
    };
    let spec = match specification(binding.bundle(), &activation) {
        Ok(spec) => spec,
        Err(_) => {
            pending_recovery::settle(db, scope, pending_recovery::PendingFailure::InvalidInput)
                .await?;
            return Ok(None);
        }
    };
    let deadline = spec.deadline.min(run.deadline);
    if !spec.timer && deadline <= now {
        pending_recovery::settle(db, scope, pending_recovery::PendingFailure::InvalidInput).await?;
        return Ok(None);
    }
    let id = Uuid::new_v4();
    let reason = if spec.timer { "timer" } else { "event" };
    // Registration facts only. Phase04/05 consume this identity for delivery, never a separate queue.
    let intent = json!({"version":1,"kind":"wait_registered","wait_id":id,"run_id":scope.run.as_uuid(),"execution_id":scope.execution.as_uuid()});
    sqlx::query("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,created_at,event_name,correlation,notification_intent) VALUES($1,$2,$3,$4,$5,$6,clock_timestamp(),$7,$8,$9)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(id).bind(reason).bind(deadline).bind(spec.name).bind(spec.correlation).bind(intent).execute(&mut *db).await?;
    completion_job::finish(db, scope, completion_job::CompletionOwner::Pure).await?;
    sqlx::query(
        "UPDATE workflow_runs SET state='waiting',waiting_reason=$3 WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(reason)
    .execute(&mut *db)
    .await?;
    audit(db, scope, "wait_parked").await?;
    Ok(Some(ParkedWorkflow {
        wait: WaitId::new(id),
        deadline,
    }))
}
async fn signal_on(db: &mut PgConnection, signal: WorkflowSignal) -> AppResult<bool> {
    let scope = signal.scope;
    let Some(run) = lock(db, scope).await? else {
        return Ok(false);
    };
    if signal.name.0.is_empty()
        || signal.name.0.len() > 128
        || signal.correlation.0.is_empty()
        || signal.correlation.0.len() > 256
    {
        return Ok(false);
    }
    let saved:Option<SavedSignal>=sqlx::query_as("SELECT run_id,execution_id,event_name,correlation,payload FROM workflow_wait_events WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(signal.id.0).fetch_optional(&mut *db).await?;
    if let Some(saved) = saved {
        return Ok(saved.run_id == scope.run.as_uuid()
            && saved.execution_id == scope.execution.as_uuid()
            && saved.event_name == signal.name.0
            && saved.correlation == signal.correlation.0
            && saved.payload == signal.payload);
    }
    let now = now(db).await?;
    if !matches!(run.state.as_str(), "queued" | "running" | "waiting") || run.deadline <= now {
        return Ok(false);
    }
    if row(db, scope)
        .await?
        .is_some_and(|wait| wait.state != "waiting")
    {
        return Ok(false);
    }
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let activation = activation::activate_on(db, scope).await?;
    let spec = specification(binding.bundle(), &activation)?;
    if spec.timer
        || spec.deadline <= now
        || spec.name.as_ref() != Some(&signal.name.0)
        || spec.correlation.as_ref() != Some(&signal.correlation.0)
    {
        return Ok(false);
    }
    crate::domain::workflow::validate_context_value(
        &signal.payload,
        binding.bundle().compiled().graph().context_limits(),
    )
    .map_err(|_| invalid())?;
    prepare(binding.bundle(), &activation, signal.payload.clone())?;
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_wait_events WHERE company_id=$1 AND run_id=$2 AND execution_id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(&mut *db).await?;
    if count >= 128 {
        return Err(AppError::Conflict("Workflow event limit reached".into()));
    }
    sqlx::query("INSERT INTO workflow_wait_events(company_id,run_id,execution_id,id,event_name,correlation,payload) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(signal.id.0).bind(signal.name.0).bind(signal.correlation.0).bind(signal.payload).execute(db).await?;
    Ok(true)
}

#[derive(sqlx::FromRow)]
struct SweepCandidate {
    company_id: Uuid,
    run_id: Uuid,
    execution_id: Uuid,
    job_id: Uuid,
}
#[derive(sqlx::FromRow)]
struct SavedSignal {
    run_id: Uuid,
    execution_id: Uuid,
    event_name: String,
    correlation: String,
    payload: Value,
}

async fn select_due(pool: &sqlx::PgPool, limit: u16) -> AppResult<Vec<SweepCandidate>> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)").execute(&mut *tx).await?;
    let rows=sqlx::query_as("SELECT wait.company_id,wait.run_id,wait.execution_id,job.id AS job_id FROM workflow_waits AS wait JOIN workflow_runs AS run ON run.company_id=wait.company_id AND run.id=wait.run_id JOIN background_tasks AS job ON job.company_id=wait.company_id AND job.workflow_execution_id=wait.execution_id WHERE wait.state='waiting' AND (wait.deadline<=clock_timestamp() OR run.deadline<=clock_timestamp() OR run.state IN ('cancelled','failed','succeeded')) AND job.queue_kind='workflow' ORDER BY wait.deadline,wait.id LIMIT $1")
            .bind(i64::from(limit)).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(rows)
}

async fn is_wait(
    db: &mut PgConnection,
    scope: ActivationRequest,
    bundle: &PublishedBundle,
) -> AppResult<bool> {
    let step: String = sqlx::query_scalar(
        "SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let step = StepId::parse(step).map_err(|_| invalid())?;
    let definition = bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&step)
        .ok_or_else(invalid)?;
    Ok(matches!(
        definition.step_type.as_str(),
        "wait.event" | "wait.timer"
    ))
}
