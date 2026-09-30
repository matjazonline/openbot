//! Exact reconciliation claim boundaries and unrelated roots on one database.
use super::*;

#[path = "action_reconciliation_claim_recovery_tests.rs"]
mod claim_recovery_tests;

fn source(activations: u32) -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["limits"]["root_budget"] =
        json!({"activations":activations,"model_calls":2,"repetitions":2});
    source
}

async fn reserve(
    f: &AdmissionFixture,
    fence: WorkflowFence,
    resource: BudgetResource,
    quantity: u32,
) {
    let reservation = BudgetReservation::new(
        fence,
        BudgetReservationKey::parse("claim-boundary-logical-work").unwrap(),
        BudgetCharge::new(resource, quantity).unwrap(),
    )
    .unwrap();
    assert_eq!(
        f.persistence()
            .reserve_budget(reservation, policy())
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
}

async fn schedule(
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    delivery: Delivery,
) -> Scheduled {
    let verifier = ledger(&fixture, &request).await;
    let receipt_only = matches!(&delivery, Delivery::Apply { .. });
    assert!(
        invoke(&fixture, &request, &LedgerProvider::new(&fixture, delivery))
            .await
            .is_err()
    );
    park(&fixture, &request).await;
    if !receipt_only {
        barrier(&fixture, &request).await;
    }
    let command = proof_command(&fixture, &request, &verifier, "claim-boundary-schedule").await;
    assert_eq!(
        run_command(&fixture, &command, verifier)
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::Scheduled { receipt_only }
    );
    Scheduled {
        fixture,
        request,
        command,
    }
}

async fn claims(s: &Scheduled) -> Vec<WorkflowFence> {
    let scope = s.request.fence.scope;
    let (a, b) = tokio::join!(
        s.fixture.persistence().claim_io(scope, worker(), policy()),
        s.fixture.persistence().claim_io(scope, worker(), policy())
    );
    [a.unwrap(), b.unwrap()]
        .into_iter()
        .flatten()
        .map(|claim| claim.fence)
        .collect()
}

fn unchanged(before: &Value, after: &Value, tables: &[&str]) {
    for table in tables {
        assert_eq!(before[table], after[table], "preserves {table}");
    }
}

const CLAIM_SAVED_FACTS: &[&str] = &[
    "workflow_root_budgets",
    "workflow_run_budgets",
    "workflow_root_budget_usage",
    "workflow_budget_receipts",
    "workflow_executions",
    "workflow_action_claim_episodes",
    "workflow_action_claim_budget_refusals",
    "workflow_action_evidence_commands",
    "workflow_action_evidence",
    "workflow_action_receipts",
    "workflow_action_dispatches",
    "workflow_action_remote_entries",
    "workflow_action_evidence_consumptions",
];

fn claimed_once(s: &Scheduled, before: &Value, after: &Value, fences: &[WorkflowFence]) {
    assert_eq!(fences.len(), 1);
    assert_eq!(fences[0].scope, s.request.fence.scope);
    assert_eq!(fences[0].attempt.0, s.request.fence.attempt.0 + 1);
    assert_ne!(fences[0].generation, s.request.fence.generation);
    unchanged(before, after, CLAIM_SAVED_FACTS);
    for (table, rows) in before.as_object().unwrap() {
        if table.starts_with("fixture_") {
            assert_eq!(
                rows, &after[table],
                "claim preserves provider/authority facts"
            );
        }
    }
    let old = before["task_attempts"].as_array().unwrap();
    let new = after["task_attempts"].as_array().unwrap();
    assert_eq!(new.len(), old.len() + 1);
    assert!(old.iter().all(|attempt| new.contains(attempt)));
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_boundary_activation_repetition_equality() {
    // Box the established fixture/action seam to retain stock 2 MiB test stacks.
    let (f, scope) = Box::pin(fixture_source(source(1))).await;
    let (f, request) = Box::pin(setup_action_on_fixture(
        f,
        scope,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await;
    reserve(&f, request.fence, BudgetResource::Repetition, 2).await;
    let s = Box::pin(schedule(f, request, Delivery::Pending)).await;
    let before = all_tables(&s.fixture).await;
    let usage = &before["workflow_root_budget_usage"][0];
    assert_eq!(usage["activations"], 1);
    assert_eq!(usage["repetitions"], 2);
    assert_eq!(usage["model_calls"], 0);
    let limits = &before["workflow_root_budgets"][0];
    assert_eq!(limits["activations"], usage["activations"]);
    assert_eq!(limits["repetitions"], usage["repetitions"]);
    let fences = claims(&s).await;
    claimed_once(&s, &before, &all_tables(&s.fixture).await, &fences);
    assert_eq!(entry_consumptions(&s.fixture).await, (1, 0));
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_boundary_receipt_only_model_headroom() {
    for quantity in [1, 2] {
        let (f, request) = Box::pin(limited()).await;
        let s = Box::pin(schedule(
            f,
            request,
            Delivery::Apply {
                result: good(),
                recover: true,
                lose: true,
            },
        ))
        .await;
        assert_eq!(effects(&s.fixture).await, 1);
        assert_eq!(
            counts(&s.fixture).await.0,
            1,
            "receipt exists before new claim"
        );
        let sibling = child(&s.fixture, s.request.fence.scope).await;
        let claim = s
            .fixture
            .persistence()
            .claim_io(sibling, worker(), policy())
            .await
            .unwrap()
            .unwrap();
        reserve(&s.fixture, claim.fence, BudgetResource::ModelCall, quantity).await;
        let before = all_tables(&s.fixture).await;
        assert_eq!(
            before["workflow_root_budget_usage"][0]["model_calls"],
            quantity
        );
        assert_eq!(before["workflow_root_budgets"][0]["model_calls"], 2);
        let fences = claims(&s).await;
        let after = all_tables(&s.fixture).await;
        if quantity == 2 {
            assert!(
                fences.is_empty(),
                "receipt-only new claim still needs model headroom"
            );
            assert_refusal(&before, &after, &s.request, &s.command);
            unchanged(
                &before,
                &after,
                &["task_attempts", "workflow_root_budget_usage"],
            );
            assert!(claims(&s).await.is_empty());
            assert_eq!(
                after,
                all_tables(&s.fixture).await,
                "refused polling stays inert"
            );
        } else {
            claimed_once(&s, &before, &after, &fences);
            let mut request = s.request.clone();
            request.fence = fences[0];
            let provider = LedgerProvider::new(&s.fixture, Delivery::Pending);
            let RemoteDispatchObservation::Committed(receipt) =
                invoke(&s.fixture, &request, &provider).await.unwrap()
            else {
                panic!("saved receipt")
            };
            assert_eq!(receipt.result, good());
            assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        }
        assert_eq!(effects(&s.fixture).await, 1);
        assert_eq!(entry_consumptions(&s.fixture).await, (1, 0));
    }
}

async fn admit_root(f: &AdmissionFixture, key: &str) -> ActivationRequest {
    let command = f.prepare(f.request(key, f.manual())).await;
    assert!(matches!(
        f.persistence().admit(&command).await.unwrap(),
        AdmissionResult::Created(_)
    ));
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    }
}

async fn exhaust_root(f: &AdmissionFixture, scope: ActivationRequest) {
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    reserve(f, claim.fence, BudgetResource::ModelCall, 2).await;
}

async fn eligible(f: &AdmissionFixture, scope: ActivationRequest) -> bool {
    sqlx::query_scalar("SELECT workflow_action_reconciliation_budget_eligible($1,$2)")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

fn run_facts(state: &Value, scope: ActivationRequest) -> Value {
    let id = json!(scope.run.as_uuid());
    let company = json!(scope.company.as_uuid());
    let execution = json!(scope.execution.as_uuid());
    let job = json!(scope.job.0);
    let executions: Vec<_> = state["workflow_executions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["company_id"] == company && row["run_id"] == id)
        .collect();
    assert_eq!(
        executions.len(),
        1,
        "this root owns exactly its start execution"
    );
    assert_eq!(executions[0]["id"], execution);
    let jobs: Vec<_> = state["background_tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["company_id"] == company && row["workflow_execution_id"] == execution)
        .collect();
    assert_eq!(jobs.len(), 1, "exact execution owns one workflow job");
    assert_eq!(jobs[0]["id"], job);
    let attempts: Vec<_> = state["task_attempts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["task_id"] == job)
        .collect();
    assert_eq!(
        attempts.len(),
        1,
        "preservation scope contains the genuine first attempt"
    );
    let mut facts = serde_json::Map::new();
    for (table, rows) in state.as_object().unwrap() {
        let scoped: Vec<_> = rows
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| {
                (row.get("company_id") == Some(&company)
                    && (row.get("run_id") == Some(&id)
                        || row.get("root_run_id") == Some(&id)
                        || (table == "workflow_runs" && row.get("id") == Some(&id))))
                    || (table == "background_tasks" && row.get("id") == Some(&job))
                    || (table == "task_attempts" && row.get("task_id") == Some(&job))
            })
            .cloned()
            .collect();
        if !scoped.is_empty() {
            facts.insert(table.clone(), json!(scoped));
        }
    }
    Value::Object(facts)
}

fn assert_root_links(state: &Value, subject: ActivationRequest, scopes: &[ActivationRequest]) {
    for scope in scopes {
        let link = state["workflow_run_budgets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["run_id"] == json!(scope.run.as_uuid()))
            .unwrap();
        assert_eq!(link["company_id"], json!(scope.company.as_uuid()));
        assert_eq!(link["root_run_id"], json!(scope.run.as_uuid()));
        let usage = state["workflow_root_budget_usage"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["root_run_id"] == json!(scope.run.as_uuid()))
            .unwrap();
        assert_eq!(usage["company_id"], json!(scope.company.as_uuid()));
        assert_eq!(
            usage["model_calls"],
            if scope.run == subject.run { 0 } else { 2 }
        );
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_boundary_roots_and_companies_same_database() {
    let s = Box::pin(scheduled()).await;
    let f = &s.fixture;
    let own = s.request.fence.scope;
    let same = Box::pin(admit_root(f, "independent-root")).await;
    let foreign_fixture =
        Fixture::in_database(f.binding.fixture._db.clone(), "-claim-boundary-foreign").await;
    let foreign = Box::pin(AdmissionFixture::with_binding(
        BindingFixture::from_fixture(foreign_fixture, source(10)).await,
    ))
    .await;
    let other = Box::pin(admit_root(&foreign, "foreign-root")).await;
    assert_eq!(same.company, own.company);
    assert_ne!(same.run, own.run);
    assert_ne!(other.company, own.company);
    assert_ne!(other.run, own.run);
    let before = all_tables(f).await;
    assert!(eligible(f, own).await);
    let same_db: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let foreign_db: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(foreign.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        same_db, foreign_db,
        "all three roots live on the same physical database"
    );
    Box::pin(exhaust_root(f, same)).await;
    Box::pin(exhaust_root(&foreign, other)).await;
    assert!(eligible(f, own).await);
    assert!(!eligible(f, same).await);
    assert!(!eligible(&foreign, other).await);
    let exhausted = all_tables(f).await;
    assert_eq!(
        run_facts(&before, own),
        run_facts(&exhausted, own),
        "unrelated debits preserve subject scope"
    );
    assert_root_links(&exhausted, own, &[own, same, other]);
    let fences = claims(&s).await;
    let after = all_tables(f).await;
    claimed_once(&s, &exhausted, &after, &fences);
    for scope in [same, other] {
        assert_eq!(
            run_facts(&exhausted, scope),
            run_facts(&after, scope),
            "subject claim preserves unrelated root"
        );
    }
    assert_eq!(entry_consumptions(f).await, (1, 0));
}
