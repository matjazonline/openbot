//! Recorded completions use immutable execution truth; fresh finalization retains authority.
use super::*;
use crate::application::workflow::completion::{
    CommitDisposition, CommittedWorkflowStep, WorkflowCompletion,
};

#[path = "reviewer_fresh_completion_hint_tests.rs"]
mod fresh_hints;

#[path = "reviewer_finalization_hint_tests.rs"]
mod finalization_hints;

#[path = "reviewer_wait_resume_hint_tests.rs"]
mod wait_resume_hints;

#[path = "reviewer_batch_hint_tests.rs"]
mod batch_hints;

#[derive(Clone, Copy)]
enum CompletionFamily {
    Pure,
    Fenced,
}

enum CompletionEvidence {
    Pure,
    Fenced(FencedWorkflowResult),
}

struct ConfiguredFinal {
    fixture: FinalFixture,
    reviewer: Uuid,
}

struct ApprovedFinal {
    fixture: FinalFixture,
    reviewer: Uuid,
    evidence: CompletionEvidence,
}

struct RecordedFinal {
    approved: ApprovedFinal,
    completed: CommittedWorkflowStep,
}

fn completion_source(family: CompletionFamily) -> Value {
    match family {
        CompletionFamily::Pure => final_source("data.map"),
        CompletionFamily::Fenced => {
            let mut source: Value =
                serde_json::from_str(&registry::example("context.history").unwrap().source)
                    .unwrap();
            source["output_schema"] = json!(true);
            source
        }
    }
}

async fn stage(f: &FinalFixture, family: CompletionFamily) -> CompletionEvidence {
    match family {
        CompletionFamily::Pure => {
            f.advance().await;
            CompletionEvidence::Pure
        }
        CompletionFamily::Fenced => {
            let policy = LeasePolicy::new(std::time::Duration::from_secs(60)).unwrap();
            let claim =
                f.p.claim_io(f.scope, WorkflowWorkerId(Uuid::new_v4()), policy)
                    .await
                    .unwrap()
                    .unwrap();
            let result = FencedWorkflowResult {
                fence: claim.fence,
                output: json!({"items":[],"token_count":0,"omitted":0}),
                policy,
            };
            assert!(f.p.complete_io(result.clone()).await.unwrap().is_none());
            CompletionEvidence::Fenced(result)
        }
    }
}

async fn configured_final(source: Value) -> ConfiguredFinal {
    let s = Box::pin(Intents::new()).await;
    let reviewer = member(&s.fixture, "admin").await;
    let principal = s.fixture.principal(reviewer).await;
    let fixture = Box::pin(FinalFixture::from_case(
        s,
        FinalCase {
            source,
            contract: literal_contract("Complete"),
            policy: Some(ReplyApprovalPolicy::new(vec![principal], 60).unwrap()),
            run_as: None,
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    ))
    .await;
    ConfiguredFinal { fixture, reviewer }
}

async fn approved_final(family: CompletionFamily) -> ApprovedFinal {
    let ConfiguredFinal { fixture, reviewer } =
        Box::pin(configured_final(completion_source(family))).await;
    // Keep fixture setup behind boxed seams just like the actual completion owners.
    let evidence = Box::pin(stage(&fixture, family)).await;
    assert_eq!(fixture.state().await["run"], "waiting");
    let mut command = fixture.command("approve").await;
    command.actor = WorkflowActor::authenticated(reviewer).unwrap();
    fixture.p.submit_decision(command).await.unwrap();
    assert_eq!(fixture.state().await["run"], "running");
    ApprovedFinal {
        fixture,
        reviewer,
        evidence,
    }
}

async fn recorded_final(family: CompletionFamily) -> RecordedFinal {
    let approved = Box::pin(approved_final(family)).await;
    let f = &approved.fixture;
    let FinalizationProgress::Completed(completed) =
        f.p.resume_finalization(f.scope).await.unwrap()
    else {
        panic!("actual approved finalization must complete");
    };
    assert_eq!(completed.disposition, CommitDisposition::Committed);
    assert_eq!(f.state().await["run"], "succeeded");
    let immutable = sqlx::query("UPDATE workflow_executions SET completed_at=NULL WHERE company_id=$1 AND run_id=$2 AND id=$3")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(f.scope.execution.as_uuid())
        .execute(f.p.pool()).await.unwrap_err();
    let guard = immutable.as_database_error().unwrap();
    assert_eq!(guard.code().as_deref(), Some("23514"));
    assert_eq!(
        guard.constraint(),
        Some("workflow_guard:workflow_output_immutable")
    );
    f.p.delete_user_account(approved.reviewer).await.unwrap();
    RecordedFinal {
        approved,
        completed: *completed,
    }
}

async fn state(f: &FinalFixture) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object( \
         'run',(SELECT to_jsonb(run) FROM workflow_runs AS run WHERE company_id=$1 AND id=$2), \
         'executions',(SELECT jsonb_agg(to_jsonb(execution) ORDER BY activation,id) FROM workflow_executions AS execution WHERE company_id=$1 AND run_id=$2), \
         'jobs',(SELECT jsonb_agg(to_jsonb(job) ORDER BY job.id) FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id WHERE execution.company_id=$1 AND execution.run_id=$2), \
         'events',(SELECT jsonb_agg(to_jsonb(event) ORDER BY sequence) FROM workflow_run_events AS event WHERE company_id=$1 AND run_id=$2), \
         'proposals',(SELECT jsonb_agg(to_jsonb(proposal) ORDER BY id) FROM channel_reply_proposals AS proposal WHERE company_id=$1 AND run_id=$2), \
         'reply',(SELECT to_jsonb(decision) FROM channel_reply_decisions AS decision WHERE company_id=$1 AND run_id=$2), \
         'decisions',(SELECT jsonb_agg(to_jsonb(decision) ORDER BY id) FROM workflow_decisions AS decision WHERE company_id=$1 AND run_id=$2))",
    )
    .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid())
    .fetch_one(f.p.pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_reviewer_follow_completed_pure_replay_ignores_retired_final_reviewers() {
    let recorded = Box::pin(recorded_final(CompletionFamily::Pure)).await;
    let f = &recorded.approved.fixture;
    let before = state(f).await;
    let replay = f.advance().await;
    assert_eq!(replay.disposition, BatchDisposition::Replay);
    assert_eq!(
        replay.response_disposition,
        Some(recorded.completed.response_disposition)
    );
    assert_eq!(state(f).await, before);
}

#[tokio::test]
async fn workflow_reviewer_follow_completed_finalization_replay_ignores_retired_final_reviewers() {
    let recorded = Box::pin(recorded_final(CompletionFamily::Pure)).await;
    let f = &recorded.approved.fixture;
    let before = state(f).await;
    let FinalizationProgress::Completed(replay) = f.p.resume_finalization(f.scope).await.unwrap()
    else {
        panic!("recorded finalization must replay");
    };
    let mut expected = recorded.completed.clone();
    expected.disposition = CommitDisposition::Replayed;
    assert_eq!(*replay, expected);
    assert_eq!(state(f).await, before);
}

#[tokio::test]
async fn workflow_reviewer_follow_completed_fenced_replay_ignores_retired_final_reviewers() {
    let recorded = Box::pin(recorded_final(CompletionFamily::Fenced)).await;
    let f = &recorded.approved.fixture;
    let CompletionEvidence::Fenced(result) = &recorded.approved.evidence else {
        panic!("actual I/O evidence required");
    };
    let before = state(f).await;
    let replay = f.p.complete_io(result.clone()).await.unwrap().unwrap();
    let mut expected = recorded.completed.clone();
    expected.disposition = CommitDisposition::Replayed;
    assert_eq!(replay, expected);
    assert_eq!(state(f).await, before);
}

#[tokio::test]
async fn workflow_reviewer_follow_uncompleted_finalization_requires_current_final_reviewers() {
    let approved = Box::pin(approved_final(CompletionFamily::Pure)).await;
    let f = &approved.fixture;
    f.p.delete_user_account(approved.reviewer).await.unwrap();
    let before = state(f).await;
    assert!(matches!(
        f.p.resume_finalization(f.scope).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(state(f).await, before);
}

#[tokio::test]
async fn workflow_reviewer_follow_nonterminal_pure_replay_ignores_unused_retired_final_reviewers() {
    let pending = Box::pin(pending_reply(BusinessContinuation::NextStep)).await;
    let f = &pending.fixture;
    let submitted = f.p.submit_decision(pending.command.clone()).await.unwrap();
    assert!(submitted.successor.is_some());
    f.p.delete_user_account(pending.retired).await.unwrap();
    let before = state(f).await;
    let replay = f.advance().await;
    assert_eq!(replay.disposition, BatchDisposition::Replay);
    assert_eq!(replay.continuation, submitted.successor);
    assert_eq!(state(f).await, before);
}

#[tokio::test]
async fn workflow_reviewer_follow_completed_wait_replay_ignores_unused_retired_final_reviewers() {
    let pending = Box::pin(pending_reply(BusinessContinuation::NextIoStep)).await;
    let f = &pending.fixture;
    let submitted = f.p.submit_decision(pending.command.clone()).await.unwrap();
    assert!(submitted.successor.is_some());
    f.p.delete_user_account(pending.retired).await.unwrap();
    let before = state(f).await;
    let WaitProgress::Completed(replay) = f.p.resume_wait(f.scope).await.unwrap() else {
        panic!("completed wait must replay its actual saved successor");
    };
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.successor, submitted.successor);
    assert_eq!(replay.output, before["executions"][0]["committed_output"]);
    assert_eq!(state(f).await, before);
}

#[tokio::test]
async fn workflow_reviewer_follow_completion_replay_company_precedes_run_during_actual_retirement()
{
    use crate::adapters::persistence::user_lifecycle::{
        finish_account_deletion_on, lock_account_on,
    };
    let recorded = Box::pin(recorded_final(CompletionFamily::Pure)).await;
    let f = &recorded.approved.fixture;
    let before = state(f).await;
    let user = member(&f.s.fixture, "admin").await;
    let mut retirement = f.p.pool().begin().await.unwrap();
    let locks = lock_account_on(&mut retirement, user).await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *retirement)
        .await
        .unwrap();
    let replay = f.p.advance_pure(
        f.scope,
        BatchBudget::new(4, std::time::Duration::from_millis(100)).unwrap(),
    );
    tokio::pin!(replay);
    tokio::select! {
        _ = blocked_by(f.p.pool(), blocker, "%companies%FOR NO KEY UPDATE%") => {},
        result = replay.as_mut() => panic!("replay must first wait for retirement's company: {result:?}"),
    }
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE NOWAIT")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .execute(&mut *retirement)
        .await
        .expect("recorded completion company ownership must precede run ownership");
    finish_account_deletion_on(&mut retirement, user, locks)
        .await
        .unwrap();
    retirement.commit().await.unwrap();
    assert_eq!(replay.await.unwrap().disposition, BatchDisposition::Replay);
    let present: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
        .bind(user)
        .fetch_one(f.p.pool())
        .await
        .unwrap();
    assert!(
        !present,
        "actual retirement and actual completion replay both commit"
    );
    assert_eq!(state(f).await, before);
}
