//! Historical audit substitution after the real current pending retirement.
use super::*;
use crate::adapters::persistence::workflow::{lease, pending_recovery};
use crate::application::app_error::AppError;

#[derive(sqlx::FromRow, Debug, PartialEq)]
struct HistoricalAudit {
    sequence: i64,
    transaction_id: i64,
    row_transaction_id: i64,
}

const EXPECTED_PROBE_ERROR: &str = "historical audit probe refusal SQLSTATE=23514 SQLERRM=invalid current workflow reconciliation budget refusal";

async fn commit_history(s: &Scheduled) -> HistoricalAudit {
    let scope = s.request.fence.scope;
    let mut tx = s.fixture.persistence().pool().begin().await.unwrap();
    let sequence = audit(&mut tx, scope, "workflow.root_budget_exhausted").await;
    let history = sqlx::query_as("SELECT sequence,pg_current_xact_id()::text::bigint AS transaction_id,xmin::text::bigint AS row_transaction_id FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 AND sequence=$3 AND execution_id=$4 AND event_kind='workflow.root_budget_exhausted'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(sequence)
        .bind(scope.execution.as_uuid()).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    history
}

async fn assert_pending_history(tx: &mut Transaction<'_, Postgres>, s: &Scheduled) {
    let scope = s.request.fence.scope;
    let valid: bool = sqlx::query_scalar(r#"
        SELECT EXISTS(SELECT 1 FROM workflow_action_claim_episodes AS episode
            JOIN workflow_runs AS owner ON owner.company_id=episode.company_id AND owner.id=episode.run_id
            JOIN background_tasks AS job ON job.company_id=episode.company_id AND job.id=episode.job_id
            JOIN workflow_executions AS step ON step.company_id=episode.company_id AND step.run_id=episode.run_id AND step.id=episode.execution_id
            WHERE episode.company_id=$1 AND episode.run_id=$2 AND episode.execution_id=$3 AND episode.job_id=$4 AND episode.command_key=$5
                AND workflow_action_pending_claim_episode($1,$2,$3,$4)=$5
                AND owner.state IN ('queued','running') AND owner.deadline>clock_timestamp()
                AND job.status='pending' AND job.retry_count=episode.retired_attempt
                AND job.worker_id IS NULL AND job.execution_generation IS NULL AND job.lock_expires_at IS NULL
                AND step.activated_at IS NOT NULL AND step.completed_at IS NULL
                AND NOT EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=job.id AND later.attempt_number>episode.retired_attempt))
            AND NOT workflow_action_reconciliation_budget_eligible($1,$2)
            AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_retirement_witnesses WHERE company_id=$1 AND command_key=$5)
            AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals WHERE company_id=$1 AND command_key=$5)
    "#).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(scope.job.0).bind(s.command.command_key.as_str()).fetch_one(&mut **tx).await.unwrap();
    assert!(
        valid,
        "historical event committed without retiring the genuine pending episode"
    );
}

// This probe observes production capture/confirm writes before the production
// audit INSERT reaches event uniqueness or its AFTER INSERT linkage trigger.
// Only the adversarial refusal INSERT belongs to the exception handler.
const PROBE_BODY: &str = r#"
DECLARE
    episode workflow_action_claim_episodes%ROWTYPE;
    witness workflow_action_claim_retirement_witnesses%ROWTYPE;
    historical_sequence bigint := TG_ARGV[5]::bigint;
    historical_transaction xid8 := TG_ARGV[6]::xid8;
    historical_row_transaction text := TG_ARGV[7];
    captured_state text;
    captured_message text;
BEGIN
    IF NEW.company_id IS DISTINCT FROM TG_ARGV[0]::uuid
        OR NEW.run_id IS DISTINCT FROM TG_ARGV[1]::uuid
        OR NEW.execution_id IS DISTINCT FROM TG_ARGV[2]::uuid
        OR NEW.event_kind IS DISTINCT FROM 'workflow.root_budget_exhausted' THEN RETURN NEW; END IF;

    SELECT binding.* INTO STRICT episode FROM workflow_action_claim_episodes AS binding
        WHERE binding.company_id=NEW.company_id AND binding.run_id=NEW.run_id
            AND binding.execution_id=NEW.execution_id AND binding.job_id=TG_ARGV[3]::uuid
            AND binding.command_key=TG_ARGV[4];
    IF (SELECT count(*) FROM workflow_action_claim_retirement_witnesses AS current
        WHERE current.company_id=episode.company_id AND current.run_id=episode.run_id
            AND current.execution_id=episode.execution_id AND current.job_id=episode.job_id
            AND current.command_key=episode.command_key AND current.transaction_id=pg_current_xact_id())<>1
    THEN RAISE EXCEPTION 'historical audit probe lacks exact current retirement witness'; END IF;
    SELECT current.* INTO STRICT witness FROM workflow_action_claim_retirement_witnesses AS current
        WHERE current.company_id=episode.company_id AND current.run_id=episode.run_id
            AND current.execution_id=episode.execution_id AND current.job_id=episode.job_id
            AND current.command_key=episode.command_key AND current.transaction_id=pg_current_xact_id();
    IF witness.retired_attempt<>episode.retired_attempt OR NOT witness.retirement_confirmed
        OR witness.audit_sequence IS NOT NULL OR witness.deadline<=clock_timestamp()
    THEN RAISE EXCEPTION 'historical audit probe retirement is unconfirmed, linked, expired or wrong ordinal'; END IF;
    IF NOT EXISTS(SELECT 1 FROM background_tasks AS job
        WHERE job.company_id=episode.company_id AND job.id=episode.job_id
            AND job.workflow_execution_id=episode.execution_id AND job.queue_kind='workflow'
            AND job.status='failed' AND job.retry_count=episode.retired_attempt
            AND job.worker_id IS NULL AND job.execution_generation IS NULL AND job.lock_expires_at IS NULL)
        OR EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=episode.job_id AND later.attempt_number>episode.retired_attempt)
    THEN RAISE EXCEPTION 'historical audit probe lacks exact lease-free pending-to-failed job'; END IF;
    IF NOT EXISTS(SELECT 1 FROM workflow_executions AS step
        WHERE step.company_id=episode.company_id AND step.run_id=episode.run_id AND step.id=episode.execution_id
            AND step.activated_at IS NOT NULL AND step.completed_at IS NULL)
    THEN RAISE EXCEPTION 'historical audit probe lacks activated uncompleted execution'; END IF;
    IF NOT EXISTS(SELECT 1 FROM workflow_runs AS owner
        WHERE owner.company_id=episode.company_id AND owner.id=episode.run_id
            AND owner.deadline>clock_timestamp() AND owner.deadline=witness.deadline
            AND ((owner.state='failed' AND owner.waiting_reason IS NULL AND owner.terminal_execution_id=episode.execution_id
                AND workflow_action_retry_safe(episode.company_id,episode.execution_id) IS DISTINCT FROM false)
                OR (owner.state='waiting' AND owner.waiting_reason='reconciliation' AND owner.terminal_execution_id IS NULL
                    AND workflow_action_retry_safe(episode.company_id,episode.execution_id) IS FALSE)))
        OR workflow_action_reconciliation_budget_eligible(episode.company_id,episode.run_id)
    THEN RAISE EXCEPTION 'historical audit probe lacks genuine retired run state or current ineligibility'; END IF;
    IF historical_transaction=pg_current_xact_id() OR historical_sequence=NEW.sequence
        OR NOT EXISTS(SELECT 1 FROM workflow_run_events AS history
            WHERE history.company_id=episode.company_id AND history.run_id=episode.run_id
                AND history.execution_id=episode.execution_id AND history.sequence=historical_sequence
                AND history.event_kind='workflow.root_budget_exhausted'
                AND history.xmin::text=historical_row_transaction
                AND history.xmin::text=(historical_transaction::text::bigint % 4294967296)::text)
    THEN RAISE EXCEPTION 'historical audit probe lacks distinct committed same-execution same-kind event'; END IF;
    IF EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
        WHERE refusal.company_id=episode.company_id AND refusal.command_key=episode.command_key)
    THEN RAISE EXCEPTION 'historical audit probe refusal already exists'; END IF;

    BEGIN
        INSERT INTO workflow_action_claim_budget_refusals(company_id,run_id,execution_id,job_id,command_key,retired_attempt,audit_sequence)
            VALUES(episode.company_id,episode.run_id,episode.execution_id,episode.job_id,episode.command_key,episode.retired_attempt,historical_sequence);
    EXCEPTION WHEN OTHERS THEN
        GET STACKED DIAGNOSTICS captured_state=RETURNED_SQLSTATE, captured_message=MESSAGE_TEXT;
        RAISE EXCEPTION USING MESSAGE=format('historical audit probe refusal SQLSTATE=%s SQLERRM=%s',captured_state,captured_message);
    END;
    RAISE EXCEPTION 'historical audit probe unexpectedly accepted refusal';
END
"#;

async fn install_probe(
    tx: &mut Transaction<'_, Postgres>,
    s: &Scheduled,
    history: &HistoricalAudit,
    name: &str,
) {
    let scope = s.request.fence.scope;
    let command = s.command.command_key.as_str().replace('\'', "''");
    let sql = format!(
        r#"
        CREATE FUNCTION public.{name}() RETURNS trigger LANGUAGE plpgsql AS $probe${PROBE_BODY}$probe$;
        CREATE TRIGGER {name} BEFORE INSERT ON workflow_run_events FOR EACH ROW EXECUTE FUNCTION public.{name}(
            '{}','{}','{}','{}','{}','{}','{}','{}');
    "#,
        scope.company.as_uuid(),
        scope.run.as_uuid(),
        scope.execution.as_uuid(),
        scope.job.0,
        command,
        history.sequence,
        history.transaction_id,
        history.row_transaction_id
    );
    sqlx::raw_sql(&sql).execute(&mut **tx).await.unwrap();
}

async fn assert_probe_absent(s: &Scheduled, name: &str) {
    let absent: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_proc AS routine JOIN pg_namespace AS namespace ON namespace.oid=routine.pronamespace WHERE namespace.nspname='public' AND routine.proname=$1) AND NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='public.workflow_run_events'::regclass AND tgname=$1)")
        .bind(name).fetch_one(s.fixture.persistence().pool()).await.unwrap();
    assert!(
        absent,
        "transactional probe function and trigger disappeared"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_historical_audit_with_current_retirement() {
    // Box the real action fixture seam to retain stock 2 MiB test stacks.
    let s = Box::pin(scheduled()).await;
    Box::pin(exhaust(&s)).await;
    let history = commit_history(&s).await;
    let before = all_tables(&s.fixture).await;
    let name = format!("historical_audit_probe_{}", Uuid::new_v4().simple());
    assert_probe_absent(&s, &name).await;
    let mut tx = s.fixture.persistence().pool().begin().await.unwrap();
    assert_pending_history(&mut tx, &s).await;
    install_probe(&mut tx, &s, &history, &name).await;
    let scope = s.request.fence.scope;
    assert!(
        lease::lock_scope(&mut tx, scope, policy())
            .await
            .unwrap()
            .is_some()
    );
    // Run the complete owner; the probe aborts its audit before event uniqueness.
    let error = pending_recovery::settle(
        &mut tx,
        scope,
        pending_recovery::PendingFailure::RootBudgetExhausted,
    )
    .await
    .unwrap_err();
    let AppError::Database(message) = error else {
        panic!("unexpected owner error: {error}");
    };
    assert_eq!(
        message,
        format!("error returned from database: {EXPECTED_PROBE_ERROR}")
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "current retirement/probe attack roll back every public table and preserve history"
    );
    assert_probe_absent(&s, &name).await;
    let saved: HistoricalAudit = sqlx::query_as("SELECT sequence,$4::bigint AS transaction_id,xmin::text::bigint AS row_transaction_id FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 AND sequence=$3 AND execution_id=$5 AND event_kind='workflow.root_budget_exhausted'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(history.sequence)
        .bind(history.transaction_id).bind(scope.execution.as_uuid()).fetch_one(s.fixture.persistence().pool()).await.unwrap();
    assert_eq!(
        history, saved,
        "original historical audit transaction identity survives rollback"
    );
}
