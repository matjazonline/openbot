use super::*;
use crate::application::workflow::{actions::FrozenAction, activation::*, batch::PureCompletion};
use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(sqlx::FromRow)]
pub(super) struct ProposalRow {
    pub id: Uuid,
    #[sqlx(try_from = "String")]
    pub integrity: ReplyIntegrity,
    pub business_output: Value,
    pub completion_route: String,
    pub recipients: Value,
    pub operation: Value,
    pub approval_decision_id: Option<Uuid>,
    pub continuation_count: i32,
}
pub(super) async fn existing_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Option<ProposalRow>> {
    Ok(sqlx::query_as("SELECT proposal.id,proposal.integrity,proposal.business_output,proposal.completion_route,proposal.recipients, \
        action.operation,proposal.approval_decision_id,proposal.continuation_count FROM channel_reply_proposals AS proposal \
        JOIN workflow_action_intents AS action ON action.company_id=proposal.company_id AND action.id=proposal.invocation_id \
        WHERE proposal.company_id=$1 AND proposal.run_id=$2 AND proposal.execution_id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(db).await?)
}

pub(super) struct NewProposal<'a> {
    pub scope: ActivationRequest,
    pub root: &'a channel_reply_completion::RootReply,
    pub id: Uuid,
    pub integrity: ReplyIntegrity,
    pub document: &'a Value,
    pub completion: &'a PureCompletion,
    pub content: &'a Value,
    pub route_snapshot: &'a Value,
    pub recipients: &'a [knowledge_release::MessageRecipient],
    pub action: &'a FrozenAction,
    pub levels: knowledge_release::ReleaseLevels,
    pub owner: completion_job::CompletionOwner,
    pub approval: Option<&'a channel_reply_approvals::PreparedApproval>,
}

struct OwnerColumns {
    kind: &'static str,
    attempt: Option<i32>,
    worker: Option<Uuid>,
    generation: Option<Uuid>,
    expiry: Option<DateTime<Utc>>,
    wait: Option<Uuid>,
}
async fn owner_columns(
    db: &mut PgConnection,
    scope: ActivationRequest,
    owner: completion_job::CompletionOwner,
) -> AppResult<OwnerColumns> {
    let mut columns = OwnerColumns {
        kind: "pure",
        attempt: None,
        worker: None,
        generation: None,
        expiry: None,
        wait: None,
    };
    match owner {
        completion_job::CompletionOwner::Pure => {}
        completion_job::CompletionOwner::Parked(wait) => {
            columns.kind = "wait";
            columns.wait = Some(wait.as_uuid());
        }
        completion_job::CompletionOwner::Fenced(fence) => {
            columns.kind = "fenced";
            columns.attempt = Some(fence.attempt.0);
            columns.worker = Some(fence.worker.0);
            columns.generation = Some(fence.generation.0);
            columns.expiry = sqlx::query_scalar(
                "SELECT lock_expires_at FROM background_tasks WHERE company_id=$1 AND id=$2",
            )
            .bind(scope.company.as_uuid())
            .bind(scope.job.0)
            .fetch_one(db)
            .await?;
        }
        _ => return Err(invalid()),
    }
    Ok(columns)
}

pub(super) fn route(completion: &PureCompletion) -> AppResult<String> {
    match &completion.route {
        RouteSelection::Success => Ok("success".into()),
        RouteSelection::Choice(choice) => Ok(format!("choice:{}", choice.as_str())),
        RouteSelection::FinalError => Err(invalid()),
    }
}

pub(super) async fn insert_on(db: &mut PgConnection, new: NewProposal<'_>) -> AppResult<Uuid> {
    let scope = new.scope;
    let owner = owner_columns(db, scope, new.owner).await?;
    let count: i16 = sqlx::query_scalar(
        "SELECT continuation_count FROM background_tasks WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.job.0)
    .fetch_one(&mut *db)
    .await?;
    if new.approval.is_some() && count >= WORKFLOW_JOB_CONTINUATION_LIMIT {
        return Err(AppError::BadRequest(
            "Workflow continuation limit reached".into(),
        ));
    }
    let invocation = Uuid::new_v4();
    sqlx::query("INSERT INTO channel_reply_proposals(company_id,run_id,execution_id,id,intent_id,invocation_id,argument_digest,operation_digest,integrity,integrity_document, \
        business_output,completion_route,content,recipients,approval_policy,approval_deadline,approval_decision_id,job_id,owner_kind,owner_attempt,owner_worker,owner_generation,owner_expiry,source_wait_id,continuation_count,route_snapshot) \
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(new.id).bind(new.root.intent_id)
        .bind(invocation).bind(new.action.argument_digest().as_str()).bind(new.action.operation_digest().as_str()).bind(new.integrity.as_hex()).bind(new.document)
        .bind(&new.completion.output).bind(route(new.completion)?).bind(new.content).bind(serde_json::to_value(new.recipients).map_err(|_|invalid())?)
        .bind(&new.root.approval_policy).bind(new.approval.map(|a|a.deadline)).bind(new.approval.map(|a|a.id)).bind(scope.job.0)
        .bind(owner.kind).bind(owner.attempt).bind(owner.worker).bind(owner.generation).bind(owner.expiry).bind(owner.wait).bind(i32::from(count)).bind(new.route_snapshot)
        .execute(&mut *db).await?;
    sqlx::query("INSERT INTO workflow_action_intents(company_id,run_id,execution_id,id,operation_key,operation_digest,argument_digest,idempotency_key,operation,policy_decision, \
        content_level_at_freeze,destination_level_at_freeze,origin_kind,reply_proposal_id,reply_integrity) \
        VALUES($1,$2,$3,$4,'message.reply',$5,$6,$7,$8,$9,$10::smallint,$11::smallint,'channel_final',$12,$13)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(invocation)
        .bind(new.action.operation_digest().as_str()).bind(new.action.argument_digest().as_str()).bind(new.action.idempotency_key().as_str()).bind(new.action.saved_operation())
        .bind(if new.approval.is_some() {"approval_required"} else {"unevaluated"})
        .bind(new.levels.content.as_i16()).bind(new.levels.destination.as_i16()).bind(new.id).bind(new.integrity.as_hex()).execute(db).await?;
    Ok(invocation)
}

pub(super) async fn resume_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    root: &channel_reply_completion::RootReply,
    completion: &PureCompletion,
    owner: completion_job::CompletionOwner,
    saved: ProposalRow,
) -> AppResult<channel_reply_completion::PreparedFinal> {
    use channel_reply_completion::PreparedFinal;
    let completion_job::CompletionOwner::Approved {
        proposal,
        decision,
        continuation,
    } = owner
    else {
        return Ok(PreparedFinal::Parked);
    };
    if proposal.as_uuid() != saved.id
        || saved.approval_decision_id != Some(decision.0)
        || saved.business_output != completion.output
        || saved.completion_route != route(completion)?
        || saved.continuation_count + 1 != i32::from(continuation)
    {
        return Err(invalid());
    }
    let accepted: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_decisions AS decision \
        JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id \
        JOIN background_tasks AS job ON job.company_id=decision.company_id AND job.id=decision.job_id \
        WHERE decision.company_id=$1 AND decision.run_id=$2 AND decision.execution_id=$3 AND decision.id=$4 \
        AND decision.reply_proposal_id=$5 AND decision.choice='approve' AND wait.state='completed' \
        AND job.id=$6 AND job.status='pending' AND job.worker_id IS NULL AND job.run_at<=clock_timestamp() \
        AND job.retry_count<job.max_retries AND job.continuation_count=$7)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(decision.0).bind(saved.id)
        .bind(scope.job.0).bind(continuation).fetch_one(&mut *db).await?;
    if !accepted {
        return Err(invalid());
    }
    let action = FrozenAction::restore(saved.operation)?;
    let recipients: Vec<knowledge_release::MessageRecipient> =
        serde_json::from_value(saved.recipients).map_err(|_| invalid())?;
    knowledge_release::evaluate_on(
        db,
        &action,
        knowledge_release::MessageRecipients::Frozen(&recipients),
    )
    .await?;
    Ok(PreparedFinal::Automatic {
        intent: root.intent_id,
        proposal: saved.id,
        integrity: saved.integrity,
    })
}
