use super::*;
use tokio::sync::Notify;

#[derive(Clone, Copy)]
enum First {
    Cancel,
    Resume,
}

async fn exact_wait(
    observer: &mut sqlx::PgConnection,
    blocker: i32,
    statement: &str,
) -> Result<i32, String> {
    let observed = async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()")
                .execute(&mut *observer)
                .await
                .map_err(|error| error.to_string())?;
            let pid: Option<i32> = sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND state='active' AND wait_event_type='Lock' AND position($1 IN query)>0 AND $2=ANY(pg_blocking_pids(pid))")
                .bind(statement).bind(blocker).fetch_optional(&mut *observer).await
                .map_err(|error| error.to_string())?;
            if let Some(pid) = pid {
                return Ok(pid);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    tokio::time::timeout(Duration::from_millis(300), observed)
        .await
        .map_err(|_| format!("no exact wait on backend {blocker} for {statement}"))?
}

async fn public_state(f: &AdmissionFixture) -> Value {
    let mut state = serde_json::Map::new();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename FROM pg_tables WHERE schemaname='public' ORDER BY tablename",
    )
    .fetch_all(f.persistence().pool())
    .await
    .unwrap();
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let rows: Value = sqlx::query_scalar(&format!(
            "SELECT COALESCE(jsonb_agg(to_jsonb(row) ORDER BY to_jsonb(row)::text),'[]') FROM public.\"{quoted}\" AS row"
        )).fetch_one(f.persistence().pool()).await.unwrap();
        state.insert(table, rows);
    }
    Value::Object(state)
}

async fn compete(
    f: &AdmissionFixture,
    scope: ActivationRequest,
    first: First,
) -> (CancelResult, WaitProgress) {
    let cmd = command(f, scope, "ordered-cancel").await;
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE id=$1 FOR UPDATE")
        .bind(scope.execution.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    let cancel_start = Notify::new();
    let resume_start = Notify::new();
    let (start, next) = match first {
        First::Cancel => (&cancel_start, &resume_start),
        First::Resume => (&resume_start, &cancel_start),
    };
    start.notify_one();
    let cancel = async {
        cancel_start.notified().await;
        f.persistence().cancel(cmd).await
    };
    let resume = async {
        resume_start.notified().await;
        f.persistence().resume_wait(scope).await
    };
    let ordering = async {
        let first_wait = exact_wait(
            &mut observer,
            blocker_pid,
            "SELECT id FROM workflow_executions",
        )
        .await;
        // Always release the second future, including observer failure paths.
        next.notify_one();
        let observed = match first_wait {
            Ok(pid) => exact_wait(&mut observer, pid, "workflow_runs")
                .await
                .map(|_| ()),
            Err(error) => Err(error),
        };
        // Release the blocker before reporting failure; join! drains both public calls.
        let released = blocker.rollback().await;
        (observed, released)
    };
    let (cancel, resume, (observed, released)) = tokio::join!(cancel, resume, ordering);
    released.unwrap();
    observed.unwrap();
    (cancel.unwrap(), resume.unwrap())
}

async fn ordered_case(first: First) {
    let (f, scope) = Box::pin(fixture_source(waiting_source())).await;
    f.persistence().park_wait(scope).await.unwrap().unwrap();
    f.persistence()
        .record_signal(wait_signal(scope))
        .await
        .unwrap();
    let before = control_state(&f).await;
    let (cancel, resume) = Box::pin(compete(&f, scope, first)).await;
    let after = control_state(&f).await;
    assert_eq!(
        after["workflow_wait_events"],
        before["workflow_wait_events"]
    );
    assert_eq!(
        after["workflow_waits"][0]["notification_intent"],
        before["workflow_waits"][0]["notification_intent"]
    );
    assert_eq!(after["workflow_waits"].as_array().unwrap().len(), 1);
    match first {
        First::Cancel => {
            assert!(matches!(cancel, CancelResult::Applied { .. }));
            assert_eq!(resume, WaitProgress::Refused);
            assert_eq!(after["workflow_runs"][0]["state"], "cancelled");
            assert_eq!(after["workflow_waits"][0]["state"], "cancelled");
            assert_eq!(after["workflow_executions"].as_array().unwrap().len(), 1);
            assert_eq!(
                after["workflow_executions"][0]["committed_output"],
                Value::Null
            );
            assert_eq!(
                after["workflow_executions"][0]["successor_execution_id"],
                Value::Null
            );
            let saved = public_state(&f).await;
            assert_eq!(
                f.persistence().resume_wait(scope).await.unwrap(),
                WaitProgress::Refused
            );
            assert_eq!(public_state(&f).await, saved);
        }
        First::Resume => {
            assert!(matches!(cancel, CancelResult::RevisionConflict { .. }));
            assert!(matches!(resume, WaitProgress::Completed(_)));
            assert_eq!(after["workflow_waits"][0]["state"], "completed");
            assert_eq!(after["workflow_executions"].as_array().unwrap().len(), 2);
        }
    }
}

#[tokio::test]
async fn workflow_control_cancel_first_competing_resume_refuses_without_writes() {
    Box::pin(ordered_case(First::Cancel)).await;
}

#[tokio::test]
async fn workflow_control_resume_first_competing_cancel_preserves_completion() {
    Box::pin(ordered_case(First::Resume)).await;
}
