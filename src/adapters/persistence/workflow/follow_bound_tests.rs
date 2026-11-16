//! D1.7: at most 256 concurrently parked follow-role decisions per company, enforced when parking
//! so every role writer can refresh the complete set in its own commit.
use super::*;

async fn pending(f: &FollowFixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_pending_follow_decisions WHERE company_id=$1")
        .bind(f.scope.company.as_uuid())
        .fetch_one(f.people.role.p().pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn workflow_reviewer_follow_park_at_the_256_bound_refuses_the_next_without_a_partial_park() {
    let f = Box::pin(FollowFixture::new(2)).await;
    let p = f.people.role.p();
    for index in 1..256 {
        let run = f
            .people
            .role
            .admitted(&format!("follow-bound-{index}"))
            .await;
        let scope = f.people.scope(run).await;
        assert!(
            p.park_decision(scope, &notices(p)).await.unwrap().is_some(),
            "park {index}"
        );
    }
    assert_eq!(pending(&f).await, 256);

    let run = f.people.role.admitted("follow-bound-over").await;
    let scope = f.people.scope(run).await;
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let refused = p.park_decision(scope, &notices(p)).await.unwrap_err();
    assert!(
        matches!(&refused, AppError::Conflict(message) if message.starts_with("workflow.pending_follow_limit")),
        "{refused:?}"
    );
    let partial: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('decisions',(SELECT count(*) FROM workflow_decisions WHERE company_id=$1 AND run_id=$2), \
         'waits',(SELECT count(*) FROM workflow_waits WHERE company_id=$1 AND run_id=$2), \
         'follows',(SELECT count(*) FROM workflow_follow_decisions WHERE company_id=$1 AND run_id=$2), \
         'state',(SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2))",
    )
    .bind(scope.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        partial,
        json!({"decisions":0,"waits":0,"follows":0,"state":state}),
        "the refused park left nothing behind and the run's state is untouched"
    );
    assert_eq!(pending(&f).await, 256);

    // One real role write refreshes the complete bounded set in its own commit.
    f.clear(0).await;
    let refreshed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_pending_follow_decisions AS pending \
         JOIN workflow_follow_decisions AS follow USING(company_id,decision_id) \
         WHERE pending.company_id=$1 AND follow.assignment_revision=2",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(refreshed, 256);
}
