//! Channel finals reuse protected-action decisions and private notice provenance.
use super::*;
use crate::application::workflow::{
    actions::FrozenAction, activation::*, channel_reply::ReplyApprovalPolicy, decisions::*,
};
use chrono::{DateTime, Utc};
use serde_json::json;

pub(super) struct PreparedApproval {
    pub id: Uuid,
    pub deadline: DateTime<Utc>,
    wait: Uuid,
    reviewers: Vec<decisions::Reviewer>,
    notices: Vec<Option<crate::transport::NewStandaloneDelivery>>,
}

pub(super) async fn prepare_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    root: &channel_reply_completion::RootReply,
    policy: Option<&ReplyApprovalPolicy>,
    notices: Option<&DecisionNotices>,
) -> AppResult<Option<PreparedApproval>> {
    let Some(policy) = policy else {
        return Ok(None);
    };
    let notices = notices.ok_or_else(|| {
        AppError::BadRequest("Final response approval notices are not configured".into())
    })?;
    let materialized = policy.materialize(waits::now(db).await?, root.deadline)?;
    let (_, deadline) = reviewer_deadline(&materialized)?;
    let reviewers = decisions::approvals::approvers(db, scope, policy.reviewers())
        .await?
        .ok_or_else(missing)?;
    for reviewer in policy.reviewers() {
        require_clearance_on(db, scope, *reviewer).await?;
    }
    let context: decisions::NoticeContext=sqlx::query_as("SELECT run.company_id,version.title,company.name AS company_label,run.correlation_id \
        FROM workflow_runs AS run JOIN workflow_versions AS version ON version.company_id=run.company_id AND version.id=run.version_id \
        JOIN companies AS company ON company.id=run.company_id WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut *db).await?;
    let step: String = sqlx::query_scalar(
        "SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let id = Uuid::new_v4();
    let notices = decisions::compose_notices(
        notices,
        &reviewers,
        &context,
        &StepId::parse(step).map_err(|_| invalid())?,
        id,
        deadline,
    )?;
    Ok(Some(PreparedApproval {
        id,
        deadline,
        wait: Uuid::new_v4(),
        reviewers,
        notices,
    }))
}

pub(super) async fn require_clearance_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    user: Uuid,
) -> AppResult<()> {
    let level = knowledge::actor_level_on(db, scope.company, Some(user)).await?;
    let content: i16 = sqlx::query_scalar(
        "SELECT content_level::smallint FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(db)
    .await?;
    if level.as_i16() < content {
        return Err(missing());
    }
    Ok(())
}

pub(super) async fn park_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    proposal: Uuid,
    invocation: Uuid,
    action: &FrozenAction,
    owner: completion_job::CompletionOwner,
    approval: PreparedApproval,
) -> AppResult<()> {
    let digest = crate::application::workflow::decisions::hex_digest(
        &decisions::approvals::proposal(invocation, action),
    )?;
    sqlx::query("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,notification_intent) VALUES($1,$2,$3,$4,'decision',$5,$6)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(approval.wait).bind(approval.deadline)
        .bind(json!({"version":1,"kind":"wait_registered","wait_id":approval.wait,"run_id":scope.run.as_uuid(),"execution_id":scope.execution.as_uuid()})).execute(&mut *db).await?;
    sqlx::query("INSERT INTO workflow_decisions(company_id,run_id,execution_id,id,wait_id,subject_kind,proposal_digest,action_invocation_id,argument_digest,operation_digest,job_id,reply_proposal_id) \
        VALUES($1,$2,$3,$4,$5,'action_approval',$6,$7,$8,$9,$10,$11)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(approval.id).bind(approval.wait).bind(digest)
        .bind(invocation).bind(action.argument_digest().as_str()).bind(action.operation_digest().as_str()).bind(scope.job.0).bind(proposal).execute(&mut *db).await?;
    decisions::insert_reviewers(
        db,
        scope.company,
        approval.id,
        &approval.reviewers,
        &approval.notices,
    )
    .await?;
    match owner {
        completion_job::CompletionOwner::Fenced(fence) => {
            decisions::approvals::close_attempt(db, fence).await?
        }
        completion_job::CompletionOwner::Pure | completion_job::CompletionOwner::Parked(_) => {
            completion_job::finish(db, scope, owner).await?;
            sqlx::query("UPDATE workflow_runs SET state='waiting',waiting_reason='decision' WHERE company_id=$1 AND id=$2")
                .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
        }
        _ => return Err(invalid()),
    }
    decisions::approvals::audit(db, scope, DecisionId(approval.id), "action_approval_opened")
        .await?;
    Ok(())
}
