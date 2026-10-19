use super::*;
use crate::application::workflow::http::HttpToolCall;
use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn workflow_action_approval_later_submission_competes_with_cancellation() {
    use crate::application::workflow::{CancelCommand, CancelResult};
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Tool)).await;
    let first = call_park(&setup, &setup.claim, 0).await;
    let claim = resume(&setup, &first).await;
    let second = call_park(&setup, &claim, 1).await;
    let p = &setup.fixture.persistence;
    let scope = claim.fence.scope;
    let command = CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: setup.fixture.target.actor,
        command_key: crate::application::workflow::IdempotencyKey::parse("cancel-later").unwrap(),
        expected_revision: p
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    let barrier = tokio::sync::Barrier::new(2);
    let submit = async {
        barrier.wait().await;
        p.submit_decision(submission(&setup, &second, "approve"))
            .await
    };
    let cancel = async {
        barrier.wait().await;
        p.cancel(command).await.unwrap()
    };
    let (submitted, cancelled) = tokio::join!(submit, cancel);
    let count = counters(&setup).await;
    assert_eq!(count["failures"], 0);
    if matches!(cancelled, CancelResult::Applied { .. }) {
        assert!(matches!(submitted, Err(AppError::Conflict(_))));
        assert_eq!(count["continuations"], 1);
    } else {
        assert!(matches!(cancelled, CancelResult::RevisionConflict { .. }));
        assert!(submitted.is_ok());
        assert_eq!(count["continuations"], 2);
    }
    assert_eq!(action_counts(&setup.fixture).await.dispatches, 0);
}

#[tokio::test]
async fn workflow_action_approval_old_comment_cannot_attach_to_later_wait() {
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Tool)).await;
    let first = call_park(&setup, &setup.claim, 0).await;
    let claim = resume(&setup, &first).await;
    let second = call_park(&setup, &claim, 1).await;
    let p = &setup.fixture.persistence;
    let mut comment = AddDecisionComment {
        company: claim.fence.scope.company,
        actor: setup.fixture.target.actor,
        decision: first.decision,
        command_key: DecisionCommandKey::parse("later-comment").unwrap(),
        body: "Review the current request".into(),
    };
    assert!(matches!(
        p.add_decision_comment(comment.clone()).await,
        Err(AppError::Conflict(_))
    ));
    comment.decision = second.decision;
    assert_eq!(
        p.add_decision_comment(comment).await.unwrap().disposition,
        DecisionDisposition::Applied
    );
    let read = p
        .read_decision(
            claim.fence.scope.company,
            setup.fixture.target.actor,
            second.decision,
        )
        .await
        .unwrap();
    assert_eq!(read.comments.len(), 1);
    assert_eq!(counters(&setup).await["continuations"], 1);
}

pub(super) fn call(setup: &Setup, index: usize) -> HttpToolCall {
    HttpToolCall {
        connection: setup.connection.id,
        operation: TypeName::parse("registered.request").unwrap(),
        arguments: json!({"method":"POST","path":"/messages","body":{"message":format!("message-{index}")}}),
        call: ModelToolCallId::parse(format!("call-{index}")).unwrap(),
    }
}

pub(super) async fn call_park(
    setup: &Setup,
    claim: &ClaimedWorkflow,
    index: usize,
) -> DecisionView {
    let result = actions(setup)
        .call_tool(claim.fence, call(setup, index), &CancellationToken::new())
        .await
        .unwrap();
    let RemoteDispatchObservation::ApprovalParked(wait) = result else {
        panic!("expected fresh approval");
    };
    let p = &setup.fixture.persistence;
    let decision =
        sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND wait_id=$2")
            .bind(claim.fence.scope.company.as_uuid())
            .bind(wait.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    p.read_decision(
        setup.fixture.target.company,
        setup.fixture.target.actor,
        DecisionId(decision),
    )
    .await
    .unwrap()
}

pub(super) async fn resume(setup: &Setup, decision: &DecisionView) -> ClaimedWorkflow {
    let p = &setup.fixture.persistence;
    p.submit_decision(submission(setup, decision, "approve"))
        .await
        .unwrap();
    p.claim_io(
        setup.claim.fence.scope,
        WorkflowWorkerId(Uuid::new_v4()),
        lease(),
    )
    .await
    .unwrap()
    .unwrap()
}

pub(super) async fn counters(setup: &Setup) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('failures',retry_count,'continuations',continuation_count,'ordinal',attempt_ordinal,'status',status) FROM background_tasks WHERE id=$1")
        .bind(setup.claim.fence.scope.job.0).fetch_one(setup.fixture.persistence.pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_action_approval_multiple_effects_resume_without_failure_debit_or_old_approval_reuse()
 {
    let server = scripted_scenario(
        (0..2)
            .map(|index| {
                ScriptedExchange::new(
                    move |request| {
                        if request.body == json!({"message":format!("message-{index}")}) {
                            Ok(())
                        } else {
                            Err("wrong effect")
                        }
                    },
                    ScriptedResponse::json(json!({"ok":true})),
                )
            })
            .collect(),
    )
    .await;
    let setup = Box::pin(protected(&server.base_url, Entrance::Tool)).await;
    let p = &setup.fixture.persistence;
    sqlx::query("UPDATE background_tasks SET max_retries=1 WHERE id=$1")
        .bind(setup.claim.fence.scope.job.0)
        .execute(p.pool())
        .await
        .unwrap();
    let first = call_park(&setup, &setup.claim, 0).await;
    let next = resume(&setup, &first).await;
    assert_eq!(next.fence.attempt.0, 2);
    assert!(!p.validate_io(setup.claim.fence, lease()).await.unwrap());
    let effect = actions(&setup)
        .call_tool(next.fence, call(&setup, 0), &CancellationToken::new())
        .await
        .unwrap();
    assert!(matches!(effect, RemoteDispatchObservation::Committed(_)));
    let second = call_park(&setup, &next, 1).await;
    assert_ne!(first.decision, second.decision);
    assert_eq!(
        p.submit_decision(submission(&setup, &first, "approve"))
            .await
            .unwrap()
            .disposition,
        DecisionDisposition::Replayed
    );
    assert_eq!(
        counters(&setup).await,
        json!({"failures":0,"continuations":1,"ordinal":2,"status":"completed"})
    );
    assert!(
        p.claim_io(next.fence.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .is_none()
    );
    let last = resume(&setup, &second).await;
    assert_eq!(last.fence.attempt.0, 3);
    assert!(!p.validate_io(next.fence, lease()).await.unwrap());
    let effect = actions(&setup)
        .call_tool(last.fence, call(&setup, 1), &CancellationToken::new())
        .await
        .unwrap();
    assert!(matches!(effect, RemoteDispatchObservation::Committed(_)));
    assert_eq!(server.finish().await, Ok(2));
    assert!(
        p.release_io(last.fence, lease(), LeaseReleaseCause::HandlerFailure)
            .await
            .unwrap()
    );
    assert_eq!(
        counters(&setup).await,
        json!({"failures":1,"continuations":2,"ordinal":4,"status":"failed"})
    );
    assert_history(&setup).await;
}

async fn assert_history(setup: &Setup) {
    let scope = setup.claim.fence.scope;
    let history: Value = sqlx::query_scalar("SELECT jsonb_build_object('attempts',(SELECT jsonb_agg(jsonb_build_array(attempt_number,status) ORDER BY attempt_number) FROM task_attempts WHERE task_id=$1),'waits',(SELECT count(*) FROM workflow_waits WHERE execution_id=$2 AND state='completed'),'approvals',(SELECT count(*) FROM workflow_decisions WHERE execution_id=$2 AND choice='approve'),'audits',(SELECT count(*) FROM workflow_run_events WHERE execution_id=$2 AND decision_id IS NOT NULL))")
        .bind(scope.job.0).bind(scope.execution.as_uuid()).fetch_one(setup.fixture.persistence.pool()).await.unwrap();
    assert_eq!(
        history,
        json!({"attempts":[[1,"completed"],[2,"completed"],[3,"failed"]],"waits":2,"approvals":2,"audits":4})
    );
}

#[tokio::test]
async fn workflow_action_approval_continuation_ceiling_refuses_without_resetting_failures() {
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Tool)).await;
    let p = &setup.fixture.persistence;
    let mut claim = setup.claim.clone();
    let max = usize::from(crate::services::harness::runs::MAX_CONTINUATIONS) - 1;
    assert_eq!(
        max, 63,
        "SQL continuation ceiling tracks the harness execution ceiling"
    );
    for index in 0..max {
        let decision = call_park(&setup, &claim, index).await;
        claim = resume(&setup, &decision).await;
        assert_eq!(claim.fence.attempt.0 as usize, index + 2);
    }
    assert_eq!(
        counters(&setup).await,
        json!({"failures":0,"continuations":63,"ordinal":64,"status":"processing"})
    );
    let refused = actions(&setup)
        .call_tool(claim.fence, call(&setup, max), &CancellationToken::new())
        .await
        .unwrap();
    assert!(matches!(
        refused,
        RemoteDispatchObservation::ApprovalRequired
    ));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_decisions WHERE execution_id=$1")
            .bind(claim.fence.scope.execution.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(count, 63);
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}

#[tokio::test]
async fn workflow_action_approval_continuation_counter_and_history_cannot_be_forged() {
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let error = sqlx::query("UPDATE background_tasks SET continuation_count=1 WHERE id=$1")
        .bind(setup.claim.fence.scope.job.0)
        .execute(p.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "workflow continuation requires an approval reopen"
    );
    parked(&setup).await;
    let decision = view(&setup).await;
    for query in [
        "UPDATE workflow_decisions SET parked_attempt_number=parked_attempt_number+1 WHERE id=$1",
        "UPDATE workflow_run_events SET event_kind='forged' WHERE decision_id=$1",
        "DELETE FROM workflow_run_events WHERE decision_id=$1",
    ] {
        let error = sqlx::query(query)
            .bind(decision.decision.0)
            .execute(p.pool())
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    let mut tx = p.pool().begin().await.unwrap();
    super::sql_tests::submitted_on(&mut tx, &setup, "approve").await;
    let error = sqlx::query("UPDATE background_tasks SET status='pending',continuation_count=continuation_count+1,retry_count=retry_count+1 WHERE id=$1")
        .bind(setup.claim.fence.scope.job.0).execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "workflow continuation requires an approval reopen"
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        counters(&setup).await,
        json!({"failures":0,"continuations":0,"ordinal":1,"status":"completed"})
    );
}

#[tokio::test]
async fn workflow_action_approval_later_expiry_and_rejection_preserve_first_decision() {
    use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};
    for choice in ["expire", "reject"] {
        let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Tool)).await;
        let first = call_park(&setup, &setup.claim, 0).await;
        let claim = resume(&setup, &first).await;
        let second = call_park(&setup, &claim, 1).await;
        let p = &setup.fixture.persistence;
        if choice == "expire" {
            sqlx::query("UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds',deadline=clock_timestamp()-interval '1 second' WHERE id=(SELECT wait_id FROM workflow_decisions WHERE id=$1)")
                .bind(second.decision.0).execute(p.pool()).await.unwrap();
            assert!(matches!(
                p.resume_wait(claim.fence.scope).await.unwrap(),
                WaitProgress::Expired(_)
            ));
        } else {
            p.submit_decision(submission(&setup, &second, choice))
                .await
                .unwrap();
        }
        let first = p
            .read_decision(
                claim.fence.scope.company,
                setup.fixture.target.actor,
                first.decision,
            )
            .await
            .unwrap();
        assert_eq!(first.state, "completed");
        assert_eq!(first.revision, 2);
        let audit: Uuid = sqlx::query_scalar(
            "SELECT decision_id FROM workflow_run_events WHERE run_id=$1 AND event_kind=$2",
        )
        .bind(claim.fence.scope.run.as_uuid())
        .bind(if choice == "expire" {
            "http.approval_expired"
        } else {
            "http.approval_rejected"
        })
        .fetch_one(p.pool())
        .await
        .unwrap();
        assert_eq!(audit, second.decision.0);
        assert_eq!(counters(&setup).await["continuations"], 1);
        assert_eq!(action_counts(&setup.fixture).await.dispatches, 0);
    }
}

#[tokio::test]
async fn workflow_action_approval_failure_retry_after_continuation_uses_failure_allowance() {
    use crate::application::workflow::lease::WorkflowFailure;
    use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Tool)).await;
    let first = call_park(&setup, &setup.claim, 0).await;
    let claim = resume(&setup, &first).await;
    let p = &setup.fixture.persistence;
    sqlx::query("UPDATE background_tasks SET max_retries=2 WHERE id=$1")
        .bind(claim.fence.scope.job.0)
        .execute(p.pool())
        .await
        .unwrap();
    let cause = LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Retryable,
            FailureCode::parse("provider.unavailable").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::SafeToRetry,
    });
    assert!(p.release_io(claim.fence, lease(), cause).await.unwrap());
    assert_eq!(
        counters(&setup).await,
        json!({"failures":1,"continuations":1,"ordinal":3,"status":"pending"})
    );
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(claim.fence.scope.job.0)
    .execute(p.pool())
    .await
    .unwrap();
    let next = p
        .claim_io(claim.fence.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.fence.attempt.0, 3);
    assert!(!p.validate_io(claim.fence, lease()).await.unwrap());
    assert!(
        p.release_io(next.fence, lease(), LeaseReleaseCause::HandlerFailure)
            .await
            .unwrap()
    );
    assert_eq!(
        counters(&setup).await,
        json!({"failures":2,"continuations":1,"ordinal":4,"status":"failed"})
    );
}

#[tokio::test]
async fn workflow_action_approval_orphan_wait_and_foreign_audit_are_rejected() {
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Tool)).await;
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    let mut tx = p.pool().begin().await.unwrap();
    let wait = Uuid::new_v4();
    sqlx::query("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,notification_intent) VALUES($1,$2,$3,$4,'decision',clock_timestamp()+interval '1 minute',$5)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait)
        .bind(json!({"version":1,"kind":"wait_registered","wait_id":wait,"run_id":scope.run.as_uuid(),"execution_id":scope.execution.as_uuid()})).execute(&mut *tx).await.unwrap();
    let error = sqlx::query("SET CONSTRAINTS workflow_decision_wait_pair_guard IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "workflow decision wait requires its exact decision"
    );
    tx.rollback().await.unwrap();
    let decision = call_park(&setup, &setup.claim, 0).await;
    let foreign = Box::pin(protected("http://127.0.0.1:1", Entrance::Tool)).await;
    let foreign_decision = call_park(&foreign, &foreign.claim, 0).await;
    let error = sqlx::query("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id,decision_id) SELECT company_id,run_id,sequence+100,'action_approved',actor_id,execution_id,$2 FROM workflow_run_events WHERE decision_id=$1")
        .bind(decision.decision.0).bind(foreign_decision.decision.0).execute(p.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "workflow approval audit requires its exact decision"
    );
}
