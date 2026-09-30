//! Assertions for explicit adversarial preparation and genuine candidate controls.
use super::*;

pub(super) enum Preparation {
    AddExecution(ActivationRequest),
    HideJob(ActivationRequest),
}

pub(super) fn assert_preparation(before: &Value, after: &Value, preparation: Preparation) {
    let mut normalized = after.clone();
    match preparation {
        Preparation::AddExecution(scope) => {
            remove_fresh_topology(before, &mut normalized, scope);
            assert_eq!(after["workflow_runs"][0]["state"], "running");
            assert!(after["workflow_runs"][0]["waiting_reason"].is_null());
        }
        Preparation::HideJob(scope) => {
            let row = normalized["background_tasks"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["id"] == json!(scope.job.0))
                .unwrap();
            assert_eq!(row["status"], "stopped");
            row["status"] = json!("pending");
            assert_eq!(after["workflow_runs"][0]["state"], "waiting");
            assert_eq!(
                after["workflow_runs"][0]["waiting_reason"],
                "reconciliation"
            );
        }
    }
    assert!(
        after["workflow_runs"][0]["revision"].as_i64().unwrap()
            > before["workflow_runs"][0]["revision"].as_i64().unwrap()
    );
    for field in ["state", "waiting_reason", "revision"] {
        normalized["workflow_runs"][0][field] = before["workflow_runs"][0][field].clone();
    }
    let old = before["workflow_action_state_witnesses"]
        .as_array()
        .unwrap();
    let new = after["workflow_action_state_witnesses"].as_array().unwrap();
    assert_eq!(new.len(), old.len() + 1);
    assert!(old.iter().all(|row| new.contains(row)));
    let added = new.iter().find(|row| !old.contains(row)).unwrap();
    assert_eq!(
        added["company_id"],
        before["workflow_runs"][0]["company_id"]
    );
    assert_eq!(added["run_id"], before["workflow_runs"][0]["id"]);
    assert_eq!(added["initial_state"], before["workflow_runs"][0]["state"]);
    assert_eq!(
        added["initial_waiting_reason"],
        before["workflow_runs"][0]["waiting_reason"]
    );
    normalized["workflow_action_state_witnesses"] =
        before["workflow_action_state_witnesses"].clone();
    assert_eq!(
        &normalized, before,
        "preparation changes only declared topology/status and generated revision/witness; protected history is byte-for-byte equal"
    );
}

fn remove_fresh_topology(before: &Value, normalized: &mut Value, scope: ActivationRequest) {
    for (table, id) in [
        ("workflow_executions", scope.execution.as_uuid()),
        ("background_tasks", scope.job.0),
    ] {
        let rows = normalized[table].as_array_mut().unwrap();
        assert_eq!(rows.len(), before[table].as_array().unwrap().len() + 1);
        let added = rows.iter().find(|row| row["id"] == json!(id)).unwrap();
        if table == "workflow_executions" {
            assert_eq!(added["activation"], 2);
            for field in [
                "activated_at",
                "completed_at",
                "successor_execution_id",
                "output",
            ] {
                assert!(added[field].is_null(), "fresh {field}");
            }
        } else {
            assert_eq!(added["status"], "pending");
            assert_eq!(added["retry_count"], 0);
            assert_eq!(
                added["payload"],
                crate::application::workflow::activation::job_payload(scope.execution)
            );
            for field in ["worker_id", "execution_generation", "lock_expires_at"] {
                assert!(added[field].is_null());
            }
        }
        rows.retain(|row| row["id"] != json!(id));
    }
}

pub(super) async fn capture_catalog(f: &AdmissionFixture) -> Value {
    let installed: bool = sqlx::query_scalar("SELECT tgfoid='public.workflow_action_capture_claim_retirement()'::regprocedure AND tgtype=19 AND tgenabled='O' AND NOT tgdeferrable FROM pg_trigger WHERE tgrelid='public.workflow_runs'::regclass AND tgname='workflow_action_capture_claim_retirement'")
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert!(
        installed,
        "actual public BEFORE UPDATE row trigger is enabled"
    );
    let names: Vec<String> = sqlx::query_scalar("SELECT tgname::text FROM pg_trigger WHERE tgrelid='public.workflow_runs'::regclass AND NOT tgisinternal AND (tgtype & 2)=2 AND (tgtype & 16)=16 ORDER BY tgname")
        .fetch_all(f.persistence().pool()).await.unwrap();
    assert_eq!(
        names,
        [
            "workflow_action_capture_claim_retirement",
            "workflow_action_capture_state_witness",
            "workflow_allocate_wakeup",
            "workflow_parent_immutable",
            "workflow_run_revision",
            "workflow_runs_preserve_association"
        ]
    );
    sqlx::query_scalar("SELECT jsonb_agg(jsonb_build_object('trigger',pg_get_triggerdef(oid),'enabled',tgenabled,'function',pg_get_functiondef(tgfoid)) ORDER BY tgname) FROM pg_trigger WHERE tgrelid='public.workflow_runs'::regclass AND NOT tgisinternal")
        .fetch_one(f.persistence().pool()).await.unwrap()
}

pub(super) async fn candidate_keys(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
) -> Vec<String> {
    sqlx::query_scalar("SELECT workflow_action_pending_claim_episode(episode.company_id,episode.run_id,episode.execution_id,episode.job_id) FROM workflow_action_claim_episodes AS episode WHERE episode.company_id=$1 AND episode.run_id=$2 AND workflow_action_pending_claim_episode(episode.company_id,episode.run_id,episode.execution_id,episode.job_id)=episode.command_key ORDER BY episode.command_key")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_all(&mut **tx).await.unwrap()
}

pub(super) async fn assert_run_prerequisites(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
) {
    let valid: bool = sqlx::query_scalar("SELECT state='running' AND waiting_reason IS NULL AND deadline>clock_timestamp() AND terminal_execution_id IS NULL AND terminal_wakeup_sequence IS NULL AND parent_run_id IS NULL AND revision<9223372036854775790 AND workflow_action_reconciliation_budget_eligible(company_id,id) FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut **tx).await.unwrap();
    assert!(
        valid,
        "live deadline, budget, terminal metadata and revision headroom"
    );
}

pub(super) async fn waiting_transition(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query("UPDATE public.workflow_runs SET state='waiting',waiting_reason='reconciliation' WHERE company_id=$1 AND id=$2 AND state='running'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut **tx).await
}

pub(super) async fn job_status(f: &AdmissionFixture, scope: ActivationRequest) -> String {
    sqlx::query_scalar("SELECT status FROM background_tasks WHERE id=$1")
        .bind(scope.job.0)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

pub(super) async fn assert_distinct_histories(
    f: &AdmissionFixture,
    first: &ActionDispatchRequest,
    second: &ActionDispatchRequest,
) {
    assert_ne!(first.fence, second.fence);
    assert_ne!(first.fence.scope.job, second.fence.scope.job);
    assert_ne!(first.scope().execution, second.scope().execution);
    assert_ne!(first.subject.invocation, second.subject.invocation);
    assert_ne!(
        scoped_marker(f, first).await,
        scoped_marker(f, second).await
    );
    let valid: bool = sqlx::query_scalar("SELECT count(*)=2 AND count(DISTINCT attempt.id)=2 AND count(DISTINCT entry.id)=2 AND bool_and(step.activated_at IS NOT NULL AND step.completed_at IS NULL AND job.status='failed' AND job.retry_count=1 AND job.worker_id IS NULL AND job.execution_generation IS NULL AND job.lock_expires_at IS NULL AND attempt.status='failed' AND attempt.finished_at IS NOT NULL AND entry.attempt_number=attempt.attempt_number) FROM workflow_executions AS step JOIN background_tasks AS job ON job.company_id=step.company_id AND job.workflow_execution_id=step.id JOIN task_attempts AS attempt ON attempt.task_id=job.id JOIN workflow_action_remote_entries AS entry ON entry.company_id=step.company_id AND entry.execution_id=step.id WHERE step.company_id=$1 AND step.run_id=$2")
        .bind(first.scope().company.as_uuid()).bind(first.scope().run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(
        valid,
        "two real activated, uncompleted, independently retired histories"
    );
    let state = all_tables(f).await;
    assert_eq!(state["workflow_runs"][0]["state"], "waiting");
    assert_eq!(
        state["workflow_runs"][0]["waiting_reason"],
        "reconciliation"
    );
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    eprintln!(
        "C fixture database={database} first={:?} second={:?}",
        first.fence, second.fence
    );
}

pub(super) async fn single_transition(
    f: &AdmissionFixture,
    scope: ActivationRequest,
    command: &ReconcileActionCommand,
) {
    let before = all_tables(f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert_eq!(
        candidate_keys(&mut tx, scope).await,
        vec![command.command_key.as_str().to_owned()]
    );
    assert_run_prerequisites(&mut tx, scope).await;
    assert_eq!(
        waiting_transition(&mut tx, scope)
            .await
            .expect("single public candidate transition succeeds")
            .rows_affected(),
        1
    );
    tx.rollback().await.unwrap();
    assert_eq!(before, all_tables(f).await);
}

pub(super) async fn normal_control() {
    // Entirely genuine normal topology; real scheduling then successful claim.
    let s = Box::pin(scheduled()).await;
    single_transition(&s.fixture, s.request.fence.scope, &s.command).await;
    let claim = s
        .fixture
        .persistence()
        .claim_io(s.request.fence.scope, worker(), policy())
        .await
        .unwrap()
        .expect("one genuine candidate is claimable");
    assert_eq!(claim.fence.attempt.0, s.request.fence.attempt.0 + 1);
    assert!(
        s.fixture
            .persistence()
            .validate_io(claim.fence, policy())
            .await
            .unwrap()
    );
    s.fixture.persistence().pool().close().await;
}

// Reuse the durable scripted-provider ledger, registering each distinct target.
pub(super) async fn verifier_for(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
) -> Arc<LedgerVerifier> {
    let action = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action;
    Arc::new(LedgerVerifier {
        persistence: f.persistence().clone(),
        registration: EvidenceVerifierRegistration::approve(
            EvidenceVerifierId::parse("fixture.ledger").unwrap(),
            EvidenceVerifierVersion::parse("v1").unwrap(),
            EvidenceProviderId::parse("fixture").unwrap(),
            &action.request().contract,
            &action.request().target,
        )
        .unwrap(),
    })
}
