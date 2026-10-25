//! 04.11 delivery-engine effects: workflow-linked `begin_part` rechecks, duplicate callbacks,
//! the reaper's parent locks and the receipt events every producer writes (D2 §4.1, §6; D3 §D).
use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::entities::transport::DeliveryStatus;
use crate::transport::{DeliveryOutcome, DeliveryQueue, PartResult, ProviderSendOutcome};
use std::time::Duration;

async fn parked_user(to: &[To]) -> (Setup, Uuid) {
    let setup = Box::pin(send(to)).await;
    parked(&setup).await;
    let delivery = delivery(setup.pool(), setup.scope().run, 0).await;
    (setup, delivery)
}

async fn begin(setup: &Setup, delivery: Uuid) -> DeliveryOutcome {
    let p = &setup.fixture.persistence;
    let claimed = claim(p, delivery).await;
    p.begin_part(&claimed.lease, claimed.parts[0].id)
        .await
        .unwrap()
}

/// No provider entry happened: unsent, no request started, one terminal event.
async fn assert_refused(setup: &Setup, delivery: Uuid, outcome: DeliveryOutcome, reason: &str) {
    assert_eq!(
        outcome,
        DeliveryOutcome::Cancelled(DeliveryStatus::DeadLetter)
    );
    let row = delivery_row(setup.pool(), delivery).await;
    assert_eq!(
        (
            row.status.as_str(),
            row.attempt_count,
            row.cancellation_reason.as_deref(),
            row.part.as_str()
        ),
        ("dead_letter", 0, Some(reason), "dead")
    );
    let started: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT request_started_at FROM message_delivery_parts WHERE delivery_id=$1",
    )
    .bind(delivery)
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert!(
        started.is_none(),
        "a refused part never reached the provider"
    );
    assert_eq!(
        events(setup.pool(), setup.scope().run).await,
        vec![(0, "sending".into(), "dead_letter".into())]
    );
    let p = &setup.fixture.persistence;
    assert_eq!(
        p.consume_message_receipts(setup.scope(), lease())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        action_state(setup.pool(), setup.scope().run).await,
        "failed"
    );
}

#[tokio::test]
async fn workflow_message_delivery_begin_part_refuses_a_recipient_no_longer_a_member() {
    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    sqlx::query("UPDATE users SET email='moved@example.test' WHERE id=$1")
        .bind(setup.user)
        .execute(setup.pool())
        .await
        .unwrap();
    let outcome = begin(&setup, delivery).await;
    assert_refused(&setup, delivery, outcome, "recipient_revoked").await;
}

#[tokio::test]
async fn workflow_message_delivery_begin_part_refuses_a_revoked_actor() {
    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    // Ownership moves to the recipient; the run actor is left a plain member.
    sqlx::query("UPDATE companies SET user_id=$2 WHERE id=$1")
        .bind(setup.fixture.target.company.as_uuid())
        .bind(setup.user)
        .execute(setup.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(setup.fixture.target.company.as_uuid())
        .bind(setup.owner)
        .execute(setup.pool())
        .await
        .unwrap();
    let outcome = begin(&setup, delivery).await;
    assert_refused(&setup, delivery, outcome, "workflow_revoked").await;
}

#[tokio::test]
async fn workflow_message_delivery_begin_part_refuses_a_run_past_its_deadline() {
    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp() WHERE id=$1")
        .bind(setup.scope().run.as_uuid())
        .execute(setup.pool())
        .await
        .unwrap();
    let outcome = begin(&setup, delivery).await;
    assert_refused(&setup, delivery, outcome, "workflow_expired").await;
}

/// The accepted path, then a duplicate unchanged callback: no status change, no second event.
#[tokio::test]
async fn workflow_message_delivery_duplicate_callback_adds_no_event() {
    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    let p = &setup.fixture.persistence;
    let claimed = claim(p, delivery).await;
    let part = claimed.parts[0].id;
    assert!(
        p.begin_part(&claimed.lease, part)
            .await
            .unwrap()
            .lease_held()
    );
    let accepted = ProviderSendOutcome::Delivered { provider_key: None };
    let result = |outcome| PartResult {
        fence: &claimed.lease,
        part_id: part,
        outcome,
    };
    assert_eq!(
        p.complete_part(result(&accepted)).await.unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Delivered)
    );
    assert_eq!(
        p.complete_part(result(&accepted)).await.unwrap(),
        DeliveryOutcome::LeaseLost
    );
    let rejected = ProviderSendOutcome::Terminal {
        class: crate::entities::transport::FailureClass::InvalidPayload,
        detail: crate::transport::FailureDetail::parse("late duplicate").unwrap(),
    };
    assert_eq!(
        p.complete_part(result(&rejected)).await.unwrap(),
        DeliveryOutcome::LeaseLost
    );
    assert_eq!(
        delivery_row(setup.pool(), delivery).await.status,
        "delivered"
    );
    assert_eq!(
        events(setup.pool(), setup.scope().run).await,
        vec![(0, "sending".into(), "delivered".into())]
    );
    // The SQL guard itself: a part that already took its outcome cannot take another.
    sqlx::query(
        "UPDATE message_delivery_parts SET status='dead', delivered_at=NULL WHERE delivery_id=$1",
    )
    .bind(delivery)
    .execute(setup.pool())
    .await
    .unwrap_err();
}

/// The reaper locks stranded parents first (SKIP LOCKED); a request that started becomes
/// unknown with one event, and the consumer makes the action uncertain.
#[tokio::test]
async fn workflow_message_delivery_reaper_locks_parents_and_marks_started_requests_unknown() {
    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    let p = &setup.fixture.persistence;
    let claimed = claim(p, delivery).await;
    assert!(
        p.begin_part(&claimed.lease, claimed.parts[0].id)
            .await
            .unwrap()
            .lease_held()
    );
    sqlx::query("UPDATE message_deliveries SET locked_at=clock_timestamp()-interval '2 minutes', lock_expires_at=clock_timestamp()-interval '1 minute' WHERE id=$1")
        .bind(delivery).execute(setup.pool()).await.unwrap();
    // A producer holding the parent: the sweep skips it rather than waiting.
    let mut holder = setup.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM message_deliveries WHERE id=$1 FOR UPDATE")
        .bind(delivery)
        .execute(&mut *holder)
        .await
        .unwrap();
    let reaped = tokio::time::timeout(Duration::from_secs(5), p.reap_expired_deliveries())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reaped.leases_expired, 0);
    holder.rollback().await.unwrap();
    let reaped = p.reap_expired_deliveries().await.unwrap();
    assert_eq!(reaped.leases_expired, 1);
    let row = delivery_row(setup.pool(), delivery).await;
    assert_eq!(
        (row.status.as_str(), row.part.as_str()),
        ("outcome_unknown", "outcome_unknown")
    );
    assert_eq!(
        events(setup.pool(), setup.scope().run).await,
        vec![(0, "sending".into(), "outcome_unknown".into())]
    );
    // Reaper and consumer concurrently: no deadlock, and the truth is consumed once.
    let (reaped, consumed) = tokio::join!(
        p.reap_expired_deliveries(),
        p.consume_message_receipts(setup.scope(), lease())
    );
    reaped.unwrap();
    assert_eq!(consumed.unwrap(), 1);
    assert_eq!(
        action_state(setup.pool(), setup.scope().run).await,
        "uncertain"
    );
}

/// F004: the lower-sequence producer is paused holding its parent while a higher one commits;
/// the consumer blocks on the parent and then observes both.
#[tokio::test]
async fn workflow_message_race_paused_producer_then_consumer_sees_both_events() {
    let _injected = Injected::mark();
    let (setup, first) = Box::pin(parked_user(&[To::User, To::Owner])).await;
    let second = delivery(setup.pool(), setup.scope().run, 1).await;
    let p = setup.fixture.persistence.clone();
    let scope = setup.scope();
    let mut paused = setup.pool().begin().await.unwrap();
    // The paused producer: its parent UPDATE (and so its event) is written, not committed.
    for (status, part) in [("sending", "sending")] {
        sqlx::query("UPDATE message_deliveries SET status=$2, execution_id=gen_random_uuid(), owner_worker_id=gen_random_uuid(), locked_at=clock_timestamp(), lock_expires_at=clock_timestamp()+interval '1 minute' WHERE id=$1")
            .bind(first).bind(status).execute(&mut *paused).await.unwrap();
        sqlx::query("UPDATE message_delivery_parts SET status=$2, request_started_at=clock_timestamp(), attempt_count=1 WHERE delivery_id=$1")
            .bind(first).bind(part).execute(&mut *paused).await.unwrap();
    }
    sqlx::query("UPDATE message_delivery_parts SET status='delivered', delivered_at=clock_timestamp() WHERE delivery_id=$1")
        .bind(first).execute(&mut *paused).await.unwrap();
    sqlx::query("UPDATE message_deliveries SET status='delivered', delivered_at=clock_timestamp(), attempt_count=1, execution_id=NULL, owner_worker_id=NULL, locked_at=NULL, lock_expires_at=NULL WHERE id=$1")
        .bind(first).execute(&mut *paused).await.unwrap();
    deliver(setup.pool(), second, "delivered").await;
    let consumer = tokio::spawn(async move { p.consume_message_receipts(scope, lease()).await });
    // The consumer is blocked behind the paused producer's parent lock, not finished early.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !consumer.is_finished(),
        "the consumer waits on the paused producer's parent"
    );
    paused.commit().await.unwrap();
    assert_eq!(consumer.await.unwrap().unwrap(), 1);
    assert_eq!(
        action_state(setup.pool(), setup.scope().run).await,
        "accepted"
    );
    assert_eq!(events(setup.pool(), setup.scope().run).await.len(), 2);
    assert_eq!(rows(setup.pool(), setup.scope().run).await.receipts, 1);
}

/// p08 D4.2: a dead workflow delivery has no task; its run resolves through the destination, so
/// it is the run actor's work while they are a member, and not unassigned.
#[tokio::test]
async fn attention_lists_a_dead_workflow_delivery_for_its_run_actor() {
    use crate::application::attention::AttentionPersistence;
    use crate::entities::attention::{
        AttentionQuery, AttentionResponsibility, AttentionSourceKind, AttentionView,
    };
    use crate::entities::transport::PrincipalId;

    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    deliver(setup.pool(), delivery, "dead").await;
    let p = &setup.fixture.persistence;
    let company = setup.scope().company.as_uuid();
    let channel: Uuid = sqlx::query_scalar("SELECT channel_id FROM message_deliveries WHERE id=$1")
        .bind(delivery)
        .fetch_one(setup.pool())
        .await
        .unwrap();
    let pool = setup.pool().clone();
    let principal = |user: Uuid| {
        let pool = pool.clone();
        async move {
            PrincipalId::new(
                sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
                    .bind(company)
                    .bind(user)
                    .fetch_one(&pool)
                    .await
                    .unwrap(),
            )
        }
    };
    let owner = principal(setup.owner).await;
    let recipient = principal(setup.user).await;
    let visible = [channel];
    let listed = |viewer: PrincipalId, view: AttentionView| {
        p.list_attention(AttentionQuery {
            company_id: company,
            principal_id: viewer,
            visible_channel_ids: &visible,
            manager: true,
            view,
            cursor: None,
            limit: 100,
        })
    };
    let failures = |items: Vec<crate::entities::attention::AttentionItem>| {
        items
            .into_iter()
            .filter(|item| item.source_kind == AttentionSourceKind::DeliveryFailure)
            .collect::<Vec<_>>()
    };

    let mine = failures(listed(owner, AttentionView::MyWork).await.unwrap().items);
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].source_id, delivery);
    assert_eq!(mine[0].run_id, Some(setup.scope().run.as_uuid()));
    assert_eq!(mine[0].state, "dead_letter");
    assert_eq!(
        mine[0].responsibility,
        AttentionResponsibility::Principal(owner)
    );
    assert_eq!(
        mine[0].href.as_deref(),
        Some(format!("/ui/deliveries?company_id={company}").as_str())
    );
    assert!(
        failures(
            listed(owner, AttentionView::Unassigned)
                .await
                .unwrap()
                .items
        )
        .is_empty()
    );
    assert!(
        failures(
            listed(recipient, AttentionView::MyWork)
                .await
                .unwrap()
                .items
        )
        .is_empty()
    );
    assert_eq!(
        failures(listed(owner, AttentionView::TeamWork).await.unwrap().items).len(),
        1
    );
    assert_eq!(
        p.operational_summary(company, &visible)
            .await
            .unwrap()
            .permanent_delivery_failure_count,
        1
    );
}

/// p08 D4.3: a dead workflow delivery notifies its run actor, once, however many projectors
/// compete for its event; the delivery's recipient, who did not start the run, is not notified.
#[tokio::test]
async fn a_dead_workflow_delivery_notifies_its_run_actor_once() {
    use crate::application::notification::{
        NotificationPersistence, NotificationProjectionCommit, NotificationProjectionCommitOutcome,
    };
    use crate::entities::notification::{
        NotificationActionKind, NotificationDisposition, NotificationSourceKind,
    };
    use crate::entities::transport::PrincipalId;
    use crate::transport::WorkerId;

    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    let p = &setup.fixture.persistence;
    let company = setup.scope().company.as_uuid();
    // In-app only, so the projection needs no composed email.
    sqlx::query(
        "INSERT INTO user_notification_preferences (user_id, delivery_failure_email_enabled) \
         VALUES ($1, false)",
    )
    .bind(setup.owner)
    .execute(setup.pool())
    .await
    .unwrap();
    deliver(setup.pool(), delivery, "dead").await;
    let (event, channel): (Uuid, Uuid) = sqlx::query_as(
        "SELECT event.id, delivery.channel_id FROM notification_events AS event \
         JOIN message_deliveries AS delivery ON delivery.id = event.source_id \
         WHERE event.company_id = $1 AND event.source_kind = 'delivery' \
           AND event.source_id = $2",
    )
    .bind(company)
    .bind(delivery)
    .fetch_one(setup.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE notification_events SET available_at = '2000-01-01' WHERE id = $1")
        .bind(event)
        .execute(setup.pool())
        .await
        .unwrap();

    let lease = std::time::Duration::from_secs(30);
    let (first, second) = tokio::join!(
        p.claim_notification_events(WorkerId::random(), lease, 1),
        p.claim_notification_events(WorkerId::random(), lease, 1),
    );
    let claimed: Vec<_> = first.unwrap().into_iter().chain(second.unwrap()).collect();
    assert_eq!(
        claimed.len(),
        1,
        "competing projectors claim the event once"
    );
    let claimed = &claimed[0];
    assert_eq!(claimed.event.id.as_uuid(), event);
    assert_eq!(
        (claimed.event.source_kind, claimed.event.action_kind),
        (
            NotificationSourceKind::Delivery,
            NotificationActionKind::DeliveryFailure
        )
    );

    let projection = p
        .resolve_notification_projection(&claimed.event)
        .await
        .unwrap();
    let NotificationDisposition::Active(recipient) = &projection.disposition else {
        panic!("the run actor is notified: {:?}", projection.disposition);
    };
    assert_eq!(recipient.user_id, setup.owner);
    assert_eq!(recipient.run_id, Some(setup.scope().run.as_uuid()));
    assert_eq!(recipient.channel_id, channel);
    assert!(recipient.thread_id.is_some());
    assert!(!projection.should_email());
    let (outcome, _) = p
        .commit_notification_projection(NotificationProjectionCommit {
            fence: &claimed.lease,
            expected: &projection,
            email: None,
        })
        .await
        .unwrap();
    assert_eq!(outcome, NotificationProjectionCommitOutcome::Applied);

    let principal = |user: Uuid| {
        let pool = setup.pool().clone();
        async move {
            PrincipalId::new(
                sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
                    .bind(company)
                    .bind(user)
                    .fetch_one(&pool)
                    .await
                    .unwrap(),
            )
        }
    };
    let owner_page = p
        .list_notifications(
            company,
            setup.owner,
            principal(setup.owner).await,
            &[channel],
        )
        .await
        .unwrap();
    assert_eq!(owner_page.items.len(), 1);
    assert_eq!(owner_page.items[0].source_id, delivery);
    let recipient_page = p
        .list_notifications(company, setup.user, principal(setup.user).await, &[channel])
        .await
        .unwrap();
    assert!(recipient_page.items.is_empty());
}

/// p08 D4.6 (F079): a message a run wrote reads back with that run, through every thread read.
#[tokio::test]
async fn a_message_a_run_wrote_reads_back_with_its_run() {
    use crate::application::use_cases::thread::ThreadPersistence;
    use crate::entities::message::CanonicalMessageId;

    let (setup, _delivery) = Box::pin(parked_user(&[To::User])).await;
    let p = &setup.fixture.persistence;
    let run = setup.scope().run.as_uuid();
    let (message, thread): (Uuid, Uuid) = sqlx::query_as(
        "SELECT message_id, thread_id FROM workflow_message_destinations WHERE run_id = $1",
    )
    .bind(run)
    .fetch_one(setup.pool())
    .await
    .unwrap();

    let views = p.list_thread_message_views(thread).await.unwrap();
    let view = views
        .iter()
        .find(|view| view.canonical_id.as_uuid() == message)
        .expect("the run's message is in its thread");
    assert_eq!(view.run_ids, vec![run]);
    let streamed = p
        .list_thread_message_views_after(thread, None, 50)
        .await
        .unwrap();
    assert_eq!(
        streamed
            .iter()
            .find(|view| view.canonical_id.as_uuid() == message)
            .unwrap()
            .run_ids,
        vec![run]
    );
    let single = p
        .get_thread_message_view(thread, CanonicalMessageId::new(message))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(single.run_ids, vec![run]);
}

/// p08 D4.4: open runs by state and readable channel, over runs the admission, claim and park
/// paths make: queued is pending, running is active, waiting is waiting; a run without a
/// channel counts toward the company alone; an unreadable channel's runs are not counted.
#[tokio::test]
async fn run_counts_group_open_runs_by_state_and_readable_channel() {
    use crate::application::run_counts::RunCountsReader;
    use crate::entities::run_counts::RunCounts;

    // The claimed run parks on its delivery: waiting.
    let (setup, _delivery) = Box::pin(parked_user(&[To::User])).await;
    let fixture = &setup.fixture;
    let p = &fixture.persistence;
    let company = setup.scope().company;
    let binding: Uuid = sqlx::query_scalar("SELECT binding_id FROM workflow_runs WHERE id = $1")
        .bind(setup.scope().run.as_uuid())
        .fetch_one(setup.pool())
        .await
        .unwrap();
    let target = BindingTarget {
        company,
        actor: fixture.target.actor,
        binding: WorkflowBindingId::new(binding),
    };
    admitted(fixture, target, setup.channel, "queued-here").await;
    let running = admitted(fixture, target, setup.channel, "running-here").await;
    p.claim_io(running, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    admitted(fixture, target, setup.other, "queued-there").await;
    admitted_as(fixture, target, RelatedAssociation::Company, "company-wide").await;
    let states: Vec<(Option<Uuid>, String)> = sqlx::query_as(
        "SELECT channel_id, state FROM workflow_runs WHERE company_id = $1 ORDER BY created_at",
    )
    .bind(company.as_uuid())
    .fetch_all(setup.pool())
    .await
    .unwrap();
    assert_eq!(
        states
            .iter()
            .map(|(_, state)| state.as_str())
            .collect::<Vec<_>>(),
        ["waiting", "queued", "running", "queued", "queued"]
    );

    let here = p
        .run_counts(company.as_uuid(), &[setup.channel])
        .await
        .unwrap();
    assert_eq!(
        here.channels[&setup.channel],
        RunCounts {
            pending: 1,
            active: 1,
            waiting: 1
        }
    );
    assert!(!here.channels.contains_key(&setup.other));
    assert_eq!(
        here.company,
        RunCounts {
            pending: 2,
            active: 1,
            waiting: 1
        },
        "the company-wide run counts; the unreadable channel's does not"
    );
    let both = p
        .run_counts(company.as_uuid(), &[setup.channel, setup.other])
        .await
        .unwrap();
    assert_eq!(both.channels[&setup.other].pending, 1);
    assert_eq!(both.company.pending, 3);
}
