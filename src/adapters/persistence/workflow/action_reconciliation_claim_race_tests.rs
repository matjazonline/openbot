//! Actual reservation/claim competitors queued in both shared-usage lock orders.
use super::*;
use std::{future::Future, pin::Pin};

#[derive(Clone, Copy, Debug)]
enum UsageOrder {
    DebitFirst,
    ClaimFirst,
    DebitRollback,
}

struct ScheduledRace {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    command: ReconcileActionCommand,
    descendant: WorkflowFence,
}

async fn scheduled() -> ScheduledRace {
    // Keep the real fixture/action service boundaries off the stock 2 MiB stack.
    let (f, request) = Box::pin(limited()).await;
    let verifier = ledger(&f, &request).await;
    let provider = LedgerProvider::new(&f, Delivery::Pending);
    assert!(invoke(&f, &request, &provider).await.is_err());
    park(&f, &request).await;
    barrier(&f, &request).await;
    let command = proof_command(&f, &request, &verifier, "claim-budget-race").await;
    assert_eq!(
        run_command(&f, &command, verifier).await.unwrap().outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let scope = child(&f, request.fence.scope).await;
    let descendant = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap()
        .fence;
    ScheduledRace {
        fixture: f,
        request,
        command,
        descendant,
    }
}

async fn wait_for_lock<T: std::fmt::Debug>(
    observer: &mut sqlx::PgConnection,
    blocker: i32,
    statement: &str,
    mut contender: Pin<&mut impl Future<Output = T>>,
) -> i32 {
    let observed = async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()")
                .execute(&mut *observer)
                .await
                .unwrap();
            let pid = sqlx::query_scalar::<_, i32>(
                "SELECT activity.pid FROM pg_stat_activity AS activity WHERE activity.datname=current_database() AND activity.pid<>pg_backend_pid() AND activity.state='active' AND activity.wait_event_type='Lock' AND position($1 IN activity.query)>0 AND $2=ANY(pg_blocking_pids(activity.pid))",
            )
            .bind(statement)
            .bind(blocker)
            .fetch_optional(&mut *observer)
            .await
            .unwrap();
            if let Some(pid) = pid {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    // Drive the actual API until its statement is demonstrably blocked. The
    // predicate has its own deadline; elapsed time alone never proves overlap.
    let result = tokio::select! {
        result = contender.as_mut() => Err(format!("contender completed before its expected lock wait: {result:?}")),
        result = tokio::time::timeout(Duration::from_secs(1), observed) => result.map_err(|_| "contender did not reach its exact PostgreSQL lock wait".to_owned()),
    };
    match result {
        Ok(pid) => pid,
        Err(reason) => {
            sqlx::query("SELECT pg_stat_clear_snapshot()")
                .execute(&mut *observer)
                .await
                .unwrap();
            let waits: Value = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(jsonb_build_object('pid',activity.pid,'query',activity.query,'wait',activity.wait_event_type,'blockers',pg_blocking_pids(activity.pid))),'[]'::jsonb) FROM pg_stat_activity AS activity WHERE activity.datname=current_database() AND activity.pid<>pg_backend_pid()")
                .fetch_one(observer).await.unwrap();
            panic!("{reason}; expected blocker {blocker}, statement {statement}; activity {waits}");
        }
    }
}

async fn compete(race: &ScheduledRace, order: UsageOrder) -> Option<WorkflowFence> {
    let f = &race.fixture;
    let before = all_tables(f).await;
    let mut gate = f.persistence().pool().begin().await.unwrap();
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT root_run_id FROM workflow_root_budget_usage WHERE company_id=$1 AND root_run_id=$2 FOR UPDATE")
        .bind(race.request.scope().company.as_uuid())
        .bind(race.request.scope().run.as_uuid())
        .execute(&mut *gate).await.unwrap();
    let reservation = BudgetReservation::new(
        race.descendant,
        BudgetReservationKey::parse("actual-racing-debit").unwrap(),
        BudgetCharge::new(BudgetResource::ModelCall, 2).unwrap(),
    )
    .unwrap();
    let debit = f.persistence().reserve_budget(reservation, policy());
    let first = f
        .persistence()
        .claim_io(race.request.fence.scope, worker(), policy());
    let second = f
        .persistence()
        .claim_io(race.request.fence.scope, worker(), policy());
    tokio::pin!(debit, first, second);
    let claim_statement = "SELECT usage.root_run_id FROM workflow_root_budget_usage";
    let debit_statement = "INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition)";
    let first_pid = match order {
        UsageOrder::DebitFirst | UsageOrder::DebitRollback => {
            let debit_pid =
                wait_for_lock(&mut observer, gate_pid, debit_statement, debit.as_mut()).await;
            wait_for_lock(&mut observer, debit_pid, claim_statement, first.as_mut()).await
        }
        UsageOrder::ClaimFirst => {
            let pid = wait_for_lock(&mut observer, gate_pid, claim_statement, first.as_mut()).await;
            wait_for_lock(&mut observer, pid, debit_statement, debit.as_mut()).await;
            pid
        }
    };
    wait_for_lock(
        &mut observer,
        first_pid,
        "SELECT binding_id, max_context_bytes FROM workflow_runs",
        second.as_mut(),
    )
    .await;
    drop(observer);
    gate.rollback().await.unwrap();
    let (debit, first, second) = tokio::join!(debit, first, second);
    match order {
        UsageOrder::DebitRollback => {
            let error = debit.unwrap_err();
            assert!(
                error.to_string().contains("injected racing debit commit"),
                "{error:?}"
            );
        }
        _ => assert_eq!(
            debit.unwrap(),
            BudgetReservationResult::Recorded(BudgetDisposition::Granted)
        ),
    }
    let claims = [first.unwrap(), second.unwrap()];
    let after = all_tables(f).await;
    assert_race_history(&before, &after, race, order, &claims);
    claims.into_iter().flatten().next().map(|claim| claim.fence)
}

fn assert_race_history(
    before: &Value,
    after: &Value,
    race: &ScheduledRace,
    order: UsageOrder,
    claims: &[Option<ClaimedWorkflow>],
) {
    let expected_claims = usize::from(!matches!(order, UsageOrder::DebitFirst));
    assert_eq!(
        claims.iter().flatten().count(),
        expected_claims,
        "{order:?}"
    );
    let attempts_before = before["task_attempts"].as_array().unwrap();
    let attempts_after = after["task_attempts"].as_array().unwrap();
    assert_eq!(
        attempts_after.len(),
        attempts_before.len() + expected_claims
    );
    for attempt in attempts_before {
        assert!(attempts_after.contains(attempt), "old attempt must survive");
    }
    for table in [
        "workflow_executions",
        "workflow_action_claim_episodes",
        "workflow_action_evidence_commands",
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_receipts",
        "workflow_action_intents",
        "workflow_action_dispatches",
        "workflow_action_remote_entries",
        "workflow_action_evidence_consumptions",
        "fixture_provider_operations",
        "fixture_evidence_ledger",
        "fixture_provider_effects",
    ] {
        assert_eq!(before[table], after[table], "{order:?} preserves {table}");
    }
    let receipts_before = before["workflow_budget_receipts"].as_array().unwrap();
    let receipts_after = after["workflow_budget_receipts"].as_array().unwrap();
    for receipt in receipts_before {
        assert!(receipts_after.contains(receipt));
    }
    let debit_count = usize::from(!matches!(order, UsageOrder::DebitRollback));
    assert_eq!(receipts_after.len(), receipts_before.len() + debit_count);
    let mut usage = before["workflow_root_budget_usage"].clone();
    usage[0]["model_calls"] = json!(2 * debit_count);
    assert_eq!(
        usage, after["workflow_root_budget_usage"],
        "claim never debits"
    );
    if matches!(order, UsageOrder::DebitFirst) {
        // Normalize only the genuine descendant debit to reuse exact refusal checks.
        let mut after_debit = before.clone();
        after_debit["workflow_budget_receipts"] = after["workflow_budget_receipts"].clone();
        after_debit["workflow_root_budget_usage"] = after["workflow_root_budget_usage"].clone();
        assert_refusal(&after_debit, after, &race.request, &race.command);
    } else {
        assert_eq!(
            before["workflow_action_claim_budget_refusals"],
            after["workflow_action_claim_budget_refusals"]
        );
        assert_eq!(
            before["workflow_action_claim_retirement_witnesses"],
            after["workflow_action_claim_retirement_witnesses"]
        );
        assert_eq!(before["workflow_run_events"], after["workflow_run_events"]);
    }
}

async fn dispatch_claim(race: &mut ScheduledRace, fence: WorkflowFence) {
    let f = &race.fixture;
    race.request.fence = fence;
    let before = all_tables(f).await;
    assert_eq!(entry_consumptions(f).await, (1, 0));
    let provider = LedgerProvider::new(
        f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    );
    let receipt = invoke(f, &race.request, &provider).await.unwrap();
    assert!(matches!(receipt, RemoteDispatchObservation::Committed(_)));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(f).await, (2, 1));
    assert_eq!(effects(f).await, 1);
    assert!(matches!(
        invoke(f, &race.request, &provider).await.unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(f).await, (2, 1));
    let after = all_tables(f).await;
    for table in [
        "workflow_budget_receipts",
        "workflow_root_budget_usage",
        "task_attempts",
        "workflow_action_claim_episodes",
    ] {
        assert_eq!(before[table], after[table], "dispatch preserves {table}");
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_race_debit_first_refuses_both_claimants() {
    let race = Box::pin(scheduled()).await;
    assert!(
        Box::pin(compete(&race, UsageOrder::DebitFirst))
            .await
            .is_none()
    );
    let before = all_tables(&race.fixture).await;
    assert!(
        race.fixture
            .persistence()
            .claim_io(race.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(before, all_tables(&race.fixture).await);
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_race_claim_first_dispatches_once() {
    let mut race = Box::pin(scheduled()).await;
    let fence = Box::pin(compete(&race, UsageOrder::ClaimFirst))
        .await
        .unwrap();
    Box::pin(dispatch_claim(&mut race, fence)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_race_rolled_back_debit_preserves_allowance() {
    let mut race = Box::pin(scheduled()).await;
    sqlx::raw_sql("CREATE FUNCTION reject_racing_debit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.reservation_key='actual-racing-debit' THEN RAISE EXCEPTION 'injected racing debit commit'; END IF; RETURN NEW; END $$; CREATE CONSTRAINT TRIGGER reject_racing_debit AFTER INSERT ON workflow_budget_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_racing_debit()")
        .execute(race.fixture.persistence().pool()).await.unwrap();
    let fence = Box::pin(compete(&race, UsageOrder::DebitRollback))
        .await
        .unwrap();
    Box::pin(dispatch_claim(&mut race, fence)).await;
}
