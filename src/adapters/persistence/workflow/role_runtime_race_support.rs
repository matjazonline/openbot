use super::*;

async fn blocked(d: &Deployment, blocker: i32, pattern: &str) -> i32 {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let pid: Option<i32> = sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)) AND query LIKE $2 ORDER BY pid LIMIT 1")
                .bind(blocker).bind(pattern).fetch_optional(d.p().pool()).await.unwrap();
            if let Some(pid) = pid { return pid; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("competing writer reaches the real lock boundary")
}

pub(super) async fn admit_while_reassigning(
    d: &Deployment,
    binding: BindingTarget,
    desk: &RoleDesk,
    replacement: PrincipalId,
    trigger: Uuid,
) -> RunId {
    let mut barrier = d.p().pool().begin().await.unwrap();
    let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *barrier)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(824203)")
        .execute(&mut *barrier)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION role_runtime_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
        PERFORM set_config('lock_timeout','4000ms',true); PERFORM pg_advisory_xact_lock(824203); RETURN NEW; END $$; \
        CREATE TRIGGER role_runtime_barrier BEFORE INSERT ON workflow_runs FOR EACH ROW EXECUTE FUNCTION role_runtime_barrier()")
        .execute(d.p().pool()).await.unwrap();
    let admissions = d.engine.services().service();
    let admission = admissions.admit(request(binding, desk.id, "queued-old", trigger));
    let mutation = async {
        let admitting = blocked(d, holder, "%INSERT INTO workflow_runs%").await;
        let departments = crate::adapters::persistence::test_support::department_use_cases(d.p());
        let department = desk.department;
        let writer = tokio::spawn(async move {
            departments
                .assign_role(
                    binding.actor.user_id(),
                    binding.company.as_uuid(),
                    department,
                    DepartmentRole::Lead,
                    replacement,
                )
                .await
        });
        blocked(d, admitting, "%FROM companies%FOR NO KEY UPDATE%").await;
        barrier.rollback().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), writer)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    };
    let (result, ()) = tokio::join!(admission, mutation);
    sqlx::raw_sql(
        "DROP TRIGGER role_runtime_barrier ON workflow_runs; DROP FUNCTION role_runtime_barrier()",
    )
    .execute(d.p().pool())
    .await
    .unwrap();
    match result.unwrap() {
        AdmissionResult::Created(run) => run,
        other => panic!("new role run expected: {other:?}"),
    }
}
