use super::*;
use crate::application::workflow::{
    activation::*, batch::PureCompletion, channel_reply::*, completion::*, decisions::*,
};
use serde_json::Value;

#[async_trait]
impl WorkflowFinalizations for PostgresPersistence {
    async fn resume_finalization(
        &self,
        scope: ActivationRequest,
    ) -> AppResult<FinalizationProgress> {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut tx = self.pool().begin().await?;
            channel_reply_publication::bounds(&mut tx).await?;
            let result = Box::pin(resume_on(&mut tx, scope, self.channel_reply_notices())).await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
}

#[derive(sqlx::FromRow)]
struct ResumeRun {
    state: String,
    max_steps: i32,
    continuation_count: i16,
    status: String,
    completed: bool,
}

pub(super) async fn resume_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    notices: Option<&DecisionNotices>,
) -> AppResult<FinalizationProgress> {
    let Some(saved) = channel_reply_proposals::existing_on(db, scope).await? else {
        return Ok(FinalizationProgress::Unstaged);
    };
    channel_reply_completion::prelock_on(db, scope).await?;
    let run: Option<ResumeRun>=sqlx::query_as("SELECT run.state,run.max_steps,job.continuation_count,job.status,(execution.completed_at IS NOT NULL) AS completed \
        FROM workflow_runs AS run JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id \
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
        WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3 AND job.id=$4 FOR UPDATE OF run,execution,job")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0).fetch_optional(&mut *db).await?;
    let Some(run) = run else {
        return Ok(FinalizationProgress::Refused);
    };
    if run.completed {
        return if run.state == "succeeded" {
            replay_on(db, scope).await
        } else {
            Ok(FinalizationProgress::Refused)
        };
    }
    if run.state == "waiting" {
        return Ok(FinalizationProgress::Pending);
    }
    if run.state != "running" || run.status != "pending" {
        return Ok(FinalizationProgress::Refused);
    }
    // The saved result, rather than a handler future, owns this continuation.
    Box::pin(complete_saved_on(db, scope, run, saved, notices)).await
}

async fn complete_saved_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: ResumeRun,
    saved: channel_reply_proposals::ProposalRow,
    notices: Option<&DecisionNotices>,
) -> AppResult<FinalizationProgress> {
    let Some(decision) = saved.approval_decision_id else {
        return Err(invalid());
    };
    let activation = activation::activate_on(db, scope)
        .await?
        .ok_or_else(invalid)?;
    let completion = PureCompletion {
        output: saved.business_output,
        route: wait_commit::saved_route(&saved.completion_route)?,
        target: TransitionTarget::End,
        state: RunState::Succeeded,
        response_disposition: ResponseDisposition::Automatic,
    };
    let owner = completion_job::CompletionOwner::Approved {
        proposal: ReplyProposalId::new(saved.id),
        decision: DecisionId(decision),
        continuation: run.continuation_count,
    };
    sqlx::query("SAVEPOINT channel_saved_completion")
        .execute(&mut *db)
        .await?;
    let result = Box::pin(batch_commit::complete(
        db,
        scope,
        &activation,
        &completion,
        run.max_steps,
        owner,
        notices,
    ))
    .await;
    let progress = match result {
        Ok(progress) => progress,
        Err(
            error @ (AppError::NotFound(_)
            | AppError::Conflict(_)
            | AppError::KnowledgeRelease { .. }),
        ) => {
            sqlx::query("ROLLBACK TO SAVEPOINT channel_saved_completion")
                .execute(&mut *db)
                .await?;
            Box::pin(channel_reply_refusal::settle_on(
                db,
                scope,
                owner,
                channel_reply_refusal::knowledge_refusal(&error),
            ))
            .await?;
            sqlx::query("RELEASE SAVEPOINT channel_saved_completion")
                .execute(&mut *db)
                .await?;
            return Ok(FinalizationProgress::Refused);
        }
        Err(error) => return Err(error),
    };
    sqlx::query("RELEASE SAVEPOINT channel_saved_completion")
        .execute(&mut *db)
        .await?;
    saved_result(progress, completion)
}

fn saved_result(
    progress: batch_commit::CompletionProgress,
    completion: PureCompletion,
) -> AppResult<FinalizationProgress> {
    if matches!(progress, batch_commit::CompletionProgress::Refused) {
        return Ok(FinalizationProgress::Refused);
    }
    let batch_commit::CompletionProgress::Advanced(successor) = progress else {
        return Err(invalid());
    };
    Ok(FinalizationProgress::Completed(Box::new(
        CommittedWorkflowStep {
            disposition: CommitDisposition::Committed,
            response_disposition: completion.response_disposition,
            output: completion.output,
            route: completion.route,
            target: completion.target,
            successor,
        },
    )))
}

#[derive(sqlx::FromRow)]
struct ReplayRow {
    committed_output: Value,
    committed_route: String,
    response_disposition: String,
}
async fn replay_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<FinalizationProgress> {
    let row: ReplayRow=sqlx::query_as("SELECT committed_output,committed_route,response_disposition FROM workflow_executions \
        WHERE company_id=$1 AND run_id=$2 AND id=$3 AND completed_at IS NOT NULL AND route_target='$end'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(db).await?;
    Ok(FinalizationProgress::Completed(Box::new(
        CommittedWorkflowStep {
            disposition: CommitDisposition::Replayed,
            response_disposition: row.response_disposition.parse().map_err(|_| invalid())?,
            output: row.committed_output,
            route: wait_commit::saved_route(&row.committed_route)?,
            target: TransitionTarget::End,
            successor: None,
        },
    )))
}
