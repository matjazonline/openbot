//! Company-first reconciliation snapshots restore parked/terminal frozen intent.
//! Settlement and dispatch permission stay separate from this read boundary.
mod facts;
mod settlement;

use super::*;
use crate::application::workflow::actions::*;
use crate::application::workflow::actions::{
    ActionRunAuthority, FrozenAction, validate_resource, validate_run_authority,
};
use crate::application::workflow::binding::ResourceStatus;
use crate::application::workflow::lease::WorkflowGeneration;
use crate::application::workflow::publication::restore_bundle;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Lock the command actor's actual resource authority through the transaction.
/// Recording past truth requires access; it does not require current I/O approval.
#[async_trait]
pub trait SqlReconciliationResources: Send + Sync {
    async fn lock_resource(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: WorkflowActor,
        frozen: &ActionRunAuthority,
    ) -> AppResult<ResourceStatus>;
}

pub struct PostgresActionReconciliation<A> {
    persistence: PostgresPersistence,
    resources: A,
}
impl<A: SqlReconciliationResources> PostgresActionReconciliation<A> {
    pub fn new(persistence: PostgresPersistence, resources: A) -> Self {
        Self {
            persistence,
            resources,
        }
    }
}

#[async_trait]
impl<A: SqlReconciliationResources> ActionReconciliation for PostgresActionReconciliation<A> {
    async fn association(&self, command: &ReconcileActionCommand) -> AppResult<RelatedAssociation> {
        let mut tx = self.persistence.pool().begin().await?;
        sqlx::query(
            "SELECT set_config('lock_timeout','5s',true),set_config('statement_timeout','5s',true)",
        )
        .execute(&mut *tx)
        .await?;
        authority::authorize_company(&mut tx, command.scope.company, command.actor).await?;
        let head = head_on(&mut tx, command).await?;
        association::authorize_association(
            &mut tx,
            command.scope.company,
            command.actor,
            head.association,
        )
        .await?;
        tx.commit().await?;
        Ok(head.association)
    }

    /// First bounded transaction; replay and revision refusals require current
    /// company/association/resource authority before reading the command namespace.
    async fn snapshot(
        &self,
        command: &ReconcileActionCommand,
    ) -> AppResult<ReconciliationPreparation> {
        let request_digest = command.request_digest()?;
        let mut tx = self.persistence.pool().begin().await?;
        sqlx::query(
            "SELECT set_config('lock_timeout','5s',true),set_config('statement_timeout','5s',true)",
        )
        .execute(&mut *tx)
        .await?;
        authority::authorize_company(&mut tx, command.scope.company, command.actor).await?;
        let head = head_on(&mut tx, command).await?;
        association::authorize_association(
            &mut tx,
            command.scope.company,
            command.actor,
            head.association,
        )
        .await?;
        let frozen = restore_on(&mut tx, command).await?;
        let resource = self
            .resources
            .lock_resource(&mut tx, command.actor, &frozen)
            .await?;
        validate_resource(frozen.action.request(), &resource)?;
        let action = frozen.action;
        let marker_created_at = marker_on(&mut tx, command).await?;
        if let Some(result) = replay_on(&mut tx, command, &request_digest, head.revision).await? {
            tx.commit().await?;
            return Ok(recorded(action, result));
        }
        let refusal = if head.revision != command.expected_revision {
            Some(ReconciliationOutcome::RevisionConflict)
        } else if overflow_on(&mut tx, command).await? {
            Some(ReconciliationOutcome::Blocked {
                reason: ReconciliationBlockedReason::BoundExceeded,
            })
        } else {
            None
        };
        if let Some(outcome) = refusal {
            record_refusal_on(&mut tx, command, &request_digest, head.revision, &outcome).await?;
            tx.commit().await?;
            return Ok(recorded(action, refusal_result(outcome, head.revision)));
        }
        let entries = entries_on(&mut tx, command).await?;
        let database_now = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx)
            .await?;
        let coverage_digest = ReconciliationSnapshot::coverage(
            &action,
            &command.subject,
            command.marker,
            marker_created_at,
            &entries,
        )?;
        let snapshot = ReconciliationSnapshot {
            action,
            subject: command.subject.clone(),
            marker: command.marker,
            marker_created_at,
            entries,
            revision: head.revision,
            database_now,
            coverage_digest,
        };
        snapshot.validate(command)?;
        tx.commit().await?;
        Ok(ReconciliationPreparation::Snapshot(Box::new(snapshot)))
    }
    async fn settle(
        &self,
        command: &ReconcileActionCommand,
        snapshot: &ReconciliationSnapshot,
        evidence: VerifiedEvidence,
    ) -> AppResult<ReconciliationResult> {
        // Box the SQL settlement seam to keep the service/provider chain bounded.
        Box::pin(settlement::settle(self, command, snapshot, evidence)).await
    }
}

async fn head_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
) -> AppResult<RunHead> {
    sqlx::query_as::<_, inspection::HeadRow>(&format!("{} FOR UPDATE OF run", inspection::HEAD))
        .bind(command.scope.company.as_uuid())
        .bind(command.scope.run.as_uuid())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(missing)?
        .restore()
}

#[derive(sqlx::FromRow)]
struct FrozenRunRow {
    actor_id: Uuid,
    version_id: Uuid,
    workflow_id: Uuid,
    bundle: Vec<u8>,
    resources: Value,
}

async fn restore_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
) -> AppResult<ActionRunAuthority> {
    let run = sqlx::query_as::<_, FrozenRunRow>("SELECT actor_id,version_id,workflow_id,bundle,resources FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid()).fetch_one(&mut **tx).await?;
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(command.scope.company.as_uuid())
    .bind(command.scope.run.as_uuid())
    .bind(command.scope.execution.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2 AND queue_kind='workflow' FOR UPDATE")
        .bind(command.scope.company.as_uuid()).bind(command.scope.execution.as_uuid()).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    let operation: Value = sqlx::query_scalar("SELECT operation FROM workflow_action_intents WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND id=$4 AND argument_digest=$5 FOR SHARE")
        .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid()).bind(command.scope.execution.as_uuid())
        .bind(command.subject.invocation.as_uuid()).bind(command.subject.argument_digest.as_str())
        .fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    let action = FrozenAction::restore(operation)?;
    let bundle = restore_bundle(
        &run.bundle,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )?;
    if bundle.compiled().graph().definition().version_id.as_uuid() != run.version_id
        || bundle.compiled().graph().definition().workflow_id.as_uuid() != run.workflow_id
    {
        return Err(invalid());
    }
    let frozen = ActionRunAuthority {
        action,
        actor: WorkflowActor::authenticated(run.actor_id)?,
        bundle,
        resources: serde_json::from_value(run.resources).map_err(|_| invalid())?,
    };
    validate_run_authority(command.scope, &command.subject, &frozen)?;
    Ok(frozen)
}

async fn marker_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
) -> AppResult<DateTime<Utc>> {
    sqlx::query_scalar("SELECT created_at FROM workflow_action_dispatches WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND invocation_id=$4 AND argument_digest=$5 AND id=$6 AND effect_kind='remote' FOR SHARE")
        .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid()).bind(command.scope.execution.as_uuid())
        .bind(command.subject.invocation.as_uuid()).bind(command.subject.argument_digest.as_str()).bind(command.marker.as_uuid())
        .fetch_optional(&mut **tx).await?.ok_or_else(missing)
}

#[derive(sqlx::FromRow)]
struct SavedCommand {
    request_digest: String,
    result_revision: i64,
    outcome: Value,
    evidence_id: Option<Uuid>,
}
async fn replay_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    digest: &ReconciliationRequestDigest,
    current_revision: RunRevision,
) -> AppResult<Option<ReconciliationResult>> {
    let saved = sqlx::query_as::<_, SavedCommand>("SELECT request_digest,result_revision,outcome,evidence_id FROM workflow_action_evidence_commands WHERE company_id=$1 AND command_key=$2")
        .bind(command.scope.company.as_uuid()).bind(command.command_key.as_str()).fetch_optional(&mut **tx).await?;
    let Some(saved) = saved else {
        return Ok(None);
    };
    if saved.request_digest != digest.as_str() {
        return Ok(Some(ReconciliationResult {
            outcome: ReconciliationOutcome::IdempotencyConflict,
            revision: current_revision,
            evidence: None,
            replayed: false,
        }));
    }
    let revision = u64::try_from(saved.result_revision).map_err(|_| invalid())?;
    if revision == 0 {
        return Err(invalid());
    }
    Ok(Some(ReconciliationResult {
        outcome: serde_json::from_value(saved.outcome).map_err(|_| invalid())?,
        revision: RunRevision(revision),
        evidence: saved.evidence_id.map(ActionEvidenceId::new),
        replayed: true,
    }))
}
async fn record_refusal_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    digest: &ReconciliationRequestDigest,
    result_revision: RunRevision,
    outcome: &ReconciliationOutcome,
) -> AppResult<()> {
    sqlx::query("INSERT INTO workflow_action_evidence_commands(company_id,command_key,id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,actor_id,request_digest,expected_revision,result_revision,outcome) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
        .bind(command.scope.company.as_uuid()).bind(command.command_key.as_str()).bind(Uuid::new_v4())
        .bind(command.scope.run.as_uuid()).bind(command.scope.execution.as_uuid()).bind(command.subject.invocation.as_uuid())
        .bind(command.subject.argument_digest.as_str()).bind(command.marker.as_uuid()).bind(command.actor.user_id())
        .bind(digest.as_str()).bind(revision(command.expected_revision.0)?).bind(revision(result_revision.0)?)
        .bind(serde_json::to_value(outcome).map_err(|_| invalid())?).execute(&mut **tx).await?;
    Ok(())
}
async fn overflow_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
) -> AppResult<bool> {
    sqlx::query_scalar("SELECT (SELECT count(*) FROM (SELECT id FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2 LIMIT 129) AS entries)>128 OR (SELECT count(*) FROM (SELECT id FROM workflow_action_dispatches WHERE company_id=$1 AND execution_id=$3 LIMIT 129) AS siblings)>128")
        .bind(command.scope.company.as_uuid()).bind(command.subject.invocation.as_uuid()).bind(command.scope.execution.as_uuid()).fetch_one(&mut **tx).await.map_err(Into::into)
}
#[derive(sqlx::FromRow)]
struct EntryRow {
    id: Uuid,
    attempt_id: Uuid,
    execution_generation: Uuid,
    worker_id: Uuid,
    created_at: DateTime<Utc>,
}
async fn entries_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
) -> AppResult<Vec<ReconciliationEntry>> {
    let rows = sqlx::query_as::<_, EntryRow>("SELECT entry.id,attempt.id AS attempt_id,entry.execution_generation,entry.worker_id,entry.created_at FROM workflow_action_remote_entries AS entry JOIN task_attempts AS attempt ON attempt.task_id=entry.job_id AND attempt.attempt_number=entry.attempt_number AND attempt.execution_generation=entry.execution_generation AND attempt.worker_id=entry.worker_id WHERE entry.company_id=$1 AND entry.invocation_id=$2 ORDER BY entry.id LIMIT 129")
        .bind(command.scope.company.as_uuid()).bind(command.subject.invocation.as_uuid()).fetch_all(&mut **tx).await?;
    if rows.len() > MAX_RECONCILIATION_COVERAGE {
        return Err(invalid());
    }
    Ok(rows
        .into_iter()
        .map(|row| ReconciliationEntry {
            id: ActionRemoteEntryId::new(row.id),
            attempt: ActionRemoteAttemptId::new(row.attempt_id),
            generation: WorkflowGeneration(row.execution_generation),
            worker: WorkerId::new(row.worker_id),
            created_at: row.created_at,
        })
        .collect())
}

fn recorded(action: FrozenAction, result: ReconciliationResult) -> ReconciliationPreparation {
    ReconciliationPreparation::Recorded {
        action: Box::new(action),
        result,
    }
}
fn refusal_result(outcome: ReconciliationOutcome, revision: RunRevision) -> ReconciliationResult {
    ReconciliationResult {
        outcome,
        revision,
        evidence: None,
        replayed: false,
    }
}
