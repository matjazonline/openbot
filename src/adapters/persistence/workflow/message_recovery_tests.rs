//! 04.11 recovery (D2 §5.1 rows 5, 10, 14; D4 §L seam): pre-marker failures retry and publish
//! once; an existing marker is never republished; retry_safe follows receipts; a control retry
//! replays the failed receipt; a model-tool marker in a Reconcile-parked execution is truth
//! only, never a job reopen.
use super::*;
use crate::application::workflow::message::MessageObservation;

async fn reclaim(setup: &Setup) -> ClaimedWorkflow {
    let p = &setup.fixture.persistence;
    sqlx::query("UPDATE background_tasks SET run_at=clock_timestamp() WHERE id=$1")
        .bind(setup.scope().job.0)
        .execute(setup.pool())
        .await
        .unwrap();
    p.claim_io(setup.scope(), WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the retried job is claimable")
}

async fn release(setup: &Setup, claim: &ClaimedWorkflow, failure: WorkflowFailure) {
    let released = setup
        .fixture
        .persistence
        .release_io(claim.fence, lease(), LeaseReleaseCause::Classified(failure))
        .await
        .unwrap();
    assert!(released);
}

/// Row 5 and D1 §6: a rollback at any publication write leaves nothing, retries within the
/// attempt budget and the next attempt publishes exactly once.
#[tokio::test]
async fn workflow_message_recovery_rolled_back_publication_retries_and_publishes_once() {
    for table in [
        "messages",
        "message_deliveries",
        "workflow_message_destinations",
        "workflow_waits",
    ] {
        let setup = Box::pin(send(&[To::Channel, To::User])).await;
        sqlx::raw_sql(&format!(
            "CREATE FUNCTION fixture_rollback() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected rollback'; END $$; \
             CREATE TRIGGER fixture_rollback BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION fixture_rollback();"
        ))
        .execute(setup.pool())
        .await
        .unwrap();
        let WorkflowHandlerOutcome::Completed(Err(failure)) =
            setup.runtime().execute(&setup.kind, &setup.claim).await
        else {
            panic!("{table}: the publication fails");
        };
        assert_eq!(
            failure.failure.code().as_str(),
            "message.unresolved",
            "{table}"
        );
        assert_eq!(
            failure.safety,
            crate::domain::workflow::RetrySafety::SafeToRetry
        );
        let run = setup.scope().run;
        assert_eq!(
            rows(setup.pool(), run).await,
            Rows {
                markers: 0,
                actions: 0,
                destinations: 0,
                deliveries: 0,
                receipts: 0,
                waits: 0,
                messages: 0
            },
            "{table}"
        );
        assert_eq!(
            retry_safe(setup.pool(), setup.scope()).await,
            None,
            "{table}: no marker"
        );
        release(&setup, &setup.claim, failure).await;
        let after = state(setup.pool(), setup.scope()).await;
        assert_eq!(
            (after.run.as_str(), after.job.as_str()),
            ("running", "pending"),
            "{table}"
        );
        sqlx::raw_sql(&format!("DROP TRIGGER fixture_rollback ON {table}"))
            .execute(setup.pool())
            .await
            .unwrap();
        let retried = reclaim(&setup).await;
        let outcome = setup.runtime().execute(&setup.kind, &retried).await;
        assert!(
            matches!(outcome, WorkflowHandlerOutcome::Suspended(_)),
            "{table}"
        );
        let rows = rows(setup.pool(), run).await;
        assert_eq!(
            (rows.markers, rows.deliveries, rows.messages),
            (1, 1, 2),
            "{table}"
        );
        let attempts: Vec<(String, Option<String>)> = sqlx::query_as("SELECT status, stop_reason FROM task_attempts WHERE task_id=$1 ORDER BY attempt_number")
            .bind(setup.scope().job.0).fetch_all(setup.pool()).await.unwrap();
        assert_eq!(
            attempts[1],
            ("completed".into(), Some("effect_parked".into())),
            "{table}"
        );
    }
}

/// Row 14: re-entry with an existing pending marker is never republished.
#[tokio::test]
async fn workflow_message_recovery_existing_marker_is_never_republished() {
    let setup = Box::pin(send(&[To::User])).await;
    parked(&setup).await;
    let before = rows(setup.pool(), setup.scope().run).await;
    // The park closed this fence: a stale worker re-entering it is refused before any write.
    let WorkflowHandlerOutcome::Completed(Err(failure)) =
        setup.runtime().execute(&setup.kind, &setup.claim).await
    else {
        panic!("a re-entry under the closed fence cannot complete");
    };
    assert_eq!(failure.failure.code().as_str(), "message.rejected");
    assert_eq!(rows(setup.pool(), setup.scope().run).await, before);
    // Row 6 (in-app): the receipt committed and the worker died before completing; the retry
    // replays that receipt and publishes nothing new.
    let in_app = Box::pin(send(&[To::Channel])).await;
    let WorkflowHandlerOutcome::Completed(Ok(first)) =
        in_app.runtime().execute(&in_app.kind, &in_app.claim).await
    else {
        panic!("in-app publication commits its receipt");
    };
    let released = in_app
        .fixture
        .persistence
        .release_io(
            in_app.claim.fence,
            lease(),
            LeaseReleaseCause::LocalInterruption,
        )
        .await
        .unwrap();
    assert!(released);
    assert_eq!(retry_safe(in_app.pool(), in_app.scope()).await, Some(true));
    let retried = reclaim(&in_app).await;
    let WorkflowHandlerOutcome::Completed(Ok(replayed)) =
        in_app.runtime().execute(&in_app.kind, &retried).await
    else {
        panic!("the retry replays the receipt");
    };
    assert_eq!(replayed, first);
    assert_eq!(rows(in_app.pool(), in_app.scope().run).await.markers, 1);
    // A direct tool re-entry with the same model call observes the same invocation.
    let fresh = Box::pin(send(&[To::Channel])).await;
    let arguments = send_arguments(&[("user", fresh.user)], "Once");
    assert!(matches!(
        Box::pin(tool(&fresh, "same-call", arguments.clone()))
            .await
            .unwrap(),
        MessageObservation::Pending
    ));
    assert!(matches!(
        Box::pin(tool(&fresh, "same-call", arguments))
            .await
            .unwrap(),
        MessageObservation::Pending
    ));
    let changed = send_arguments(&[("user", fresh.user)], "Changed");
    assert!(
        Box::pin(tool(&fresh, "same-call", changed)).await.is_err(),
        "changed arguments need a new model call"
    );
    assert_eq!(rows(fresh.pool(), fresh.scope().run).await.markers, 1);
}

/// Competing dispatchers of one step publish once; competing consumers record one receipt.
#[tokio::test]
async fn workflow_message_publish_competing_dispatchers_publish_once() {
    let setup = Box::pin(send(&[To::Channel, To::User])).await;
    let (a, b) = (setup.runtime(), setup.runtime());
    let (first, second) = tokio::join!(
        a.execute(&setup.kind, &setup.claim),
        b.execute(&setup.kind, &setup.claim)
    );
    let suspended = [&first, &second]
        .iter()
        .filter(|outcome| matches!(outcome, WorkflowHandlerOutcome::Suspended(_)))
        .count();
    assert!(suspended >= 1, "{first:?} / {second:?}");
    let run = setup.scope().run;
    let rows_after = rows(setup.pool(), run).await;
    assert_eq!(
        (rows_after.markers, rows_after.deliveries, rows_after.waits),
        (1, 1, 1)
    );
    deliver(
        setup.pool(),
        delivery(setup.pool(), run, 1).await,
        "delivered",
    )
    .await;
    let p = &setup.fixture.persistence;
    let (one, two) = tokio::join!(
        p.consume_message_receipts(setup.scope(), lease()),
        p.consume_message_receipts(setup.scope(), lease())
    );
    assert!(one.unwrap() + two.unwrap() >= 1);
    assert_eq!(rows(setup.pool(), run).await.receipts, 1);
    assert_eq!(action_state(setup.pool(), run).await, "accepted");
}

/// Row 10 with a control retry: the reopened step replays the same failed receipt and can
/// create no new delivery.
#[tokio::test]
async fn workflow_message_recovery_control_retry_replays_the_failed_receipt() {
    let setup = Box::pin(send(&[To::User])).await;
    parked(&setup).await;
    let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
    deliver(setup.pool(), delivery(setup.pool(), run, 0).await, "dead").await;
    p.consume_message_receipts(scope, lease()).await.unwrap();
    p.resume_wait(scope).await.unwrap();
    assert_eq!(state(setup.pool(), scope).await.run, "failed");
    let head = p.head(scope.company, run).await.unwrap().unwrap();
    let retried = p
        .retry(RetryCommand {
            company_id: scope.company,
            actor: setup.fixture.target.actor,
            run_id: run,
            expected_revision: head.revision,
            command_key: IdempotencyKey::parse("retry-failed-message").unwrap(),
        })
        .await
        .unwrap();
    let before = rows(setup.pool(), run).await;
    match retried {
        RetryResult::Applied { .. } => {
            let claim = reclaim(&setup).await;
            let WorkflowHandlerOutcome::Completed(Err(failure)) =
                setup.runtime().execute(&setup.kind, &claim).await
            else {
                panic!("the replayed receipt is the same failure");
            };
            assert_eq!(failure.failure.code().as_str(), "message.delivery_failed");
        }
        other => assert!(
            matches!(other, RetryResult::Unsafe { .. } | RetryResult::NotFound),
            "{other:?}"
        ),
    }
    assert_eq!(
        rows(setup.pool(), run).await,
        before,
        "no new delivery either way"
    );
}

/// D4 §L port seam: a model-tool marker with a pending email in an execution parked for
/// reconciliation. Its receipt is truth only; the run stays parked, the job failed, no
/// successor; a delivery reopen reopens only the delivery.
#[tokio::test]
async fn workflow_message_recovery_model_tool_marker_never_reopens_its_job() {
    let setup = Box::pin(send(&[To::Channel])).await;
    let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
    let observed = Box::pin(tool(
        &setup,
        "agent-call",
        send_arguments(&[("user", setup.user)], "From the agent"),
    ))
    .await
    .unwrap();
    assert!(matches!(observed, MessageObservation::Pending));
    // The enclosing execution is interrupted with the pending marker: Reconcile.
    let released = p
        .release_io(
            setup.claim.fence,
            lease(),
            LeaseReleaseCause::LocalInterruption,
        )
        .await
        .unwrap();
    assert!(released);
    let parked_state = state(setup.pool(), scope).await;
    assert_eq!(
        (
            parked_state.run.as_str(),
            parked_state.waiting.as_deref(),
            parked_state.job.as_str()
        ),
        ("waiting", Some("reconciliation"), "failed")
    );
    assert_eq!(retry_safe(setup.pool(), scope).await, Some(false));
    deliver(
        setup.pool(),
        delivery(setup.pool(), run, 0).await,
        "delivered",
    )
    .await;
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(rows(setup.pool(), run).await.receipts, 1);
    let after = state(setup.pool(), scope).await;
    assert_eq!(
        (
            after.run.as_str(),
            after.waiting.as_deref(),
            after.job.as_str()
        ),
        ("waiting", Some("reconciliation"), "failed")
    );
    assert_eq!(retry_safe(setup.pool(), scope).await, Some(true));
    let executions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_executions WHERE run_id=$1")
            .bind(run.as_uuid())
            .fetch_one(setup.pool())
            .await
            .unwrap();
    assert_eq!(executions, 1);
    assert!(candidates(&setup).await.is_empty());
}
