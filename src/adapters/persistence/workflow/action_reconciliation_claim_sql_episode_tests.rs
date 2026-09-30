//! Genuine owner checkpoints isolate episode constraints before automatic binding.
use super::*;
use crate::application::workflow::lease::{
    FencedWorkflowResult, LeaseReleaseCause, WorkflowFailure,
};
use crate::application::workflow::{RetryCommand, RetryResult};
use crate::domain::workflow::RetrySafety;

struct PreparedSchedule {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    command: ReconcileActionCommand,
    verifier: Arc<LedgerVerifier>,
}

async fn prepare_schedule(f: AdmissionFixture, request: ActionDispatchRequest) -> PreparedSchedule {
    let verifier = ledger(&f, &request).await;
    assert!(
        invoke(&f, &request, &LedgerProvider::new(&f, Delivery::Pending))
            .await
            .is_err()
    );
    park(&f, &request).await;
    barrier(&f, &request).await;
    let command = proof_command(&f, &request, &verifier, "episode-checkpoint").await;
    PreparedSchedule {
        fixture: f,
        request,
        command,
        verifier,
    }
}

async fn successor_schedule() -> (PreparedSchedule, ActivationRequest) {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":2,"repetitions":2});
    source["steps"]["second"] = source["steps"]["start"].clone();
    source["steps"]["start"]["routes"]["success"] = json!("second");
    let (f, first) = fixture_source(source).await;
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
    assert_eq!(first.run, second.run);
    assert_ne!(first.execution, second.execution);
    let (f, request) = Box::pin(setup_action_on_fixture(
        f,
        second,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await;
    (Box::pin(prepare_schedule(f, request)).await, first)
}

async fn second_attempt_schedule() -> PreparedSchedule {
    let (f, mut request) = Box::pin(limited()).await;
    let cause = LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("provider.rejected").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::SafeToRetry,
    });
    assert!(
        f.persistence()
            .release_io(request.fence, policy(), cause)
            .await
            .unwrap()
    );
    let scope = request.fence.scope;
    let revision = f
        .persistence()
        .head(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    assert!(matches!(
        f.persistence()
            .retry(RetryCommand {
                company_id: scope.company,
                run_id: scope.run,
                actor: f.binding.target.actor,
                command_key: IdempotencyKey::parse("ordinary-before-episode").unwrap(),
                expected_revision: revision,
            })
            .await
            .unwrap(),
        RetryResult::Applied { .. }
    ));
    let next = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(request.fence.attempt.0, 1);
    assert_eq!(next.fence.attempt.0, 2);
    request.fence = next.fence;
    Box::pin(prepare_schedule(f, request)).await
}

// The outer production NEW is untouched. Only adversarial nested INSERTs enter
// the handler; unrelated events and nested probe entries return NEW unchanged.
const EPISODE_PROBE: &str = r#"
DECLARE candidate workflow_action_claim_episodes%ROWTYPE; expected_fk text;
    actual_state text; actual_message text; actual_constraint text;
BEGIN
    IF NEW.company_id<>TG_ARGV[0]::uuid OR NEW.run_id<>TG_ARGV[1]::uuid
        OR NEW.execution_id<>TG_ARGV[2]::uuid OR NEW.job_id<>TG_ARGV[3]::uuid
        OR NEW.command_key<>TG_ARGV[4] OR pg_trigger_depth()<>2 THEN RETURN NEW; END IF;
    IF EXISTS(SELECT 1 FROM workflow_action_claim_episodes WHERE company_id=NEW.company_id
        AND (command_key=NEW.command_key OR (job_id=NEW.job_id AND retired_attempt=NEW.retired_attempt)))
        THEN RAISE EXCEPTION 'episode probe occupied fresh binding'; END IF;
    IF NOT EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command
        JOIN workflow_runs AS owner ON owner.company_id=command.company_id AND owner.id=command.run_id
        JOIN background_tasks AS job ON job.company_id=command.company_id AND job.id=command.scheduled_job_id
        JOIN task_attempts AS attempt ON attempt.task_id=job.id AND attempt.attempt_number=NEW.retired_attempt
        JOIN workflow_action_schedule_witnesses AS schedule ON schedule.company_id=NEW.company_id
            AND schedule.run_id=NEW.run_id AND schedule.execution_id=NEW.execution_id AND schedule.job_id=NEW.job_id
            AND schedule.retired_attempt=NEW.retired_attempt AND schedule.transaction_id=pg_current_xact_id()
        JOIN workflow_action_state_witnesses AS state ON state.company_id=NEW.company_id AND state.run_id=NEW.run_id
            AND state.transaction_id=pg_current_xact_id()
        WHERE command.company_id=NEW.company_id AND command.command_key=NEW.command_key AND command.run_id=NEW.run_id
            AND command.execution_id=NEW.execution_id AND command.scheduled_job_id=NEW.job_id
            AND command.outcome->>'kind'='scheduled' AND command.result_revision=owner.revision
            AND state.initial_state='waiting' AND state.initial_waiting_reason='reconciliation'
            AND job.status='pending' AND job.retry_count=NEW.retired_attempt
            AND job.worker_id IS NULL AND job.execution_generation IS NULL AND job.lock_expires_at IS NULL
            AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
            AND NOT EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=job.id
                AND later.attempt_number>NEW.retired_attempt))
        THEN RAISE EXCEPTION 'episode probe lacks genuine current schedule'; END IF;
    candidate:=NEW;
    IF TG_ARGV[5]='scope' THEN
        candidate.execution_id:=TG_ARGV[6]::uuid; candidate.job_id:=TG_ARGV[7]::uuid;
        candidate.retired_attempt:=1;
        SELECT conname INTO STRICT expected_fk FROM pg_constraint
            WHERE conrelid='workflow_action_claim_episodes'::regclass AND contype='f'
                AND confrelid='workflow_action_evidence_commands'::regclass;
    ELSE
        IF NEW.retired_attempt<>2 THEN RAISE EXCEPTION 'episode probe expected actual attempt2'; END IF;
        candidate.retired_attempt:=1;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM workflow_executions AS execution JOIN background_tasks AS job
        ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=job.id AND attempt.attempt_number=candidate.retired_attempt
        WHERE execution.company_id=candidate.company_id AND execution.run_id=candidate.run_id
            AND execution.id=candidate.execution_id AND job.id=candidate.job_id AND attempt.finished_at IS NOT NULL)
        OR EXISTS(SELECT 1 FROM workflow_action_claim_episodes WHERE company_id=candidate.company_id
            AND (command_key=candidate.command_key OR (job_id=candidate.job_id AND retired_attempt=candidate.retired_attempt)))
        THEN RAISE EXCEPTION 'episode probe invalid attack references'; END IF;
    BEGIN
        INSERT INTO workflow_action_claim_episodes(company_id,run_id,execution_id,job_id,command_key,retired_attempt)
            VALUES(candidate.company_id,candidate.run_id,candidate.execution_id,candidate.job_id,candidate.command_key,candidate.retired_attempt);
        IF TG_ARGV[5]='ordinal' THEN SET CONSTRAINTS workflow_action_claim_episode_guard IMMEDIATE; END IF;
    EXCEPTION WHEN OTHERS THEN
        GET STACKED DIAGNOSTICS actual_state=RETURNED_SQLSTATE, actual_message=MESSAGE_TEXT,
            actual_constraint=CONSTRAINT_NAME;
        IF TG_ARGV[5]='scope' AND actual_state='23503' AND actual_constraint=expected_fk THEN
            RAISE EXCEPTION 'episode scope probe exact command FK SQLSTATE=23503 CONSTRAINT=%',actual_constraint;
        END IF;
        RAISE EXCEPTION 'episode % probe SQLSTATE=% SQLERRM=% CONSTRAINT=%',
            TG_ARGV[5],actual_state,actual_message,actual_constraint;
    END;
    RAISE EXCEPTION 'episode probe unexpected acceptance';
END
"#;

async fn install_episode_probe(
    s: &PreparedSchedule,
    mode: &str,
    other: ActivationRequest,
) -> String {
    let name = format!("episode_probe_{}", Uuid::new_v4().simple());
    let scope = s.request.fence.scope;
    let sql = format!(
        r#"CREATE FUNCTION public.{name}() RETURNS trigger LANGUAGE plpgsql AS $probe${EPISODE_PROBE}$probe$;
        CREATE TRIGGER {name} BEFORE INSERT ON workflow_action_claim_episodes FOR EACH ROW EXECUTE FUNCTION public.{name}('{}','{}','{}','{}','{}','{}','{}','{}');"#,
        scope.company.as_uuid(),
        scope.run.as_uuid(),
        scope.execution.as_uuid(),
        scope.job.0,
        s.command.command_key.as_str(),
        mode,
        other.execution.as_uuid(),
        other.job.0
    );
    sqlx::raw_sql(&sql)
        .execute(s.fixture.persistence().pool())
        .await
        .unwrap();
    name
}

async fn remove_episode_probe(s: &PreparedSchedule, name: &str) {
    sqlx::raw_sql(&format!(
        "DROP TRIGGER {name} ON workflow_action_claim_episodes; DROP FUNCTION public.{name}();"
    ))
    .execute(s.fixture.persistence().pool())
    .await
    .unwrap();
    let absent: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_proc WHERE proname=$1) AND NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgname=$1)")
        .bind(name).fetch_one(s.fixture.persistence().pool()).await.unwrap();
    assert!(absent, "fixture-owned probe DDL removed");
}

async fn rejected_schedule(s: &PreparedSchedule, expected: &str) {
    let before = all_tables(&s.fixture).await;
    let result = run_command(&s.fixture, &s.command, s.verifier.clone()).await;
    let AppError::Database(message) = result.unwrap_err() else {
        panic!("unexpected schedule error")
    };
    assert_eq!(message, format!("error returned from database: {expected}"));
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "whole production owner transaction rolls back"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_episode_wrong_scoped_command() {
    // Box the extended owner fixture seam to retain stock 2 MiB test stacks.
    let (s, first) = Box::pin(successor_schedule()).await;
    let constraint: String = sqlx::query_scalar("SELECT conname FROM pg_constraint WHERE conrelid='workflow_action_claim_episodes'::regclass AND contype='f' AND confrelid='workflow_action_evidence_commands'::regclass")
        .fetch_one(s.fixture.persistence().pool()).await.unwrap();
    let name = install_episode_probe(&s, "scope", first).await;
    rejected_schedule(
        &s,
        &format!("episode scope probe exact command FK SQLSTATE=23503 CONSTRAINT={constraint}"),
    )
    .await;
    remove_episode_probe(&s, &name).await;
    assert!(matches!(
        run_command(&s.fixture, &s.command, s.verifier.clone())
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    ));
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_episode_wrong_retired_ordinal() {
    // Actual safe ordinary retry supplies an earlier genuine attempt without an episode.
    let s = Box::pin(second_attempt_schedule()).await;
    let name = install_episode_probe(&s, "ordinal", s.request.fence.scope).await;
    rejected_schedule(&s, "episode ordinal probe SQLSTATE=23514 SQLERRM=invalid current workflow reconciliation claim episode CONSTRAINT=").await;
    remove_episode_probe(&s, &name).await;
    assert!(matches!(
        run_command(&s.fixture, &s.command, s.verifier.clone())
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    ));
    let ordinal: i32 = sqlx::query_scalar("SELECT retired_attempt FROM workflow_action_claim_episodes WHERE company_id=$1 AND command_key=$2")
        .bind(s.request.fence.scope.company.as_uuid()).bind(s.command.command_key.as_str())
        .fetch_one(s.fixture.persistence().pool()).await.unwrap();
    assert_eq!(
        ordinal, 2,
        "proper attempt2 schedule commits through deferred guards"
    );
}

#[derive(sqlx::FromRow)]
struct HistoricalRetirement {
    old_transaction: bool,
    current_count: i64,
    other_predicates: bool,
    audit_sequence: i64,
}

const HISTORICAL_RETIREMENT: &str = r#"SELECT witness.transaction_id<>pg_current_xact_id() AS old_transaction,
    (SELECT count(*) FROM workflow_action_claim_retirement_witnesses AS current_witness
        WHERE current_witness.company_id=witness.company_id AND current_witness.command_key=witness.command_key
            AND current_witness.transaction_id=pg_current_xact_id()) AS current_count,
    NOT workflow_action_reconciliation_budget_eligible(witness.company_id,witness.run_id)
        AND witness.retirement_confirmed AND witness.deadline>clock_timestamp() AND owner.deadline>clock_timestamp()
        AND ((owner.state='failed' AND owner.terminal_execution_id=witness.execution_id AND owner.waiting_reason IS NULL
            AND workflow_action_retry_safe(witness.company_id,witness.execution_id) IS DISTINCT FROM false)
            OR (owner.state='waiting' AND owner.waiting_reason='reconciliation' AND owner.terminal_execution_id IS NULL
                AND workflow_action_retry_safe(witness.company_id,witness.execution_id) IS FALSE))
        AND job.status='failed' AND job.queue_kind='workflow' AND job.workflow_execution_id=witness.execution_id
        AND job.retry_count=witness.retired_attempt AND job.worker_id IS NULL AND job.execution_generation IS NULL
        AND job.lock_expires_at IS NULL AND execution.activated_at IS NOT NULL AND execution.completed_at IS NULL
        AND audit.execution_id=witness.execution_id AND audit.event_kind='workflow.root_budget_exhausted'
        AND NOT EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=witness.job_id
            AND later.attempt_number>witness.retired_attempt) AS other_predicates, witness.audit_sequence
    FROM workflow_action_claim_retirement_witnesses AS witness
    JOIN workflow_action_claim_episodes AS episode ON episode.company_id=witness.company_id AND episode.run_id=witness.run_id
        AND episode.execution_id=witness.execution_id AND episode.job_id=witness.job_id AND episode.command_key=witness.command_key
        AND episode.retired_attempt=witness.retired_attempt
    JOIN workflow_runs AS owner ON owner.company_id=witness.company_id AND owner.id=witness.run_id
    JOIN background_tasks AS job ON job.company_id=witness.company_id AND job.id=witness.job_id
    JOIN workflow_executions AS execution ON execution.company_id=witness.company_id AND execution.run_id=witness.run_id AND execution.id=witness.execution_id
    JOIN workflow_run_events AS audit ON audit.company_id=witness.company_id AND audit.run_id=witness.run_id AND audit.sequence=witness.audit_sequence
    JOIN workflow_action_claim_budget_refusals AS refusal ON refusal.company_id=witness.company_id AND refusal.run_id=witness.run_id
        AND refusal.execution_id=witness.execution_id AND refusal.job_id=witness.job_id AND refusal.command_key=witness.command_key
        AND refusal.retired_attempt=witness.retired_attempt AND refusal.audit_sequence=witness.audit_sequence
    WHERE witness.company_id=$1 AND witness.command_key=$2"#;

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_historical_genuine_retirement_current_xid() {
    let s = Box::pin(scheduled()).await;
    Box::pin(exhaust(&s)).await;
    let before = all_tables(&s.fixture).await;
    assert!(
        s.fixture
            .persistence()
            .claim_io(s.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let saved = all_tables(&s.fixture).await;
    assert_refusal(&before, &saved, &s.request, &s.command);
    let mut tx = s.fixture.persistence().pool().begin().await.unwrap();
    let history: HistoricalRetirement = sqlx::query_as(HISTORICAL_RETIREMENT)
        .bind(s.request.fence.scope.company.as_uuid())
        .bind(s.command.command_key.as_str())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(history.old_transaction && history.other_predicates);
    assert_eq!(
        history.current_count, 0,
        "only historical genuine retirement exists"
    );
    let error = refusal(&mut tx, &s, history.audit_sequence)
        .await
        .unwrap_err();
    guard(
        &error,
        "invalid current workflow reconciliation budget refusal",
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        saved,
        all_tables(&s.fixture).await,
        "genuine historical retirement/refusal and all public history survive"
    );
}

#[path = "action_reconciliation_claim_sql_witness_tests.rs"]
mod witness_tests;
