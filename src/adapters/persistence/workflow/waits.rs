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
    async fn record_event(&self, event: WorkflowEvent) -> AppResult<EventIngress> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            let result = record_event_on(&mut tx, event).await?;
            if result == EventIngress::Recorded {
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
    async fn consume_message_receipts(
        &self,
        scope: ActivationRequest,
        policy: crate::application::workflow::lease::LeasePolicy,
    ) -> AppResult<u16> {
        message::receipts::consume_page(self, scope, policy).await
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
    let payload:Option<Value>=sqlx::query_scalar("SELECT payload FROM background_tasks WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status IN ('pending','completed','failed') AND worker_id IS NULL AND execution_generation IS NULL AND lock_expires_at IS NULL FOR UPDATE")
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
pub(super) enum WaitSelection {
    Execution,
    Decision,
}
pub(super) async fn row(
    db: &mut PgConnection,
    scope: ActivationRequest,
    selection: WaitSelection,
) -> AppResult<Option<WaitRow>> {
    let selection = match selection {
        WaitSelection::Execution => "execution",
        WaitSelection::Decision => "decision",
    };
    // A completed approval does not complete its execution. Only the active wait or
    // the actual terminal step wait can answer execution resume/replay.
    Ok(sqlx::query_as::<_,WaitRow>("SELECT id,deadline,reason,state,event_name,correlation,failure_code FROM workflow_waits AS wait WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND (($4='decision' AND reason='decision') OR ($4='execution' AND (state='waiting' OR NOT EXISTS(SELECT 1 FROM workflow_decisions AS decision WHERE decision.company_id=wait.company_id AND decision.wait_id=wait.id AND decision.subject_kind='action_approval')))) ORDER BY (state='waiting') DESC,created_at DESC LIMIT 1 FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(selection).fetch_optional(db).await?)
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
    if let Some(wait) = row(db, scope, WaitSelection::Execution).await? {
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
    // The response is the stored deadline, as a replay reads it: timestamptz keeps microseconds,
    // so returning the capped Rust value would answer the first park differently from the next.
    let deadline: DateTime<Utc> = sqlx::query_scalar("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,created_at,event_name,correlation,notification_intent) VALUES($1,$2,$3,$4,$5,$6,clock_timestamp(),$7,$8,$9) RETURNING deadline")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(id).bind(reason).bind(deadline).bind(spec.name).bind(spec.correlation).bind(intent).fetch_one(&mut *db).await?;
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
/// Persist a run-scoped event before any worker wakes for it (05-D1 §11, 05-D2 §11). Producers
/// to one run serialize on an advisory lock, so the unconsumed bound is exact; consumers never
/// take it, and a concurrent consumption only lowers the count.
async fn record_event_on(db: &mut PgConnection, event: WorkflowEvent) -> AppResult<EventIngress> {
    if event.name.0.is_empty()
        || event.name.0.len() > 128
        || event.correlation.0.is_empty()
        || event.correlation.0.len() > 256
    {
        return Err(AppError::BadRequest("Invalid workflow event".into()));
    }
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)").execute(&mut *db).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('workflow_wait_events:' || $1::text || ':' || $2::text, 0))")
        .bind(event.company.as_uuid()).bind(event.run.as_uuid()).execute(&mut *db).await?;
    let saved: Option<SavedEvent> = sqlx::query_as("SELECT run_id,event_name,correlation,payload FROM workflow_wait_events WHERE company_id=$1 AND id=$2")
        .bind(event.company.as_uuid()).bind(event.id.0).fetch_optional(&mut *db).await?;
    if let Some(saved) = saved {
        return if saved.run_id == event.run.as_uuid()
            && saved.event_name == event.name.0
            && saved.correlation == event.correlation.0
            && saved.payload == event.payload
        {
            Ok(EventIngress::Duplicate)
        } else {
            Err(AppError::Conflict("Workflow event id was reused".into()))
        };
    }
    let run: Option<(Uuid, bool)> = sqlx::query_as("SELECT binding_id,state IN ('queued','running','waiting') AND deadline>clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(event.company.as_uuid()).bind(event.run.as_uuid()).fetch_optional(&mut *db).await?;
    let Some((binding_id, true)) = run else {
        return Ok(EventIngress::Unmatched);
    };
    let binding = admission_binding::read_saved_binding(
        db,
        event.company,
        WorkflowBindingId::new(binding_id),
        event.run.as_uuid(),
    )
    .await?;
    let compiled = binding.bundle().compiled();
    let declared = Value::String(event.name.0.clone());
    let Some(step) = compiled
        .graph()
        .definition()
        .steps
        .iter()
        .find(|(_, step)| {
            step.step_type.as_str() == "wait.event"
                && matches!(step.inputs.get("event"), Some(Binding::Literal(name)) if *name == declared)
        })
        .map(|(id, _)| id.clone())
    else {
        return Ok(EventIngress::Unmatched);
    };
    // Every wait for one name shares one payload schema (registry rule), so this validates
    // before any wait exists, as the execution-scoped signal did.
    compiled
        .validate_step_output(&step, &event.payload)
        .map_err(|_| AppError::BadRequest("Invalid workflow event payload".into()))?;
    let unconsumed: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_wait_events WHERE company_id=$1 AND run_id=$2 AND consumed_execution_id IS NULL")
        .bind(event.company.as_uuid()).bind(event.run.as_uuid()).fetch_one(&mut *db).await?;
    if unconsumed >= MAX_UNCONSUMED_EVENTS {
        return Err(AppError::Conflict("Workflow event limit reached".into()));
    }
    sqlx::query("INSERT INTO workflow_wait_events(company_id,run_id,id,event_name,correlation,payload) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(event.company.as_uuid()).bind(event.run.as_uuid()).bind(event.id.0).bind(event.name.0).bind(event.correlation.0).bind(event.payload).execute(db).await.map_err(|error| {
            // Different runs hold different producer locks but share the event-id namespace.
            if error.as_database_error().is_some_and(|error| error.is_unique_violation() && error.constraint() == Some("workflow_wait_events_pkey")) {
                AppError::Conflict("Workflow event id was reused".into())
            } else {
                error.into()
            }
        })?;
    Ok(EventIngress::Recorded)
}

#[derive(sqlx::FromRow)]
struct SweepCandidate {
    company_id: Uuid,
    run_id: Uuid,
    execution_id: Uuid,
    job_id: Uuid,
}
#[derive(sqlx::FromRow)]
struct SavedEvent {
    run_id: Uuid,
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
