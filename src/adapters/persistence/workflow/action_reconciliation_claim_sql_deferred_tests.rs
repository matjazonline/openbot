//! Revalidate a genuine refusal after a rollback-only invalid attempt insertion.
use super::*;

#[derive(sqlx::FromRow)]
struct RefusalPrerequisites {
    other_predicates: bool,
    current_witnesses: i64,
    refusal_rows: i64,
    greater_attempts: i64,
    retired_attempt: i32,
    audit_sequence: i64,
}

const REFUSAL_PREREQUISITES: &str = r#"SELECT
    NOT workflow_action_reconciliation_budget_eligible(witness.company_id,witness.run_id)
        AND witness.transaction_id=pg_current_xact_id() AND witness.retirement_confirmed
        AND witness.audit_sequence=$6 AND witness.deadline>clock_timestamp() AND owner.deadline>clock_timestamp()
        AND ((owner.state='failed' AND owner.terminal_execution_id=witness.execution_id AND owner.waiting_reason IS NULL
            AND workflow_action_retry_safe(witness.company_id,witness.execution_id) IS DISTINCT FROM false)
            OR (owner.state='waiting' AND owner.waiting_reason='reconciliation' AND owner.terminal_execution_id IS NULL
                AND workflow_action_retry_safe(witness.company_id,witness.execution_id) IS FALSE))
        AND job.status='failed' AND job.queue_kind='workflow' AND job.workflow_execution_id=witness.execution_id
        AND job.retry_count=witness.retired_attempt AND job.worker_id IS NULL AND job.execution_generation IS NULL
        AND job.lock_expires_at IS NULL AND execution.activated_at IS NOT NULL AND execution.completed_at IS NULL
        AND audit.execution_id=witness.execution_id AND audit.event_kind='workflow.root_budget_exhausted' AS other_predicates,
    (SELECT count(*) FROM workflow_action_claim_retirement_witnesses AS current_witness
        WHERE current_witness.company_id=witness.company_id AND current_witness.command_key=witness.command_key
            AND current_witness.transaction_id=pg_current_xact_id()) AS current_witnesses,
    (SELECT count(*) FROM workflow_action_claim_budget_refusals AS refusal
        WHERE refusal.company_id=witness.company_id AND refusal.run_id=witness.run_id
            AND refusal.execution_id=witness.execution_id AND refusal.job_id=witness.job_id
            AND refusal.command_key=witness.command_key AND refusal.retired_attempt=witness.retired_attempt
            AND refusal.audit_sequence=witness.audit_sequence) AS refusal_rows,
    (SELECT count(*) FROM task_attempts AS later
        WHERE later.task_id=witness.job_id AND later.attempt_number>witness.retired_attempt) AS greater_attempts,
    witness.retired_attempt,witness.audit_sequence
    FROM workflow_action_claim_retirement_witnesses AS witness
    JOIN workflow_action_claim_episodes AS episode ON episode.company_id=witness.company_id AND episode.run_id=witness.run_id
        AND episode.execution_id=witness.execution_id AND episode.job_id=witness.job_id AND episode.command_key=witness.command_key
        AND episode.retired_attempt=witness.retired_attempt
    JOIN workflow_runs AS owner ON owner.company_id=witness.company_id AND owner.id=witness.run_id
    JOIN background_tasks AS job ON job.company_id=witness.company_id AND job.id=witness.job_id
    JOIN workflow_executions AS execution ON execution.company_id=witness.company_id AND execution.run_id=witness.run_id
        AND execution.id=witness.execution_id
    JOIN workflow_run_events AS audit ON audit.company_id=witness.company_id AND audit.run_id=witness.run_id
        AND audit.sequence=witness.audit_sequence
    WHERE witness.company_id=$1 AND witness.run_id=$2 AND witness.execution_id=$3
        AND witness.job_id=$4 AND witness.command_key=$5"#;

async fn prerequisites(
    tx: &mut Transaction<'_, Postgres>,
    scheduled: &Scheduled,
    audit_sequence: i64,
) -> RefusalPrerequisites {
    let scope = scheduled.request.fence.scope;
    let facts: RefusalPrerequisites = sqlx::query_as(REFUSAL_PREREQUISITES)
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(scope.execution.as_uuid())
        .bind(scope.job.0)
        .bind(scheduled.command.command_key.as_str())
        .bind(audit_sequence)
        .fetch_one(&mut **tx)
        .await
        .unwrap();
    assert!(
        facts.other_predicates,
        "all unrelated refusal predicates pass"
    );
    assert_eq!(facts.current_witnesses, 1);
    assert_eq!(facts.retired_attempt, scheduled.request.fence.attempt.0);
    assert_eq!(facts.audit_sequence, audit_sequence);
    facts
}

async fn insert_invalid_attempt(
    tx: &mut Transaction<'_, Postgres>,
    job: WorkflowJobId,
    retired_attempt: i32,
) {
    // Keep its original deferred event queued. This row is an invalid-state
    // attack, not a next claim: the genuine failed job and counter stay unchanged.
    let attempt_id = Uuid::new_v4();
    let generation = Uuid::new_v4();
    let worker_id = Uuid::new_v4();
    let greater_attempt = retired_attempt.checked_add(1).unwrap();
    let inserted = sqlx::query("INSERT INTO task_attempts(id,task_id,attempt_number,status,execution_generation,worker_id,machine_id) VALUES($1,$2,$3,'processing',$4,$5,'deferred-refusal-attack')")
        .bind(attempt_id).bind(job.0).bind(greater_attempt)
        .bind(generation).bind(worker_id).execute(&mut **tx).await.unwrap();
    assert_eq!(inserted.rows_affected(), 1);
    let exact_attempt: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts AS attempt WHERE attempt.id=$1 AND attempt.task_id=$2 AND attempt.attempt_number=$3 AND attempt.status='processing' AND attempt.execution_generation=$4 AND attempt.worker_id=$5 AND attempt.machine_id='deferred-refusal-attack' AND attempt.finished_at IS NULL AND attempt.workflow_failure_class IS NULL AND attempt.workflow_failure_code IS NULL AND attempt.workflow_retry_safety IS NULL AND attempt.workflow_retirement IS NULL")
        .bind(attempt_id).bind(job.0).bind(greater_attempt)
        .bind(generation).bind(worker_id).fetch_one(&mut **tx).await.unwrap();
    assert_eq!(
        exact_attempt, 1,
        "the otherwise valid ledger row actually exists"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_deferred_greater_attempt_refusal() {
    // Box the real action fixture seams to retain stock 2 MiB test stacks.
    let scheduled = Box::pin(scheduled()).await;
    Box::pin(exhaust(&scheduled)).await;
    let before = all_tables(&scheduled.fixture).await;
    let mut tx = scheduled
        .fixture
        .persistence()
        .pool()
        .begin()
        .await
        .unwrap();
    let audit_sequence = current_retirement(&mut tx, &scheduled).await;
    let facts = prerequisites(&mut tx, &scheduled, audit_sequence).await;
    assert_eq!(facts.refusal_rows, 0, "genuine refusal key is unused");
    assert_eq!(facts.greater_attempts, 0);
    refusal(&mut tx, &scheduled, audit_sequence).await.unwrap();
    let facts = prerequisites(&mut tx, &scheduled, audit_sequence).await;
    assert_eq!(
        facts.refusal_rows, 1,
        "public INSERT passes BEFORE guard and FKs"
    );
    assert_eq!(facts.greater_attempts, 0);

    insert_invalid_attempt(
        &mut tx,
        scheduled.request.fence.scope.job,
        facts.retired_attempt,
    )
    .await;
    let facts = prerequisites(&mut tx, &scheduled, audit_sequence).await;
    assert_eq!(facts.refusal_rows, 1);
    assert_eq!(
        facts.greater_attempts, 1,
        "only greater-attempt prerequisite changed"
    );
    let error =
        sqlx::query("SET CONSTRAINTS workflow_action_claim_budget_refusal_commit_guard IMMEDIATE")
            .execute(&mut *tx)
            .await
            .expect_err("deferred refusal must reject the greater attempt");
    guard(
        &error,
        "invalid current workflow reconciliation budget refusal",
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        before,
        all_tables(&scheduled.fixture).await,
        "retirement, refusal, invalid attempt and all public facts roll back together"
    );

    // The same unchanged genuine fixture commits through the complete claim owner
    // when no invalid attempt is inserted; its deferred refusal guard must pass.
    assert!(
        scheduled
            .fixture
            .persistence()
            .claim_io(scheduled.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_refusal(
        &before,
        &all_tables(&scheduled.fixture).await,
        &scheduled.request,
        &scheduled.command,
    );
}
