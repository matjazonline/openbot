//! `goal_rollup_cost_` (Plan08 03D.1, measured in p04): the rollup's cost with more runs than
//! `GOAL_ROLLUP_RUN_LIMIT`. Ignored: it writes 10,001 runs, so run it alone on a scratch database
//! with `--ignored --nocapture` and read the timings and plan it prints.
use super::channel_call_support::*;
use super::goal_trail_tests::{goal_with_desk, try_admit};
use super::*;
use crate::adapters::persistence::department::goal_rollup::goal_rollup_on;
use crate::application::ports::department::GOAL_ROLLUP_RUN_LIMIT;
use crate::entities::department::DepartmentId;

#[tokio::test]
#[ignore = "measurement: writes 10,001 runs; run alone on a scratch database"]
async fn goal_rollup_cost_at_the_run_limit() {
    let s = CallSetup::new().await;
    let (goal, front) = goal_with_desk(&s, "launch").await;
    let association = RelatedAssociation::Channel(RelatedChannelId::new(front));
    let AdmissionResult::Created(seed) =
        try_admit(&s, association, "cost-seed", None).await.unwrap()
    else {
        panic!("the seed run is created");
    };
    // Clone the admitted run under new ids, one second apart, until the goal has one run more
    // than the limit. Each clone carries the seed's goal, thread and association, and a copy of
    // its root budget (the two reference each other, deferred, so one transaction writes both).
    let clones: Vec<Uuid> = (0..GOAL_ROLLUP_RUN_LIMIT).map(|_| Uuid::new_v4()).collect();
    let columns: String = sqlx::query_scalar(
        "SELECT string_agg(quote_ident(column_name), ', ' ORDER BY ordinal_position) \
           FROM information_schema.columns \
          WHERE table_schema = 'public' AND table_name = 'workflow_runs' \
            AND column_name NOT IN ('id', 'created_at') AND is_generated = 'NEVER'",
    )
    .fetch_one(s.pool())
    .await
    .unwrap();
    let mut tx = s.pool().begin().await.unwrap();
    sqlx::query(&format!(
        "INSERT INTO workflow_runs (id, created_at, {columns}) \
         SELECT clone.id, run.created_at - make_interval(secs => clone.n::double precision), \
                {columns} \
           FROM workflow_runs AS run CROSS JOIN unnest($3::uuid[]) WITH ORDINALITY AS clone(id, n) \
          WHERE run.company_id = $1 AND run.id = $2"
    ))
    .bind(s.company().as_uuid())
    .bind(seed.as_uuid())
    .bind(&clones)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO workflow_root_budgets \
                (company_id, root_run_id, activations, model_calls, repetitions, token_policy) \
         SELECT budget.company_id, clone.id, budget.activations, budget.model_calls, \
                budget.repetitions, budget.token_policy \
           FROM workflow_root_budgets AS budget CROSS JOIN unnest($3::uuid[]) AS clone(id) \
          WHERE budget.company_id = $1 AND budget.root_run_id = $2",
    )
    .bind(s.company().as_uuid())
    .bind(seed.as_uuid())
    .bind(&clones)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("ANALYZE workflow_runs")
        .execute(s.pool())
        .await
        .unwrap();
    let mut db = s.pool().acquire().await.unwrap();
    for attempt in 1..=3 {
        let started = std::time::Instant::now();
        let status = goal_rollup_on(
            &mut db,
            s.company(),
            DepartmentId::new(goal),
            GOAL_ROLLUP_RUN_LIMIT,
        )
        .await
        .unwrap();
        println!(
            "rollup attempt {attempt}: {} ms",
            started.elapsed().as_millis()
        );
        assert!(status.truncated);
        assert_eq!(status.runs_counted, GOAL_ROLLUP_RUN_LIMIT);
    }
    let plan: Vec<String> = sqlx::query_scalar(
        "EXPLAIN (ANALYZE, BUFFERS) \
         SELECT run.id, \
                COALESCE((SELECT scope.unit_id FROM workflow_run_scopes AS scope \
                           WHERE scope.company_id = run.company_id AND scope.run_id = run.id \
                             AND scope.acting_principal_id IS NULL AND scope.access = 'write' LIMIT 1), \
                         (SELECT root.id FROM departments AS root \
                           WHERE root.company_id = run.company_id AND root.is_root)) AS unit \
           FROM workflow_runs AS run \
          WHERE run.company_id = $1 AND run.goal_unit_id = $2 \
          ORDER BY run.created_at DESC, run.id DESC LIMIT $3",
    )
    .bind(s.company().as_uuid())
    .bind(goal)
    .bind(GOAL_ROLLUP_RUN_LIMIT + 1)
    .fetch_all(&mut *db)
    .await
    .unwrap();
    println!("{}", plan.join("\n"));
}
