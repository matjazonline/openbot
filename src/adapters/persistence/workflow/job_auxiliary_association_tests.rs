//! Every newly guarded scoped owner rejects raw foreign/workflow/invisible links.
use super::auxiliary_fixtures as fixtures;
use super::*;

#[tokio::test]
async fn workflow_job_auxiliary_scoped_insert_update_matrix() {
    let f = JobFixture::new().await;
    let mut tx = f.hypothetical().await;
    let workflow = f.insert(&mut tx, f.payload()).await.unwrap();
    let task = legacy(&f, &mut tx).await;
    fixtures::seed(&f, &mut tx, task).await;
    for table in fixtures::OWNERS {
        let source: serde_json::Value = sqlx::query_scalar(&format!(
            "SELECT to_jsonb(source) FROM {table} AS source WHERE task_id = $1 LIMIT 1"
        ))
        .bind(task)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        for (target, company) in [
            (workflow, f.binding.target.company.as_uuid()),
            (Uuid::new_v4(), f.binding.target.company.as_uuid()),
            (task, Uuid::new_v4()),
        ] {
            let mut record = source.clone();
            record["task_id"] = json!(target);
            record["company_id"] = json!(company);
            sqlx::query("SAVEPOINT auxiliary")
                .execute(&mut *tx)
                .await
                .unwrap();
            let error = fixtures::insert_record(&mut tx, table, &record)
                .await
                .unwrap_err();
            constraint(error, "legacy_auxiliary_task_queue_kind");
            sqlx::query("ROLLBACK TO SAVEPOINT auxiliary")
                .execute(&mut *tx)
                .await
                .unwrap();
            let error = sqlx::query(&format!(
                "UPDATE {table} SET task_id = $2, company_id = $3 WHERE task_id = $1"
            ))
            .bind(task)
            .bind(target)
            .bind(company)
            .execute(&mut *tx)
            .await
            .unwrap_err();
            constraint(error, "legacy_auxiliary_task_queue_kind");
            sqlx::query("ROLLBACK TO SAVEPOINT auxiliary")
                .execute(&mut *tx)
                .await
                .unwrap();
        }
    }
    // Workflow jobs retain the shared attempt owner after every auxiliary guard is installed.
    sqlx::query("INSERT INTO task_attempts (id, task_id, attempt_number, status, execution_generation, worker_id, machine_id) VALUES (gen_random_uuid(), $1, 1, 'processing', gen_random_uuid(), gen_random_uuid(), 'test')")
        .bind(workflow).execute(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_job_auxiliary_reject_invisible_parent_before_fk_wait() {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let mut creator = pool.begin().await.unwrap();
    let task = legacy(&f, &mut creator).await;
    let mut writer = pool.begin().await.unwrap();
    for table in fixtures::OWNERS {
        sqlx::query("SAVEPOINT invisible")
            .execute(&mut *writer)
            .await
            .unwrap();
        // BEFORE guards must run before other required-column/FK checks, and fail
        // immediately while the other connection still owns its invisible task.
        let record = json!({"company_id":f.binding.target.company.as_uuid(),"task_id":task});
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            fixtures::insert_record(&mut writer, table, &record),
        )
        .await
        .expect("must reject without waiting for task commit")
        .unwrap_err();
        constraint(error, "legacy_auxiliary_task_queue_kind");
        sqlx::query("ROLLBACK TO SAVEPOINT invisible")
            .execute(&mut *writer)
            .await
            .unwrap();
    }
    creator.commit().await.unwrap();
    fixtures::seed(&f, &mut writer, task).await;
    writer.rollback().await.unwrap();
    sqlx::query("DELETE FROM background_tasks WHERE id = $1")
        .bind(task)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn workflow_job_auxiliary_fk_serializes_delete_and_preserves_cascades() {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let mut setup = pool.begin().await.unwrap();
    let task = legacy(&f, &mut setup).await;
    fixtures::seed(&f, &mut setup, task).await;
    setup.commit().await.unwrap();
    let mut listener = sqlx::postgres::PgListener::connect_with(pool)
        .await
        .unwrap();
    listener
        .listen_all(["task_chain_changed", "auxiliary_delete_done"])
        .await
        .unwrap();
    let mut writer = pool.begin().await.unwrap();
    fixtures::insert_record(
        &mut writer,
        "start_agent_task_commands",
        &json!({
        "company_id":f.binding.target.company.as_uuid(),"task_id":task,
        "command_id":Uuid::new_v4(),"command_fingerprint":"concurrent"}),
    )
    .await
    .unwrap();
    let mut deleter = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *deleter)
        .await
        .unwrap();
    let deletion = tokio::spawn(async move {
        sqlx::query("DELETE FROM background_tasks WHERE id = $1")
            .bind(task)
            .execute(&mut *deleter)
            .await
            .unwrap();
        sqlx::query("SELECT pg_notify('auxiliary_delete_done', 'done')")
            .execute(&mut *deleter)
            .await
            .unwrap();
        deleter.commit().await.unwrap();
    });
    // Observe actual lock contention, not a scheduler-dependent short sleep.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT COALESCE(wait_event_type = 'Lock', false) FROM pg_stat_activity WHERE pid = $1")
                .bind(pid).fetch_one(pool).await.unwrap();
            if waiting { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("FK must hold deletion until auxiliary writer commits");
    writer.commit().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), deletion)
        .await
        .unwrap()
        .unwrap();
    for table in fixtures::OWNERS {
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE task_id = $1"))
                .bind(task)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(count, 0, "cascade must remove {table}");
    }
    for table in ["task_outreach_targets", "task_outreach_replies"] {
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM {table} WHERE company_id = $1"
        ))
        .bind(f.binding.target.company.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(count, 0, "cascade must remove {table}");
    }
    loop {
        let notice = tokio::time::timeout(std::time::Duration::from_secs(5), listener.recv())
            .await
            .unwrap()
            .unwrap();
        if notice.channel() == "auxiliary_delete_done" {
            break;
        }
        assert_eq!(notice.channel(), "task_chain_changed");
        assert!(notice.payload().contains(&task.to_string()));
    }
}
