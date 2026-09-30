//! Reachable descendant first claims preserve the non-child reconciliation policy.
use super::*;

struct Descendant {
    fixture: AdmissionFixture,
    root: ActivationRequest,
    parent: ActivationRequest,
    descendant: ActivationRequest,
}

async fn unique_child(f: &AdmissionFixture, parent: ActivationRequest) -> ActivationRequest {
    let trigger = TriggerRef::new(
        parent.company,
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Child {
            parent: ChildCause::Execution(ExecutionRef::new(
                parent.company,
                parent.run,
                parent.execution,
                StepId::parse("start").unwrap(),
            )),
        },
    )
    .unwrap();
    // Each genuine admission has a distinct logical idempotency key.
    let key = format!("held-ancestor-child-{}", Uuid::new_v4().simple());
    let c = f.prepare(f.request(&key, trigger)).await;
    assert!(matches!(
        f.persistence().admit(&c).await.unwrap(),
        AdmissionResult::Created(_)
    ));
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(c.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    ActivationRequest {
        company: parent.company,
        run: c.proposed_run_id(),
        execution: c.first_execution_id(),
        job: WorkflowJobId(job),
    }
}

async fn descendant_fixture() -> Descendant {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":2,"repetitions":2});
    let (fixture, root) = fixture_source(source).await;
    let parent = unique_child(&fixture, root).await;
    let descendant = unique_child(&fixture, parent).await;
    Descendant {
        fixture,
        root,
        parent,
        descendant,
    }
}

fn ancestor_facts(state: &Value, s: &Descendant) -> Value {
    let rows = |table: &str, ids: [Uuid; 2]| {
        let rows: Vec<_> = state[table]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| ids.iter().any(|id| row["id"] == json!(id)))
            .cloned()
            .collect();
        assert_eq!(rows.len(), 2, "both genuine ancestor rows in {table}");
        rows
    };
    json!({
        "runs": rows("workflow_runs", [s.root.run.as_uuid(), s.parent.run.as_uuid()]),
        "executions": rows("workflow_executions", [s.root.execution.as_uuid(), s.parent.execution.as_uuid()])
    })
}

async fn held_ancestor_claims(s: &Descendant) -> Vec<WorkflowFence> {
    let p = s.fixture.persistence();
    let mut held = p.pool().begin().await.unwrap();
    let runs = sqlx::query(
        "SELECT id FROM workflow_runs WHERE company_id=$1 AND id IN ($2,$3) FOR UPDATE",
    )
    .bind(s.root.company.as_uuid())
    .bind(s.root.run.as_uuid())
    .bind(s.parent.run.as_uuid())
    .fetch_all(&mut *held)
    .await
    .unwrap();
    assert_eq!(runs.len(), 2);
    let executions = sqlx::query(
        "SELECT id FROM workflow_executions WHERE company_id=$1 AND id IN ($2,$3) FOR UPDATE",
    )
    .bind(s.root.company.as_uuid())
    .bind(s.root.execution.as_uuid())
    .bind(s.parent.execution.as_uuid())
    .fetch_all(&mut *held)
    .await
    .unwrap();
    assert_eq!(executions.len(), 2);
    // Both real claims must commit while every ancestor lock is still held.
    // A hidden ancestor FK/run/execution lock fails before this rollback.
    let first_worker = worker();
    let second_worker = worker();
    assert_ne!(first_worker, second_worker);
    let barrier = tokio::sync::Barrier::new(2);
    let start = &barrier;
    let claim = |worker| async move {
        start.wait().await;
        p.claim_io(s.descendant, worker, policy()).await
    };
    let (first, second) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(claim(first_worker), claim(second_worker))
    })
    .await
    .expect("descendant ordinary first claim commits with root and parent locked");
    let claims = [first.unwrap(), second.unwrap()]
        .into_iter()
        .flatten()
        .map(|claim| claim.fence)
        .collect();
    held.rollback().await.unwrap();
    claims
}

fn row(state: &Value, table: &str, key: &str, id: Uuid) -> Value {
    let rows: Vec<_> = state[table]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row[key] == json!(id))
        .collect();
    assert_eq!(rows.len(), 1, "exact scoped {table} row");
    rows[0].clone()
}

fn assert_lineage(s: &Descendant, state: &Value) {
    assert_eq!(state["workflow_root_budgets"].as_array().unwrap().len(), 1);
    for (scope, parent) in [
        (s.root, None),
        (s.parent, Some(s.root.run)),
        (s.descendant, Some(s.parent.run)),
    ] {
        let link = row(state, "workflow_run_budgets", "run_id", scope.run.as_uuid());
        assert_eq!(link["company_id"], json!(s.root.company.as_uuid()));
        assert_eq!(link["root_run_id"], json!(s.root.run.as_uuid()));
        assert_eq!(link["parent_run_id"], json!(parent.map(|id| id.as_uuid())));
        let owner = row(state, "workflow_runs", "id", scope.run.as_uuid());
        assert_eq!(owner["parent_run_id"], link["parent_run_id"]);
        let admission = row(state, "workflow_admissions", "run_id", scope.run.as_uuid());
        let source: Value = serde_json::from_str(
            admission["source_key"]
                .as_str()
                .unwrap()
                .strip_prefix("v1:")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(source[0], if parent.is_some() { "child" } else { "manual" });
    }
}

fn assert_first_claim(s: &Descendant, before: &Value, after: &Value, claims: &[WorkflowFence]) {
    assert_eq!(
        claims.len(),
        1,
        "two competing workers own exactly one first attempt"
    );
    assert_eq!(claims[0].scope, s.descendant);
    assert_eq!(claims[0].attempt.0, 1);
    assert_eq!(ancestor_facts(before, s), ancestor_facts(after, s));
    for table in [
        "workflow_root_budgets",
        "workflow_run_budgets",
        "workflow_admissions",
        "workflow_action_claim_episodes",
        "workflow_action_claim_budget_refusals",
        "workflow_action_evidence",
        "workflow_action_evidence_commands",
        "workflow_action_remote_entries",
        "workflow_action_evidence_consumptions",
    ] {
        assert_eq!(
            before[table], after[table],
            "ordinary first claim preserves {table}"
        );
    }
    let attempts = after["task_attempts"].as_array().unwrap();
    assert_eq!(
        attempts.len(),
        before["task_attempts"].as_array().unwrap().len() + 1
    );
    for prior in before["task_attempts"].as_array().unwrap() {
        assert!(attempts.contains(prior));
    }
    let attempt = row(after, "task_attempts", "task_id", s.descendant.job.0);
    assert_eq!(attempt["attempt_number"], 1);
    assert_eq!(attempt["status"], "processing");
    assert_eq!(attempt["worker_id"], json!(claims[0].worker.0));
    let receipts = after["workflow_budget_receipts"].as_array().unwrap();
    assert_eq!(
        receipts.len(),
        before["workflow_budget_receipts"].as_array().unwrap().len() + 1
    );
    for prior in before["workflow_budget_receipts"].as_array().unwrap() {
        assert!(receipts.contains(prior));
    }
    let receipt = row(
        after,
        "workflow_budget_receipts",
        "run_id",
        s.descendant.run.as_uuid(),
    );
    for (key, value) in [
        ("company_id", json!(s.descendant.company.as_uuid())),
        ("execution_id", json!(s.descendant.execution.as_uuid())),
        ("root_run_id", json!(s.root.run.as_uuid())),
        ("resource", json!("activation")),
        ("reservation_key", json!("activation")),
        ("quantity", json!(1)),
        ("disposition", json!("granted")),
    ] {
        assert_eq!(receipt[key], value);
    }
    let mut expected = before["workflow_root_budget_usage"].clone();
    expected[0]["activations"] = json!(expected[0]["activations"].as_i64().unwrap() + 1);
    assert_eq!(
        after["workflow_root_budget_usage"], expected,
        "only one shared activation debit"
    );
    let execution = row(
        after,
        "workflow_executions",
        "id",
        s.descendant.execution.as_uuid(),
    );
    assert!(!execution["activated_at"].is_null());
    assert!(execution["completed_at"].is_null());
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_fairness_descendant_first_claim_with_ancestors_locked()
 {
    // Box real admission seams to retain stock 2 MiB test stacks.
    let s = Box::pin(descendant_fixture()).await;
    for ancestor in [s.root, s.parent] {
        s.fixture.persistence().activate(ancestor).await.unwrap();
    }
    let before = all_tables(&s.fixture).await;
    assert_lineage(&s, &before);
    assert!(
        row(
            &before,
            "workflow_executions",
            "id",
            s.descendant.execution.as_uuid()
        )["activated_at"]
            .is_null()
    );
    assert!(
        before["workflow_budget_receipts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|receipt| receipt["run_id"] != json!(s.descendant.run.as_uuid()))
    );
    assert!(before["task_attempts"].as_array().unwrap().is_empty());
    assert!(
        before["workflow_action_claim_episodes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(before["workflow_root_budget_usage"][0]["activations"], 2);
    assert_eq!(before["workflow_root_budget_usage"][0]["model_calls"], 0);
    assert_eq!(before["workflow_root_budget_usage"][0]["repetitions"], 0);
    let claims = held_ancestor_claims(&s).await;
    let after = all_tables(&s.fixture).await;
    assert_first_claim(&s, &before, &after, &claims);
    assert!(
        s.fixture
            .persistence()
            .validate_io(claims[0], policy())
            .await
            .unwrap()
    );
    assert_eq!(entry_consumptions(&s.fixture).await, (0, 0));
}

#[path = "action_reconciliation_child_policy_tests.rs"]
mod child_policy_tests;
