//! Human decisions (05-D1 amended by D2–D4). Lock order, the controls' order:
//! company `FOR SHARE` → member ⋈ principal `FOR SHARE` → run `FOR UPDATE` → association rows
//! `FOR SHARE` → execution → job → wait → decision. Workers take the run first and read
//! authority without locks at park; submission rechecks it under the locks.
use super::*;
use crate::application::workflow::{
    activation::*,
    decisions::*,
    waits::{ParkedWorkflow, WaitFailureReason},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::time::Duration;

#[async_trait]
impl WorkflowDecisions for PostgresPersistence {
    async fn park_decision(
        &self,
        scope: ActivationRequest,
        notices: &DecisionNotices,
    ) -> AppResult<Option<ParkedWorkflow>> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            let parked = Box::pin(park_on(&mut tx, scope, notices)).await?;
            tx.commit().await?;
            Ok(parked)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
    async fn submit_decision(&self, command: SubmitDecision) -> AppResult<DecisionSubmission> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            let submitted = Box::pin(submit_on(&mut tx, &command)).await?;
            tx.commit().await.map_err(closed_at_commit)?;
            Ok(submitted)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
    async fn add_decision_comment(
        &self,
        command: AddDecisionComment,
    ) -> AppResult<DecisionComment> {
        command.validate()?;
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            let comment = comment_on(&mut tx, &command).await?;
            tx.commit().await?;
            Ok(comment)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
    async fn read_decision(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        decision: DecisionId,
    ) -> AppResult<DecisionView> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            let view = read_on(&mut tx, company, actor, decision).await?;
            tx.commit().await?;
            Ok(view)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
}

/// The deferred wait guard refuses a settlement that crossed a deadline before commit.
fn closed_at_commit(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code == "23514")
    {
        return closed();
    }
    AppError::from(error)
}
fn closed() -> AppError {
    AppError::Conflict("Workflow decision is closed".into())
}
fn stale() -> AppError {
    AppError::Conflict("Workflow decision revision or artifact is stale".into())
}

#[derive(sqlx::FromRow)]
struct Reviewer {
    principal_id: Uuid,
    email: String,
}
#[derive(sqlx::FromRow)]
struct NoticeContext {
    title: String,
    company_label: String,
    correlation_id: Uuid,
}

async fn park_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
    notices: &DecisionNotices,
) -> AppResult<Option<ParkedWorkflow>> {
    let Some(run) = waits::lock(tx, scope).await? else {
        return Ok(None);
    };
    if let Some(wait) = waits::row(tx, scope, waits::WaitSelection::Decision).await? {
        return Ok(Some(ParkedWorkflow {
            wait: WaitId::new(wait.id),
            deadline: wait.deadline,
        }));
    }
    let now = waits::now(tx).await?;
    if !matches!(run.state.as_str(), "queued" | "running") || run.deadline <= now {
        return Ok(None);
    }
    let binding = admission_binding::read_saved_binding(
        tx,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let pending_recovery::Pending::Ready(activation) =
        pending_recovery::activate(tx, scope).await?
    else {
        return Ok(None);
    };
    if binding
        .bundle()
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&activation.step)
        .is_none_or(|step| step.step_type.as_str() != "decision.human")
    {
        return Ok(None);
    }
    // An unparsable deadline, or one already past, never parks (05-D2 §4.2).
    let spec = match specification(binding.bundle(), &activation) {
        Ok(spec) if spec.deadline > now => spec,
        _ => {
            pending_recovery::settle(tx, scope, pending_recovery::PendingFailure::InvalidInput)
                .await?;
            return Ok(None);
        }
    };
    let deadline = spec.deadline.min(run.deadline);
    let mut reviewers = Vec::with_capacity(spec.reviewers.len());
    for user in &spec.reviewers {
        match eligible_reviewer(tx, scope, *user).await? {
            Some(reviewer) => reviewers.push(reviewer),
            None => return unavailable(tx, scope).await,
        }
    }
    let context: NoticeContext = sqlx::query_as("SELECT version.title,company.name AS company_label,run.correlation_id FROM workflow_runs AS run JOIN workflow_versions AS version ON version.company_id=run.company_id AND version.workflow_id=run.workflow_id AND version.id=run.version_id JOIN companies AS company ON company.id=run.company_id WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut **tx).await?;
    let decision = Uuid::new_v4();
    let composed = match compose_notices(
        notices,
        &reviewers,
        &context,
        &activation.step,
        decision,
        deadline,
    ) {
        Ok(deliveries) => deliveries,
        Err(AppError::BadRequest(_)) => return unavailable(tx, scope).await,
        Err(error) => return Err(error),
    };
    let wait = Uuid::new_v4();
    let intent = json!({"version":1,"kind":"wait_registered","wait_id":wait,"run_id":scope.run.as_uuid(),"execution_id":scope.execution.as_uuid()});
    let deadline: DateTime<Utc> = sqlx::query_scalar("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,created_at,notification_intent) VALUES($1,$2,$3,$4,'decision',$5,clock_timestamp(),$6) RETURNING deadline")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait).bind(deadline).bind(intent).fetch_one(&mut **tx).await?;
    sqlx::query("INSERT INTO workflow_decisions(company_id,run_id,execution_id,id,wait_id,subject_kind,proposal_digest) VALUES($1,$2,$3,$4,$5,'business',$6)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(decision).bind(wait).bind(&spec.proposal_digest).execute(&mut **tx).await?;
    insert_reviewers(tx, scope.company, decision, &reviewers, &composed).await?;
    completion_job::finish(tx, scope, completion_job::CompletionOwner::Pure).await?;
    sqlx::query("UPDATE workflow_runs SET state='waiting',waiting_reason='decision' WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut **tx).await?;
    waits::audit(tx, scope, "decision_opened").await?;
    Ok(Some(ParkedWorkflow {
        wait: WaitId::new(wait),
        deadline,
    }))
}

async fn insert_reviewers(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    decision: Uuid,
    reviewers: &[Reviewer],
    composed: &[crate::transport::NewStandaloneDelivery],
) -> AppResult<()> {
    for (ordinal, (reviewer, delivery)) in reviewers.iter().zip(composed.iter()).enumerate() {
        // The delivery first, then the immutable reviewer row that carries its id (05-D3 §3.1).
        let crate::transport::DeliveryCreation::Created(notice) =
            crate::adapters::persistence::delivery::enqueue::insert_standalone_delivery_on(
                tx, delivery,
            )
            .await?
        else {
            return Err(invalid());
        };
        sqlx::query("INSERT INTO workflow_decision_reviewers(company_id,decision_id,ordinal,principal_id,notice_delivery_id) VALUES($1,$2,$3,$4,$5)")
            .bind(company.as_uuid()).bind(decision).bind(i16::try_from(ordinal).map_err(|_| invalid())?).bind(reviewer.principal_id).bind(notice.as_uuid()).execute(&mut **tx).await?;
    }
    Ok(())
}

/// Compose all notices before writing anything, so deterministic errors settle without a park.
fn compose_notices(
    notices: &DecisionNotices,
    reviewers: &[Reviewer],
    context: &NoticeContext,
    step: &StepId,
    decision: Uuid,
    deadline: DateTime<Utc>,
) -> AppResult<Vec<crate::transport::NewStandaloneDelivery>> {
    reviewers
        .iter()
        .map(|reviewer| {
            notices.compose(&DecisionNotice {
                decision,
                principal: reviewer.principal_id,
                recipient: reviewer.email.clone().into(),
                correlation: context.correlation_id,
                workflow_title: &context.title,
                step,
                company_label: &context.company_label,
                deadline,
            })
        })
        .collect()
}

/// `decision.reviewer_unavailable`: the run fails unrouted, nothing parks (05-D2 §7.1).
async fn unavailable(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
) -> AppResult<Option<ParkedWorkflow>> {
    pending_recovery::settle(
        tx,
        scope,
        pending_recovery::PendingFailure::ReviewerUnavailable,
    )
    .await?;
    Ok(None)
}

/// Park-time assignment check, configuration validation without locks (05-D2 §9.3): a company
/// member's person principal who can see the run's association; owner or admin on a
/// channel-less run. Submission rechecks the same rules under the §8 locks.
async fn eligible_reviewer(
    db: &mut PgConnection,
    scope: ActivationRequest,
    user: Uuid,
) -> AppResult<Option<Reviewer>> {
    Ok(sqlx::query_as("SELECT principal.id AS principal_id,account.email::text AS email FROM principals AS principal \
        JOIN company_members AS member ON member.company_id=principal.company_id AND member.user_id=principal.user_id \
        JOIN users AS account ON account.id=principal.user_id \
        JOIN companies AS company ON company.id=principal.company_id \
        JOIN workflow_runs AS run ON run.company_id=principal.company_id AND run.id=$3 \
        LEFT JOIN channels AS channel ON channel.company_id=run.company_id AND channel.id=run.channel_id \
        WHERE principal.company_id=$1 AND principal.user_id=$2 AND principal.kind='person' \
          AND (company.user_id=principal.user_id \
            OR (run.channel_id IS NULL AND member.role IN ('owner','admin')) \
            OR (run.channel_id IS NOT NULL AND (channel.access_mode IN ('team','public') \
              OR EXISTS (SELECT 1 FROM channel_principal_grants AS access_grant WHERE access_grant.company_id=$1 \
                AND access_grant.channel_id=run.channel_id AND access_grant.principal_id=principal.id \
                AND access_grant.capability='view')))) \
          AND (run.thread_id IS NULL OR EXISTS (SELECT 1 FROM threads AS thread WHERE thread.company_id=$1 \
            AND thread.channel_id=run.channel_id AND thread.id=run.thread_id)) \
        ORDER BY principal.id LIMIT 1 FOR SHARE OF member, principal")
        .bind(scope.company.as_uuid()).bind(user).bind(scope.run.as_uuid()).fetch_optional(db).await?)
}

#[derive(sqlx::FromRow)]
struct DecisionHead {
    run_id: Uuid,
    execution_id: Uuid,
}
#[derive(sqlx::FromRow)]
struct DecisionRow {
    subject_kind: String,
    choice: Option<String>,
    revision: i64,
    proposal_digest: String,
    submitted_by: Option<Uuid>,
    command_key: Option<String>,
    command_fingerprint: Option<String>,
}
#[derive(sqlx::FromRow)]
struct RunAssociation {
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
}

/// Locks, in the documented order, everything a decision command reads, and rechecks current
/// authority. Missing, foreign or inaccessible records are a non-disclosing `NotFound`.
struct Locked {
    scope: ActivationRequest,
    run: waits::WaitRun,
    reviewer: authority::ReviewerAuthority,
}
async fn lock_decision(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    decision: DecisionId,
) -> AppResult<Locked> {
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)").execute(&mut **tx).await?;
    let head: DecisionHead = sqlx::query_as(
        "SELECT run_id,execution_id FROM workflow_decisions WHERE company_id=$1 AND id=$2",
    )
    .bind(company.as_uuid())
    .bind(decision.0)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    let reviewer = authority::authorize_reviewer_on(tx, company, actor).await?;
    let job: Uuid = sqlx::query_scalar("SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2 AND queue_kind='workflow'")
        .bind(company.as_uuid()).bind(head.execution_id).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    let scope = ActivationRequest {
        company,
        run: RunId::new(head.run_id),
        execution: ExecutionId::new(head.execution_id),
        job: WorkflowJobId(job),
    };
    // Run → execution → job, as `waits::lock`, but whatever the job's state: a settled
    // decision still answers reads and replays.
    let run: waits::WaitRun = sqlx::query_as("SELECT binding_id,state,deadline,max_steps FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(company.as_uuid()).bind(head.run_id).fetch_one(&mut **tx).await?;
    let association: RunAssociation = sqlx::query_as(
        "SELECT channel_id,thread_id FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(company.as_uuid())
    .bind(head.run_id)
    .fetch_one(&mut **tx)
    .await?;
    let association = match (association.channel_id, association.thread_id) {
        (None, _) => {
            if !reviewer.privileged {
                return Err(missing());
            }
            RelatedAssociation::Company
        }
        (Some(channel), None) => RelatedAssociation::Channel(RelatedChannelId::new(channel)),
        (Some(channel), Some(thread)) => RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(channel),
            thread_id: RelatedThreadId::new(thread),
        },
    };
    association::authorize_association(tx, company, actor, association).await?;
    sqlx::query(
        "SELECT id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(company.as_uuid())
    .bind(head.run_id)
    .bind(head.execution_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query("SELECT id FROM background_tasks WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(company.as_uuid())
        .bind(job)
        .execute(&mut **tx)
        .await?;
    Ok(Locked {
        scope,
        run,
        reviewer,
    })
}

async fn submit_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &SubmitDecision,
) -> AppResult<DecisionSubmission> {
    let Locked {
        scope,
        run,
        reviewer,
    } = lock_decision(tx, command.company, command.actor, command.decision).await?;
    let wait = decision_wait(tx, scope, command.decision).await?;
    let row: DecisionRow = sqlx::query_as("SELECT subject_kind,choice,revision,proposal_digest,submitted_by,command_key,command_fingerprint FROM workflow_decisions WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(command.decision.0).fetch_one(&mut **tx).await?;
    if matches!(
        row.subject_kind.as_str(),
        "action_approval" | "token_allowance"
    ) && !reviewer.privileged
    {
        return Err(missing());
    }
    let fingerprint = command.fingerprint()?;
    if row.submitted_by.is_some() {
        return if row.submitted_by == Some(reviewer.principal)
            && row.command_key.as_deref() == Some(command.command_key.as_str())
            && row.command_fingerprint.as_deref() == Some(fingerprint.as_str())
        {
            Ok(DecisionSubmission {
                disposition: DecisionDisposition::Replayed,
                revision: row.revision,
                successor: if row.subject_kind == "token_allowance"
                    || (row.subject_kind == "action_approval"
                        && row.choice.as_deref() == Some("approve"))
                {
                    None
                } else {
                    successor_of(tx, scope).await?
                },
            })
        } else {
            Err(closed())
        };
    }
    let now = waits::now(tx).await?;
    if wait.reason != "decision"
        || wait.state != "waiting"
        || run.state != "waiting"
        || wait.deadline <= now
        || run.deadline <= now
    {
        return Err(closed());
    }
    let listed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_decision_reviewers WHERE company_id=$1 AND decision_id=$2 AND principal_id=$3)")
        .bind(scope.company.as_uuid()).bind(command.decision.0).bind(reviewer.principal).fetch_one(&mut **tx).await?;
    if !listed {
        return Err(missing());
    }
    if command.expected_revision != row.revision || command.expected_artifact != row.proposal_digest
    {
        return Err(stale());
    }
    if row.subject_kind == "token_allowance" {
        return tokens::submit(tx, scope, reviewer.principal, command, &fingerprint).await;
    }
    if row.subject_kind == "action_approval" {
        return approvals::submit(tx, scope, reviewer.principal, command, &fingerprint).await;
    }
    // Box the business completion phase; approvals never activate or complete this step.
    Box::pin(submit_business_on(
        tx,
        scope,
        &run,
        reviewer.principal,
        command,
        &fingerprint,
    ))
    .await
}

async fn submit_business_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
    run: &waits::WaitRun,
    principal: Uuid,
    command: &SubmitDecision,
    fingerprint: &str,
) -> AppResult<DecisionSubmission> {
    let binding = admission_binding::read_saved_binding(
        tx,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let activation = activation::activate_on(tx, scope)
        .await?
        .ok_or_else(invalid)?;
    // Invalid choice, data or feedback: `BadRequest`, the transaction rolls back and the
    // decision stays open (R5).
    let completion = prepare_submission(binding.bundle(), &activation, command)?;
    sqlx::query("UPDATE workflow_decisions SET submitted_at=clock_timestamp(),submitted_by=$3,command_key=$4,command_fingerprint=$5,choice=$6,feedback=$7,data=$8,revision=revision+1 WHERE company_id=$1 AND id=$2 AND submitted_at IS NULL")
        .bind(scope.company.as_uuid()).bind(command.decision.0).bind(principal).bind(command.command_key.as_str()).bind(fingerprint).bind(&command.choice).bind(&command.feedback).bind(&command.data).execute(&mut **tx).await?;
    let revision = command.expected_revision + 1;
    waits::audit(tx, scope, "decision_submitted").await?;
    let max_steps = u64::try_from(run.max_steps).map_err(|_| invalid())?;
    if activation.ordinal > max_steps
        || (activation.ordinal >= max_steps
            && matches!(completion.target, TransitionTarget::Step(_)))
    {
        wait_commit::fail(tx, scope, WaitFailureReason::ActivationLimit).await?;
        return Ok(DecisionSubmission {
            disposition: DecisionDisposition::Applied,
            revision,
            successor: None,
        });
    }
    sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).execute(&mut **tx).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut **tx).await?;
    let successor = Box::pin(batch_commit::complete(
        tx,
        scope,
        &activation,
        &completion,
        run.max_steps,
        completion_job::CompletionOwner::Parked,
    ))
    .await?;
    Ok(DecisionSubmission {
        disposition: DecisionDisposition::Applied,
        revision,
        successor,
    })
}

async fn successor_of(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Option<ActivationRequest>> {
    let next: Option<Option<Uuid>> = sqlx::query_scalar("SELECT successor_execution_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(&mut *db).await?;
    let Some(Some(next)) = next else {
        return Ok(None);
    };
    let job: Uuid = sqlx::query_scalar("SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2 AND queue_kind='workflow'")
        .bind(scope.company.as_uuid()).bind(next).fetch_one(db).await?;
    Ok(Some(ActivationRequest {
        execution: ExecutionId::new(next),
        job: WorkflowJobId(job),
        ..scope
    }))
}

async fn comment_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &AddDecisionComment,
) -> AppResult<DecisionComment> {
    let Locked {
        scope, reviewer, ..
    } = lock_decision(tx, command.company, command.actor, command.decision).await?;
    let saved: Option<(i64, String)> = sqlx::query_as("SELECT sequence,body FROM workflow_decision_comments WHERE company_id=$1 AND decision_id=$2 AND author_principal_id=$3 AND command_key=$4")
        .bind(scope.company.as_uuid()).bind(command.decision.0).bind(reviewer.principal).bind(command.command_key.as_str()).fetch_optional(&mut **tx).await?;
    if let Some((sequence, body)) = saved {
        return if body == command.body {
            Ok(DecisionComment {
                disposition: DecisionDisposition::Replayed,
                sequence,
            })
        } else {
            Err(AppError::Conflict(
                "Workflow decision comment key was reused".into(),
            ))
        };
    }
    let wait = decision_wait(tx, scope, command.decision).await?;
    if wait.state != "waiting" {
        return Err(closed());
    }
    let last: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sequence),0) FROM workflow_decision_comments WHERE company_id=$1 AND decision_id=$2")
        .bind(scope.company.as_uuid()).bind(command.decision.0).fetch_one(&mut **tx).await?;
    if last >= MAX_DECISION_COMMENTS {
        return Err(AppError::Conflict(
            "Workflow decision comment limit reached".into(),
        ));
    }
    sqlx::query("INSERT INTO workflow_decision_comments(company_id,decision_id,sequence,author_principal_id,command_key,body) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(scope.company.as_uuid()).bind(command.decision.0).bind(last + 1).bind(reviewer.principal).bind(command.command_key.as_str()).bind(&command.body).execute(&mut **tx).await?;
    Ok(DecisionComment {
        disposition: DecisionDisposition::Applied,
        sequence: last + 1,
    })
}

#[derive(sqlx::FromRow)]
struct ViewRow {
    subject_kind: String,
    step_id: String,
    revision: i64,
    proposal_digest: String,
    proposal: Value,
    deadline: DateTime<Utc>,
    state: String,
    submitted_by: Option<Uuid>,
    submitted_at: Option<DateTime<Utc>>,
    choice: Option<String>,
    feedback: Option<String>,
    data: Option<Value>,
}

async fn read_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    decision: DecisionId,
) -> AppResult<DecisionView> {
    let Locked { scope, .. } = lock_decision(tx, company, actor, decision).await?;
    let row: ViewRow = sqlx::query_as("SELECT decision.subject_kind,execution.step_id,decision.revision,decision.proposal_digest,COALESCE(execution.frozen_inputs->'proposal','null'::jsonb) AS proposal,wait.deadline,wait.state,decision.submitted_by,decision.submitted_at,decision.choice,decision.feedback,decision.data FROM workflow_decisions AS decision JOIN workflow_executions AS execution ON execution.company_id=decision.company_id AND execution.id=decision.execution_id JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id WHERE decision.company_id=$1 AND decision.id=$2")
        .bind(company.as_uuid()).bind(decision.0).fetch_one(&mut **tx).await?;
    let reviewers: Vec<Uuid> = sqlx::query_scalar("SELECT reviewer.user_id FROM workflow_decision_reviewers AS reviewer WHERE reviewer.company_id=$1 AND reviewer.decision_id=$2 ORDER BY reviewer.ordinal")
        .bind(company.as_uuid()).bind(decision.0).fetch_all(&mut **tx).await?;
    let comments: Vec<(i64, Uuid, String, DateTime<Utc>)> = sqlx::query_as("SELECT sequence,author_principal_id,body,created_at FROM workflow_decision_comments WHERE company_id=$1 AND decision_id=$2 ORDER BY sequence")
        .bind(company.as_uuid()).bind(decision.0).fetch_all(&mut **tx).await?;
    let submission = match (
        row.submitted_by,
        row.submitted_at,
        row.choice,
        row.feedback,
        row.data,
    ) {
        (Some(submitted_by), Some(submitted_at), Some(choice), Some(feedback), Some(data)) => {
            Some(DecisionSubmissionView {
                submitted_by,
                submitted_at,
                choice,
                feedback,
                data,
            })
        }
        (None, None, None, None, None) => None,
        _ => return Err(invalid()),
    };
    Ok(DecisionView {
        decision,
        run: scope.run,
        execution: scope.execution,
        step: StepId::parse(row.step_id).map_err(|_| invalid())?,
        revision: row.revision,
        artifact_digest: row.proposal_digest,
        proposal: if row.subject_kind == "action_approval" {
            approvals::view_proposal(tx, company, decision).await?
        } else if row.subject_kind == "token_allowance" {
            sqlx::query_scalar(
                "SELECT token_proposal FROM workflow_decisions WHERE company_id=$1 AND id=$2",
            )
            .bind(company.as_uuid())
            .bind(decision.0)
            .fetch_one(&mut **tx)
            .await?
        } else {
            row.proposal
        },
        deadline: row.deadline,
        state: row.state,
        reviewers,
        submission,
        comments: comments
            .into_iter()
            .map(
                |(sequence, author_principal, body, created_at)| DecisionCommentView {
                    sequence,
                    author_principal,
                    body,
                    created_at,
                },
            )
            .collect(),
    })
}

#[path = "decision_approvals.rs"]
pub(super) mod approvals;

async fn decision_wait(
    db: &mut PgConnection,
    scope: ActivationRequest,
    decision: DecisionId,
) -> AppResult<waits::WaitRow> {
    sqlx::query_as("SELECT wait.id,wait.deadline,wait.reason,wait.state,wait.event_name,wait.correlation,wait.failure_code FROM workflow_decisions AS decision JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id WHERE decision.company_id=$1 AND decision.run_id=$2 AND decision.execution_id=$3 AND decision.id=$4 FOR UPDATE OF wait")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(decision.0).fetch_optional(db).await?.ok_or_else(missing)
}

#[path = "decision_tokens.rs"]
pub(super) mod tokens;
