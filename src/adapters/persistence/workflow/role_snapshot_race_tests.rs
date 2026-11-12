use super::*;
use std::time::Duration;

struct Barrier {
    transaction: Transaction<'static, Postgres>,
    pid: i32,
}

impl Barrier {
    async fn install(f: &RoleFixture, relation: &str, operation: &str) -> Self {
        let mut transaction = f.p().pool().begin().await.unwrap();
        let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(824202)")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::raw_sql(&format!("CREATE FUNCTION role_admission_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
            PERFORM set_config('lock_timeout','4000ms',true); PERFORM pg_advisory_xact_lock(824202); RETURN NEW; END $$; \
            CREATE TRIGGER role_admission_barrier BEFORE {operation} ON {relation} FOR EACH ROW EXECUTE FUNCTION role_admission_barrier()"))
            .execute(f.p().pool()).await.unwrap();
        Self { transaction, pid }
    }

    async fn waiting(&self, f: &RoleFixture) -> i32 {
        blocked(f, self.pid, "%").await
    }

    async fn release(self) {
        self.transaction.rollback().await.unwrap();
    }
}

async fn blocked(f: &RoleFixture, blocker: i32, query: &str) -> i32 {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let pid: Option<i32> = sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)) AND query LIKE $2 ORDER BY pid LIMIT 1")
                .bind(blocker).bind(query).fetch_optional(f.p().pool()).await.unwrap();
            if let Some(pid) = pid { return pid; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("the real competing writer reaches the expected lock boundary")
}

fn published_conflict(result: AppResult<()>) {
    assert!(
        matches!(result, Err(AppError::Conflict(ref message)) if message == "This agent has published revisions and cannot be deleted. Deactivate it instead."),
        "{result:?}"
    );
}

#[tokio::test]
async fn workflow_role_competing_admission_first_serializes_delete_before_agent_principal_and_desk_cleanup()
 {
    let f = RoleFixture::new().await;
    let barrier = Barrier::install(&f, "workflow_runs", "INSERT").await;
    let command = f.prepared("admission-first").await;
    let p = f.p().clone();
    let admission = tokio::spawn(async move { p.admit(&command).await });
    let admitting = barrier.waiting(&f).await;
    let p = f.p().clone();
    let agent = f.agent;
    let deletion = tokio::spawn(async move { AgentPersistence::delete(&p, agent).await });
    blocked(&f, admitting, "%FROM companies%FOR NO KEY UPDATE%").await;
    barrier.release().await;
    let admitted = tokio::time::timeout(Duration::from_secs(5), admission)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let AdmissionResult::Created(run) = admitted else {
        panic!("{admitted:?}")
    };
    published_conflict(
        tokio::time::timeout(Duration::from_secs(5), deletion)
            .await
            .unwrap()
            .unwrap(),
    );
    assert_eq!(
        f.manifest(run).await.roles[0].principals[0].revision_id,
        Some(f.revision)
    );
}

#[tokio::test]
async fn workflow_role_competing_delete_first_serializes_admission_before_channel_and_agent_locks()
{
    let f = RoleFixture::new().await;
    let command = f.prepared("delete-first").await;
    let barrier = Barrier::install(&f, "principals", "UPDATE").await;
    let p = f.p().clone();
    let agent = f.agent;
    let deletion = tokio::spawn(async move { AgentPersistence::delete(&p, agent).await });
    let deleting = barrier.waiting(&f).await;
    let p = f.p().clone();
    let admission = tokio::spawn(async move { p.admit(&command).await });
    blocked(&f, deleting, "%FROM companies%FOR NO KEY UPDATE%").await;
    barrier.release().await;
    published_conflict(
        tokio::time::timeout(Duration::from_secs(5), deletion)
            .await
            .unwrap()
            .unwrap(),
    );
    let admitted = tokio::time::timeout(Duration::from_secs(5), admission)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let AdmissionResult::Created(run) = admitted else {
        panic!("{admitted:?}")
    };
    assert_eq!(
        f.manifest(run).await.roles[0].principals[0].revision_id,
        Some(f.revision)
    );
}

#[tokio::test]
async fn workflow_role_competing_reassignment_that_commits_waits_for_capture_then_next_admission_sees_it()
 {
    let f = RoleFixture::new().await;
    let (new_agent, new_principal) = create_agent(&f.admission.binding, "replacement").await;
    let new_revision = f
        .admission
        .binding
        .fixture
        .publish_agent(new_agent)
        .await
        .id;
    let command = f.prepared("before-assignment").await;
    let barrier = Barrier::install(&f, "workflow_runs", "INSERT").await;
    let p = f.p().clone();
    let admission = tokio::spawn(async move { p.admit(&command).await });
    let admitting = barrier.waiting(&f).await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.p());
    let actor = f.admission.binding.target.actor.user_id();
    let company = f.company();
    let department = f.department;
    let reassignment = tokio::spawn(async move {
        departments
            .assign_role(
                actor,
                company.as_uuid(),
                department,
                DepartmentRole::Lead,
                new_principal,
            )
            .await
    });
    blocked(&f, admitting, "%FROM companies%FOR NO KEY UPDATE%").await;
    barrier.release().await;
    let admitted = admission.await.unwrap().unwrap();
    let AdmissionResult::Created(old) = admitted else {
        panic!("{admitted:?}")
    };
    reassignment.await.unwrap().unwrap();
    let new = f.admitted("after-assignment").await;
    let before = f.manifest(old).await;
    let after = f.manifest(new).await;
    assert_eq!(before.roles[0].principals[0].principal_id, f.principal);
    assert_eq!(before.roles[0].principals[0].revision_id, Some(f.revision));
    assert_eq!(after.roles[0].principals[0].principal_id, new_principal);
    assert_eq!(after.roles[0].principals[0].revision_id, Some(new_revision));
}
