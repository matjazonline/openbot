use std::time::Duration;

use uuid::Uuid;

use super::*;
use crate::{
    adapters::persistence::test_support::{own_database, test_pool},
    application::notification::{
        ClaimedNotificationEvent, NOTIFICATION_EVENT_CLAIM_BATCH, NOTIFICATION_EVENT_LEASE_SECONDS,
        NotificationPersistence, NotificationProjectionCommit, NotificationProjectionCommitOutcome,
        NotificationProjectionFailure,
    },
    entities::notification::NotificationDisposition,
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

/// Insert `count` due events, each already charged `attempts` of its ten attempts.
async fn insert_events(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    count: i64,
    attempts: i32,
) -> Vec<Uuid> {
    let source_ids = (0..count).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
    sqlx::query_scalar(
        r#"INSERT INTO notification_events (
               company_id, source_kind, source_id, action_kind, source_generation, available_at,
               attempt_count
           )
           SELECT $1, 'delivery', source.source_id, 'delivery_failure', 1, '2000-01-01', $3
             FROM UNNEST($2::uuid[]) AS source(source_id)
           RETURNING id"#,
    )
    .bind(company_id)
    .bind(&source_ids)
    .bind(attempts)
    .fetch_all(persistence.pool())
    .await
    .unwrap()
}

async fn claim_batch(persistence: &PostgresPersistence) -> Vec<ClaimedNotificationEvent> {
    persistence
        .claim_notification_events(
            WorkerId::random(),
            Duration::from_secs(NOTIFICATION_EVENT_LEASE_SECONDS as u64),
            NOTIFICATION_EVENT_CLAIM_BATCH,
        )
        .await
        .unwrap()
}

/// Commit a projection that no longer matches its source: the worker resolved it, and by commit
/// time the source says something else.
async fn commit_stale(
    persistence: &PostgresPersistence,
    claimed: &ClaimedNotificationEvent,
) -> NotificationProjectionCommitOutcome {
    let mut expected = persistence
        .resolve_notification_projection(&claimed.event)
        .await
        .unwrap();
    expected.disposition = match expected.disposition {
        NotificationDisposition::Resolved => NotificationDisposition::Withdrawn,
        _ => NotificationDisposition::Resolved,
    };
    persistence
        .commit_notification_projection(NotificationProjectionCommit {
            fence: &claimed.lease,
            expected: &expected,
            email: None,
            escalation_ack_hours: Default::default(),
        })
        .await
        .unwrap()
        .0
}

async fn event_state(persistence: &PostgresPersistence, event: Uuid) -> (String, i32, bool) {
    sqlx::query_as(
        "SELECT status, attempt_count, available_at > CURRENT_TIMESTAMP \
           FROM notification_events WHERE id = $1",
    )
    .bind(event)
    .fetch_one(persistence.pool())
    .await
    .unwrap()
}

/// The last attempt coming back stale is terminal: dead-lettered, never a `pending` row at 10/10
/// that no claim can take and the census counts for ever.
#[tokio::test]
async fn notification_stale_last_attempt_is_dead_lettered() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let fixture = make_fixture(&persistence).await;
    let events = insert_events(&persistence, fixture.company_id, 1, 9).await;

    let claimed = claim_batch(&persistence).await;
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].event.id.as_uuid(), events[0]);
    assert_eq!(
        commit_stale(&persistence, &claimed[0]).await,
        NotificationProjectionCommitOutcome::Stale
    );

    let (status, attempts, _) = event_state(&persistence, events[0]).await;
    assert_eq!((status.as_str(), attempts), ("dead_letter", 10));
    let class: String =
        sqlx::query_scalar("SELECT last_error_class FROM notification_events WHERE id=$1")
            .bind(events[0])
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    assert_eq!(class, "stale_projection");
    assert!(claim_batch(&persistence).await.is_empty());
}

/// Below the limit a stale projection is requeued, but behind the fail path's backoff rather than
/// due now, so a source that keeps changing cannot hot-loop the projector.
#[tokio::test]
async fn notification_stale_below_the_limit_returns_to_pending_with_backoff() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let fixture = make_fixture(&persistence).await;
    let events = insert_events(&persistence, fixture.company_id, 1, 0).await;

    let claimed = claim_batch(&persistence).await;
    assert_eq!(claimed.len(), 1);
    assert_eq!(
        commit_stale(&persistence, &claimed[0]).await,
        NotificationProjectionCommitOutcome::Stale
    );

    let (status, attempts, backed_off) = event_state(&persistence, events[0]).await;
    assert_eq!((status.as_str(), attempts), ("pending", 1));
    assert!(backed_off, "a stale projection is not due again at once");
}

/// `src/application/AGENTS.md` poison batch: a full batch that comes back stale is moved out of
/// the claimable set, so the next iteration, without time advancing, claims none of it.
#[tokio::test]
async fn notification_stale_poison_batch_is_not_reclaimed_by_the_next_iteration() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let fixture = make_fixture(&persistence).await;
    let events = insert_events(
        &persistence,
        fixture.company_id,
        NOTIFICATION_EVENT_CLAIM_BATCH,
        0,
    )
    .await;

    let first = claim_batch(&persistence).await;
    assert_eq!(
        first.len() as i64,
        NOTIFICATION_EVENT_CLAIM_BATCH,
        "a full batch"
    );
    for claimed in &first {
        assert!(events.contains(&claimed.event.id.as_uuid()));
        assert_eq!(
            commit_stale(&persistence, claimed).await,
            NotificationProjectionCommitOutcome::Stale
        );
    }

    let second = claim_batch(&persistence).await;
    assert!(
        second.is_empty(),
        "the stale batch is not reclaimed: {} rows",
        second.len()
    );
    for event in events {
        let (status, attempts, backed_off) = event_state(&persistence, event).await;
        assert_eq!(
            (status.as_str(), attempts, backed_off),
            ("pending", 1, true)
        );
    }
}

#[tokio::test]
async fn schema_constraint_notification_failure_requires_class_and_bounded_detail() {
    let database = own_database()
        .await
        .expect("native notification constraints");
    let p = PostgresPersistence::new(database.pool.clone());
    let fixture = make_fixture(&p).await;
    let event = insert_events(&p, fixture.company_id, 1, 0).await[0];
    struct FailureRow {
        status: &'static str,
        class: Option<&'static str>,
        detail: Option<String>,
    }
    for row in [
        FailureRow {
            status: "dead_letter",
            class: None,
            detail: None,
        },
        FailureRow {
            status: "pending",
            class: None,
            detail: Some("detail without class".into()),
        },
        FailureRow {
            status: "pending",
            class: Some("internal"),
            detail: Some("é".repeat(257)),
        },
    ] {
        let error = sqlx::query("UPDATE notification_events SET status=$2,last_error_class=$3,last_error_detail=$4 WHERE id=$1")
            .bind(event).bind(row.status).bind(row.class).bind(row.detail).execute(p.pool()).await.unwrap_err();
        let error = error.as_database_error().unwrap();
        assert_eq!(error.code().as_deref(), Some("23514"));
        assert_eq!(error.constraint(), Some("notification_events_error_check"));
    }
    sqlx::query("UPDATE notification_events SET last_error_class='internal',last_error_detail=$2 WHERE id=$1")
        .bind(event).bind("é".repeat(256)).execute(p.pool()).await.unwrap();
}
