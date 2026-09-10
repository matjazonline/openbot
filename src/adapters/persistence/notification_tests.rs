use std::{sync::Arc, time::Duration};

use uuid::Uuid;

use super::*;
use crate::{
    adapters::{
        persistence::test_support::{UNSCOPED_CLAIM, test_pool},
        protocols::email::EmailRenderer,
    },
    application::notification::{
        NOTIFICATION_EVENT_CLAIM_BATCH, NOTIFICATION_EVENT_LEASE_SECONDS, NotificationPersistence,
        NotificationProjectionCommit, NotificationProjectionCommitOutcome,
        NotificationProjectionFailure,
    },
    entities::{
        creation::CreationProvenance,
        notification::{NotificationDisposition, NotificationPreferences},
        task::{
            TaskOwner, TaskOwnershipActor, TaskOwnershipAuthority, TaskOwnershipCommand,
            TaskOwnershipOperation, TaskOwnershipReason,
        },
        transport::{DeliveryPurpose, PrincipalId},
    },
    task_queue::TaskPersistence,
    transport::{
        CanonicalContent, DeliveryComposer, DeliveryContext, EmailDeliveryContext, EmailThreading,
        StandaloneDeliveryRequest, TransportRenderers, WorkerId,
    },
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};

struct Fixture {
    company_id: Uuid,
    channel_id: Uuid,
    task_id: Uuid,
    owner_principal: PrincipalId,
    member_user_id: Uuid,
    member_principal: PrincipalId,
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
    let owner_principal = principal_for_user(persistence, company.id, owner.id).await;

    let member = persistence
        .create_user(
            &format!("notification-member-{suffix}"),
            &format!("notification-member-{suffix}@example.test"),
            "hash",
        )
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role) VALUES ($1, $2, $3, 'member')",
    )
    .bind(Uuid::new_v4())
    .bind(company.id)
    .bind(member.id)
    .execute(persistence.pool())
    .await
    .unwrap();
    let member_principal = PrincipalId::random();
    sqlx::query(
        r#"INSERT INTO principals (id, company_id, kind, user_id, display_label)
           VALUES ($1, $2, 'person', $3, 'Notification Member')"#,
    )
    .bind(member_principal.as_uuid())
    .bind(company.id)
    .bind(member.id)
    .execute(persistence.pool())
    .await
    .unwrap();

    let agent = AgentPersistence::create(
        persistence,
        company.id,
        AgentWrite {
            name: "Notification Agent".into(),
            slug: format!("notification-agent-{suffix}"),
            created_by: Some(CreationProvenance::system()),
            harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        persistence,
        company.id,
        ChannelWrite {
            name: "Operations".into(),
            slug: format!("operations-{suffix}"),
            agent_ids: Some(vec![agent.id]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let agent_principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2")
            .bind(company.id)
            .bind(agent.id)
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    let task_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO background_tasks (
               id, company_id, channel_id, correlation_id, task_type, payload,
               owner_principal_id, owner_principal_kind, ownership_version
           ) VALUES ($1, $2, $3, $4, 'notification_test', '{}', $5, 'agent', 1)"#,
    )
    .bind(task_id)
    .bind(company.id)
    .bind(channel.id)
    .bind(Uuid::new_v4())
    .bind(agent_principal)
    .execute(persistence.pool())
    .await
    .unwrap();
    Fixture {
        company_id: company.id,
        channel_id: channel.id,
        task_id,
        owner_principal,
        member_user_id: member.id,
        member_principal,
    }
}

async fn principal_for_user(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    user_id: Uuid,
) -> PrincipalId {
    PrincipalId::new(
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company_id)
            .bind(user_id)
            .fetch_one(persistence.pool())
            .await
            .unwrap(),
    )
}

async fn assign(
    persistence: &PostgresPersistence,
    fixture: &Fixture,
    actor: PrincipalId,
    target: PrincipalId,
    expected_version: u64,
) {
    persistence
        .change_task_ownership(TaskOwnershipCommand {
            execution: None,
            invocation: None,
            task_id: fixture.task_id,
            company_id: fixture.company_id,
            command_id: Uuid::new_v4(),
            expected_version,
            actor: TaskOwnershipActor {
                principal_id: actor,
                authority: TaskOwnershipAuthority::Manager,
            },
            operation: TaskOwnershipOperation::Transfer,
            new_owner: TaskOwner::Human(target),
            reason: TaskOwnershipReason::ManualAssignment,
            reason_detail: None,
            handoff_instruction: Some(
                "Continue the operational work without exposing this note.".into(),
            ),
        })
        .await
        .unwrap();
}

async fn claim_event(
    persistence: &PostgresPersistence,
    event_id: Uuid,
) -> crate::application::notification::ClaimedNotificationEvent {
    sqlx::query(
        "UPDATE notification_events SET available_at = '2000-01-01' WHERE id = $1 AND status = 'pending'",
    )
    .bind(event_id)
    .execute(persistence.pool())
    .await
    .unwrap();
    persistence
        .claim_notification_events(
            WorkerId::random(),
            Duration::from_secs(NOTIFICATION_EVENT_LEASE_SECONDS as u64),
            1,
        )
        .await
        .unwrap()
        .into_iter()
        .find(|claimed| claimed.event.id.as_uuid() == event_id)
        .expect("the event made oldest is claimed")
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

fn notification_email(
    persistence: &PostgresPersistence,
    projection: &NotificationProjection,
) -> NewStandaloneDelivery {
    let NotificationDisposition::Active(recipient) = &projection.disposition else {
        panic!("projection is active");
    };
    let renderer = Arc::new(EmailRenderer::new("example.test"));
    let renderers = Arc::new(TransportRenderers::new().register(renderer).unwrap());
    let composer = DeliveryComposer::new(renderers, Arc::new(persistence.clone()));
    let content = CanonicalContent::parse(
        format!("Action required: {}", projection.event.action_kind.label()),
        format!(
            "{} in {} / {}.\n\nOpen it securely: https://example.test/ui/notifications/{}/open",
            projection.event.action_kind.label(),
            recipient.company_label,
            recipient.channel_label,
            projection.event.notification_id,
        ),
    )
    .unwrap();
    composer
        .compose_standalone(StandaloneDeliveryRequest {
            correlation_id: recipient.correlation_id,
            purpose: DeliveryPurpose::Notification,
            source_key: format!(
                "actionable-notification:event:{}:user:{}",
                projection.event.id, recipient.user_id
            ),
            content: &content,
            context: DeliveryContext::Email(EmailDeliveryContext {
                from: "notifications@example.test".into(),
                from_name: Some("Mail Agents".into()),
                recipient_to: recipient.email.clone(),
                recipients_cc: Vec::new(),
                threading: EmailThreading::Standalone,
                relay: None,
            }),
        })
        .unwrap()
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
async fn assignment_projection_is_fenced_idempotent_and_never_owns_task_state() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = make_fixture(&persistence).await;
    assign(
        &persistence,
        &fixture,
        fixture.owner_principal,
        fixture.member_principal,
        1,
    )
    .await;
    let event_id: Uuid = sqlx::query_scalar(
        r#"SELECT id FROM notification_events
            WHERE company_id = $1 AND source_kind = 'task' AND source_id = $2
              AND action_kind = 'assignment'"#,
    )
    .bind(fixture.company_id)
    .bind(fixture.task_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    sqlx::query("UPDATE notification_events SET available_at = '2000-01-01' WHERE id = $1")
        .bind(event_id)
        .execute(&pool)
        .await
        .unwrap();
    let first =
        persistence.claim_notification_events(WorkerId::random(), Duration::from_secs(30), 1);
    let second =
        persistence.claim_notification_events(WorkerId::random(), Duration::from_secs(30), 1);
    let (first, second) = tokio::join!(first, second);
    let all_claimed = first
        .unwrap()
        .into_iter()
        .chain(second.unwrap())
        .collect::<Vec<_>>();
    let claimed = all_claimed
        .iter()
        .filter(|claimed| claimed.event.id.as_uuid() == event_id)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        claimed.len(),
        1,
        "competing projectors claim the event once"
    );
    let claimed = claimed.into_iter().next().unwrap();
    for foreign in all_claimed
        .iter()
        .filter(|claimed| claimed.event.id.as_uuid() != event_id)
    {
        release_foreign_claim(&persistence, foreign).await;
    }
    let projection = persistence
        .resolve_notification_projection(&claimed.event)
        .await
        .unwrap();
    assert!(projection.should_email());
    let email = notification_email(&persistence, &projection);
    let (outcome, _) = persistence
        .commit_notification_projection(NotificationProjectionCommit {
            fence: &claimed.lease,
            expected: &projection,
            email: Some(&email),
        })
        .await
        .unwrap();
    assert_eq!(outcome, NotificationProjectionCommitOutcome::Applied);

    let page = persistence
        .list_notifications(
            fixture.company_id,
            fixture.member_user_id,
            fixture.member_principal,
            &[fixture.channel_id],
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.unread_count, 1);
    // A non-NULL age exercises SQLx's PostgreSQL-to-f64 type check.
    let census = persistence.notification_census().await.unwrap();
    assert!(
        census
            .oldest_active_age_seconds
            .is_some_and(|seconds| seconds.is_finite() && seconds >= 0.0)
    );
    let notification = &page.items[0];
    let other = make_fixture(&persistence).await;
    assert!(
        persistence
            .list_notifications(
                other.company_id,
                fixture.member_user_id,
                fixture.member_principal,
                &[other.channel_id],
            )
            .await
            .unwrap()
            .items
            .is_empty(),
        "a valid sibling company scope cannot expose the notification"
    );
    assert!(matches!(
        persistence
            .open_notification(
                other.company_id,
                fixture.member_user_id,
                fixture.member_principal,
                &[other.channel_id],
                notification.id,
            )
            .await,
        Err(AppError::NotFound(_))
    ));
    let href = persistence
        .open_notification(
            fixture.company_id,
            fixture.member_user_id,
            fixture.member_principal,
            &[fixture.channel_id],
            notification.id,
        )
        .await
        .unwrap();
    assert!(href.contains(&fixture.task_id.to_string()));
    let task_status: String =
        sqlx::query_scalar("SELECT status FROM background_tasks WHERE id = $1")
            .bind(fixture.task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        task_status, "pending",
        "opening an alert does not complete work"
    );

    sqlx::query(
        r#"UPDATE notification_events
              SET status = 'pending', projected_at = NULL, available_at = '2000-01-01'
            WHERE id = $1"#,
    )
    .bind(event_id)
    .execute(&pool)
    .await
    .unwrap();
    let retry = claim_event(&persistence, event_id).await;
    let retry_projection = persistence
        .resolve_notification_projection(&retry.event)
        .await
        .unwrap();
    let retry_email = notification_email(&persistence, &retry_projection);
    persistence
        .commit_notification_projection(NotificationProjectionCommit {
            fence: &retry.lease,
            expected: &retry_projection,
            email: Some(&retry_email),
        })
        .await
        .unwrap();
    let counts: (i64, i64) = sqlx::query_as(
        r#"SELECT COUNT(*), COUNT(email_delivery_id)
             FROM notifications WHERE event_id = $1"#,
    )
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        counts,
        (1, 1),
        "projection and logical email are idempotent"
    );

    sqlx::query("DELETE FROM company_members WHERE company_id = $1 AND user_id = $2")
        .bind(fixture.company_id)
        .bind(fixture.member_user_id)
        .execute(&pool)
        .await
        .unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM notifications WHERE event_id = $1")
        .bind(event_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        state, "withdrawn",
        "removing membership withdraws the now-unauthorized alert"
    );

    CompanyPersistence::delete(&persistence, other.company_id)
        .await
        .unwrap();
    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn preferences_and_self_actions_suppress_email_without_hiding_in_app_work() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = make_fixture(&persistence).await;
    UserPersistence::set_notification_preferences(
        &persistence,
        fixture.member_user_id,
        NotificationPreferences {
            assignment_email_enabled: false,
            ..NotificationPreferences::default()
        },
    )
    .await
    .unwrap();
    assign(
        &persistence,
        &fixture,
        fixture.owner_principal,
        fixture.member_principal,
        1,
    )
    .await;
    let event_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM notification_events WHERE company_id = $1 AND source_id = $2",
    )
    .bind(fixture.company_id)
    .bind(fixture.task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let claimed = claim_event(&persistence, event_id).await;
    let projection = persistence
        .resolve_notification_projection(&claimed.event)
        .await
        .unwrap();
    assert!(!projection.should_email());
    persistence
        .commit_notification_projection(NotificationProjectionCommit {
            fence: &claimed.lease,
            expected: &projection,
            email: None,
        })
        .await
        .unwrap();
    assert_eq!(
        persistence
            .list_notifications(
                fixture.company_id,
                fixture.member_user_id,
                fixture.member_principal,
                &[fixture.channel_id],
            )
            .await
            .unwrap()
            .items
            .len(),
        1,
        "email opt-out does not hide owned in-app work"
    );

    assign(
        &persistence,
        &fixture,
        fixture.owner_principal,
        fixture.owner_principal,
        2,
    )
    .await;
    let self_event_id: Uuid = sqlx::query_scalar(
        r#"SELECT id FROM notification_events WHERE company_id = $1 AND source_id = $2
            ORDER BY source_generation DESC LIMIT 1"#,
    )
    .bind(fixture.company_id)
    .bind(fixture.task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let self_claimed = claim_event(&persistence, self_event_id).await;
    let self_projection = persistence
        .resolve_notification_projection(&self_claimed.event)
        .await
        .unwrap();
    assert!(
        !self_projection.should_email(),
        "self-assignment is not emailed"
    );
    persistence
        .commit_notification_projection(NotificationProjectionCommit {
            fence: &self_claimed.lease,
            expected: &self_projection,
            email: None,
        })
        .await
        .unwrap();
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE company_id = $1 AND source_id = $2 AND state = 'active'",
    )
    .bind(fixture.company_id)
    .bind(fixture.task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active, 1, "the newer owner gets one active in-app alert");
    let emails: i64 = sqlx::query_scalar(
        "SELECT COUNT(email_delivery_id) FROM notifications WHERE company_id = $1 AND source_id = $2",
    )
    .bind(fixture.company_id)
    .bind(fixture.task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(emails, 0);

    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn projection_resolution_serializes_with_an_owner_transfer() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = make_fixture(&persistence).await;
    assign(
        &persistence,
        &fixture,
        fixture.owner_principal,
        fixture.member_principal,
        1,
    )
    .await;
    let event_id: Uuid = sqlx::query_scalar(
        r#"SELECT event.id FROM notification_events AS event
            WHERE event.company_id = $1 AND event.source_id = $2
              AND event.action_kind = 'assignment'"#,
    )
    .bind(fixture.company_id)
    .bind(fixture.task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let claimed = claim_event(&persistence, event_id).await;

    let mut projection_tx = pool.begin().await.unwrap();
    let projection = resolve_projection_on(&mut projection_tx, &claimed.event)
        .await
        .unwrap();
    assert!(matches!(
        projection.disposition,
        NotificationDisposition::Active(ref recipient)
            if recipient.principal_id == fixture.member_principal
    ));

    let transfer_persistence = persistence.clone();
    let transfer_command = TaskOwnershipCommand {
        execution: None,
        invocation: None,
        task_id: fixture.task_id,
        company_id: fixture.company_id,
        command_id: Uuid::new_v4(),
        expected_version: 2,
        actor: TaskOwnershipActor {
            principal_id: fixture.owner_principal,
            authority: TaskOwnershipAuthority::Manager,
        },
        operation: TaskOwnershipOperation::Transfer,
        new_owner: TaskOwner::Human(fixture.owner_principal),
        reason: TaskOwnershipReason::WorkloadRebalance,
        reason_detail: None,
        handoff_instruction: Some("Take over this work.".into()),
    };
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let mut transfer = tokio::spawn(async move {
        let _ = started_tx.send(());
        transfer_persistence
            .change_task_ownership(transfer_command)
            .await
    });
    started_rx.await.expect("transfer task started");
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut transfer)
            .await
            .is_err(),
        "ownership waits while recipient resolution holds the source lock"
    );
    projection_tx.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), transfer)
        .await
        .expect("ownership resumes after the projector transaction")
        .expect("ownership task joins")
        .expect("ownership transfer succeeds");

    let stale = persistence
        .resolve_notification_projection(&claimed.event)
        .await
        .unwrap();
    assert_eq!(stale.disposition, NotificationDisposition::Withdrawn);
    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_failed_projection_batch_backs_off_before_it_can_be_claimed_again() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = make_fixture(&persistence).await;
    let source_ids = (0..NOTIFICATION_EVENT_CLAIM_BATCH)
        .map(|_| Uuid::new_v4())
        .collect::<Vec<_>>();
    let event_ids: Vec<Uuid> = sqlx::query_scalar(
        r#"INSERT INTO notification_events (
               company_id, source_kind, source_id, action_kind, source_generation, available_at
           )
           SELECT $1, 'task', source.source_id, 'task_failure', 1, '2000-01-01'
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
