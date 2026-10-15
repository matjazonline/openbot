//! 04.8 group B3: an actual action receipt, then actual shared completion against an actual
//! CancelCommand in both orders with the exact run blockers observed (DEC-002/005); the ordinary
//! completion attempt told apart from ambiguous-COMMIT recovery (R-008/DEC-012); and cancelled
//! SafeRepeat/ProviderIdempotency history refused at the terminal fence (DEC-001).
use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::{Injected, Observed};
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::cancellation_order_tests::{
    RunLock, assert_drained, blocked, probe_run,
};
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{
    Gate, HANG_GUARD, Stall, connect, gated_policy, resolution_behind,
};
use crate::application::workflow::RunRevision;
use crate::application::workflow::completion::{
    CommitDisposition, CommittedWorkflowStep, WorkflowCompletion,
};
use crate::domain::workflow::{RouteSelection, StepId, TransitionTarget};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug)]
enum Route {
    /// The step routes to `$end`: a committed completion makes the run terminal.
    End,
    /// The step routes to `second`: a committed completion schedules a successor.
    Successor,
}

/// The same single step, routed `success -> second` (as the claim-SQL two-step fixture does).
fn successor_source() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":2,"repetitions":2});
    source["steps"]["second"] = source["steps"]["start"].clone();
    source["steps"]["start"]["routes"]["success"] = json!("second");
    source
}

async fn route_fixture(route: Route) -> (AdmissionFixture, ActionDispatchRequest) {
    match route {
        Route::End => {
            let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
            (f, request)
        }
        Route::Successor => {
            let (f, scope) = Box::pin(fixture_source(successor_source())).await;
            let (f, request) = Box::pin(setup_action_on_fixture(
                f,
                scope,
                ActionRecovery::Reconcile,
                json!({"value":1}),
            ))
            .await;
            sqlx::raw_sql("CREATE TABLE fixture_remote_effects (provider_key text PRIMARY KEY)")
                .execute(f.persistence().pool())
                .await
                .unwrap();
            (f, request)
        }
    }
}

/// The one canonical receipt and actual observation, bound to the exact invocation and result.
fn receipt_kept(state: &Value, request: &ActionDispatchRequest) {
    for table in [
        "workflow_action_receipts",
        "workflow_action_actual_receipt_observations",
    ] {
        let rows = rows(state, table);
        assert_eq!(rows.len(), 1, "{table}");
        assert_eq!(rows[0]["result"], step_output(), "{table}");
        assert_eq!(
            rows[0]["invocation_id"],
            json!(request.subject.invocation.as_uuid()),
            "{table}"
        );
    }
}

/// DEC-005 premise: the actual service commits the receipt for a result valid as the step's
/// output, and that alone has not completed the step. The fence is then freshly renewed.
async fn receipted<'a>(
    f: &'a AdmissionFixture,
    request: &ActionDispatchRequest,
) -> DeduplicatingProvider<'a> {
    let provider = provider(f);
    heartbeat(f, request).await;
    let RemoteDispatchObservation::Committed(receipt) = ActionService::new(adapter(f, 0))
        .dispatch_remote(request, &provider, &CancellationToken::new())
        .await
        .unwrap()
    else {
        panic!("a live owner commits its receipt")
    };
    assert_eq!(receipt.result, step_output());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effect_count(f).await, 1);
    let state = all_tables(f).await;
    receipt_kept(&state, request);
    assert!(
        rows(&state, "workflow_executions")[0]["completed_at"].is_null(),
        "receipt commitment alone has not completed the step"
    );
    heartbeat(f, request).await;
    provider
}

fn row_with<'a>(state: &'a Value, table: &str, column: &str, id: Uuid) -> &'a Value {
    rows(state, table)
        .iter()
        .find(|row| row[column] == json!(id))
        .unwrap_or_else(|| panic!("{table}.{column}={id}"))
}

fn events_of(state: &Value, kind: &str) -> usize {
    rows(state, "workflow_run_events")
        .iter()
        .filter(|row| row["event_kind"] == kind)
        .count()
}

#[derive(Clone, Copy, Debug)]
enum First {
    Cancel,
    Completion,
}

struct Race {
    completion: Option<CommittedWorkflowStep>,
    cancel: CancelResult,
    /// The revision the competing command was built from (after the receipt).
    expected: RunRevision,
    /// Every bounded operation the race finished, from the test-only recorder.
    events: Vec<Observed>,
}

/// DEC-002 for the run-first completion owner. A gate holds only the exact execution row.
/// Cancel-first: the actual cancellation stops at its execution lock owning company and run,
/// and the actual completion waits on that exact run behind it. Completion-first: the actual
/// completion stops at its execution lock owning the run, and the actual cancellation (company
/// already authorized) waits at its head/run query behind it.
async fn race(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    first: First,
    injected: &Injected,
) -> Race {
    let command = cancel_command(f, request, "b3-completion-race").await;
    let expected = command.expected_revision;
    let mut gate = f.persistence().pool().begin().await.unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().execution.as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    probe_run(&mut observer, request, RunLock::Free).await;
    let recorded = injected.outcomes().len();
    let started = tokio::time::Instant::now();
    let completion = f.persistence().complete_io(FencedWorkflowResult {
        policy: policy(),
        fence: request.fence,
        output: step_output(),
    });
    let cancel = f.persistence().cancel(command);
    tokio::pin!(completion, cancel);
    let (first_pid, second_pid) = match first {
        First::Cancel => {
            let cancel_pid = blocked(
                &mut observer,
                gate_pid,
                "SELECT id FROM workflow_executions",
                cancel.as_mut(),
            )
            .await;
            probe_run(&mut observer, request, RunLock::Held).await;
            let completion_pid = tokio::select! {
                result = cancel.as_mut() => panic!("cancel escaped its execution gate: {result:?}"),
                pid = blocked(&mut observer, cancel_pid, "SELECT binding_id,max_steps FROM workflow_runs", completion.as_mut()) => pid,
            };
            (cancel_pid, completion_pid)
        }
        First::Completion => {
            let completion_pid = blocked(
                &mut observer,
                gate_pid,
                "SELECT completed_at,committed_output",
                completion.as_mut(),
            )
            .await;
            probe_run(&mut observer, request, RunLock::Held).await;
            let cancel_pid = tokio::select! {
                result = completion.as_mut() => panic!("completion escaped its execution gate: {result:?}"),
                pid = blocked(&mut observer, completion_pid, "FROM workflow_runs AS run", cancel.as_mut()) => pid,
            };
            (completion_pid, cancel_pid)
        }
    };
    assert_ne!(first_pid, second_pid);
    gate.rollback().await.unwrap();
    let released_ms = started.elapsed().as_millis();
    let (completion, cancel) =
        tokio::time::timeout(HANG_GUARD, async { tokio::join!(completion, cancel) })
            .await
            .expect("both real operations drain");
    probe_run(&mut observer, request, RunLock::Free).await;
    assert_drained(&mut observer, first_pid, second_pid).await;
    let race = Race {
        completion: completion.unwrap(),
        cancel: cancel.unwrap(),
        expected,
        events: injected.outcomes()[recorded..].to_vec(),
    };
    eprintln!(
        "completion race first={first:?} gate_released_ms={released_ms} drained_ms={} cancel={:?} events={:?}",
        started.elapsed().as_millis(),
        race.cancel,
        race.events
    );
    race
}

/// R-008/DEC-012: the race's completion was the ordinary primary attempt, exactly one
/// `completion` operation that `completed`. A resolved ambiguous COMMIT (`resolved_applied`,
/// whose winner replays the saved step as `Replayed`), an unknown, or a not-applied attempt
/// followed by a retry belongs to the separate ambiguity contract and cannot pass.
fn ordinary_completion(
    events: &[Observed],
    winner: Option<&CommittedWorkflowStep>,
) -> Result<(), String> {
    let attempts: Vec<&Observed> = events
        .iter()
        .filter(|seen| seen.op == "completion")
        .collect();
    if attempts.len() != 1 || attempts[0].outcome != "completed" {
        return Err(format!(
            "not one completed primary completion attempt: {attempts:?}"
        ));
    }
    match winner {
        Some(step) if step.disposition != CommitDisposition::Committed => Err(format!(
            "the winning step was {:?}, not committed by the primary attempt",
            step.disposition
        )),
        _ => Ok(()),
    }
}

/// Cancel commits first: the receipt, observation and effect stay; nothing advances the step,
/// and no successor is scheduled even where the route has one.
fn cancel_won(state: &Value, request: &ActionDispatchRequest) {
    receipt_kept(state, request);
    assert_eq!(rows(state, "workflow_runs")[0]["state"], "cancelled");
    for table in ["workflow_executions", "background_tasks", "task_attempts"] {
        assert_eq!(
            rows(state, table).len(),
            1,
            "no successor or new attempt: {table}"
        );
    }
    let execution = &rows(state, "workflow_executions")[0];
    for field in [
        "completed_at",
        "committed_output",
        "committed_route",
        "route_target",
        "successor_execution_id",
    ] {
        assert!(
            execution[field].is_null(),
            "no cancelled advancement: {field}"
        );
    }
    assert_eq!(
        rows(state, "task_attempts")[0]["workflow_retirement"],
        "cancel"
    );
    assert_eq!(events_of(state, "io_step_completed"), 0);
    let commands = rows(state, "workflow_control_commands");
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0]["operation"], "cancel");
    assert_eq!(commands[0]["result"], "applied");
}

/// Completion commits first: its committed result stands, and the command built from the
/// earlier revision is exactly a revision conflict whose only write is its own receipt.
fn completion_won(
    state: &Value,
    request: &ActionDispatchRequest,
    step: &CommittedWorkflowStep,
    route: Route,
    race: &Race,
) {
    assert_eq!(step.disposition, CommitDisposition::Committed);
    assert_eq!(step.output, step_output());
    assert_eq!(step.route, RouteSelection::Success);
    receipt_kept(state, request);
    let CancelResult::RevisionConflict { current_revision } = race.cancel else {
        panic!(
            "the stale command reports a revision conflict: {:?}",
            race.cancel
        )
    };
    assert!(current_revision.0 > race.expected.0);
    let run = &rows(state, "workflow_runs")[0];
    assert_eq!(run["revision"], json!(current_revision.0));
    let predecessor = row_with(
        state,
        "workflow_executions",
        "id",
        request.scope().execution.as_uuid(),
    );
    assert!(!predecessor["completed_at"].is_null());
    assert_eq!(predecessor["committed_output"], step_output());
    assert_eq!(predecessor["committed_route"], "success");
    match route {
        Route::End => {
            assert_eq!(step.target, TransitionTarget::End);
            assert!(step.successor.is_none());
            assert_eq!(run["state"], "succeeded");
            assert_eq!(predecessor["route_target"], "$end");
            assert!(predecessor["successor_execution_id"].is_null());
            assert_eq!(rows(state, "workflow_executions").len(), 1);
        }
        Route::Successor => {
            assert_eq!(
                step.target,
                TransitionTarget::Step(StepId::parse("second").unwrap())
            );
            let successor = step.successor.expect("a scheduled successor");
            assert_ne!(successor.execution, request.scope().execution);
            assert_eq!(run["state"], "running");
            assert_eq!(predecessor["route_target"], "second");
            assert_eq!(
                predecessor["successor_execution_id"],
                json!(successor.execution.as_uuid())
            );
            assert_eq!(rows(state, "workflow_executions").len(), 2);
            let job = row_with(state, "background_tasks", "id", successor.job.0);
            assert_eq!(job["status"], "pending");
        }
    }
    let attempt = row_with(state, "task_attempts", "task_id", request.fence.scope.job.0);
    assert_eq!(attempt["status"], "completed");
    assert_eq!(events_of(state, "io_step_completed"), 1);
    assert_eq!(events_of(state, "control_cancel"), 0);
    let commands = rows(state, "workflow_control_commands");
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0]["operation"], "cancel");
    assert_eq!(commands[0]["result"], "conflict");
    assert_eq!(commands[0]["expected_revision"], json!(race.expected.0));
    assert_eq!(commands[0]["result_revision"], json!(current_revision.0));
}

/// The identical completion replays its committed result and duplicates nothing.
async fn replays_without_duplicate(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    step: &CommittedWorkflowStep,
) {
    let before = all_tables(f).await;
    let replayed = f
        .persistence()
        .settled()
        .complete_io(FencedWorkflowResult {
            policy: policy(),
            fence: request.fence,
            output: step_output(),
        })
        .await
        .unwrap()
        .expect("the committed step replays");
    assert_eq!(replayed.disposition, CommitDisposition::Replayed);
    assert_eq!(
        (
            &replayed.output,
            &replayed.route,
            &replayed.target,
            &replayed.successor
        ),
        (&step.output, &step.route, &step.target, &step.successor)
    );
    assert_eq!(all_tables(f).await, before, "replay writes nothing");
}

/// End route: a command at the current revision finds the run terminal and adds only its own
/// receipt; the committed result is not rewritten.
async fn terminal_command_keeps_result(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let before = all_tables(f).await;
    let command = cancel_command(f, request, "b3-terminal").await;
    let current = command.expected_revision;
    assert_eq!(
        f.persistence().cancel(command).await.unwrap(),
        CancelResult::AlreadyTerminalOrApplied { revision: current }
    );
    let after = all_tables(f).await;
    for (table, rows) in before.as_object().unwrap() {
        if table != "workflow_control_commands" {
            assert_eq!(
                &after[table], rows,
                "a terminal command rewrites nothing: {table}"
            );
        }
    }
    let (old, new) = (
        rows(&before, "workflow_control_commands"),
        rows(&after, "workflow_control_commands"),
    );
    assert_eq!(new.len(), old.len() + 1);
    let added = new.iter().find(|row| !old.contains(row)).unwrap();
    assert_eq!(added["result"], "terminal");
}

/// Successor route: a fresh, correctly revisioned cancel retires the remaining work but cannot
/// erase the predecessor's committed result or completed attempt.
async fn fresh_cancel_keeps_predecessor(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    step: &CommittedWorkflowStep,
) {
    let successor = step.successor.expect("a scheduled successor");
    let before = all_tables(f).await;
    cancel(f, request, "b3-fresh-cancel").await;
    let after = all_tables(f).await;
    for (table, column, id) in [
        (
            "workflow_executions",
            "id",
            request.scope().execution.as_uuid(),
        ),
        ("background_tasks", "id", request.fence.scope.job.0),
        ("task_attempts", "task_id", request.fence.scope.job.0),
    ] {
        assert_eq!(
            row_with(&after, table, column, id),
            row_with(&before, table, column, id),
            "the predecessor's committed truth stays: {table}"
        );
    }
    assert_eq!(rows(&after, "workflow_runs")[0]["state"], "cancelled");
    assert!(
        row_with(
            &after,
            "workflow_executions",
            "id",
            successor.execution.as_uuid()
        )["completed_at"]
            .is_null()
    );
    assert_eq!(
        row_with(&after, "background_tasks", "id", successor.job.0)["status"],
        "failed"
    );
    assert!(
        f.persistence()
            .settled()
            .claim_io(successor, worker(), policy())
            .await
            .unwrap()
            .is_none(),
        "the retired successor cannot be claimed"
    );
    assert_eq!(all_tables(f).await, after);
}

#[tokio::test]
async fn workflow_action_cancellation_completion_race_cancel_first() {
    let injected = Injected::mark();
    for route in [Route::End, Route::Successor] {
        let (f, request) = route_fixture(route).await;
        let provider = receipted(&f, &request).await;
        let race = race(&f, &request, First::Cancel, &injected).await;
        assert!(
            matches!(race.cancel, CancelResult::Applied { .. }),
            "{:?}",
            race.cancel
        );
        assert!(
            race.completion.is_none(),
            "{route:?}: shared completion under the cancelled fence"
        );
        ordinary_completion(&race.events, None).unwrap();
        let state = all_tables(&f).await;
        cancel_won(&state, &request);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(effect_count(&f).await, 1);
        inert(&f, &request, &state).await;
    }
}

#[tokio::test]
async fn workflow_action_cancellation_completion_race_completion_first() {
    let injected = Injected::mark();
    for route in [Route::End, Route::Successor] {
        let (f, request) = route_fixture(route).await;
        let provider = receipted(&f, &request).await;
        let race = race(&f, &request, First::Completion, &injected).await;
        let step = race
            .completion
            .clone()
            .unwrap_or_else(|| panic!("{route:?}: the completion committed first"));
        ordinary_completion(&race.events, Some(&step)).unwrap();
        completion_won(&all_tables(&f).await, &request, &step, route, &race);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(effect_count(&f).await, 1);
        replays_without_duplicate(&f, &request, &step).await;
        match route {
            Route::End => terminal_command_keeps_result(&f, &request).await,
            Route::Successor => fresh_cancel_keeps_predecessor(&f, &request, &step).await,
        }
    }
}

/// Calibrated negative control: completion-first where the completion's COMMIT is held until
/// its primary attempt elapsed and its ambiguous-COMMIT resolution queued on the run; the
/// actual cancellation then waits behind that resolution. The business outcome matches the
/// ordinary race, and the ordinary oracle must reject it.
#[tokio::test]
async fn workflow_action_cancellation_completion_race_resolved_commit_fails_ordinary_oracle() {
    let injected = Injected::mark();
    let (f, request) = route_fixture(Route::End).await;
    let _provider = receipted(&f, &request).await;
    // DEC-012: the gate policy's 2 s attempt, through genuine renewal, for this control only.
    let policy = gated_policy();
    assert!(
        f.persistence()
            .renew_io(request.fence, policy)
            .await
            .unwrap()
            .is_some()
    );
    let command = cancel_command(&f, &request, "b3-resolved-commit").await;
    let (mut observer, _) = connect(&f).await;
    probe_run(&mut observer, &request, RunLock::Free).await;
    let mut gate = Gate::arm(
        &f,
        "task_attempts",
        "UPDATE",
        "OLD.status='processing' AND NEW.status='completed'",
        Stall::Commit,
    )
    .await;
    let recorded = injected.outcomes().len();
    let done = AtomicBool::new(false);
    let completion = async {
        let result = f
            .persistence()
            .complete_io(FencedWorkflowResult {
                policy,
                fence: request.fence,
                output: step_output(),
            })
            .await;
        done.store(true, Ordering::SeqCst);
        result
    };
    let cancel = f.persistence().cancel(command);
    tokio::pin!(completion, cancel);
    let running = || !done.load(Ordering::SeqCst);
    let waiting: [&dyn Fn() -> bool; 1] = [&running];
    let completing = tokio::select! {
        result = completion.as_mut() => panic!("completion escaped its COMMIT gate: {result:?}"),
        pid = gate.stalled(&mut observer, &waiting) => pid,
    };
    probe_run(&mut observer, &request, RunLock::Held).await;
    let resolution = tokio::select! {
        result = completion.as_mut() => panic!("completion escaped its COMMIT gate: {result:?}"),
        pid = resolution_behind(&mut observer, completing, policy, &waiting) => pid,
    };
    let cancel_pid = tokio::select! {
        result = completion.as_mut() => panic!("completion escaped its COMMIT gate: {result:?}"),
        pid = blocked(&mut observer, resolution, "FROM workflow_runs AS run", cancel.as_mut()) => pid,
    };
    eprintln!(
        "resolved-commit control: completing={completing} resolution={resolution} cancel={cancel_pid}"
    );
    gate.release().await;
    let (completion, cancel) =
        tokio::time::timeout(HANG_GUARD, async { tokio::join!(completion, cancel) })
            .await
            .expect("both real operations drain");
    probe_run(&mut observer, &request, RunLock::Free).await;
    assert_drained(&mut observer, completing, cancel_pid).await;
    assert_drained(&mut observer, resolution, resolution).await;
    let step = completion.unwrap().expect("the stalled COMMIT applied");
    assert_eq!(step.output, step_output());
    assert!(
        matches!(cancel.unwrap(), CancelResult::RevisionConflict { .. }),
        "the same business outcome as the ordinary completion-first race"
    );
    let state = all_tables(&f).await;
    assert_eq!(rows(&state, "workflow_runs")[0]["state"], "succeeded");
    assert_eq!(
        rows(&state, "workflow_executions")[0]["committed_output"],
        step_output()
    );
    let events = injected.outcomes()[recorded..].to_vec();
    assert!(
        events
            .iter()
            .any(|seen| seen.op == "completion" && seen.outcome == "resolved_applied"),
        "the control exercised resolution: {events:?}"
    );
    assert_eq!(step.disposition, CommitDisposition::Replayed);
    let verdict = ordinary_completion(&events, Some(&step));
    assert!(
        verdict.is_err(),
        "a resolution path is not the ordinary race"
    );
    eprintln!("resolved-commit control rejected: {}", verdict.unwrap_err());
}

/// The oracle alone over constructed recorder events and winners.
#[test]
fn workflow_action_cancellation_completion_ordinary_oracle_discriminates_attempt_paths() {
    let attempt = |outcome: &'static str| Observed {
        op: "completion",
        outcome,
        cause: None,
    };
    let other = Observed {
        op: "action.commit_remote",
        outcome: "completed",
        cause: None,
    };
    let step = |disposition| CommittedWorkflowStep {
        disposition,
        output: step_output(),
        route: RouteSelection::Success,
        target: TransitionTarget::End,
        successor: None,
    };
    let (committed, replayed) = (
        step(CommitDisposition::Committed),
        step(CommitDisposition::Replayed),
    );
    assert!(ordinary_completion(&[other, attempt("completed")], None).is_ok());
    assert!(ordinary_completion(&[attempt("completed")], Some(&committed)).is_ok());
    for (case, events, winner) in [
        ("resolved applied", vec![attempt("resolved_applied")], None),
        (
            "resolved applied winner",
            vec![attempt("resolved_applied")],
            Some(&replayed),
        ),
        ("unknown", vec![attempt("unknown")], None),
        (
            "not applied then retried",
            vec![attempt("not_applied"), attempt("completed")],
            Some(&committed),
        ),
        ("no attempt recorded", vec![other], None),
        (
            "completed attempt but replayed winner",
            vec![attempt("completed")],
            Some(&replayed),
        ),
    ] {
        assert!(ordinary_completion(&events, winner).is_err(), "{case}");
    }
}

async fn retry_safe(f: &AdmissionFixture, request: &ActionDispatchRequest) -> Option<bool> {
    sqlx::query_scalar("SELECT workflow_action_retry_safe($1,$2)")
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().execution.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Cancelled {
    /// The lost-response history is still on the original attempt.
    BeforeReclaim,
    /// A genuine reclaim granted the live replay permission on attempt 2.
    AfterReclaim,
}

/// The live replay contract stays valid for this history but cannot override the durable
/// terminal state: no call, entry, attempt or claim follows the cancellation.
async fn replay_contract_refused(recovery: ActionRecovery, point: Cancelled) {
    let (f, mut request, contract) = receipt_fixture(recovery, true).await;
    let mut provider = dedup(&f, contract, true);
    provider.result = step_output();
    let service = ActionService::new(adapter(&f, 0));
    assert!(
        service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .is_err(),
        "the response is lost after the applied effect"
    );
    assert_eq!(effect_count(&f).await, 1);
    assert!(matches!(
        service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    assert_eq!(retry_safe(&f, &request).await, Some(true), "live premise");
    if matches!(point, Cancelled::AfterReclaim) {
        reclaim(&f, &mut request).await;
    }
    heartbeat(&f, &request).await;
    let before = all_tables(&f).await;
    cancel(&f, &request, "b3-replay-contract").await;
    let cancelled = all_tables(&f).await;
    retired_once(&before, &cancelled, &request);
    assert_eq!(
        retry_safe(&f, &request).await,
        Some(true),
        "{recovery:?}/{point:?}: the contract itself is unchanged"
    );
    for service in [service, ActionService::new(adapter(&f, 0))] {
        refused(
            service
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .map(|_| ()),
        );
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1, "no replay call");
    assert_eq!(effect_count(&f).await, 1);
    assert_eq!(rows(&cancelled, "workflow_action_remote_entries").len(), 1);
    assert_eq!(
        all_tables(&f).await,
        cancelled,
        "{recovery:?}/{point:?}: refused replay writes nothing"
    );
    inert(&f, &request, &cancelled).await;
}

#[tokio::test]
async fn workflow_action_cancellation_replay_contract_cannot_override_terminal() {
    for recovery in [
        ActionRecovery::SafeRepeat,
        ActionRecovery::ProviderIdempotency,
    ] {
        for point in [Cancelled::BeforeReclaim, Cancelled::AfterReclaim] {
            replay_contract_refused(recovery, point).await;
        }
    }
}
