//! Final publication and provider entry share the original grant and current release checks.
use super::channel_reply_publish_read::{FinalIdentity, PublicationArtifact, PublicationRow};
use super::*;
use crate::application::workflow::channel_reply::{ReplyApprovalPolicy, ReplyPublicationFailure};
use sqlx::PgConnection;

pub(super) enum RunAuthority {
    Available,
    Revoked,
    Unavailable,
}

pub(super) enum FinalAuthority {
    Allowed(Box<PublicationArtifact>),
    Refused(ReplyPublicationFailure),
}

pub(super) async fn lock_run_on(
    db: &mut PgConnection,
    row: &PublicationRow,
) -> AppResult<RunAuthority> {
    let actor = action_authority::prelock_reply_on(db, row.scope(), None).await?;
    let succeeded: Option<bool> = sqlx::query_scalar(
        "SELECT state='succeeded' AND terminal_execution_id=$3 AND parent_run_id IS NULL \
         FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR SHARE",
    )
    .bind(row.company_id)
    .bind(row.run_id)
    .bind(row.execution_id)
    .fetch_optional(db)
    .await?;
    Ok(match (succeeded, actor) {
        (Some(true), Some(_)) => RunAuthority::Available,
        (Some(true), None) => RunAuthority::Revoked,
        _ => RunAuthority::Unavailable,
    })
}

pub(super) async fn validate_on(
    db: &mut PgConnection,
    row: &PublicationRow,
    identity: FinalIdentity,
) -> AppResult<FinalAuthority> {
    use ReplyPublicationFailure as Failure;
    let Some(root) = channel_reply_completion::root_on(db, row.activation()).await? else {
        return Ok(FinalAuthority::Refused(Failure::SourceUnavailable));
    };
    let artifact = match row.artifact(identity) {
        Ok(artifact) => artifact,
        Err(_) => return Ok(FinalAuthority::Refused(Failure::InvalidProposal)),
    };
    if root.intent_id != row.intent_id
        || root.message_id != artifact.draft.source.as_uuid()
        || root.actor_id != artifact.action.request().context.actor
        || root.approval_policy != row.approval_policy
        || artifact.action.request().context.approval_required != row.approval_policy.is_some()
    {
        return Ok(FinalAuthority::Refused(Failure::InvalidProposal));
    }
    if let Err(error) =
        channel_reply_completion::authorize_source_on(db, row.activation(), &root).await
    {
        return Ok(FinalAuthority::Refused(classify_refusal(
            error,
            Failure::AuthorityRevoked,
        )?));
    }
    if !channel_reply_completion::policy_current_on(db, row.activation(), &root).await? {
        return Ok(FinalAuthority::Refused(Failure::PolicyChanged));
    }
    let current_route =
        match message::final_reply_route_on(db, row.scope(), artifact.draft.source).await {
            Ok(route) => route,
            Err(error) => {
                return Ok(FinalAuthority::Refused(classify_refusal(
                    error,
                    Failure::SourceUnavailable,
                )?));
            }
        };
    if serde_json::to_value(current_route).map_err(|_| invalid())? != row.route_snapshot {
        return Ok(FinalAuthority::Refused(Failure::SourceUnavailable));
    }
    if let Some(reason) = approval_on(db, row, identity, &artifact).await? {
        return Ok(FinalAuthority::Refused(reason));
    }
    for recipients in [
        knowledge_release::MessageRecipients::Current,
        knowledge_release::MessageRecipients::Frozen(&artifact.recipients),
    ] {
        if let Err(error) = knowledge_release::evaluate_on(db, &artifact.action, recipients).await {
            return Ok(FinalAuthority::Refused(classify_refusal(
                error,
                Failure::ReleaseDenied,
            )?));
        }
    }
    Ok(FinalAuthority::Allowed(Box::new(artifact)))
}

async fn approval_on(
    db: &mut PgConnection,
    row: &PublicationRow,
    identity: FinalIdentity,
    artifact: &PublicationArtifact,
) -> AppResult<Option<ReplyPublicationFailure>> {
    use ReplyPublicationFailure as Failure;
    let Some(policy) = &row.approval_policy else {
        return Ok(
            (row.approval_decision_id.is_some() || row.approval_deadline.is_some())
                .then_some(Failure::InvalidProposal),
        );
    };
    let policy: ReplyApprovalPolicy = match serde_json::from_value(policy.clone()) {
        Ok(policy) => policy,
        Err(_) => return Ok(Some(Failure::InvalidProposal)),
    };
    let (Some(decision), Some(deadline)) = (row.approval_decision_id, row.approval_deadline) else {
        return Ok(Some(Failure::InvalidProposal));
    };
    if decisions::approvals::approvers(db, row.activation(), policy.reviewers())
        .await?
        .is_none()
    {
        return Ok(Some(Failure::AuthorityRevoked));
    }
    for reviewer in policy.reviewers() {
        if let Err(error) =
            channel_reply_approvals::require_principal_clearance_on(db, row.activation(), *reviewer)
                .await
        {
            return Ok(Some(classify_refusal(error, Failure::AuthorityRevoked)?));
        }
    }
    let digest = crate::application::workflow::decisions::hex_digest(
        &decisions::approvals::proposal(row.invocation_id, &artifact.action),
    )?;
    // The deadline validates the historical submission, never a new post-success lease.
    let approved: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_decisions AS decision \
        JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id \
        WHERE decision.company_id=$1 AND decision.run_id=$2 AND decision.execution_id=$3 AND decision.id=$4 \
            AND decision.subject_kind='action_approval' AND decision.reply_proposal_id=$5 AND decision.action_invocation_id=$6 \
            AND decision.argument_digest=$7 AND decision.operation_digest=$8 AND decision.proposal_digest=$9 \
            AND decision.job_id=$10 AND decision.choice='approve' AND decision.submitted_at IS NOT NULL \
            AND decision.submitted_at<=$11 AND wait.state='completed' AND wait.execution_id=$3 AND wait.deadline=$11)",
    )
    .bind(row.company_id).bind(row.run_id).bind(row.execution_id).bind(decision)
    .bind(identity.proposal.as_uuid()).bind(row.invocation_id)
    .bind(artifact.action.argument_digest().as_str()).bind(artifact.action.operation_digest().as_str())
    .bind(digest).bind(row.job_id).bind(deadline).fetch_one(db).await?;
    Ok((!approved).then_some(Failure::InvalidProposal))
}

fn classify_refusal(
    error: AppError,
    reason: ReplyPublicationFailure,
) -> AppResult<ReplyPublicationFailure> {
    match error {
        AppError::KnowledgeRelease { .. } => Ok(ReplyPublicationFailure::ReleaseDenied),
        AppError::NotFound(_)
        | AppError::Forbidden(_)
        | AppError::InvalidCredentials
        | AppError::BadRequest(_) => Ok(reason),
        error => Err(error),
    }
}

fn invalid() -> AppError {
    AppError::BadRequest("Invalid final reply authority evidence".into())
}
