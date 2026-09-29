use super::*;

#[tokio::test]
async fn workflow_maintenance_wait_deadline_crossing_uses_run_expiry() {
    let mut source: Value =
        serde_json::from_str(&registry::example("wait.event").unwrap().source).unwrap();
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(30)).to_rfc3339()});
    let (f, scope) = fixture_source(source).await;
    f.persistence().park_wait(scope).await.unwrap().unwrap();
    sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp()+interval '300 milliseconds' WHERE id=$1")
        .bind(scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE id=$1 FOR UPDATE")
        .bind(scope.execution.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let resume = f.persistence().resume_wait(scope);
    tokio::pin!(resume);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut resume)
            .await
            .is_err()
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    blocker.commit().await.unwrap();
    assert_eq!(
        resume.await.unwrap(),
        WaitProgress::Expired(CommitDisposition::Committed)
    );
    let saved = evidence(&f).await;
    assert_eq!(
        saved["runs"][0]["terminal_execution_id"],
        json!(scope.execution.as_uuid())
    );
    assert!(
        saved["audit"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event_kind"] == "run_deadline_expired")
    );
    assert!(
        !saved["audit"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event_kind"] == "wait_expired")
    );
    assert_eq!(
        f.persistence().resume_wait(scope).await.unwrap(),
        WaitProgress::Expired(CommitDisposition::Replayed)
    );
    assert_eq!(evidence(&f).await, saved);
}

#[tokio::test]
async fn workflow_maintenance_forged_retirement_is_rejected_and_rolled_back() {
    for forgery in [
        "predeadline",
        "generation",
        "worker",
        "attempt",
        "debit",
        "retry",
        "successor",
    ] {
        let (f, scope) = fixture().await;
        claimed(&f, scope).await;
        if forgery != "predeadline" {
            overdue(&f, scope).await;
        }
        let before = evidence(&f).await;
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let result: Result<(), sqlx::Error> = async {
            sqlx::query("UPDATE task_attempts SET status='failed',finished_at=clock_timestamp(),workflow_failure_class='terminal',workflow_failure_code='workflow.run_deadline',workflow_retry_safety='unknown',workflow_retirement='deadline',stop_reason='timed_out' WHERE task_id=$1")
                .bind(scope.job.0).execute(&mut *tx).await?;
            let tamper = match forgery {
                "generation" => Some("UPDATE task_attempts SET execution_generation=gen_random_uuid() WHERE task_id=$1"),
                "worker" => Some("UPDATE task_attempts SET worker_id=gen_random_uuid() WHERE task_id=$1"),
                "attempt" => Some("UPDATE task_attempts SET attempt_number=attempt_number+1 WHERE task_id=$1"),
                _ => None,
            };
            if let Some(sql) = tamper { sqlx::query(sql).bind(scope.job.0).execute(&mut *tx).await?; }
            sqlx::query("UPDATE background_tasks SET status=$2,retry_count=retry_count+$3,worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL,run_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
                .bind(scope.job.0).bind(if forgery == "retry" { "pending" } else { "failed" }).bind(if forgery == "debit" { 0i32 } else { 1 }).execute(&mut *tx).await?;
            sqlx::query("UPDATE workflow_runs SET state='failed',waiting_reason=NULL WHERE id=$1")
                .bind(scope.run.as_uuid()).execute(&mut *tx).await?;
            if forgery == "successor" {
                let next = Uuid::new_v4();
                sqlx::query("INSERT INTO workflow_executions(company_id,run_id,id,step_id,activation) SELECT company_id,run_id,$2,'next',activation+1 FROM workflow_executions WHERE id=$1")
                    .bind(scope.execution.as_uuid()).bind(next).execute(&mut *tx).await?;
                sqlx::query("UPDATE workflow_executions SET completed_at=clock_timestamp(),committed_output='null'::jsonb,committed_route='final_error',route_target='next',successor_execution_id=$2 WHERE id=$1")
                    .bind(scope.execution.as_uuid()).bind(next).execute(&mut *tx).await?;
            }
            Ok(())
        }.await;
        result.unwrap_or_else(|error| panic!("forgery fixture {forgery}: {error}"));
        let error = sqlx::query("SET CONSTRAINTS workflow_fenced_retirement_guard IMMEDIATE")
            .execute(&mut *tx)
            .await
            .expect_err("forged retirement must fail its own guard");
        let database = error.as_database_error().unwrap();
        assert_eq!(database.code().as_deref(), Some("23514"), "{forgery}");
        let expected = if matches!(forgery, "predeadline" | "successor") {
            "invalid workflow deadline retirement"
        } else {
            "workflow failure lost ownership or retry eligibility"
        };
        assert_eq!(database.message(), expected, "{forgery}");
        tx.rollback().await.unwrap();
        assert_eq!(evidence(&f).await, before, "{forgery}");
    }
}
