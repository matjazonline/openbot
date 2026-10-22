//! Competing claimants and recovery for the `child_parked` protocol (07-D2 §D2.5, F019), and the
//! schema guards that hold it whatever writes the rows. Raw SQL is deliberate in the guards.
use super::*;
use crate::application::workflow::{
    activation::ActivationRequest, agent::AgentToolObservation, tools::WorkflowToolCall,
};
use crate::domain::workflow::ChildOrdinal;
use std::time::Duration;

use super::workflow_tool_support::*;

async fn call_for(setup: &ToolSetup, question: &str) -> WorkflowToolCall {
    setup.call(
        setup.frozen_pin(setup.parent).await,
        json!({"question": question}),
    )
}

/// The parent's state, its children, and the children not yet terminal.
async fn lineage(p: &PostgresPersistence, run: RunId) -> (String, i64, i64) {
    sqlx::query_as(
        "SELECT run.state, \
           (SELECT count(*) FROM workflow_runs WHERE parent_run_id=run.id), \
           (SELECT count(*) FROM workflow_runs WHERE parent_run_id=run.id \
              AND state NOT IN ('succeeded','failed','cancelled','expired')) \
         FROM workflow_runs AS run WHERE run.id=$1",
    )
    .bind(run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap()
}

async fn job_status(p: &PostgresPersistence, scope: ActivationRequest) -> (String, i16) {
    sqlx::query_as("SELECT status, continuation_count FROM background_tasks WHERE id=$1")
        .bind(scope.job.0)
        .fetch_one(p.pool())
        .await
        .unwrap()
}

/// Dispatch a call in a new run; it parks.
async fn parked_call(setup: &ToolSetup, key: &str) -> (ActivationRequest, WorkflowToolCall) {
    let claim = setup.claim(key).await;
    let call = call_for(setup, key).await;
    assert!(matches!(
        setup.dispatch(&claim, &call).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    (claim.fence.scope, call)
}

/// A cancel of `scope`'s run at the revision read now, to race whatever runs beside it.
async fn racing_cancel(setup: &ToolSetup, scope: ActivationRequest, key: &str) -> CancelCommand {
    let p = setup.persistence();
    CancelCommand {
        company_id: scope.company,
        actor: setup.fixture.target.actor,
        run_id: scope.run,
        command_key: IdempotencyKey::parse(key).unwrap(),
        expected_revision: p
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    }
}

#[tokio::test]
async fn workflow_tool_cancel_and_dispatch_never_leave_a_live_child_under_a_cancelled_run() {
    let setup = Box::pin(ToolSetup::new(decision_child())).await;
    let p = setup.persistence();
    // Cancel first: the dispatch finds no fence and starts nothing.
    let claim = setup.claim("cancel-first").await;
    setup.cancel(claim.fence.scope, "cancel-first").await;
    let call = call_for(&setup, "q").await;
    assert!(setup.dispatch(&claim, &call).await.is_err());
    assert_eq!(
        lineage(p, claim.fence.scope.run).await,
        ("cancelled".into(), 0, 0)
    );
    // Dispatch first: the cancel cascades to the call's child.
    let (scope, _) = parked_call(&setup, "dispatch-first").await;
    setup.cancel(scope, "dispatch-first").await;
    assert_eq!(lineage(p, scope.run).await, ("cancelled".into(), 1, 0));
    // Racing: whichever commits first, no child outlives its cancelled parent.
    for round in 0..4 {
        let key = format!("race-{round}");
        let claim = setup.claim(&key).await;
        let scope = claim.fence.scope;
        let call = call_for(&setup, &key).await;
        let command = racing_cancel(&setup, scope, &key).await;
        let (cancelled, dispatched) =
            tokio::join!(p.cancel(command), setup.dispatch(&claim, &call));
        if !matches!(cancelled, Ok(CancelResult::Applied { .. })) {
            setup.cancel(scope, &format!("{key}-retry")).await;
        }
        let parked = matches!(dispatched, Ok(AgentToolObservation::Parked(_)));
        assert!(parked || dispatched.is_err(), "{round}");
        assert_eq!(
            lineage(p, scope.run).await,
            ("cancelled".into(), i64::from(parked), 0),
            "{round}"
        );
    }
}

#[tokio::test]
async fn workflow_tool_dispatch_after_or_during_lease_expiry_starts_nothing() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let call = call_for(&setup, "q").await;
    // An expired lease: the park's fenced completion is refused and nothing commits.
    let claim = setup.claim("expired").await;
    let scope = claim.fence.scope;
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(scope.job.0)
        .execute(p.pool())
        .await
        .unwrap();
    assert!(setup.dispatch(&claim, &call).await.is_err());
    assert_eq!(children(p, scope.run).await, 0);
    assert_eq!(call_parked(p, scope).await.stop, None);
    // Retired by the sweeper first: the fence is gone.
    assert!(p.retire_expired_io(scope, lease()).await.unwrap());
    assert!(setup.dispatch(&claim, &call).await.is_err());
    assert_eq!(children(p, scope.run).await, 0);
    setup.cancel(scope, "expired").await;
    // A lease that expires inside the dispatch, after its fence was locked: the child version's
    // row is held, so inserting the child waits past the expiry; the park is then refused and
    // the whole call rolls back.
    let claim = setup.claim("expires-inside").await;
    let scope = claim.fence.scope;
    let mut blocker = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_versions WHERE id=$1 FOR UPDATE")
        .bind(setup.child.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '300 milliseconds' WHERE id=$1")
        .bind(scope.job.0)
        .execute(p.pool())
        .await
        .unwrap();
    let release = async {
        tokio::time::sleep(Duration::from_millis(600)).await;
        blocker.commit().await.unwrap();
    };
    let (_, dispatched) = tokio::join!(release, setup.dispatch(&claim, &call));
    let Err(AppError::Conflict(refusal)) = dispatched else {
        panic!(
            "{:?}",
            dispatched.map(|observation| format!("{observation:?}"))
        );
    };
    assert_eq!(
        refusal,
        "Workflow tool park refused: run or lease window closed"
    );
    assert_eq!(children(p, scope.run).await, 0);
    assert_eq!(
        call_parked(p, scope).await,
        CallParked {
            run: "running".into(),
            reason: None,
            job: "processing".into(),
            stop: None,
            children: 0,
        }
    );
    setup.cancel(scope, "expires-inside").await;
}

#[tokio::test]
async fn workflow_tool_cancel_and_continuation_settle_the_call_once() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    // Continuation first: the job reopens, and a cancel afterwards closes the run and fails the
    // reopened job.
    let (scope, _) = parked_call(&setup, "continued-first").await;
    setup.finish_child(scope).await;
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Continued);
    setup.cancel(scope, "continued-first").await;
    assert_eq!(run_state(p, scope.run).await, "cancelled");
    assert_eq!(job_status(p, scope).await, ("failed".into(), 1));
    // Cancel first: the continuation finds the settled wait and writes nothing.
    let (scope, _) = parked_call(&setup, "cancelled-first").await;
    setup.finish_child(scope).await;
    setup.cancel(scope, "cancelled-first").await;
    let before = (job_status(p, scope).await, call_parked(p, scope).await);
    let progress = p.resume_wait(scope).await;
    assert!(
        !matches!(progress, Ok(WaitProgress::Continued)),
        "{progress:?}"
    );
    assert_eq!(
        (job_status(p, scope).await, call_parked(p, scope).await),
        before
    );
    assert_eq!(before.0, ("completed".into(), 0));
    // Racing: the run ends cancelled with at most one continuation and no claimable job.
    for round in 0..4 {
        let key = format!("race-{round}");
        let (scope, _) = parked_call(&setup, &key).await;
        setup.finish_child(scope).await;
        let command = racing_cancel(&setup, scope, &key).await;
        let (cancelled, progress) = tokio::join!(p.cancel(command), p.resume_wait(scope));
        if !matches!(cancelled, Ok(CancelResult::Applied { .. })) {
            setup.cancel(scope, &format!("{key}-retry")).await;
        }
        let continued = matches!(progress, Ok(WaitProgress::Continued));
        assert_eq!(run_state(p, scope.run).await, "cancelled", "{round}");
        let expected = if continued { "failed" } else { "completed" };
        assert_eq!(
            job_status(p, scope).await,
            (expected.into(), i16::from(continued)),
            "{round}"
        );
    }
}

#[tokio::test]
async fn workflow_tool_reopened_attempt_lost_before_saving_replays_the_same_result() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let (scope, call) = parked_call(&setup, "lost-attempt").await;
    setup.finish_child(scope).await;
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Continued);
    let mut answers = Vec::new();
    let mut attempts = Vec::new();
    for _ in 0..2 {
        let claim = p
            .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .unwrap();
        let AgentToolObservation::Completed(result) = setup.dispatch(&claim, &call).await.unwrap()
        else {
            panic!("a finished call answers");
        };
        answers.push(result);
        attempts.push(claim.fence.attempt.0);
        // The attempt loses its lease before the agent saves the result.
        sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
            .bind(scope.job.0)
            .execute(p.pool())
            .await
            .unwrap();
        assert!(p.retire_expired_io(scope, lease()).await.unwrap());
        // The retry is due at once rather than after its backoff.
        sqlx::query(
            "UPDATE background_tasks SET run_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        )
        .bind(scope.job.0)
        .execute(p.pool())
        .await
        .unwrap();
    }
    assert!(attempts[1] > attempts[0]);
    assert_eq!(answers[0], answers[1]);
    assert_eq!(
        answers[0],
        json!({"choice": "approve", "answer": "lost-attempt"})
    );
    assert_eq!(children(p, scope.run).await, 1);
}

/// The SQLSTATE and message of a statement batch, raised at once or at commit.
async fn refused(
    mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    statements: &[(&str, Uuid)],
) -> (String, String) {
    for (sql, bind) in statements {
        if let Err(error) = sqlx::query(sql).bind(*bind).execute(&mut *tx).await {
            return described(error);
        }
    }
    described(tx.commit().await.unwrap_err())
}

fn described(error: sqlx::Error) -> (String, String) {
    let error = error.as_database_error().unwrap();
    (
        error.code().unwrap().into_owned(),
        error.message().to_owned(),
    )
}

#[tokio::test]
async fn workflow_tool_schema_guards_refuse_parks_reopens_and_lineage_written_by_hand() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    // A park without the call's wait.
    let claim = setup.claim("raw-park").await;
    let scope = claim.fence.scope;
    let tx = p.pool().begin().await.unwrap();
    assert_eq!(
        refused(tx, &[
            ("UPDATE task_attempts SET status='completed',stop_reason='child_parked',finished_at=clock_timestamp() WHERE task_id=$1 AND status='processing'", scope.job.0),
            ("UPDATE workflow_runs SET state='waiting',waiting_reason='child_run' WHERE id=$1", scope.run.as_uuid()),
            ("UPDATE background_tasks SET status='completed',worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL WHERE id=$1", scope.job.0),
        ]).await,
        ("23514".into(), "workflow completion lost ownership before commit".into())
    );
    setup.cancel(scope, "raw-park").await;
    // A reopen while the call's child still runs, and a completed call wait without a reopen.
    let (scope, _) = parked_call(&setup, "raw-reopen").await;
    let tx = p.pool().begin().await.unwrap();
    assert_eq!(
        refused(tx, &[
            ("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE id=$1", scope.run.as_uuid()),
            ("UPDATE background_tasks SET status='pending',continuation_count=continuation_count+1 WHERE id=$1", scope.job.0),
        ]).await,
        ("23514".into(), "workflow job reopen requires an approval, delivery or child continuation".into())
    );
    let tx = p.pool().begin().await.unwrap();
    assert_eq!(
        refused(tx, &[
            ("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE run_id=$1 AND child_tool_call_id IS NOT NULL", scope.run.as_uuid()),
        ]).await,
        ("23514".into(), "workflow tool child wait only continues its agent job".into())
    );
    // A child admission naming both, or neither, of a call and a round; an exact copy passes
    // the source check and only meets the admission's own uniqueness.
    let child = setup.child_of(scope).await;
    let copy = |source: &str| {
        format!(
            "INSERT INTO workflow_admissions SELECT (jsonb_populate_record(NULL::workflow_admissions, \
             to_jsonb(admission) || jsonb_build_object('source_key', \
             substring(admission.source_key FROM 1 FOR 3) || ({source})::text))).* \
             FROM workflow_admissions AS admission WHERE admission.run_id=$1"
        )
    };
    let source = "substring(admission.source_key FROM 4)::jsonb";
    for (shape, sql, code) in [
        (
            "both",
            copy(&format!("jsonb_set({source}, '{{5}}', '1')")),
            "23514",
        ),
        (
            "neither",
            copy(&format!("jsonb_set({source}, '{{4}}', 'null')")),
            "23514",
        ),
        ("exact", copy(source), "23505"),
    ] {
        let tx = p.pool().begin().await.unwrap();
        assert_eq!(
            refused(tx, &[(sql.as_str(), child.run.as_uuid())]).await.0,
            code,
            "{shape}"
        );
    }
    setup.cancel(scope, "raw-reopen").await;
    // A child whose wait names another call, or another round, than its admission.
    for origin in ["call", "round"] {
        let claim = setup.claim(&format!("mismatch-{origin}")).await;
        let scope = claim.fence.scope;
        let mut tx = p.pool().begin().await.unwrap();
        let mismatch = if origin == "call" {
            let call = call_for(&setup, "q").await;
            workflow_tool_calls::call_on(&mut tx, claim.fence, &call)
                .await
                .unwrap();
            "UPDATE workflow_waits SET child_tool_call_id=gen_random_uuid() WHERE run_id=$1 AND child_tool_call_id IS NOT NULL"
        } else {
            round_child(&mut tx, &setup, scope).await;
            "UPDATE workflow_waits SET child_ordinal=2 WHERE run_id=$1 AND child_ordinal=1"
        };
        assert_eq!(
            refused(tx, &[(mismatch, scope.run.as_uuid())]).await,
            (
                "23514".into(),
                "Workflow child run has no parent child_run wait".into()
            ),
            "{origin}"
        );
        setup.cancel(scope, &format!("mismatch-{origin}")).await;
    }
}

/// Start the pinned child as round 1 of the parent execution, through the runtime child writer.
async fn round_child(db: &mut PgConnection, setup: &ToolSetup, scope: ActivationRequest) {
    let (binding_id, deadline): (Uuid, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as("SELECT binding_id, deadline FROM workflow_runs WHERE id=$1")
            .bind(scope.run.as_uuid())
            .fetch_one(&mut *db)
            .await
            .unwrap();
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(binding_id),
        scope.run.as_uuid(),
    )
    .await
    .unwrap();
    let configured = children::child_binding(&binding, setup.child, json!({"reviewer": "x"}))
        .unwrap()
        .unwrap();
    children::create_on(
        db,
        scope,
        &children::ChildStart {
            step: &StepId::parse("start").unwrap(),
            origin: children::ChildOrigin::Round(ChildOrdinal::new(1).unwrap()),
            binding: &configured,
            input: &json!({"question": "q"}),
            deadline,
        },
    )
    .await
    .unwrap();
}
