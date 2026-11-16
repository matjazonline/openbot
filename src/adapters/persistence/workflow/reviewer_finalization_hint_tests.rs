//! Pending/refused saved-result observation does not consume final-response authority.
use super::*;
use crate::app_error::AppResult;
use crate::application::workflow::{
    CancelCommand, CancelResult, IdempotencyKey, WorkflowInspection, WorkflowRunTransitions,
};
use std::{future::Future, pin::Pin};

async fn pending_finalization() -> ConfiguredFinal {
    let s = Box::pin(Intents::new()).await;
    let reviewer = member(&s.fixture, "admin").await;
    let principal = s.fixture.principal(reviewer).await;
    // Keep a live reviewer as well as the one whose account will retire. This is
    // still a real open approval, not an empty-role or terminal-decision fixture.
    let policy = ReplyApprovalPolicy::new(vec![principal, s.owner_principal()], 60).unwrap();
    let fixture = Box::pin(FinalFixture::from_case(
        s,
        FinalCase {
            source: completion_source(CompletionFamily::Pure),
            contract: literal_contract("Complete"),
            policy: Some(policy),
            run_as: None,
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    ))
    .await;
    Box::pin(stage(&fixture, CompletionFamily::Pure)).await;
    assert_eq!(fixture.state().await["run"], "waiting");
    ConfiguredFinal { fixture, reviewer }
}

#[tokio::test]
async fn workflow_reviewer_follow_pending_finalization_ignores_unused_retired_final_reviewers() {
    let configured = Box::pin(pending_finalization()).await;
    let f = &configured.fixture;
    f.p.delete_user_account(configured.reviewer).await.unwrap();
    let before = state(f).await;
    assert_eq!(before["run"]["state"], "waiting");
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Pending
    ));
    assert_eq!(state(f).await, before);
    let batch = f.advance().await;
    assert_eq!(batch.disposition, BatchDisposition::Deferred);
    assert_eq!(batch.completed, 0);
    assert_eq!(state(f).await, before);
}

#[tokio::test]
async fn workflow_reviewer_follow_cancelled_finalization_ignores_unused_retired_final_reviewers() {
    let configured = Box::pin(pending_finalization()).await;
    let f = &configured.fixture;
    f.p.delete_user_account(configured.reviewer).await.unwrap();
    let command = CancelCommand {
        company_id: f.scope.company,
        run_id: f.scope.run,
        actor: f.s.owner(),
        command_key: IdempotencyKey::parse("cancel-pending-finalization").unwrap(),
        expected_revision: f
            .p
            .head(f.scope.company, f.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    assert!(matches!(
        f.p.cancel(command).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    let before = state(f).await;
    assert_eq!(before["run"]["state"], "cancelled");
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Refused
    ));
    assert_eq!(state(f).await, before);
    let batch = f.advance().await;
    assert_eq!(batch.disposition, BatchDisposition::Deferred);
    assert_eq!(batch.completed, 0);
    assert_eq!(state(f).await, before);
}

struct UpgradeOwners {
    submit: i32,
    resume: i32,
}

async fn queued_upgrade<Submission, Resume>(
    pool: &sqlx::PgPool,
    blocker: i32,
    mut submit: Pin<&mut Submission>,
    mut resume: Pin<&mut Resume>,
) -> UpgradeOwners
where
    Submission: Future<Output = AppResult<DecisionSubmission>>,
    Resume: Future<Output = AppResult<FinalizationProgress>>,
{
    let submit_pid = tokio::select! {
        _ = submit.as_mut() => panic!("actual approval escaped its execution gate"),
        pid = blocked_by(pool, blocker, "%FROM workflow_executions%FOR UPDATE%") => pid,
    };
    let resume_pid = tokio::select! {
        _ = submit.as_mut() => panic!("approval must still hold company before its execution gate"),
        _ = resume.as_mut() => panic!("pending observation bypassed approval's company ownership"),
        pid = blocked_by(pool, submit_pid, "%companies%FOR NO KEY UPDATE%") => pid,
    };
    UpgradeOwners {
        submit: submit_pid,
        resume: resume_pid,
    }
}

struct UpgradeResult {
    submitted: DecisionSubmission,
    resumed: AppResult<FinalizationProgress>,
}

async fn finish_upgrade<Submission, Resume>(
    pool: &sqlx::PgPool,
    blocker: i32,
    mut submit: Pin<&mut Submission>,
    mut resume: Pin<&mut Resume>,
) -> UpgradeResult
where
    Submission: Future<Output = AppResult<DecisionSubmission>>,
    Resume: Future<Output = AppResult<FinalizationProgress>>,
{
    tokio::select! {
        (submitted, resumed) = async { tokio::join!(submit.as_mut(), resume.as_mut()) } => {
            UpgradeResult { submitted: submitted.unwrap(), resumed }
        }
        _ = blocked_by(pool, blocker, "%FROM users%FOR SHARE%") => {
            panic!("a pending observer may not acquire newly required actor accounts after company")
        }
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_pending_finalization_replans_before_new_actor_accounts() {
    let configured = Box::pin(pending_finalization()).await;
    let f = &configured.fixture;
    let mut command = f.command("approve").await;
    command.actor = WorkflowActor::authenticated(configured.reviewer).unwrap();
    let mut blocker = f.p.pool().begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    // The actual submitter is distinct from the run actor. Keep this actor's
    // retirement-style account lock while releasing only the execution gate.
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(f.s.owner().user_id())
        .execute(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SAVEPOINT finalization_execution_gate")
        .execute(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.execution.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let submit = f.p.submit_decision(command);
    let resume = f.p.resume_finalization(f.scope);
    tokio::pin!(submit, resume);
    // Box the concurrent observation phase rather than growing the test parent frame.
    let owners = Box::pin(queued_upgrade(
        f.p.pool(),
        blocker_pid,
        submit.as_mut(),
        resume.as_mut(),
    ))
    .await;
    assert_ne!(owners.submit, owners.resume);
    sqlx::query("ROLLBACK TO SAVEPOINT finalization_execution_gate")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let result = Box::pin(finish_upgrade(
        f.p.pool(),
        blocker_pid,
        submit.as_mut(),
        resume.as_mut(),
    ))
    .await;
    assert_eq!(result.submitted.disposition, DecisionDisposition::Applied);
    assert!(matches!(result.resumed, Err(AppError::Conflict(_))));
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
        .bind(f.scope.company.as_uuid())
        .execute(&mut *blocker)
        .await
        .expect("the stale observer must release company before returning its retry result");
    let before_retry = state(f).await;
    assert_eq!(before_retry["run"]["state"], "running");
    assert_eq!(before_retry["executions"][0]["completed_at"], Value::Null);
    assert_eq!(before_retry["reply"], Value::Null);
    assert_eq!(before_retry["proposals"].as_array().unwrap().len(), 1);
    blocker.rollback().await.unwrap();
    let FinalizationProgress::Completed(done) = f.p.resume_finalization(f.scope).await.unwrap()
    else {
        panic!("a fresh transaction must acquire the real actor accounts and complete once");
    };
    assert_eq!(done.disposition, CommitDisposition::Committed);
    let stored = state(f).await;
    assert_eq!(stored["run"]["state"], "succeeded");
    assert_eq!(stored["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(stored["reply"]["state"], "pending");
}
