//! A same-run audit FK cannot substitute for the exact execution's retirement audit.
use super::*;
use crate::application::workflow::lease::FencedWorkflowResult;

#[derive(sqlx::FromRow)]
struct AuditIdentity {
    company_id: Uuid,
    run_id: Uuid,
    execution_id: Uuid,
    event_kind: String,
}

fn two_step_source() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":2,"repetitions":2});
    source["steps"]["second"] = source["steps"]["start"].clone();
    source["steps"]["start"]["routes"]["success"] = json!("second");
    source
}

async fn scheduled_successor() -> (Scheduled, ActivationRequest) {
    let (f, first) = fixture_source(two_step_source()).await;
    let claim = f
        .persistence()
        .claim_io(first, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let completed = f
        .persistence()
        .complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output: json!({"items":[],"token_count":0}),
        })
        .await
        .unwrap()
        .unwrap();
    let second = completed.successor.unwrap();
    assert_eq!(first.company, second.company);
    assert_eq!(first.run, second.run);
    assert_ne!(first.execution, second.execution);
    assert_ne!(first.job, second.job);
    // Preserve the validated two-step source while freezing the action dependency.
    let (f, request) = Box::pin(setup_action_on_fixture(
        f,
        second,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await;
    let verifier = ledger(&f, &request).await;
    assert!(
        invoke(&f, &request, &LedgerProvider::new(&f, Delivery::Pending))
            .await
            .is_err()
    );
    park(&f, &request).await;
    barrier(&f, &request).await;
    let command = proof_command(&f, &request, &verifier, "wrong-execution-scheduled").await;
    assert_eq!(
        run_command(&f, &command, verifier).await.unwrap().outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    (
        Scheduled {
            fixture: f,
            request,
            command,
        },
        first,
    )
}

async fn assert_audit_identity(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
    sequence: i64,
) {
    let identity: AuditIdentity = sqlx::query_as(
        "SELECT company_id,run_id,execution_id,event_kind FROM workflow_run_events \
         WHERE company_id=$1 AND run_id=$2 AND sequence=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(sequence)
    .fetch_one(&mut **tx)
    .await
    .unwrap();
    assert_eq!(identity.company_id, scope.company.as_uuid());
    assert_eq!(identity.run_id, scope.run.as_uuid());
    assert_eq!(identity.execution_id, scope.execution.as_uuid());
    assert_eq!(identity.event_kind, "workflow.root_budget_exhausted");
}

async fn correct_audit_control(tx: &mut Transaction<'_, Postgres>, s: &Scheduled, genuine: i64) {
    // Validate immediate and deferred guards against the very same retirement,
    // then remove only the positive refusal before proposing the substituted audit.
    sqlx::query("SAVEPOINT correct_audit_control")
        .execute(&mut **tx)
        .await
        .unwrap();
    refusal(tx, s, genuine).await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut **tx)
        .await
        .unwrap();
    sqlx::query("ROLLBACK TO SAVEPOINT correct_audit_control")
        .execute(&mut **tx)
        .await
        .unwrap();
    sqlx::query("RELEASE SAVEPOINT correct_audit_control")
        .execute(&mut **tx)
        .await
        .unwrap();
}

async fn exhaust_shared_root(s: &Scheduled, first: ActivationRequest) {
    // The shared child fixture names step "start": attach it to that genuine
    // completed execution, whose root budget also governs the second execution.
    let descendant = child(&s.fixture, first).await;
    let claim = s
        .fixture
        .persistence()
        .claim_io(descendant, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    debit(&s.fixture, claim.fence).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_wrong_execution_with_current_retirement() {
    // Box the completion/action fixture seam to preserve stock 2 MiB stacks.
    let (s, first) = Box::pin(scheduled_successor()).await;
    Box::pin(exhaust_shared_root(&s, first)).await;
    let before = all_tables(&s.fixture).await;
    let mut tx = s.fixture.persistence().pool().begin().await.unwrap();
    let genuine = current_retirement(&mut tx, &s).await;
    assert_audit_identity(&mut tx, s.request.fence.scope, genuine).await;
    let wrong = audit(&mut tx, first, "workflow.root_budget_exhausted").await;
    assert_ne!(genuine, wrong);
    assert_audit_identity(&mut tx, first, wrong).await;
    correct_audit_control(&mut tx, &s, genuine).await;
    let error = refusal(&mut tx, &s, wrong).await.unwrap_err();
    guard(
        &error,
        "invalid current workflow reconciliation budget refusal",
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "both executions, current retirement, audits and refusal roll back with all public tables"
    );
}
