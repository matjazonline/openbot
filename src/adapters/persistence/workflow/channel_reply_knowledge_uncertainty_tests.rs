use super::*;
use crate::adapters::persistence::workflow::{channel_reply_refusal, completion_job, lease};
use crate::domain::workflow::KnowledgeRefusal;
use crate::entities::access_level::AccessLevel;

#[tokio::test]
async fn channel_reply_knowledge_fenced_owner_preserves_actual_unknown_http_effect() {
    let (origin, server) = server("HTTP/1.1 500 Error\r\nContent-Length: 0\r\n\r\n").await;
    let setup = Box::pin(setup(&origin, Entrance::Http)).await;
    assert!(
        runtime(&setup)
            .run(&setup.kind, &setup.claim)
            .await
            .completed_result()
            .is_err()
    );
    server.await.unwrap();
    let p = &setup.fixture.persistence;
    let fence = setup.claim.fence;
    let mut tx = p.pool().begin().await.unwrap();
    assert!(
        lease::lock_fence(&mut tx, fence, Duration::from_secs(2))
            .await
            .unwrap()
    );
    let safe: Option<bool> = sqlx::query_scalar("SELECT workflow_action_retry_safe($1,$2)")
        .bind(fence.scope.company.as_uuid())
        .bind(fence.scope.execution.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        safe,
        Some(false),
        "a real dispatch marker has no conclusive receipt"
    );
    // Exercise the final-response owner with an established typed release refusal.
    // The HTTP marker is real; this is a boundary case, not a fabricated provider failure.
    channel_reply_refusal::settle_on(
        &mut tx,
        fence.scope,
        completion_job::CompletionOwner::Fenced(fence),
        Some(KnowledgeRefusal {
            content: AccessLevel::L5,
            destination: AccessLevel::PUBLIC,
        }),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        retired(p, &setup.claim).await,
        Retired::new(
            "waiting",
            Some("reconciliation"),
            "knowledge.audience_below_content",
            "terminal",
            "unknown",
            1
        )
    );
    let saved:Value=sqlx::query_scalar("SELECT jsonb_build_object('content',knowledge_content_level,
        'destination',knowledge_destination_level) FROM task_attempts WHERE task_id=$1 AND attempt_number=$2")
        .bind(fence.scope.job.0).bind(fence.attempt.0).fetch_one(p.pool()).await.unwrap();
    assert_eq!(saved, json!({"content":5,"destination":0}));
    assert!(
        !p.release_io(fence, lease(), LeaseReleaseCause::HandlerFailure)
            .await
            .unwrap()
    );
    let markers: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_dispatches WHERE company_id=$1 AND execution_id=$2",
    )
    .bind(fence.scope.company.as_uuid())
    .bind(fence.scope.execution.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(markers, 1);
}
