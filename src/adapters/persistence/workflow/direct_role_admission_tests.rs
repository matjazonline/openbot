//! Actual principal-bound role capture competing with the production roster writer.
use super::*;
use std::time::Duration;

async fn direct_binding(f: &mut RoleFixture) {
    let binding = &mut f.admission.binding;
    // Each binding keeps its original association; principal scope gets its own head.
    binding.target.binding = WorkflowBindingId::new(Uuid::new_v4());
    let mut configuration = binding.request(None);
    configuration.association = RelatedAssociation::Principal(f.principal);
    let configured = binding.service().configure(configuration).await.unwrap();
    binding
        .service()
        .activate(SetBindingActivity {
            target: binding.target,
            expected: configured.revision,
        })
        .await
        .unwrap();
}

async fn support(f: &RoleFixture) -> DepartmentId {
    crate::adapters::persistence::test_support::department_use_cases(f.p())
        .create_department(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            DepartmentWrite {
                name: "Support".into(),
                slug: "support".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id
}

async fn both_waiting(observer: &mut PgConnection, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            // PostgreSQL queues a later tuple waiter behind the first waiter, so
            // follow the full wait chain back to this transaction's company gate.
            let count: i64 = sqlx::query_scalar(
                "WITH RECURSIVE waiting(pid) AS (\
                    SELECT $1::integer UNION \
                    SELECT activity.pid FROM pg_stat_activity AS activity \
                    JOIN waiting AS blocker ON blocker.pid=ANY(pg_blocking_pids(activity.pid)) \
                    WHERE activity.datname=current_database() AND activity.wait_event_type='Lock'\
                 ) SELECT count(*) FROM waiting WHERE pid<>$1",
            )
            .bind(blocker)
            .fetch_one(&mut *observer)
            .await
            .unwrap();
            if count >= 2 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both real writers reached the held company lock");
}

#[tokio::test]
async fn direct_binding_role_admission_and_competing_roster_write_capture_one_current_department() {
    use sqlx::Connection;
    let mut f = Box::pin(RoleFixture::new()).await;
    direct_binding(&mut f).await;
    let other = support(&f).await;
    let p = f.p();
    let mut request = f
        .admission
        .request("direct-role-roster", f.admission.manual());
    request.association = RelatedAssociation::Principal(f.principal);
    let command = f.admission.prepare(request).await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(p);
    let mut observer = PgConnection::connect_with(&p.pool().connect_options())
        .await
        .unwrap();
    let mut gate = p.pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.company().as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let competitors = async {
        tokio::join!(
            p.admit(&command),
            departments.add_member(
                f.admission.binding.target.actor.user_id(),
                f.company().as_uuid(),
                other,
                f.principal
            )
        )
    };
    let release = async {
        both_waiting(&mut observer, blocker).await;
        gate.commit().await.unwrap();
    };
    let ((admission, placement), ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(competitors, release)
    })
    .await
    .expect("ordinary admission and roster outcomes, never a deadlock/timeout refusal");
    let AdmissionResult::Created(run) = admission.unwrap() else {
        panic!("one current direct role admission");
    };
    let error = placement.unwrap_err();
    assert!(format!("{error:?}").contains("binding.role_needs_department"));
    let manifest = f.manifest(run).await;
    assert_eq!(manifest.roles.len(), 1);
    assert_eq!(manifest.roles[0].department_id, f.department);
    assert_eq!(manifest.roles[0].principals[0].principal_id, f.principal);
    assert_eq!(
        manifest.roles[0].principals[0].revision_id,
        Some(f.revision)
    );
    let summary = p.run_summary(f.company(), run).await.unwrap().unwrap();
    assert_eq!(
        summary.association,
        RelatedAssociation::Principal(f.principal)
    );
    let context: Vec<Uuid> = sqlx::query_scalar(
        "SELECT department FROM workflow_target_departments($1,NULL,$2) AS department",
    )
    .bind(f.company().as_uuid())
    .bind(f.principal.as_uuid())
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(context, vec![f.department.as_uuid()]);
}
