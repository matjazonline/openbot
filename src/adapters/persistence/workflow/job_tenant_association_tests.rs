//! Single-column legacy links now carry actual tenant/occurrence scope.
use super::auxiliary_fixtures as fixtures;
use super::*;
use crate::application::use_cases::schedule::SchedulePersistence;

#[path = "job_tenant_fixtures.rs"]
mod tenant_fixtures;
use tenant_fixtures::Links;

async fn reject_insert_update(
    db: &mut PgConnection,
    table: &str,
    source: &serde_json::Value,
    field: &str,
    value: Uuid,
    expected: &str,
) {
    let mut bad = source.clone();
    bad[field] = json!(value);
    sqlx::query("SAVEPOINT invalid_link")
        .execute(&mut *db)
        .await
        .unwrap();
    constraint(
        fixtures::insert_record(db, table, &bad).await.unwrap_err(),
        expected,
    );
    sqlx::query("ROLLBACK TO SAVEPOINT invalid_link")
        .execute(&mut *db)
        .await
        .unwrap();
    let error = sqlx::query(&format!("UPDATE {table} SET {field} = $1"))
        .bind(value)
        .execute(&mut *db)
        .await
        .unwrap_err();
    constraint(
        error,
        if table == "schedule_runs" && ["company_id", "channel_id"].contains(&field) {
            "schedule_occurrence_identity_immutable"
        } else {
            expected
        },
    );
    sqlx::query("ROLLBACK TO SAVEPOINT invalid_link")
        .execute(db)
        .await
        .unwrap();
}

#[tokio::test]
async fn workflow_job_tenant_association_insert_update_matrix() {
    let f = JobFixture::new().await;
    let mut tx = f.hypothetical().await;
    let links = Links::seed(&f, &mut tx).await;
    let workflow = f.insert(&mut tx, f.payload()).await.unwrap();
    let provision = links.provision(&f);
    let run = links.run();
    fixtures::insert_record(&mut tx, "agent_channel_provisions", &provision)
        .await
        .unwrap();
    fixtures::insert_record(&mut tx, "schedule_runs", &run)
        .await
        .unwrap();
    for target in [workflow, Uuid::new_v4(), links.foreign_task] {
        reject_insert_update(
            &mut tx,
            "agent_channel_provisions",
            &provision,
            "task_id",
            target,
            "legacy_provision_task_scope",
        )
        .await;
        reject_insert_update(
            &mut tx,
            "schedule_runs",
            &run,
            "task_id",
            target,
            "legacy_schedule_task_scope",
        )
        .await;
    }
    for (field, value) in [
        ("agent_id", links.foreign_agent),
        ("channel_id", links.foreign_channel),
        ("company_id", links.foreign_company),
    ] {
        reject_insert_update(
            &mut tx,
            "agent_channel_provisions",
            &provision,
            field,
            value,
            "legacy_provision_task_scope",
        )
        .await;
    }
    for (field, value, expected) in [
        (
            "company_id",
            links.foreign_company,
            "legacy_schedule_owner_scope",
        ),
        (
            "channel_id",
            links.foreign_channel,
            "legacy_schedule_owner_scope",
        ),
        ("thread_id", Uuid::new_v4(), "legacy_schedule_task_scope"),
    ] {
        reject_insert_update(&mut tx, "schedule_runs", &run, field, value, expected).await;
    }
    let no_thread = legacy(&f, &mut tx).await;
    reject_insert_update(
        &mut tx,
        "schedule_runs",
        &run,
        "task_id",
        no_thread,
        "legacy_schedule_task_scope",
    )
    .await;
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_job_tenant_association_preserves_taskless_and_delete_atomicity() {
    let f = JobFixture::new().await;
    let mut tx = f.binding.fixture.persistence.pool().begin().await.unwrap();
    let links = Links::seed(&f, &mut tx).await;
    fixtures::insert_record(&mut tx, "agent_channel_provisions", &links.provision(&f))
        .await
        .unwrap();
    fixtures::insert_record(&mut tx, "schedule_runs", &links.run())
        .await
        .unwrap();
    for state in ["pending", "failed"] {
        let mut taskless = links.run();
        taskless["task_id"] = serde_json::Value::Null;
        taskless["thread_id"] = serde_json::Value::Null;
        taskless["materialization_status"] = json!(state);
        taskless["materialization_attempts"] = json!(if state == "failed" { 5 } else { 0 });
        fixtures::insert_record(&mut tx, "schedule_runs", &taskless)
            .await
            .unwrap();
    }
    // The existing materialized CHECK still rejects SET NULL atomically on deletion.
    sqlx::query("SAVEPOINT delete_task")
        .execute(&mut *tx)
        .await
        .unwrap();
    constraint(
        sqlx::query("DELETE FROM background_tasks WHERE id = $1")
            .bind(links.task)
            .execute(&mut *tx)
            .await
            .unwrap_err(),
        "schedule_runs_materialization_state_check",
    );
    sqlx::query("ROLLBACK TO SAVEPOINT delete_task")
        .execute(&mut *tx)
        .await
        .unwrap();
    let retained: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_channel_provisions WHERE task_id = $1")
            .bind(links.task)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(retained, 1);
    sqlx::query("DELETE FROM schedule_runs WHERE task_id = $1")
        .bind(links.task)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM background_tasks WHERE id = $1")
        .bind(links.task)
        .execute(&mut *tx)
        .await
        .unwrap();
    let retained: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_channel_provisions WHERE task_id = $1")
            .bind(links.task)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(retained, 0);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_job_tenant_association_rejects_invisible_task() {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let mut setup = pool.begin().await.unwrap();
    let links = Links::seed(&f, &mut setup).await;
    setup.commit().await.unwrap();
    let mut creator = pool.begin().await.unwrap();
    let invisible = legacy(&f, &mut creator).await;
    sqlx::query("UPDATE background_tasks SET thread_id = $2 WHERE id = $1")
        .bind(invisible)
        .bind(links.thread)
        .execute(&mut *creator)
        .await
        .unwrap();
    let mut writer = pool.begin().await.unwrap();
    for (table, mut record, expected) in [
        (
            "agent_channel_provisions",
            links.provision(&f),
            "legacy_provision_task_scope",
        ),
        ("schedule_runs", links.run(), "legacy_schedule_task_scope"),
    ] {
        record["task_id"] = json!(invisible);
        sqlx::query("SAVEPOINT invisible")
            .execute(&mut *writer)
            .await
            .unwrap();
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            fixtures::insert_record(&mut writer, table, &record),
        )
        .await
        .expect("reject before FK wait")
        .unwrap_err();
        constraint(error, expected);
        sqlx::query("ROLLBACK TO SAVEPOINT invisible")
            .execute(&mut *writer)
            .await
            .unwrap();
    }
    creator.commit().await.unwrap();
    let mut provision = links.provision(&f);
    provision["task_id"] = json!(invisible);
    fixtures::insert_record(&mut writer, "agent_channel_provisions", &provision)
        .await
        .unwrap();
    let mut run = links.run();
    run["task_id"] = json!(invisible);
    fixtures::insert_record(&mut writer, "schedule_runs", &run)
        .await
        .unwrap();
    writer.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_job_schedule_record_preserves_scope_and_lease_fence() {
    let f = JobFixture::new().await;
    let persistence = &f.binding.fixture.persistence;
    let pool = persistence.pool();
    let mut tx = f.hypothetical().await;
    let links = Links::seed(&f, &mut tx).await;
    let workflow = f.insert(&mut tx, f.payload()).await.unwrap();
    let worker = Uuid::new_v4();
    let generation = Uuid::new_v4();
    let mut run = links.run();
    run["task_id"] = serde_json::Value::Null;
    run["materialization_status"] = json!("materializing");
    run["materialization_attempts"] = json!(1);
    run["materialization_worker_id"] = json!(worker);
    run["materialization_generation"] = json!(generation);
    run["materialization_locked_at"] = json!(chrono::Utc::now());
    run["materialization_lock_expires_at"] =
        json!(chrono::Utc::now() + chrono::Duration::minutes(5));
    let run_id = serde_json::from_value::<Uuid>(run["id"].clone()).unwrap();
    fixtures::insert_record(&mut tx, "schedule_runs", &run)
        .await
        .unwrap();
    tx.commit().await.unwrap(); // only this disposable DB admits hypothetical workflow rows
    let before: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(run) FROM schedule_runs AS run WHERE id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .unwrap();
    for (task, claimant, epoch) in [
        (workflow, worker, generation),
        (Uuid::new_v4(), worker, generation),
        (links.foreign_task, worker, generation),
        (links.task, Uuid::new_v4(), generation),
        (links.task, worker, Uuid::new_v4()),
    ] {
        assert!(
            !persistence
                .record_run_task(run_id, claimant, epoch, task)
                .await
                .unwrap()
        );
        let after: serde_json::Value =
            sqlx::query_scalar("SELECT to_jsonb(run) FROM schedule_runs AS run WHERE id = $1")
                .bind(run_id)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(before, after);
    }
    sqlx::query("UPDATE schedule_runs SET materialization_locked_at = CURRENT_TIMESTAMP - INTERVAL '2 minutes', materialization_lock_expires_at = CURRENT_TIMESTAMP - INTERVAL '1 minute' WHERE id = $1")
        .bind(run_id).execute(pool).await.unwrap();
    assert!(
        !persistence
            .record_run_task(run_id, worker, generation, links.task)
            .await
            .unwrap()
    );
    sqlx::query("UPDATE schedule_runs SET materialization_lock_expires_at = CURRENT_TIMESTAMP + INTERVAL '5 minutes' WHERE id = $1")
        .bind(run_id).execute(pool).await.unwrap();
    // The schedule channel cannot change; rejection leaves the captured run usable.
    let error = sqlx::query(
        "UPDATE channel_schedules SET channel_id = $2, name = 'Rejected edit' WHERE id = $1",
    )
    .bind(links.schedule)
    .bind(links.moved_channel)
    .execute(pool)
    .await
    .unwrap_err();
    constraint(error, "schedule_channel_immutable");
    assert!(
        persistence
            .record_run_task(run_id, worker, generation, links.task)
            .await
            .unwrap()
    );
    assert!(
        !persistence
            .record_run_task(run_id, worker, generation, links.task)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn workflow_job_tenant_association_serializes_parent_change() {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let mut setup = pool.begin().await.unwrap();
    let links = Links::seed(&f, &mut setup).await;
    setup.commit().await.unwrap();
    let mut child = pool.begin().await.unwrap();
    fixtures::insert_record(&mut child, "schedule_runs", &links.run())
        .await
        .unwrap();
    let mut parent = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *parent)
        .await
        .unwrap();
    let task = links.task;
    let mutation = tokio::spawn(async move {
        let result = sqlx::query("UPDATE background_tasks SET thread_id = NULL WHERE id = $1")
            .bind(task)
            .execute(&mut *parent)
            .await;
        parent.rollback().await.unwrap();
        result
    });
    wait_for_lock(pool, pid).await;
    child.commit().await.unwrap();
    let error = tokio::time::timeout(std::time::Duration::from_secs(5), mutation)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    constraint(error, "schedule_runs_task_scope_fk");
}

#[tokio::test]
async fn workflow_job_schedule_capture_rejects_concurrent_reassignment() {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let mut setup = pool.begin().await.unwrap();
    let links = Links::seed(&f, &mut setup).await;
    setup.commit().await.unwrap();
    let before: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(schedule) FROM channel_schedules AS schedule WHERE id = $1",
    )
    .bind(links.schedule)
    .fetch_one(pool)
    .await
    .unwrap();
    let mut capture = pool.begin().await.unwrap();
    fixtures::insert_record(&mut capture, "schedule_runs", &links.run())
        .await
        .unwrap();
    let mut change = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *change)
        .await
        .unwrap();
    let schedule = links.schedule;
    let channel = links.moved_channel;
    let mutation = tokio::spawn(async move {
        let result = sqlx::query(
            "UPDATE channel_schedules SET channel_id = $2, name = 'Rejected edit' WHERE id = $1",
        )
        .bind(schedule)
        .bind(channel)
        .execute(&mut *change)
        .await;
        change.rollback().await.unwrap();
        result
    });
    wait_for_lock(pool, pid).await;
    capture.commit().await.unwrap();
    let error = tokio::time::timeout(std::time::Duration::from_secs(5), mutation)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    constraint(error, "schedule_channel_immutable");
    let after: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(schedule) FROM channel_schedules AS schedule WHERE id = $1",
    )
    .bind(schedule)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(before, after);
    let captured: Uuid =
        sqlx::query_scalar("SELECT channel_id FROM schedule_runs WHERE schedule_id = $1")
            .bind(schedule)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(captured, f.channel);
}

async fn wait_for_lock(pool: &sqlx::PgPool, pid: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT COALESCE(wait_event_type = 'Lock', false) FROM pg_stat_activity WHERE pid = $1")
                .bind(pid).fetch_one(pool).await.unwrap();
            if waiting { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("association FK must serialize concurrent parent mutation");
}

#[tokio::test]
async fn workflow_job_schedule_snapshot_cannot_reassign_occurrence_scope() {
    let f = JobFixture::new().await;
    let mut tx = f.binding.fixture.persistence.pool().begin().await.unwrap();
    let links = Links::seed(&f, &mut tx).await;
    let record = links.run();
    fixtures::insert_record(&mut tx, "schedule_runs", &record)
        .await
        .unwrap();
    for field in ["id", "company_id", "channel_id"] {
        let mut invalid = links.run();
        invalid["schedule_snapshot"] = json!({field:Uuid::new_v4()});
        sqlx::query("SAVEPOINT snapshot")
            .execute(&mut *tx)
            .await
            .unwrap();
        constraint(
            fixtures::insert_record(&mut tx, "schedule_runs", &invalid)
                .await
                .unwrap_err(),
            "schedule_occurrence_snapshot_scope",
        );
        sqlx::query("ROLLBACK TO SAVEPOINT snapshot")
            .execute(&mut *tx)
            .await
            .unwrap();
        constraint(
            sqlx::query("UPDATE schedule_runs SET schedule_snapshot = $1")
                .bind(&invalid["schedule_snapshot"])
                .execute(&mut *tx)
                .await
                .unwrap_err(),
            "schedule_occurrence_identity_immutable",
        );
        sqlx::query("ROLLBACK TO SAVEPOINT snapshot")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.rollback().await.unwrap();
}
