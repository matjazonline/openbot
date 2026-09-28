//! Historical event provenance is checked once; deletion must not erase history.
use super::*;
use std::time::Duration;

#[path = "job_handoff_consumer_tests.rs"]
mod consumer_tests;

async fn event(
    db: &mut PgConnection,
    company: Uuid,
    task: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO thread_handoff_events
         (company_id, handoff_id, generation, command_id, command_fingerprint, operation,
          actor_kind, to_state, from_version, to_version, task_id)
         VALUES ($1, gen_random_uuid(), gen_random_uuid(), gen_random_uuid(), 'history',
                 'draft_failed', 'system', 'needs_instruction', 1, 2, $2) RETURNING id",
    )
    .bind(company)
    .bind(task)
    .fetch_one(db)
    .await
}

async fn wait_for_lock(pool: &sqlx::PgPool, pid: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT COALESCE(wait_event_type = 'Lock', false)
                 FROM pg_stat_activity WHERE pid = $1",
            )
            .bind(pid)
            .fetch_one(pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("competing transaction must wait on the task lock");
}

#[tokio::test]
async fn workflow_job_handoff_events_require_visible_scoped_legacy_provenance() {
    let f = JobFixture::new().await;
    let mut tx = f.hypothetical().await;
    let company = f.binding.target.company.as_uuid();
    let workflow = f.insert(&mut tx, f.payload()).await.unwrap();
    let task = legacy(&f, &mut tx).await;
    let foreign_company = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO companies (id, user_id, name, slug) VALUES ($1, $2, 'Foreign', $1::text)",
    )
    .bind(foreign_company)
    .bind(f.binding.target.actor.user_id())
    .execute(&mut *tx)
    .await
    .unwrap();
    event(&mut tx, company, Some(task)).await.unwrap();
    event(&mut tx, company, None).await.unwrap();
    for (owner, task) in [
        (company, workflow),
        (company, Uuid::new_v4()),
        (foreign_company, task),
    ] {
        sqlx::query("SAVEPOINT provenance")
            .execute(&mut *tx)
            .await
            .unwrap();
        constraint(
            event(&mut tx, owner, Some(task)).await.unwrap_err(),
            "legacy_handoff_event_task_provenance",
        );
        sqlx::query("ROLLBACK TO SAVEPOINT provenance")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM thread_handoff_events")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 2);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_job_handoff_events_reject_invisible_task_without_waiting() {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let company = f.binding.target.company.as_uuid();
    let mut creator = pool.begin().await.unwrap();
    let task = legacy(&f, &mut creator).await;
    let mut writer = pool.acquire().await.unwrap();
    constraint(
        tokio::time::timeout(
            Duration::from_secs(5),
            event(&mut writer, company, Some(task)),
        )
        .await
        .expect("invisible task must be rejected before waiting")
        .unwrap_err(),
        "legacy_handoff_event_task_provenance",
    );
    creator.commit().await.unwrap();
    event(&mut writer, company, Some(task)).await.unwrap();
}

#[tokio::test]
async fn workflow_job_handoff_event_locks_task_and_retains_immutable_history_after_delete() {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let company = f.binding.target.company.as_uuid();
    let task = legacy(&f, &mut pool.acquire().await.unwrap()).await;
    let mut writer = pool.begin().await.unwrap();
    let id = event(&mut writer, company, Some(task)).await.unwrap();
    let before: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(event) FROM thread_handoff_events AS event WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&mut *writer)
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
        deleter.commit().await.unwrap();
    });
    wait_for_lock(pool, pid).await;
    writer.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), deletion)
        .await
        .unwrap()
        .unwrap();
    let after: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(event) FROM thread_handoff_events AS event WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(before, after, "history must not depend on a live task");
    for query in [
        "UPDATE thread_handoff_events SET task_id = NULL WHERE id = $1",
        "DELETE FROM thread_handoff_events WHERE id = $1",
    ] {
        let error = sqlx::query(query).bind(id).execute(pool).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("thread handoff events are immutable")
        );
    }
    constraint(
        event(&mut pool.acquire().await.unwrap(), company, Some(task))
            .await
            .unwrap_err(),
        "legacy_handoff_event_task_provenance",
    );
}

#[tokio::test]
async fn workflow_job_handoff_event_waits_for_prior_delete_then_rejects() {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let company = f.binding.target.company.as_uuid();
    let task = legacy(&f, &mut pool.acquire().await.unwrap()).await;
    let mut deleter = pool.begin().await.unwrap();
    sqlx::query("DELETE FROM background_tasks WHERE id = $1")
        .bind(task)
        .execute(&mut *deleter)
        .await
        .unwrap();
    let mut writer = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *writer)
        .await
        .unwrap();
    let insertion = tokio::spawn(async move { event(&mut writer, company, Some(task)).await });
    wait_for_lock(pool, pid).await;
    deleter.commit().await.unwrap();
    constraint(
        tokio::time::timeout(Duration::from_secs(5), insertion)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err(),
        "legacy_handoff_event_task_provenance",
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM thread_handoff_events")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
