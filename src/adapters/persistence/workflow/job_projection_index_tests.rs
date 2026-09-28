//! Exercise the production lookup on retained mixed history in an isolated migrated database.
use super::*;
use crate::adapters::persistence::thread::THREAD_TASK_LOOKUP_SQL;

async fn seed_mixed_history(f: &JobFixture, legacy: Uuid, hot: Uuid, absent: Uuid) -> Uuid {
    let pool = f.binding.fixture.persistence.pool();
    let company = f.binding.target.company.as_uuid();
    let thread = f.thread;
    // Use a non-main legacy task, older than workflow history. Merely including queue_kind in
    // the index would still walk all the newer workflow rows before finding this legacy winner.
    sqlx::query("UPDATE background_tasks SET thread_id = $2, correlation_id = $3, created_at = '2000-01-01' WHERE id = $1")
        .bind(legacy).bind(thread).bind(hot).execute(pool).await.unwrap();
    sqlx::query(
        r#"WITH executions AS (
            INSERT INTO workflow_executions (company_id,run_id,id,step_id,activation)
            SELECT original.company_id,original.run_id,gen_random_uuid(),original.step_id,history.ordinal + 1
            FROM workflow_executions AS original CROSS JOIN generate_series(1,12000) AS history(ordinal)
            WHERE original.company_id = $1 AND original.id = $4
            RETURNING id,activation)
        INSERT INTO background_tasks
               (id, company_id, channel_id, thread_id, correlation_id, task_type, status,
                queue_kind, workflow_execution_id, payload)
           SELECT gen_random_uuid(), $1, $2, $3, ($5::uuid[])[CASE WHEN executions.activation <= 6001 THEN 1 ELSE 2 END],
                  'workflow_execution', 'completed', 'workflow', executions.id,
                  jsonb_build_object('version',1,'execution_id',executions.id::text)
           FROM executions"#,
    )
    .bind(company)
    .bind(f.channel)
    .bind(thread)
    .bind(f.execution)
    .bind([hot, absent])
    .execute(pool)
    .await
    .unwrap();
    // Singleton correlations keep the planner's average low, exposing an uncovered fallback.
    sqlx::query(
        r#"INSERT INTO background_tasks
               (id, company_id, channel_id, thread_id, correlation_id, task_type, status)
           SELECT gen_random_uuid(), $1, $2, $3, gen_random_uuid(), 'history', 'completed'
           FROM generate_series(1, 10000)"#,
    )
    .bind(company)
    .bind(f.channel)
    .bind(thread)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("VACUUM (ANALYZE) background_tasks")
        .execute(pool)
        .await
        .unwrap();
    thread
}

fn assert_bounded_plan(plan: &serde_json::Value) {
    let mut pending = vec![&plan[0]["Plan"]];
    let mut fallback = None;
    while let Some(node) = pending.pop() {
        assert!(
            node["Node Type"] != "Sort" && node["Node Type"] != "Incremental Sort",
            "matching must not sort retained mixed history: {plan}"
        );
        if node["Index Name"] == "background_tasks_thread_correlation_match_idx" {
            fallback = Some(node);
        }
        if let Some(children) = node["Plans"].as_array() {
            pending.extend(children);
        }
    }
    let fallback = fallback.expect("fallback uses the ordered covering index");
    assert!(fallback["Actual Rows"].as_f64().unwrap() <= 1.0, "{plan}");
    assert_eq!(fallback["Actual Loops"].as_u64(), Some(200), "{plan}");
    assert!(
        fallback["Index Cond"]
            .as_str()
            .unwrap()
            .contains("queue_kind"),
        "the discriminator must bound the index probe: {plan}"
    );
    assert_eq!(
        fallback["Rows Removed by Filter"].as_f64().unwrap_or(0.0),
        0.0,
        "workflow history must not be scanned then discarded: {plan}"
    );
}

#[tokio::test]
async fn workflow_job_projection_lookup_bounds_workflow_heavy_history() {
    let (f, _, legacy) = mixed_fixture().await;
    let (hot, absent) = (Uuid::new_v4(), Uuid::new_v4());
    let thread = seed_mixed_history(&f, legacy, hot, absent).await;
    let messages: Vec<Uuid> = (0..200).map(|_| Uuid::new_v4()).collect();
    let correlations: Vec<Uuid> = (0..200)
        .map(|i| if i % 2 == 0 { hot } else { absent })
        .collect();
    let mut db = f
        .binding
        .fixture
        .persistence
        .pool()
        .acquire()
        .await
        .unwrap();
    for mode in ["force_custom_plan", "force_generic_plan"] {
        sqlx::query(&format!("SET plan_cache_mode = {mode}"))
            .execute(&mut *db)
            .await
            .unwrap();
        let matches = sqlx::query_as::<_, (Uuid, Option<Uuid>)>(THREAD_TASK_LOOKUP_SQL)
            .bind(f.binding.target.company.as_uuid())
            .bind(thread)
            .bind(&messages)
            .bind(&correlations)
            .fetch_all(&mut *db)
            .await
            .unwrap();
        assert_eq!(matches.len(), 200);
        for (i, (message, task)) in matches.iter().enumerate() {
            assert_eq!(*message, messages[i]);
            assert_eq!(*task, if i % 2 == 0 { Some(legacy) } else { None });
        }
        let plan: serde_json::Value = sqlx::query_scalar(&format!(
            "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) {THREAD_TASK_LOOKUP_SQL}"
        ))
        .bind(f.binding.target.company.as_uuid())
        .bind(thread)
        .bind(&messages)
        .bind(&correlations)
        .fetch_one(&mut *db)
        .await
        .unwrap();
        assert_bounded_plan(&plan);
    }
}
