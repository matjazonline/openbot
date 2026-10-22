//! 04.11 provider-neutral messaging against real PostgreSQL (decisions D2 + D3 + D4).
use super::*;
use crate::application::workflow::{
    lease::*,
    polling::{PollWork, WorkflowPolling},
    waits::{WaitProgress, WorkflowWaits},
    worker::WorkflowHandler,
};
use serde_json::{Value, json};

#[path = "message_test_support.rs"]
pub(super) mod support;
use support::*;
#[path = "message_agent_tests.rs"]
mod agent_tests;
#[path = "message_authority_tests.rs"]
mod authority_tests;
#[path = "message_delivery_tests.rs"]
mod delivery_tests;
#[path = "message_race_tests.rs"]
mod race_tests;
#[path = "message_reconcile_tests.rs"]
mod reconcile_tests;
#[path = "message_recovery_tests.rs"]
mod recovery_tests;
#[path = "message_sql_tests.rs"]
mod sql_tests;
#[path = "message_thread_tests.rs"]
mod thread_tests;
#[path = "workflow_tool_race_tests.rs"]
mod workflow_tool_race_tests;
#[path = "workflow_tool_registration_tests.rs"]
mod workflow_tool_registration_tests;
#[path = "workflow_tool_revocation_tests.rs"]
mod workflow_tool_revocation_tests;
#[path = "workflow_tool_support.rs"]
mod workflow_tool_support;
#[path = "workflow_tool_tests.rs"]
mod workflow_tool_tests;

#[path = "action_approval_message_tests.rs"]
mod approval_tests;

fn completed(outcome: WorkflowHandlerOutcome) -> WorkflowHandlerResult {
    match outcome {
        WorkflowHandlerOutcome::Completed(result) => result,
        WorkflowHandlerOutcome::Suspended(wait) => panic!("suspended in {:?}", wait.wait),
    }
}

async fn candidates(setup: &Setup) -> Vec<PollWork> {
    setup
        .fixture
        .persistence
        .poll_work(None, 100)
        .await
        .unwrap()
        .candidates
        .into_iter()
        .filter(|candidate| candidate.scope.execution == setup.scope().execution)
        .map(|candidate| candidate.work)
        .collect()
}

#[tokio::test]
async fn workflow_message_publish_in_app_channel_commits_receipt_atomically() {
    let setup = Box::pin(send(&[To::Channel])).await;
    let result = completed(setup.runtime().execute(&setup.kind, &setup.claim).await).unwrap();
    assert_eq!(result["status"], "accepted");
    assert_eq!(result["messages"][0]["destination"]["kind"], "channel");
    assert_eq!(
        result["messages"][0]["destination"]["id"],
        setup.other.to_string()
    );
    let run = setup.scope().run;
    assert_eq!(
        rows(setup.pool(), run).await,
        Rows {
            markers: 1,
            actions: 1,
            destinations: 1,
            deliveries: 0,
            receipts: 1,
            waits: 0,
            messages: 1
        }
    );
    assert_eq!(action_state(setup.pool(), run).await, "accepted");
    // The step still owns its fence: completion is the supervisor's, not publication's.
    let state = state(setup.pool(), setup.scope()).await;
    assert_eq!(
        (
            state.run.as_str(),
            state.job.as_str(),
            state.attempt.as_str()
        ),
        ("running", "processing", "processing")
    );
    // Replay: the same invocation observes its receipt and writes nothing new.
    let replay = completed(setup.runtime().execute(&setup.kind, &setup.claim).await).unwrap();
    assert_eq!(replay, result);
    assert_eq!(rows(setup.pool(), run).await.markers, 1);
    // The in-app message opened its own thread in the destination channel.
    let (channel, audience, entry): (Uuid, String, String) = sqlx::query_as(
        "SELECT thread.channel_id, message.audience, entry.entry_kind FROM workflow_message_destinations AS target \
         JOIN messages AS message ON message.id=target.message_id JOIN threads AS thread ON thread.id=target.thread_id \
         JOIN thread_messages AS entry ON entry.message_id=target.message_id WHERE target.run_id=$1",
    )
    .bind(run.as_uuid())
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert_eq!(channel, setup.other);
    assert_eq!(
        (audience.as_str(), entry.as_str()),
        ("internal_only", "conversation")
    );
}

#[tokio::test]
async fn workflow_message_wait_user_email_parks_then_resumes_on_accepted_receipt() {
    let setup = Box::pin(send(&[To::Channel, To::User])).await;
    let outcome = setup.runtime().execute(&setup.kind, &setup.claim).await;
    let WorkflowHandlerOutcome::Suspended(parked) = outcome else {
        panic!("an email destination parks the step");
    };
    let p = &setup.fixture.persistence;
    assert!(
        p.confirm_suspension(setup.claim.fence, parked.wait)
            .await
            .unwrap()
    );
    let scope = setup.scope();
    let run = scope.run;
    assert_eq!(
        state(setup.pool(), scope).await,
        State {
            run: "waiting".into(),
            waiting: Some("effect".into()),
            job: "completed".into(),
            attempt: "completed".into(),
            stop: Some("effect_parked".into()),
            wait: Some("waiting".into()),
        }
    );
    assert_eq!(
        rows(setup.pool(), run).await,
        Rows {
            markers: 1,
            actions: 1,
            destinations: 2,
            deliveries: 1,
            receipts: 0,
            waits: 1,
            messages: 2
        }
    );
    let (wait_deadline, run_deadline): (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as("SELECT wait.deadline, run.deadline FROM workflow_waits AS wait JOIN workflow_runs AS run ON run.id=wait.run_id WHERE wait.id=$1")
            .bind(parked.wait.as_uuid()).fetch_one(setup.pool()).await.unwrap();
    assert_eq!(wait_deadline, run_deadline);
    // Without a receipt the parked step is not rediscovered, scan after scan.
    assert!(candidates(&setup).await.is_empty());
    assert!(candidates(&setup).await.is_empty());
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Pending
    ));
    assert_eq!(
        state(setup.pool(), scope).await.wait.as_deref(),
        Some("waiting")
    );

    let delivery = delivery(setup.pool(), run, 1).await;
    let (purpose, recipient): (String, String) = sqlx::query_as(
        "SELECT purpose, recipient_address FROM message_deliveries JOIN workflow_message_destinations AS target ON target.delivery_id=message_deliveries.id WHERE message_deliveries.id=$1",
    )
    .bind(delivery)
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert_eq!(purpose, "workflow_send");
    assert!(recipient.ends_with("@example.test"), "{recipient}");
    deliver(setup.pool(), delivery, "delivered").await;
    assert_eq!(candidates(&setup).await, vec![PollWork::MessageReceipt]);
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), run).await, "accepted");
    assert_eq!(rows(setup.pool(), run).await.receipts, 1);
    assert_eq!(candidates(&setup).await, vec![PollWork::Wait]);
    let progress = p.resume_wait(scope).await.unwrap();
    let WaitProgress::Completed(step) = progress else {
        panic!("an accepted receipt completes the step: {progress:?}");
    };
    assert_eq!(step.output["status"], "accepted");
    assert_eq!(step.output["messages"].as_array().unwrap().len(), 2);
    let state = state(setup.pool(), scope).await;
    assert_eq!(
        (state.run.as_str(), state.wait.as_deref()),
        ("succeeded", Some("completed"))
    );
    assert!(candidates(&setup).await.is_empty());
}

async fn parked(setup: &Setup) -> crate::domain::workflow::WaitId {
    match setup.runtime().execute(&setup.kind, &setup.claim).await {
        WorkflowHandlerOutcome::Suspended(parked) => parked.wait,
        WorkflowHandlerOutcome::Completed(result) => panic!("expected a park: {result:?}"),
    }
}

/// Row 10: a dead-lettered email fails the receipt (the accepted sibling retained), the step
/// takes the final-error writer and nothing is resent, on resume or on replay.
#[tokio::test]
async fn workflow_message_receipt_failed_delivery_takes_final_error_and_never_resends() {
    let setup = Box::pin(send(&[To::Channel, To::User])).await;
    parked(&setup).await;
    let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
    let delivery = delivery(setup.pool(), run, 1).await;
    deliver(setup.pool(), delivery, "dead").await;
    assert_eq!(
        events(setup.pool(), run).await,
        vec![(1, "sending".into(), "dead_letter".into())]
    );
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), run).await, "failed");
    let receipt: Value =
        sqlx::query_scalar("SELECT result FROM workflow_action_receipts WHERE run_id=$1")
            .bind(run.as_uuid())
            .fetch_one(setup.pool())
            .await
            .unwrap();
    assert_eq!(receipt["status"], "failed");
    assert_eq!(receipt["messages"][0]["status"], "accepted");
    assert_eq!(receipt["messages"][1]["status"], "failed");
    assert_eq!(retry_safe(setup.pool(), scope).await, Some(true));
    let invocation = invocations(setup.pool(), run).await[0];
    assert_eq!(
        effect_state(setup.pool(), invocation).await,
        ("committed".into(), false)
    );
    let progress = p.resume_wait(scope).await.unwrap();
    let WaitProgress::Completed(step) = progress else {
        panic!("a failed receipt completes the wait through the final-error writer: {progress:?}");
    };
    assert_eq!(
        step.route,
        crate::domain::workflow::RouteSelection::FinalError
    );
    let after = state(setup.pool(), scope).await;
    assert_eq!(
        (
            after.run.as_str(),
            after.job.as_str(),
            after.wait.as_deref()
        ),
        ("failed", "failed", Some("completed"))
    );
    // The closed job replays the persisted final outcome without another delivery.
    let WaitProgress::Completed(replay) = p.resume_wait(scope).await.unwrap() else {
        panic!("completed effect wait must replay its final outcome");
    };
    assert_eq!(
        replay.disposition,
        crate::application::workflow::completion::CommitDisposition::Replayed
    );
    assert_eq!(
        replay.route,
        crate::domain::workflow::RouteSelection::FinalError
    );
    assert!(replay.successor.is_none());
    let row = delivery_row(setup.pool(), delivery).await;
    assert_eq!((row.status.as_str(), row.attempt_count), ("dead_letter", 1));
    assert_eq!(rows(setup.pool(), run).await.deliveries, 1);
    assert!(candidates(&setup).await.is_empty());
}

/// Row 11: an unknown delivery leaves the action uncertain (no receipt), recorded for
/// reconciliation; the parked step is not rediscovered until the run deadline fails it.
#[tokio::test]
async fn workflow_message_receipt_unknown_is_uncertain_until_the_deadline() {
    let setup = Box::pin(send(&[To::User])).await;
    parked(&setup).await;
    let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
    let invocation = invocations(setup.pool(), run).await[0];
    assert_eq!(
        effect_state(setup.pool(), invocation).await.0,
        "possible_dispatch"
    );
    assert_eq!(retry_safe(setup.pool(), scope).await, Some(false));
    deliver(
        setup.pool(),
        delivery(setup.pool(), run, 0).await,
        "unknown",
    )
    .await;
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), run).await, "uncertain");
    let observation: String = sqlx::query_scalar(
        "SELECT failure_code FROM workflow_action_reconciliations WHERE invocation_id=$1 AND effect_kind='message'",
    )
    .bind(invocation).fetch_one(setup.pool()).await.unwrap();
    assert_eq!(observation, "message.outcome_unknown");
    assert_eq!(
        effect_state(setup.pool(), invocation).await.0,
        "needs_reconciliation"
    );
    assert_eq!(rows(setup.pool(), run).await.receipts, 0);
    assert!(candidates(&setup).await.is_empty());
    assert!(candidates(&setup).await.is_empty());
    sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp() WHERE id=$1")
        .bind(run.as_uuid())
        .execute(setup.pool())
        .await
        .unwrap();
    assert_eq!(candidates(&setup).await, vec![PollWork::ExpiredRun]);
    assert!(p.expire_run(scope).await.unwrap());
    let after = state(setup.pool(), scope).await;
    assert_eq!(
        (after.run.as_str(), after.wait.as_deref()),
        ("failed", Some("expired"))
    );
    assert_eq!(
        effect_state(setup.pool(), invocation).await.0,
        "needs_reconciliation"
    );
    assert_eq!(rows(setup.pool(), run).await.receipts, 0);
}

/// Row 13 and D2 §6: cancellation supersedes unsent deliveries and keeps the in-flight one
/// with only the intent; its late acceptance is recorded as truth without advancing.
#[tokio::test]
async fn workflow_message_wait_cancel_supersedes_unsent_and_records_late_truth() {
    // Two email destinations of one parked step: one in flight, one unsent. (A saved model
    // tool delivery parks its own job, so it can no longer be an unsent sibling here.)
    let setup = Box::pin(setup_with_owner(|_, _, user, owner| {
        (
            "message.send",
            json!({"content":{"literal":{"subject":"Status","body":"Two recipients."}},
                   "destinations":{"literal":[{"kind":"user","id":user.to_string()},{"kind":"user","id":owner.to_string()}]}}),
        )
    }))
    .await;
    let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
    parked(&setup).await;
    let first = delivery(setup.pool(), run, 0).await;
    let sibling = delivery(setup.pool(), run, 1).await;
    // The first is in flight: claimed alone, its request started.
    let claimed = claim(p, first).await;
    use crate::transport::DeliveryQueue;
    assert!(
        p.begin_part(&claimed.lease, claimed.parts[0].id)
            .await
            .unwrap()
            .lease_held()
    );
    cancel(&setup, "cancel-messages").await;
    let unsent = delivery_row(setup.pool(), sibling).await;
    assert_eq!(
        (
            unsent.status.as_str(),
            unsent.last_error_class.as_deref(),
            unsent.cancellation_reason.as_deref(),
            unsent.part.as_str()
        ),
        (
            "dead_letter",
            Some("superseded"),
            Some("workflow_cancelled"),
            "dead"
        )
    );
    let in_flight = delivery_row(setup.pool(), first).await;
    assert_eq!(
        (
            in_flight.status.as_str(),
            in_flight.cancellation_reason.as_deref()
        ),
        ("sending", Some("workflow_cancelled"))
    );
    // The provider accepted the in-flight request after all: truth is recorded.
    let settled = p
        .complete_part(crate::transport::PartResult {
            fence: &claimed.lease,
            part_id: claimed.parts[0].id,
            outcome: &crate::transport::ProviderSendOutcome::Delivered { provider_key: None },
        })
        .await
        .unwrap();
    assert!(
        matches!(
            settled,
            crate::transport::DeliveryOutcome::Cancelled(
                crate::entities::transport::DeliveryStatus::Delivered
            )
        ),
        "{settled:?}"
    );
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    let states: Vec<String> = sqlx::query_scalar(
        "SELECT state FROM workflow_message_actions WHERE run_id=$1 ORDER BY origin DESC",
    )
    .bind(run.as_uuid())
    .fetch_all(setup.pool())
    .await
    .unwrap();
    assert_eq!(states, vec!["failed".to_string()]);
    let after = state(setup.pool(), scope).await;
    assert_eq!(
        (after.run.as_str(), after.wait.as_deref()),
        ("cancelled", Some("cancelled"))
    );
    let successors: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_executions WHERE run_id=$1")
            .bind(run.as_uuid())
            .fetch_one(setup.pool())
            .await
            .unwrap();
    assert_eq!(successors, 1);
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Refused
    ));
}

/// D4: saved model-tool deliveries are sequential. Each parks the job on its own effect wait;
/// its receipt continues the job (continuation only) and a fresh fence publishes the next.
#[tokio::test]
async fn workflow_message_model_tool_deliveries_park_and_continue_sequentially() {
    let mut setup = Box::pin(send(&[To::Channel])).await;
    let run = setup.scope().run;
    for call in 0..9 {
        let observed = Box::pin(tool(
            &setup,
            &format!("page-{call}"),
            send_arguments(&[("user", setup.user)], &format!("Body {call}")),
        ))
        .await
        .unwrap();
        let crate::application::workflow::message::MessageObservation::DeliveryParked(_) = observed
        else {
            panic!("model-tool delivery must park");
        };
        // A second publication in the parked attempt has no fence.
        assert!(
            Box::pin(tool(
                &setup,
                "late",
                send_arguments(&[("user", setup.user)], "Late")
            ))
            .await
            .is_err()
        );
        let after = state(setup.pool(), setup.scope()).await;
        assert_eq!(
            (after.run.as_str(), after.job.as_str()),
            ("waiting", "completed")
        );
        let delivery = latest_delivery(setup.pool(), run).await;
        Box::pin(continue_delivery(&mut setup, delivery, "delivered")).await;
    }
    let (accepted, waits, continuations): (i64, i64, i16) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_message_actions WHERE run_id=$1 AND origin='model_tool' AND state='accepted'),
                (SELECT count(*) FROM workflow_waits WHERE run_id=$1 AND reason='effect' AND state='completed'),
                (SELECT continuation_count FROM background_tasks WHERE workflow_execution_id=(SELECT execution_id FROM workflow_waits WHERE run_id=$1 LIMIT 1))",
    )
    .bind(run.as_uuid())
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert_eq!((accepted, waits, continuations), (9, 9, 9));
    // Replay of an earlier saved call reads its receipt; nothing is republished.
    let replay = Box::pin(tool(
        &setup,
        "page-0",
        send_arguments(&[("user", setup.user)], "Body 0"),
    ))
    .await
    .unwrap();
    assert!(matches!(
        replay,
        crate::application::workflow::message::MessageObservation::Committed(_)
    ));
    let deliveries: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_message_destinations WHERE run_id=$1")
            .bind(run.as_uuid())
            .fetch_one(setup.pool())
            .await
            .unwrap();
    assert_eq!(deliveries, 9);
}

/// F002 poison: a failing page is not reclaimed without advancing time, and stalls at five.
#[tokio::test]
async fn workflow_message_receipt_poison_backs_off_and_stalls_at_five() {
    let setup = Box::pin(send(&[To::Channel])).await;
    let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
    // One saved model-tool delivery parks the job (D4); its receipt consumption is poisoned.
    Box::pin(tool(
        &setup,
        "poison-0",
        send_arguments(&[("user", setup.user)], "Body 0"),
    ))
    .await
    .unwrap();
    let deliveries: Vec<Uuid> =
        sqlx::query_scalar("SELECT delivery_id FROM workflow_message_destinations WHERE run_id=$1")
            .bind(run.as_uuid())
            .fetch_all(setup.pool())
            .await
            .unwrap();
    for delivery in deliveries {
        deliver(setup.pool(), delivery, "delivered").await;
    }
    // A consumer defect: every cursor advance fails; the poison bookkeeping does not.
    sqlx::raw_sql(
        "CREATE FUNCTION fixture_consume_defect() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
           IF NEW.consumed_through<>OLD.consumed_through THEN RAISE EXCEPTION 'injected consumer defect'; END IF; RETURN NEW; END $$; \
         CREATE TRIGGER fixture_consume_defect BEFORE UPDATE ON workflow_message_actions FOR EACH ROW EXECUTE FUNCTION fixture_consume_defect();",
    )
    .execute(setup.pool())
    .await
    .unwrap();
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 0);
    assert!(candidates(&setup).await.is_empty());
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 0);
    let failures: Vec<i16> = sqlx::query_scalar("SELECT consume_failures FROM workflow_message_actions WHERE run_id=$1 AND origin='model_tool'")
        .bind(run.as_uuid()).fetch_all(setup.pool()).await.unwrap();
    assert_eq!(failures, vec![1]);
    for _ in 0..4 {
        sqlx::query("UPDATE workflow_message_actions SET consume_after=clock_timestamp() WHERE run_id=$1 AND state='pending'")
            .bind(run.as_uuid()).execute(setup.pool()).await.unwrap();
        assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 0);
    }
    let stalled: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_message_actions WHERE run_id=$1 AND state='stalled' AND consume_failures=5")
        .bind(run.as_uuid()).fetch_one(setup.pool()).await.unwrap();
    assert_eq!(stalled, 1);
    sqlx::query(
        "UPDATE workflow_message_actions SET consume_after=clock_timestamp() WHERE run_id=$1",
    )
    .bind(run.as_uuid())
    .execute(setup.pool())
    .await
    .unwrap();
    // Stalled is final for the consumer: no backoff reset makes it a candidate again.
    assert!(candidates(&setup).await.is_empty());
    let invocation = *invocations(setup.pool(), run).await.last().unwrap();
    assert_eq!(
        effect_state(setup.pool(), invocation).await.0,
        "needs_reconciliation"
    );
}

/// D2 §6: the run deadline supersedes unsent deliveries with `workflow_expired`.
#[tokio::test]
async fn workflow_message_wait_deadline_supersedes_unsent_deliveries() {
    let setup = Box::pin(send(&[To::User])).await;
    parked(&setup).await;
    let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
    let unsent = delivery(setup.pool(), run, 0).await;
    sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp() WHERE id=$1")
        .bind(run.as_uuid())
        .execute(setup.pool())
        .await
        .unwrap();
    assert!(p.expire_run(scope).await.unwrap());
    let row = delivery_row(setup.pool(), unsent).await;
    assert_eq!(
        (
            row.status.as_str(),
            row.cancellation_reason.as_deref(),
            row.attempt_count,
            row.part.as_str()
        ),
        ("dead_letter", Some("workflow_expired"), 0, "dead")
    );
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), run).await, "failed");
    let after = state(setup.pool(), scope).await;
    assert_eq!(
        (after.run.as_str(), after.wait.as_deref()),
        ("failed", Some("expired"))
    );
}
