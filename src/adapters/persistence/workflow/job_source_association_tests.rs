//! Provenance-losing legacy sources cannot attach to workflow jobs, even through raw SQL.
use super::*;

#[tokio::test]
async fn workflow_job_sources_reject_insert_and_reassociation() {
    let f = JobFixture::new().await;
    let mut tx = f.hypothetical().await;
    let workflow = f.insert(&mut tx, f.payload()).await.unwrap();
    let task = legacy(&f, &mut tx).await;
    let legacy_sources = notification_fixtures::auxiliary_sources(&f, &mut tx, Some(task)).await;
    let taskless_sources = notification_fixtures::auxiliary_sources(&f, &mut tx, None).await;
    for (index, table) in ["human_approvals", "response_drafts", "message_deliveries"]
        .into_iter()
        .enumerate()
    {
        for source in [legacy_sources[index], taskless_sources[index]] {
            assert_rejected_source(&mut tx, table, source, workflow).await;
        }
    }
    // Attempts are the shared execution ledger, not a legacy notification source.
    sqlx::query("INSERT INTO task_attempts (id, task_id, attempt_number, status, execution_generation, worker_id, machine_id) VALUES (gen_random_uuid(), $1, 1, 'processing', gen_random_uuid(), gen_random_uuid(), 'test')")
        .bind(workflow).execute(&mut *tx).await.unwrap();
    let attempts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_attempts WHERE task_id = $1")
        .bind(workflow)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(attempts, 1);
    tx.rollback().await.unwrap();
}

async fn assert_rejected_source(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    source: Uuid,
    workflow: Uuid,
) {
    // Table names come exclusively from the fixed test matrix above.
    let columns: String = sqlx::query_scalar(
        "SELECT string_agg(quote_ident(attname), ', ' ORDER BY attnum) FROM pg_attribute \
         WHERE attrelid = $1::regclass AND attnum > 0 AND NOT attisdropped AND attgenerated = ''",
    )
    .bind(table)
    .fetch_one(&mut **tx)
    .await
    .unwrap();
    let statements = [
        format!(
            "INSERT INTO {table} ({columns}) SELECT {columns} FROM (SELECT (jsonb_populate_record(NULL::{table}, to_jsonb(source) || jsonb_build_object('id', gen_random_uuid(), 'task_id', $2::uuid))).* FROM {table} AS source WHERE id = $1) AS copy"
        ),
        format!("UPDATE {table} SET task_id = $2 WHERE id = $1"),
    ];
    for statement in statements {
        sqlx::query("SAVEPOINT source_guard")
            .execute(&mut **tx)
            .await
            .unwrap();
        let error = sqlx::query(&statement)
            .bind(source)
            .bind(workflow)
            .execute(&mut **tx)
            .await
            .unwrap_err();
        constraint(error, "legacy_source_task_queue_kind");
        sqlx::query("ROLLBACK TO SAVEPOINT source_guard")
            .execute(&mut **tx)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn workflow_job_sources_writer_selection_is_scoped_and_legacy_only() {
    use crate::adapters::persistence::legacy_task::require_legacy_task_on;
    let f = JobFixture::new().await;
    let mut tx = f.hypothetical().await;
    let workflow = f.insert(&mut tx, f.payload()).await.unwrap();
    let task = legacy(&f, &mut tx).await;
    let company = f.binding.target.company.as_uuid();
    require_legacy_task_on(&mut tx, company, None)
        .await
        .unwrap();
    require_legacy_task_on(&mut tx, company, Some(task))
        .await
        .unwrap();
    for (company, task) in [
        (company, workflow),
        (company, Uuid::new_v4()),
        (Uuid::new_v4(), task),
    ] {
        assert!(matches!(
            require_legacy_task_on(&mut tx, company, Some(task)).await,
            Err(AppError::NotFound(_))
        ));
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_job_sources_reject_a_concurrently_uncommitted_task() {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let mut creator = pool.begin().await.unwrap();
    let task = legacy(&f, &mut creator).await;
    let mut source_writer = pool.begin().await.unwrap();
    let sources = notification_fixtures::auxiliary_sources(&f, &mut source_writer, None).await;
    sqlx::query("SAVEPOINT invisible_task")
        .execute(&mut *source_writer)
        .await
        .unwrap();
    // Another connection's task is invisible. Reject it before the FK can wait and
    // accept a newly committed row whose discriminator this statement never checked.
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        sqlx::query("UPDATE human_approvals SET task_id = $2 WHERE id = $1")
            .bind(sources[0])
            .bind(task)
            .execute(&mut *source_writer),
    )
    .await
    .expect("guard must reject without waiting for the creator")
    .unwrap_err();
    constraint(error, "legacy_source_task_queue_kind");
    sqlx::query("ROLLBACK TO SAVEPOINT invisible_task")
        .execute(&mut *source_writer)
        .await
        .unwrap();
    creator.commit().await.unwrap();
    sqlx::query("UPDATE human_approvals SET task_id = $2 WHERE id = $1")
        .bind(sources[0])
        .bind(task)
        .execute(&mut *source_writer)
        .await
        .unwrap();
    source_writer.rollback().await.unwrap();
    // Do not leave a globally claimable legacy fixture after the test.
    sqlx::query("DELETE FROM background_tasks WHERE id = $1")
        .bind(task)
        .execute(pool)
        .await
        .unwrap();
}
