use std::time::Duration;

use uuid::Uuid;

use super::*;
use crate::{
    adapters::persistence::test_support::{own_database, test_pool},
    application::notification::{
        NOTIFICATION_EVENT_CLAIM_BATCH, NOTIFICATION_EVENT_LEASE_SECONDS, NotificationPersistence,
        NotificationProjectionFailure,
    },
    transport::WorkerId,
    use_cases::{
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};

/// A company for the queue tests: the sources themselves are workflow deliveries, tested beside
/// their fixture (`workflow::tests`).
struct Fixture {
    company_id: Uuid,
}

async fn make_fixture(persistence: &PostgresPersistence) -> Fixture {
    let suffix = Uuid::new_v4().simple().to_string();
    let owner = persistence
        .create_user(
            &format!("notification-owner-{suffix}"),
            &format!("notification-owner-{suffix}@example.test"),
            "hash",
        )
        .await
        .unwrap();
    let company = CompanyPersistence::create(
        persistence,
        owner.id,
        CompanyWrite {
            name: "Notification Test".into(),
            slug: format!("notification-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    Fixture {
        company_id: company.id,
    }
}

async fn release_foreign_claim(
    persistence: &PostgresPersistence,
    claimed: &crate::application::notification::ClaimedNotificationEvent,
) {
    sqlx::query(
        r#"UPDATE notification_events
              SET status = 'pending', attempt_count = GREATEST(attempt_count - 1, 0),
                  execution_id = NULL, owner_worker_id = NULL,
                  locked_at = NULL, lock_expires_at = NULL
            WHERE id = $1 AND status = 'processing'
              AND execution_id = $2 AND owner_worker_id = $3"#,
    )
    .bind(claimed.event.id.as_uuid())
    .bind(claimed.lease.execution.as_uuid())
    .bind(claimed.lease.owner.as_uuid())
    .execute(persistence.pool())
    .await
    .unwrap();
}

#[tokio::test]
async fn notification_census_executes_against_postgres() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);

    let census = persistence.notification_census().await.unwrap();

    assert!(
        census
            .oldest_active_age_seconds
            .is_none_or(|seconds| seconds >= 0.0)
    );
}

#[tokio::test]
async fn a_failed_projection_batch_backs_off_before_it_can_be_claimed_again() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = make_fixture(&persistence).await;
    let source_ids = (0..NOTIFICATION_EVENT_CLAIM_BATCH)
        .map(|_| Uuid::new_v4())
        .collect::<Vec<_>>();
    let event_ids: Vec<Uuid> = sqlx::query_scalar(
        r#"INSERT INTO notification_events (
               company_id, source_kind, source_id, action_kind, source_generation, available_at
           )
           SELECT $1, 'delivery', source.source_id, 'delivery_failure', 1, '2000-01-01'
             FROM UNNEST($2::uuid[]) AS source(source_id)
           RETURNING id"#,
    )
    .bind(fixture.company_id)
    .bind(&source_ids)
    .fetch_all(&pool)
    .await
    .unwrap();

    let claimed = persistence
        .claim_notification_events(
            WorkerId::random(),
            Duration::from_secs(NOTIFICATION_EVENT_LEASE_SECONDS as u64),
            NOTIFICATION_EVENT_CLAIM_BATCH,
        )
        .await
        .unwrap();
    assert_eq!(claimed.len() as i64, NOTIFICATION_EVENT_CLAIM_BATCH);
    assert!(
        claimed
            .iter()
            .all(|event| event_ids.contains(&event.event.id.as_uuid())),
        "the deliberately oldest batch excludes unrelated queue rows"
    );
    for event in &claimed {
        assert!(
            persistence
                .fail_notification_projection(NotificationProjectionFailure {
                    fence: &event.lease,
                    class: "internal",
                    detail: "synthetic poison event",
                })
                .await
                .unwrap()
        );
    }

    let immediate_retry = persistence
        .claim_notification_events(
            WorkerId::random(),
            Duration::from_secs(NOTIFICATION_EVENT_LEASE_SECONDS as u64),
            NOTIFICATION_EVENT_CLAIM_BATCH,
        )
        .await
        .unwrap();
    assert!(
        immediate_retry
            .iter()
            .all(|event| !event_ids.contains(&event.event.id.as_uuid())),
        "backoff keeps the poison batch out of the next iteration"
    );
    for foreign in &immediate_retry {
        release_foreign_claim(&persistence, foreign).await;
    }
    let backed_off: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM notification_events AS event
            WHERE event.id = ANY($1) AND event.status = 'pending'
              AND event.available_at > CURRENT_TIMESTAMP"#,
    )
    .bind(&event_ids)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(backed_off, NOTIFICATION_EVENT_CLAIM_BATCH);

    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}
