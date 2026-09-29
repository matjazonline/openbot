use super::*;

#[tokio::test]
async fn workflow_control_revision_material_transitions_and_heartbeat_noops() {
    let (f, scope) = fixture().await;
    let initial = command(&f, scope, "initial").await.expected_revision;
    f.persistence().activate(scope).await.unwrap();
    let activated = command(&f, scope, "activated").await.expected_revision;
    assert!(activated > initial);
    f.persistence().activate(scope).await.unwrap();
    assert_eq!(
        command(&f, scope, "replay").await.expected_revision,
        activated
    );
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let claimed = command(&f, scope, "claimed").await.expected_revision;
    assert!(claimed > activated);
    f.persistence()
        .renew_io(claim.fence, policy())
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE workflow_runs SET state=state WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_executions SET step_id=step_id WHERE id=$1")
        .bind(scope.execution.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query("UPDATE background_tasks SET status=status WHERE id=$1")
        .bind(scope.job.0)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        command(&f, scope, "heartbeat").await.expected_revision,
        claimed
    );
    f.persistence()
        .complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output: json!({"items":[],"token_count":0}),
        })
        .await
        .unwrap()
        .unwrap();
    assert!(command(&f, scope, "completed").await.expected_revision > claimed);
    let (f, scope) = fixture_source(waiting_source()).await;
    let initial = command(&f, scope, "initial").await.expected_revision;
    f.persistence().park_wait(scope).await.unwrap().unwrap();
    let parked = command(&f, scope, "parked").await.expected_revision;
    assert!(parked > initial);
    sqlx::query("UPDATE workflow_waits SET state=state WHERE run_id=$1")
        .bind(scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    f.persistence()
        .record_signal(wait_signal(scope))
        .await
        .unwrap();
    assert_eq!(command(&f, scope, "event").await.expected_revision, parked);
    f.persistence().resume_wait(scope).await.unwrap();
    assert!(command(&f, scope, "resumed").await.expected_revision > parked);
}

#[tokio::test]
async fn workflow_control_revision_overflow_refuses_without_partial_writes() {
    let (f, scope) = fixture().await;
    // Isolated fixture setup reaches the otherwise impractical bigint boundary.
    // Restore the trigger before any operation being tested.
    sqlx::raw_sql("ALTER TABLE workflow_runs DISABLE TRIGGER workflow_run_revision; UPDATE workflow_runs SET revision=9223372036854775807; ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_revision")
        .execute(f.persistence().pool()).await.unwrap();
    let before = control_state(&f).await;
    let cmd = command(&f, scope, "cancel").await;
    assert_eq!(cmd.expected_revision.0, i64::MAX as u64);
    let error = f.persistence().cancel(cmd).await.unwrap_err();
    assert!(
        error.to_string().contains("workflow revision exhausted")
            || error.to_string().contains("bigint out of range"),
        "{error}"
    );
    assert_eq!(control_state(&f).await, before);
    let error = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("bigint out of range")
            || error.to_string().contains("revision exhausted"),
        "{error}"
    );
    assert_eq!(control_state(&f).await, before);
}

async fn forge_cancel(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope: ActivationRequest,
    forgery: &str,
) {
    sqlx::query("UPDATE task_attempts SET status='failed',finished_at=clock_timestamp(),workflow_failure_class='terminal',workflow_failure_code='workflow.cancelled',workflow_retry_safety='unknown',workflow_retirement='cancel',stop_reason=NULL WHERE task_id=$1")
        .bind(scope.job.0).execute(&mut **tx).await.unwrap();
    let tamper = match forgery {
        "generation" => {
            Some("UPDATE task_attempts SET execution_generation=gen_random_uuid() WHERE task_id=$1")
        }
        "worker" => Some("UPDATE task_attempts SET worker_id=gen_random_uuid() WHERE task_id=$1"),
        "attempt" => {
            Some("UPDATE task_attempts SET attempt_number=attempt_number+1 WHERE task_id=$1")
        }
        _ => None,
    };
    if let Some(sql) = tamper {
        sqlx::query(sql)
            .bind(scope.job.0)
            .execute(&mut **tx)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE background_tasks SET status=$2,retry_count=retry_count+$3,worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL,run_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
        .bind(scope.job.0).bind(if forgery == "retry" { "pending" } else { "failed" }).bind(if forgery == "debit" { 0i32 } else { 1 }).execute(&mut **tx).await.unwrap();
    if forgery != "state" {
        sqlx::query("UPDATE workflow_runs SET state='cancelled',waiting_reason=NULL WHERE id=$1")
            .bind(scope.run.as_uuid())
            .execute(&mut **tx)
            .await
            .unwrap();
    }
    if forgery == "successor" {
        let next = Uuid::new_v4();
        sqlx::query("INSERT INTO workflow_executions(company_id,run_id,id,step_id,activation) SELECT company_id,run_id,$2,'next',activation+1 FROM workflow_executions WHERE id=$1")
            .bind(scope.execution.as_uuid()).bind(next).execute(&mut **tx).await.unwrap();
        sqlx::query("UPDATE workflow_executions SET completed_at=clock_timestamp(),committed_output='null'::jsonb,committed_route='final_error',route_target='next',successor_execution_id=$2 WHERE id=$1")
            .bind(scope.execution.as_uuid()).bind(next).execute(&mut **tx).await.unwrap();
    }
}

#[tokio::test]
async fn workflow_control_forged_cancellation_retirement_fails_own_guard() {
    for forgery in [
        "state",
        "generation",
        "worker",
        "attempt",
        "debit",
        "retry",
        "successor",
    ] {
        let (f, scope) = fixture().await;
        f.persistence()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .unwrap();
        let before = control_state(&f).await;
        let mut tx = f.persistence().pool().begin().await.unwrap();
        forge_cancel(&mut tx, scope, forgery).await;
        let error = sqlx::query("SET CONSTRAINTS workflow_fenced_retirement_guard IMMEDIATE")
            .execute(&mut *tx)
            .await
            .unwrap_err();
        let database = error.as_database_error().unwrap();
        assert_eq!(database.code().as_deref(), Some("23514"), "{forgery}");
        let expected = if matches!(forgery, "state" | "successor") {
            "invalid workflow cancellation retirement"
        } else {
            "workflow failure lost ownership or retry eligibility"
        };
        assert_eq!(database.message(), expected, "{forgery}");
        tx.rollback().await.unwrap();
        assert_eq!(control_state(&f).await, before, "{forgery}");
    }
}

#[tokio::test]
async fn workflow_control_direct_retry_without_receipt_fails_own_guard() {
    let (f, scope) = fixture().await;
    failed(&f, scope, RetrySafety::SafeToRetry).await;
    let before = control_state(&f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("UPDATE workflow_runs SET state='running',terminal_execution_id=NULL WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE background_tasks SET status='pending' WHERE id=$1")
        .bind(scope.job.0)
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("SET CONSTRAINTS workflow_control_retry_guard IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "invalid explicit workflow retry"
    );
    tx.rollback().await.unwrap();
    assert_eq!(control_state(&f).await, before);
}

#[tokio::test]
async fn workflow_control_cancel_commit_crossing_lease_expiry_preserves_retirement() {
    let (f, scope) = fixture().await;
    f.persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '400 milliseconds' WHERE id=$1")
        .bind(scope.job.0).execute(f.persistence().pool()).await.unwrap();
    sqlx::raw_sql("CREATE FUNCTION control_slow_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(0.6); RETURN NEW; END $$; CREATE CONSTRAINT TRIGGER control_slow_commit AFTER INSERT ON workflow_control_commands DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION control_slow_commit()")
        .execute(f.persistence().pool()).await.unwrap();
    assert!(matches!(
        f.persistence()
            .cancel(command(&f, scope, "cancel").await)
            .await
            .unwrap(),
        CancelResult::Applied { .. }
    ));
    let state = control_state(&f).await;
    assert_eq!(state["task_attempts"][0]["workflow_retirement"], "cancel");
    assert_eq!(state["background_tasks"][0]["retry_count"], 1);
    assert_eq!(state["workflow_runs"][0]["state"], "cancelled");
}
