use super::*;
use sqlx::Connection;
use std::time::Duration;

#[derive(Clone, Copy)]
enum First {
    Admission,
    Topology,
}

async fn waiter(observer: &mut PgConnection, blocker: i32) -> i32 {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let pid: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE datname=current_database()
                 AND $1=ANY(pg_blocking_pids(pid)) AND wait_event_type='Lock' ORDER BY pid LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(&mut *observer)
            .await
            .unwrap();
            if let Some(pid) = pid {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("a real production claimant reached the company prefix")
}

async fn ordered_release(
    gate: sqlx::Transaction<'_, sqlx::Postgres>,
    observer: &mut PgConnection,
    blocker: i32,
    first: tokio::sync::oneshot::Sender<()>,
    second: tokio::sync::oneshot::Sender<()>,
) {
    first.send(()).unwrap();
    let first_pid = waiter(observer, blocker).await;
    second.send(()).unwrap();
    waiter(observer, first_pid).await;
    gate.commit().await.unwrap();
}

fn without_report(before: &Value, report: DepartmentId) -> Value {
    let id = json!(report.as_uuid());
    Value::Array(
        before
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["unit_id"] != id)
            .cloned()
            .collect(),
    )
}

async fn competing_run(
    f: &RoleFixture,
    report: DepartmentId,
    root: DepartmentId,
    first: First,
) -> RunId {
    let command = f.prepared("scope-competing-move").await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.p());
    let mut observer = PgConnection::connect_with(&f.p().pool().connect_options())
        .await
        .unwrap();
    let mut gate = f.p().pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.company().as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let (admit_start, admit_ready) = tokio::sync::oneshot::channel();
    let (move_start, move_ready) = tokio::sync::oneshot::channel();
    let admitting = async {
        admit_ready.await.unwrap();
        f.p().admit(&command).await
    };
    let moving = async {
        move_ready.await.unwrap();
        departments
            .update_department(
                f.admission.binding.target.actor.user_id(),
                f.company().as_uuid(),
                report,
                crate::ports::department::DepartmentEdit {
                    parent_id: Some(Some(root)),
                    ..Default::default()
                },
            )
            .await
    };
    let (start, next) = match first {
        First::Admission => (admit_start, move_start),
        First::Topology => (move_start, admit_start),
    };
    let ((admitted, moved), ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(
            async { tokio::join!(admitting, moving) },
            ordered_release(gate, &mut observer, blocker, start, next),
        )
    })
    .await
    .expect("both production writers finish without a timeout or deadlock");
    moved.unwrap();
    let AdmissionResult::Created(run) = admitted.unwrap() else {
        panic!("new admission")
    };
    run
}

async fn race(first: First) {
    let f = RoleFixture::new().await;
    let report = report(&f).await;
    let root = super::seals::root(&f).await;
    let old = f.admitted("scope-before-competing-move").await;
    let before = tuples(&f, old).await;
    let after = without_report(&before, report);
    assert_ne!(before, after, "the acting lead initially reads the report");
    let run = Box::pin(competing_run(&f, report, root, first)).await;
    let expected = match first {
        First::Admission => &before,
        First::Topology => &after,
    };
    assert_eq!(
        &tuples(&f, run).await,
        expected,
        "exact actor/base projection at its commit order"
    );
    assert_eq!(
        tuples(&f, old).await,
        before,
        "old topology remains immutable"
    );
    let next = f.admitted("scope-after-competing-move").await;
    assert_eq!(
        tuples(&f, next).await,
        after,
        "later runs see the committed reparenting"
    );
}

#[tokio::test]
async fn memory_scope_admission_competing_topology_change_captures_coherent_scopes_in_both_orders()
{
    Box::pin(race(First::Admission)).await;
    Box::pin(race(First::Topology)).await;
}
