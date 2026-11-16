//! Real personal ingress, the durable worker, current list/open authorization and retirement.
use super::*;
use crate::entities::thread::Thread;
use crate::{
    adapters::persistence::test_support::notification_worker,
    application::notification::NotificationPersistence,
    entities::notification::{NotificationSourceKind, NotificationState},
};
use futures::future::LocalBoxFuture;

// Erase heavyweight ingress/worker setup futures at the helper boundary so each test
// does not duplicate their nested types in Tokio and debug metadata.
fn received(f: &Fixture, principal: Uuid, sender: Uuid) -> LocalBoxFuture<'_, (Thread, Uuid)> {
    Box::pin(async move {
        let result = threads(f)
            .ingest_canonical(canonical(f, principal, sender).await)
            .await
            .unwrap();
        assert!(result.accepted, "{:?}", result.rejection);
        let thread = result.thread.unwrap();
        let association: Uuid = sqlx::query_scalar(
            "SELECT id FROM thread_messages WHERE company_id=$1 AND principal_id=$2 AND thread_id=$3",
        )
        .bind(f.company.id)
        .bind(principal)
        .bind(thread.id)
        .fetch_one(f.pool())
        .await
        .unwrap();
        (thread, association)
    })
}

fn list(
    f: &Fixture,
    user: Uuid,
    principal: Uuid,
) -> LocalBoxFuture<'_, crate::entities::notification::NotificationPage> {
    Box::pin(async move {
        f.persistence
            .list_notifications(f.company.id, user, PrincipalId::new(principal), &[])
            .await
            .unwrap()
    })
}

fn projected(f: &Fixture) -> LocalBoxFuture<'_, usize> {
    Box::pin(async move {
        let claimed = notification_worker(&f.persistence, Default::default())
            .drain_once()
            .await
            .unwrap();
        let unfinished: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM notification_events WHERE company_id=$1 AND status<>'projected'",
        )
        .bind(f.company.id)
        .fetch_one(f.pool())
        .await
        .unwrap();
        assert_eq!(
            unfinished, 0,
            "claim count alone does not prove successful projection"
        );
        claimed
    })
}

#[tokio::test]
async fn direct_thread_notification_real_self_ingress_worker_list_open_and_read_state() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let principal = f.principal_of(f.owner).await;
    let (thread, association) = received(&f, principal, f.owner).await;
    let source: (Uuid, Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT event.source_id,event.actor_principal_id,event.direct_membership_id,membership.id \
         FROM notification_events AS event JOIN LATERAL membership_of($2, event.company_id) AS membership ON true \
         WHERE event.company_id=$1 AND event.source_kind='direct_message'",
    )
    .bind(f.company.id)
    .bind(f.owner)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(source, (association, principal, source.3, source.3));
    assert_eq!(projected(&f).await, 1);
    assert_eq!(projected(&f).await, 0);
    let before = list(&f, f.owner, principal).await;
    assert_eq!(before.items.len(), 1);
    assert_eq!(before.unread_count, 1);
    let item = &before.items[0];
    assert_eq!(item.source_kind, NotificationSourceKind::DirectMessage);
    assert_eq!(item.source_id, association);
    assert_eq!(item.inbox, Some(thread.target));
    assert!(item.read_at.is_none());
    let expected = format!(
        "/ui?company_id={}&principal_id={principal}&thread_id={}",
        f.company.id, thread.id
    );
    assert_eq!(
        f.persistence
            .notification_href(
                f.company.id,
                f.owner,
                PrincipalId::new(principal),
                &[],
                item.id
            )
            .await
            .unwrap(),
        expected
    );
    assert_eq!(
        f.persistence
            .open_notification(
                f.company.id,
                f.owner,
                PrincipalId::new(principal),
                &[],
                item.id
            )
            .await
            .unwrap(),
        expected
    );
    let after = list(&f, f.owner, principal).await;
    assert_eq!(after.unread_count, 0);
    assert_eq!(after.items[0].state, NotificationState::Active);
    assert!(after.items[0].read_at.is_some());
    let mail: i64 =
        sqlx::query_scalar("SELECT count(*) FROM message_deliveries WHERE purpose='notification'")
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(
        mail, 0,
        "personal receipts never email, including self-mail"
    );
}

#[tokio::test]
async fn direct_thread_notification_unprocessed_retirement_and_rejoin_never_revive_an_old_source() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (user, name) = account(&f.persistence, "receipt_member").await;
    let principal = f.join(user, &name).await;
    let (_, association) = received(&f, principal, f.owner).await;
    let old_member: Uuid = sqlx::query_scalar(
        "SELECT id FROM membership_of($2, $1) WHERE company_id=$1 AND user_id=$2",
    )
    .bind(f.company.id)
    .bind(user)
    .fetch_one(f.pool())
    .await
    .unwrap();
    f.persistence
        .remove_member(crate::use_cases::company_invite::RemoveCompanyMember {
            actor_id: f.company.user_id,
            company_id: f.company.id,
            member_user_id: user,
        })
        .await
        .unwrap();
    assert_eq!(f.join(user, &name).await, principal);
    let new_member: Uuid = sqlx::query_scalar(
        "SELECT id FROM membership_of($2, $1) WHERE company_id=$1 AND user_id=$2",
    )
    .bind(f.company.id)
    .bind(user)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_ne!(old_member, new_member);
    assert_eq!(projected(&f).await, 1);
    assert!(list(&f, user, principal).await.items.is_empty());
    assert!(
        list(&f, f.owner, f.principal_of(f.owner).await)
            .await
            .items
            .is_empty(),
        "owner gets no duplicate"
    );
    received(&f, principal, f.owner).await;
    assert_eq!(projected(&f).await, 1);
    let current = list(&f, user, principal).await;
    assert_eq!(current.items.len(), 1);
    assert_ne!(current.items[0].source_id, association);
}

#[tokio::test]
async fn direct_thread_notification_projected_retirement_withdraws_before_open_and_stays_hidden_on_rejoin()
 {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (user, name) = account(&f.persistence, "receipt_open").await;
    let principal = f.join(user, &name).await;
    received(&f, principal, f.owner).await;
    assert_eq!(projected(&f).await, 1);
    let item = list(&f, user, principal).await.items.remove(0);
    f.persistence
        .open_notification(
            f.company.id,
            user,
            PrincipalId::new(principal),
            &[],
            item.id,
        )
        .await
        .unwrap();
    let read_before: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT read_at FROM notifications WHERE id=$1")
            .bind(item.id.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap();

    f.persistence
        .remove_member(crate::use_cases::company_invite::RemoveCompanyMember {
            actor_id: f.company.user_id,
            company_id: f.company.id,
            member_user_id: user,
        })
        .await
        .unwrap();
    let (state, read_after): (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT state,read_at FROM notifications WHERE id=$1")
            .bind(item.id.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(state, "withdrawn");
    assert_eq!(read_after, Some(read_before));
    assert_eq!(f.join(user, &name).await, principal);
    assert!(list(&f, user, principal).await.items.is_empty());
    assert!(
        f.persistence
            .open_notification(
                f.company.id,
                user,
                PrincipalId::new(principal),
                &[],
                item.id
            )
            .await
            .is_err()
    );
    assert!(
        f.persistence
            .notification_href(
                f.company.id,
                user,
                PrincipalId::new(principal),
                &[],
                item.id
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn direct_thread_notification_competing_mixed_redelivery_emits_only_each_persons_exact_event()
{
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (user, name) = account(&f.persistence, "receipt_mixed").await;
    let person = f.join(user, &name).await;
    let owner = f.principal_of(f.owner).await;
    let agent = production_agent(&f, false).await;
    let desk = f.desk("receipt-desk", &[]).await.unwrap();
    let mut addresses = Vec::new();
    for target in [
        AddressTarget::Principal(PrincipalId::new(person)),
        AddressTarget::Principal(PrincipalId::new(owner)),
        AddressTarget::Principal(PrincipalId::new(agent)),
        AddressTarget::Desk(desk),
    ] {
        let address = f
            .persistence
            .address_for_target(f.company.id, target)
            .await
            .unwrap()
            .unwrap();
        addresses.push(format!("{}@{}.{DOMAIN}", address.slug, f.company.slug));
    }
    let mail = EmailIngressAdapter::new(DOMAIN)
        .accept(
            crate::adapters::protocols::email::parser::RawInboundPayload {
                from: email_of(f.pool(), f.owner).await,
                to: addresses.join(","),
                subject: Some("Mixed receipts".into()),
                text: Some("One canonical message".into()),
                headers: Some("Message-ID: <mixed-receipt@sender.test>\n".into()),
                ..Default::default()
            },
            EmailIngressTrust::Application,
        )
        .unwrap()
        .into_inbound(IngressOrigin::TrustedApplication, ReplyDelivery::InAppOnly);
    let service = threads(&f);
    let refused = service.ingest(mail.clone()).await.unwrap();
    assert!(
        !refused.accepted,
        "the namespace fixture deliberately creates a disabled desk"
    );
    let committed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM thread_messages WHERE company_id=$1")
            .bind(f.company.id)
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(
        committed, 0,
        "one disabled recipient refuses the whole message"
    );
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM notification_events WHERE company_id=$1")
            .bind(f.company.id)
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(events, 0);
    ChannelPersistence::update(
        &f.persistence,
        ChannelUpdate {
            company_id: f.company.id,
            channel_id: desk,
            write: ChannelWrite {
                enabled: true,
                ..desk_write("receipt-desk", &[])
            },
        },
    )
    .await
    .unwrap();
    let (first, second) = tokio::join!(service.ingest(mail.clone()), service.ingest(mail));
    let first = first.unwrap();
    let second = second.unwrap();
    assert!(
        first.accepted && second.accepted,
        "first={:?}; second={:?}",
        first.rejection,
        second.rejection
    );
    let sources: Vec<(Uuid,Uuid)> = sqlx::query_as(
        "SELECT association.principal_id,event.source_id FROM notification_events AS event \
         JOIN thread_messages AS association ON association.company_id=event.company_id AND association.id=event.source_id \
         WHERE event.company_id=$1 AND event.source_kind='direct_message' ORDER BY association.principal_id")
        .bind(f.company.id).fetch_all(f.pool()).await.unwrap();
    let mut targets = vec![person, owner];
    targets.sort();
    assert_eq!(sources.iter().map(|row| row.0).collect::<Vec<_>>(), targets);
    let messages: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE company_id=$1")
        .bind(f.company.id)
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(messages, 1);
    assert_eq!(projected(&f).await, 2);
    for (account, target) in [(user, person), (f.owner, owner)] {
        let page = list(&f, account, target).await;
        assert_eq!(page.items.len(), 1);
        assert_eq!(
            page.items[0].inbox,
            Some(AddressTarget::Principal(PrincipalId::new(target)))
        );
    }
}

#[tokio::test]
async fn direct_thread_notification_source_deletion_resolves_without_an_actionable_link() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let owner = f.principal_of(f.owner).await;
    let (thread, association) = received(&f, owner, f.owner).await;
    assert_eq!(projected(&f).await, 1);
    let item = list(&f, f.owner, owner).await.items.remove(0);
    sqlx::query("DELETE FROM thread_messages WHERE company_id=$1 AND id=$2")
        .bind(f.company.id)
        .bind(association)
        .execute(f.pool())
        .await
        .unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM notifications WHERE id=$1")
        .bind(item.id.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(state, "resolved");
    assert!(list(&f, f.owner, owner).await.items.is_empty());
    sqlx::query("DELETE FROM threads WHERE company_id=$1 AND id=$2")
        .bind(f.company.id)
        .bind(thread.id)
        .execute(f.pool())
        .await
        .unwrap();
    let (replacement, _) = account(&f.persistence, "replacement_owner").await;
    crate::adapters::persistence::test_support::transfer_company_ownership(
        f.pool(),
        f.company.id,
        replacement,
        crate::entities::company_member::CompanyAccessRole::Member,
    )
    .await;
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND id=$2")
        .bind(f.company.id)
        .bind(owner)
        .execute(f.pool())
        .await
        .unwrap();
    let retained: i64 = sqlx::query_scalar("SELECT count(*) FROM notifications WHERE id=$1")
        .bind(item.id.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(
        retained, 0,
        "unused principal deletion must allow its native recipient FK cleanup"
    );

    assert!(
        f.persistence
            .open_notification(f.company.id, f.owner, PrincipalId::new(owner), &[], item.id)
            .await
            .is_err()
    );
}

#[path = "notification_authority_tests.rs"]
mod authority;

#[path = "notification_race_tests.rs"]
mod races;
