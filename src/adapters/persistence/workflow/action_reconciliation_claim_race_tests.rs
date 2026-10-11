//! Actual reservation/claim competitors queued in both shared-usage lock orders.
use super::super::super::control_serialization_tests::contention::try_wait_for_lock;
use super::*;

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
    // Setup premise: the provider was called and its outcome is uncertain. A reservation
    // Unknown or a definite NotApplied leaves no call, so the test must not continue.
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
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
        .settled()
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

/// One race on fresh state. A racer whose own attempt bound elapsed while it was queued
/// (before or after the gate released) reports a definite NotApplied: the order under test
/// is then unproven, so the attempt is void. The backstop is asserted first: the NotApplied
/// racers wrote nothing and the claim stayed exclusive. Any other early completion fails as
/// before.
enum Contest {
    Proven(Option<WorkflowFence>),
    Void,
}

async fn compete(race: &ScheduledRace, order: UsageOrder, attempt: usize) -> Contest {
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
    let workers = [worker(), worker()];
    let debit = f.persistence().reserve_budget(reservation, policy());
    let first = f
        .persistence()
        .claim_io(race.request.fence.scope, workers[0], policy());
    let second = f
        .persistence()
        .claim_io(race.request.fence.scope, workers[1], policy());
    tokio::pin!(debit, first, second);
    let claim_statement = "SELECT usage.root_run_id FROM workflow_root_budget_usage";
    let debit_statement = "INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition)";
    let (mut early_debit, mut early_first, mut early_second) = (None, None, None);
    let early: Option<&str> = 'observe: {
        let first_pid = match order {
            UsageOrder::DebitFirst | UsageOrder::DebitRollback => {
                let debit_pid = match try_wait_for_lock(
                    &mut observer,
                    gate_pid,
                    debit_statement,
                    debit.as_mut(),
                )
                .await
                {
                    Ok(pid) => pid,
                    Err(output) => {
                        early_debit = Some(output);
                        break 'observe Some("debit");
                    }
                };
                match try_wait_for_lock(&mut observer, debit_pid, claim_statement, first.as_mut())
                    .await
                {
                    Ok(pid) => pid,
                    Err(output) => {
                        early_first = Some(output);
                        break 'observe Some("first");
                    }
                }
            }
            UsageOrder::ClaimFirst => {
                let pid = match try_wait_for_lock(
                    &mut observer,
                    gate_pid,
                    claim_statement,
                    first.as_mut(),
                )
                .await
                {
                    Ok(pid) => pid,
                    Err(output) => {
                        early_first = Some(output);
                        break 'observe Some("first");
                    }
                };
                if let Err(output) =
                    try_wait_for_lock(&mut observer, pid, debit_statement, debit.as_mut()).await
                {
                    early_debit = Some(output);
                    break 'observe Some("debit");
                }
                pid
            }
        };
        if let Err(output) = try_wait_for_lock(
            &mut observer,
            first_pid,
            "SELECT binding_id, max_context_bytes FROM workflow_runs",
            second.as_mut(),
        )
        .await
        {
            early_second = Some(output);
            break 'observe Some("second");
        }
        None
    };
    drop(observer);
    gate.rollback().await.unwrap();
    let (debit, first, second) = tokio::join!(
        async {
            match early_debit {
                Some(output) => output,
                None => debit.as_mut().await,
            }
        },
        async {
            match early_first {
                Some(output) => output,
                None => first.as_mut().await,
            }
        },
        async {
            match early_second {
                Some(output) => output,
                None => second.as_mut().await,
            }
        },
    );
    let not_applied = [
        matches!(debit, Err(AppError::DatabaseTimeout(_))),
        matches!(first, Err(AppError::DatabaseTimeout(_))),
        matches!(second, Err(AppError::DatabaseTimeout(_))),
    ];
    if let Some(racer) = early {
        let index = ["debit", "first", "second"]
            .iter()
            .position(|name| *name == racer)
            .unwrap();
        assert!(
            not_applied[index],
            "{racer} completed before its observed lock wait: debit={debit:?} first={first:?} second={second:?}"
        );
    }
    if not_applied.iter().any(|timed_out| *timed_out) {
        // Void only when the NotApplied racers are the only deviation: every other claimant
        // answered, and a debit that did not elapse has its expected outcome. An Unknown, a
        // deadlock or any other error beside a NotApplied still fails.
        for (index, claim) in [&first, &second].into_iter().enumerate() {
            assert!(
                not_applied[index + 1] || claim.is_ok(),
                "a void race must not hide another claimant's outcome: {claim:?}"
            );
        }
        if !not_applied[0] {
            let expected = match order {
                UsageOrder::DebitRollback => matches!(&debit, Err(error)
                    if error.to_string().contains("injected racing debit commit")),
                _ => matches!(
                    &debit,
                    Ok(BudgetReservationResult::Recorded(
                        BudgetDisposition::Granted
                    ))
                ),
            };
            assert!(
                expected,
                "a void race must not hide the debit's outcome: {debit:?}"
            );
        }
        void_race(
            race,
            order,
            attempt,
            &not_applied,
            workers,
            [&first, &second],
        )
        .await;
        return Contest::Void;
    }
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
    Contest::Proven(claims.into_iter().flatten().next().map(|claim| claim.fence))
}

async fn void_race(
    race: &ScheduledRace,
    order: UsageOrder,
    attempt: usize,
    not_applied: &[bool; 3],
    workers: [WorkflowWorkerId; 2],
    claims: [&AppResult<Option<ClaimedWorkflow>>; 2],
) {
    let pool = race.fixture.persistence().pool();
    if not_applied[0] {
        let debits: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workflow_budget_receipts WHERE reservation_key='actual-racing-debit'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(debits, 0, "a NotApplied debit wrote no receipt");
    }
    for (index, worker) in workers.into_iter().enumerate() {
        if not_applied[index + 1] {
            let attempts: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM task_attempts WHERE task_id=$1 AND worker_id=$2",
            )
            .bind(race.request.fence.scope.job.0)
            .bind(worker.0)
            .fetch_one(pool)
            .await
            .unwrap();
            assert_eq!(attempts, 0, "a NotApplied claimant wrote no attempt");
        }
    }
    assert!(
        claims
            .iter()
            .filter(|claim| matches!(claim, Ok(Some(_))))
            .count()
            <= 1,
        "the claim stays exclusive"
    );
    uncaptured(&format!(
        "VOID attempt scenario=claim_race order={order:?} attempt={attempt} not_applied=debit:{} first:{} second:{}",
        not_applied[0], not_applied[1], not_applied[2]
    ));
}

/// Runs `order` on at most 3 fresh races and returns the first proven one; every attempt
/// void fails the test, so the order is proven at least once per run.
async fn proven_race(
    order: UsageOrder,
    rolled_back: bool,
) -> (ScheduledRace, Option<WorkflowFence>) {
    for attempt in 1..=3 {
        let race = Box::pin(scheduled()).await;
        if rolled_back {
            sqlx::raw_sql("CREATE FUNCTION reject_racing_debit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.reservation_key='actual-racing-debit' THEN RAISE EXCEPTION 'injected racing debit commit'; END IF; RETURN NEW; END $$; CREATE CONSTRAINT TRIGGER reject_racing_debit AFTER INSERT ON workflow_budget_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_racing_debit()")
                .execute(race.fixture.persistence().pool()).await.unwrap();
        }
        if let Contest::Proven(fence) = Box::pin(compete(&race, order, attempt)).await {
            return (race, fence);
        }
    }
    panic!("every claim_race attempt for {order:?} was void");
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
    let (race, fence) = Box::pin(proven_race(UsageOrder::DebitFirst, false)).await;
    assert!(fence.is_none());
    let before = all_tables(&race.fixture).await;
    assert!(
        race.fixture
            .persistence()
            .settled()
            .claim_io(race.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(before, all_tables(&race.fixture).await);
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_race_claim_first_dispatches_once() {
    let (mut race, fence) = Box::pin(proven_race(UsageOrder::ClaimFirst, false)).await;
    let fence = fence.unwrap();
    Box::pin(dispatch_claim(&mut race, fence)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_race_rolled_back_debit_preserves_allowance() {
    let (mut race, fence) = Box::pin(proven_race(UsageOrder::DebitRollback, true)).await;
    let fence = fence.unwrap();
    Box::pin(dispatch_claim(&mut race, fence)).await;
}
