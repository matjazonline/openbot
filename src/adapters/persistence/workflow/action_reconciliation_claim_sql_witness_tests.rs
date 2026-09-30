//! Layered A coverage: the production function on a reference relation, plus
//! catalog attachment and genuine public scheduling. Not a public stale INSERT.
use super::*;

async fn production_definitions(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('function',pg_get_functiondef('public.workflow_action_claim_episode_guard()'::regprocedure),'trigger',pg_get_triggerdef(oid),'enabled',tgenabled) FROM pg_trigger WHERE tgrelid='public.workflow_action_claim_episodes'::regclass AND tgname='workflow_action_claim_episode_guard'")
        .fetch_one(f.persistence().pool()).await.unwrap()
}

async fn install_reference(s: &PreparedSchedule, schema: &str) {
    // Generated identifier only. Explicit six-column relation: no production keys/history.
    sqlx::raw_sql(&format!(r#"
        CREATE SCHEMA {schema};
        CREATE TABLE {schema}.episode (
            company_id uuid NOT NULL, run_id uuid NOT NULL, execution_id uuid NOT NULL,
            job_id uuid NOT NULL, command_key text NOT NULL, retired_attempt integer NOT NULL);
        CREATE CONSTRAINT TRIGGER reference_guard AFTER INSERT ON {schema}.episode
            DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
            EXECUTE FUNCTION public.workflow_action_claim_episode_guard();
        CREATE FUNCTION {schema}.checkpoint() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN
            INSERT INTO {schema}.episode(company_id,run_id,execution_id,job_id,command_key,retired_attempt)
                VALUES(NEW.company_id,NEW.run_id,NEW.execution_id,NEW.job_id,NEW.command_key,NEW.retired_attempt);
            SET CONSTRAINTS {schema}.reference_guard IMMEDIATE;
            RETURN NEW;
        END $$;
    "#)).execute(s.fixture.persistence().pool()).await.unwrap();
    let scope = s.request.fence.scope;
    // UUIDs, integer and fixed fixture key, never arbitrary client SQL.
    assert_eq!(s.command.command_key.as_str(), "episode-checkpoint");
    sqlx::raw_sql(&format!(
        r#"
        CREATE TRIGGER {schema} BEFORE INSERT ON public.workflow_action_claim_episodes
        FOR EACH ROW WHEN (NEW.company_id='{}' AND NEW.run_id='{}'
            AND NEW.execution_id='{}' AND NEW.job_id='{}'
            AND NEW.command_key='episode-checkpoint' AND NEW.retired_attempt={})
        EXECUTE FUNCTION {schema}.checkpoint();
    "#,
        scope.company.as_uuid(),
        scope.run.as_uuid(),
        scope.execution.as_uuid(),
        scope.job.0,
        s.request.fence.attempt.0
    ))
    .execute(s.fixture.persistence().pool())
    .await
    .unwrap();
    let attached: bool = sqlx::query_scalar("SELECT count(*)=2 AND bool_and(tgfoid='public.workflow_action_claim_episode_guard()'::regprocedure AND tgenabled='O' AND tgtype=5 AND tgdeferrable AND tginitdeferred) FROM pg_trigger WHERE (tgrelid='public.workflow_action_claim_episodes'::regclass AND tgname='workflow_action_claim_episode_guard') OR (tgrelid=to_regclass($1) AND tgname='reference_guard')")
        .bind(format!("{schema}.episode")).fetch_one(s.fixture.persistence().pool()).await.unwrap();
    assert!(
        attached,
        "reference and public deferred row AFTER INSERT use the production OID"
    );
}

const PREREQUISITES: &str = r#"
SELECT count(*)=1
    AND bool_and(schedule.transaction_id<>pg_current_xact_id()
        AND state.transaction_id<>pg_current_xact_id()
        AND command.outcome->>'kind'='scheduled' AND command.result_revision=owner.revision
        AND state.initial_state='waiting' AND state.initial_waiting_reason='reconciliation'
        AND job.retry_count=episode.retired_attempt AND job.status='pending'
        AND job.worker_id IS NULL AND job.execution_generation IS NULL AND job.lock_expires_at IS NULL
        AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
        AND NOT EXISTS(SELECT 1 FROM task_attempts AS later
            WHERE later.task_id=episode.job_id AND later.attempt_number>episode.retired_attempt)
        AND NOT EXISTS(SELECT 1 FROM workflow_action_schedule_witnesses AS current_schedule
            WHERE current_schedule.company_id=episode.company_id AND current_schedule.run_id=episode.run_id
                AND current_schedule.execution_id=episode.execution_id AND current_schedule.job_id=episode.job_id
                AND current_schedule.retired_attempt=episode.retired_attempt
                AND current_schedule.transaction_id=pg_current_xact_id())
        AND NOT EXISTS(SELECT 1 FROM workflow_action_state_witnesses AS current_state
            WHERE current_state.company_id=episode.company_id AND current_state.run_id=episode.run_id
                AND current_state.transaction_id=pg_current_xact_id()))
FROM workflow_action_claim_episodes AS episode
JOIN workflow_action_evidence_commands AS command ON command.company_id=episode.company_id
    AND command.run_id=episode.run_id AND command.execution_id=episode.execution_id
    AND command.scheduled_job_id=episode.job_id AND command.command_key=episode.command_key
JOIN workflow_runs AS owner ON owner.company_id=episode.company_id AND owner.id=episode.run_id
JOIN background_tasks AS job ON job.company_id=episode.company_id AND job.id=episode.job_id
JOIN task_attempts AS attempt ON attempt.task_id=episode.job_id AND attempt.attempt_number=episode.retired_attempt
JOIN workflow_action_schedule_witnesses AS schedule ON schedule.company_id=episode.company_id
    AND schedule.run_id=episode.run_id AND schedule.execution_id=episode.execution_id
    AND schedule.job_id=episode.job_id AND schedule.retired_attempt=episode.retired_attempt
JOIN workflow_action_state_witnesses AS state ON state.company_id=episode.company_id
    AND state.run_id=episode.run_id AND state.transaction_id=schedule.transaction_id
WHERE episode.company_id=$1 AND episode.command_key=$2
"#;

async fn reject_historical(s: &PreparedSchedule, schema: &str) {
    let before = all_tables(&s.fixture).await;
    let pool = s.fixture.persistence().pool();
    let mut tx = pool.begin().await.unwrap();
    let valid: bool = sqlx::query_scalar(PREREQUISITES)
        .bind(s.request.fence.scope.company.as_uuid())
        .bind(s.command.command_key.as_str())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(
        valid,
        "fixture must retain every non-xid guard prerequisite and only historical witnesses"
    );
    sqlx::query(&format!("INSERT INTO {schema}.episode(company_id,run_id,execution_id,job_id,command_key,retired_attempt) SELECT company_id,run_id,execution_id,job_id,command_key,retired_attempt FROM public.workflow_action_claim_episodes WHERE company_id=$1 AND command_key=$2"))
        .bind(s.request.fence.scope.company.as_uuid()).bind(s.command.command_key.as_str())
        .execute(&mut *tx).await.expect("reference INSERT preparation must succeed");
    let error = sqlx::query(&format!(
        "SET CONSTRAINTS {schema}.reference_guard IMMEDIATE"
    ))
    .execute(&mut *tx)
    .await
    .expect_err("historical pair must fail the production guard");
    guard(
        &error,
        "invalid current workflow reconciliation claim episode",
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "every public table rolls back"
    );
    let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {schema}.episode"))
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "only the committed current-transaction control survives"
    );
}

async fn remove_reference(s: &PreparedSchedule, schema: &str) {
    let pool = s.fixture.persistence().pool();
    sqlx::raw_sql(&format!("DROP TRIGGER reference_guard ON {schema}.episode; DROP TABLE {schema}.episode; DROP FUNCTION {schema}.checkpoint(); DROP SCHEMA {schema};"))
        .execute(pool).await.unwrap();
    let absent: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_namespace WHERE nspname=$1) AND NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgname=$1) AND to_regclass($2) IS NULL AND to_regprocedure($3) IS NULL")
        .bind(schema).bind(format!("{schema}.episode")).bind(format!("{schema}.checkpoint()"))
        .fetch_one(pool).await.unwrap();
    assert!(absent, "all reference instrumentation is removed");
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_layered_historical_witness_pair() {
    // Box real action/scheduling seams to retain stock 2 MiB test stacks.
    let (f, request) = Box::pin(limited()).await;
    let s = Box::pin(prepare_schedule(f, request)).await;
    let definitions = production_definitions(&s.fixture).await;
    let schema = format!("witness_{}", Uuid::new_v4().simple());
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(s.fixture.persistence().pool())
        .await
        .unwrap();
    eprintln!(
        "A fixture database={database} schema={schema} scope={:?}",
        s.request.fence.scope
    );
    install_reference(&s, &schema).await;
    let result = run_command(&s.fixture, &s.command, s.verifier.clone())
        .await
        .unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let head = s
        .fixture
        .persistence()
        .head(s.command.scope.company, s.command.scope.run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.revision, head.revision);
    let reference: Value = sqlx::query_scalar(&format!(
        "SELECT jsonb_agg(to_jsonb(episode)) FROM {schema}.episode AS episode"
    ))
    .fetch_one(s.fixture.persistence().pool())
    .await
    .unwrap();
    assert_eq!(
        reference,
        all_tables(&s.fixture).await["workflow_action_claim_episodes"]
    );
    assert_eq!(reference.as_array().unwrap().len(), 1);
    sqlx::query(&format!(
        "DROP TRIGGER {schema} ON public.workflow_action_claim_episodes"
    ))
    .execute(s.fixture.persistence().pool())
    .await
    .unwrap();
    reject_historical(&s, &schema).await;
    remove_reference(&s, &schema).await;
    assert_eq!(definitions, production_definitions(&s.fixture).await);
    s.fixture.persistence().pool().close().await;
    // AdmissionFixture owns OwnDatabase; its Drop also disposes the database on panic.
}
