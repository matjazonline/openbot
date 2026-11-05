use super::*;
use chrono::Utc;

fn constraint(error: sqlx::Error, code: &str, name: &str) {
    let error = error.as_database_error().expect("database refusal");
    assert_eq!(error.code().as_deref(), Some(code), "{error:?}");
    assert_eq!(error.constraint(), Some(name), "{error:?}");
}

#[tokio::test]
async fn schema_constraint_attempt_stop_reason_matrix() {
    // The real admission/claim seam is boxed to keep native test stacks bounded.
    let (f, scope) = Box::pin(fixture()).await;
    let p = f.persistence();
    let claim = p
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let reasons = [
        None,
        Some("completed"),
        Some("effect_parked"),
        Some("approval_parked"),
        Some("child_parked"),
        Some("timed_out"),
        Some("lease_lost"),
    ];
    for status in ["processing", "completed", "failed"] {
        for reason in reasons {
            let accepted = match status {
                "processing" => reason.is_none(),
                "completed" => matches!(
                    reason,
                    Some("completed" | "effect_parked" | "approval_parked" | "child_parked")
                ),
                "failed" => matches!(reason, None | Some("timed_out" | "lease_lost")),
                _ => unreachable!(),
            };
            let mut tx = p.pool().begin().await.unwrap();
            let result = sqlx::query("UPDATE task_attempts SET status=$2, stop_reason=$3, finished_at=$4 WHERE task_id=$1")
                .bind(claim.fence.scope.job.0).bind(status).bind(reason)
                .bind((status != "processing").then(Utc::now)).execute(&mut *tx).await;
            if accepted {
                assert!(result.is_ok(), "{status}/{reason:?}: {result:?}");
            } else {
                constraint(
                    result.unwrap_err(),
                    "23514",
                    "task_attempts_stop_reason_check",
                );
            }
            tx.rollback().await.unwrap();
        }
    }
}

#[tokio::test]
async fn schema_constraint_attempt_finish_stamp_matches_status() {
    let (f, scope) = Box::pin(fixture()).await;
    let p = f.persistence();
    let claim = p
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    struct FinishCase {
        status: &'static str,
        reason: Option<&'static str>,
        finished: Option<chrono::DateTime<Utc>>,
    }
    for case in [
        FinishCase {
            status: "processing",
            reason: None,
            finished: Some(Utc::now()),
        },
        FinishCase {
            status: "failed",
            reason: None,
            finished: None,
        },
        FinishCase {
            status: "completed",
            reason: Some("completed"),
            finished: None,
        },
    ] {
        let mut tx = p.pool().begin().await.unwrap();
        let error = sqlx::query(
            "UPDATE task_attempts SET status=$2, stop_reason=$3, finished_at=$4 WHERE task_id=$1",
        )
        .bind(claim.fence.scope.job.0)
        .bind(case.status)
        .bind(case.reason)
        .bind(case.finished)
        .execute(&mut *tx)
        .await
        .unwrap_err();
        constraint(error, "23514", "task_attempts_finished_check");
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn schema_constraint_competing_claimants_leave_one_processing_attempt() {
    let (f, scope) = Box::pin(fixture()).await;
    let p = f.persistence();
    let settled = p.settled();
    let (first, second) = tokio::join!(
        settled.claim_io(scope, worker(), policy()),
        settled.claim_io(scope, worker(), policy()),
    );
    let claims: Vec<_> = [first.unwrap(), second.unwrap()]
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(claims.len(), 1);
    let job = claims[0].fence.scope.job.0;
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM task_attempts WHERE task_id=$1 AND status='processing'",
    )
    .bind(job)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(active, 1);
    let mut tx = p.pool().begin().await.unwrap();
    let error = sqlx::query("INSERT INTO task_attempts(id,task_id,attempt_number,status,execution_generation,worker_id) \
        SELECT gen_random_uuid(),task_id,attempt_number+100,'processing',execution_generation,worker_id \
        FROM task_attempts WHERE task_id=$1 AND status='processing'")
        .bind(job).execute(&mut *tx).await.unwrap_err();
    constraint(error, "23505", "task_attempts_one_processing");
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn schema_constraint_workflow_retry_count_cannot_exceed_budget() {
    let (f, scope) = Box::pin(fixture()).await;
    let maximum: i32 = sqlx::query_scalar("SELECT max_retries FROM background_tasks WHERE id=$1")
        .bind(scope.job.0)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    for count in [-1_i32, maximum.checked_add(1).unwrap()] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let error = sqlx::query("UPDATE background_tasks SET retry_count=$2 WHERE id=$1")
            .bind(scope.job.0)
            .bind(count)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        constraint(error, "23514", "background_tasks_retry_count_check");
        tx.rollback().await.unwrap();
    }
}
