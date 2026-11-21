//! Native source/recipient integrity and account/content authority changes.
use super::*;

#[tokio::test]
async fn direct_thread_notification_native_guards_refuse_wrong_membership_retargeting_and_receipt_owner()
 {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (user, name) = account(&f.persistence, "receipt_guard").await;
    let target = f.join(user, &name).await;
    let (other, name) = account(&f.persistence, "receipt_guard_other").await;
    let other_target = f.join(other, &name).await;
    let (_, source) = received(&f, target, f.owner).await;
    let (event,membership,actor):(Uuid,Uuid,Uuid)=sqlx::query_as(
        "SELECT id,direct_membership_id,actor_principal_id FROM notification_events WHERE company_id=$1 AND source_id=$2")
        .bind(f.company.id).bind(source).fetch_one(f.pool()).await.unwrap();
    let wrong_member: Uuid = sqlx::query_scalar(
        "SELECT id FROM membership_of($2, $1) WHERE company_id=$1 AND user_id=$2",
    )
    .bind(f.company.id)
    .bind(other)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_ne!(membership, wrong_member);
    let error=sqlx::query("INSERT INTO notification_events(company_id,source_kind,source_id,action_kind,source_generation,actor_principal_id,direct_membership_id) \
        VALUES($1,'direct_message',$2,'direct_message_received',1,$3,$4)")
        .bind(f.company.id).bind(source).bind(actor).bind(wrong_member).execute(f.pool()).await.unwrap_err();
    assert_eq!(
        refused_by(error),
        ("23514".into(), "direct_message_event_source".into())
    );
    let error = sqlx::query("UPDATE notification_events SET direct_membership_id=$2 WHERE id=$1")
        .bind(event)
        .bind(wrong_member)
        .execute(f.pool())
        .await
        .unwrap_err();
    assert_eq!(
        refused_by(error),
        (
            "23514".into(),
            "workflow_guard:guard_notification_event_identity".into()
        )
    );
    let error=sqlx::query("INSERT INTO notifications(company_id,recipient_user_id,recipient_principal_id,event_id,source_kind,source_id,action_kind,source_generation,principal_id) \
        VALUES($1,$2,$3,$4,'direct_message',$5,'direct_message_received',1,$6)")
        .bind(f.company.id).bind(other).bind(other_target).bind(event).bind(source).bind(target)
        .execute(f.pool()).await.unwrap_err();
    assert_eq!(
        refused_by(error),
        (
            "23514".into(),
            "direct_message_notification_recipient".into()
        )
    );
    assert_eq!(projected(&f).await, 1);
    let item = list(&f, user, target).await.items.remove(0);
    let error = sqlx::query("UPDATE notifications SET recipient_principal_id=$2 WHERE id=$1")
        .bind(item.id.as_uuid())
        .bind(other_target)
        .execute(f.pool())
        .await
        .unwrap_err();
    assert_eq!(
        refused_by(error),
        (
            "23514".into(),
            "direct_message_notification_identity".into()
        )
    );
    assert!(list(&f, other, other_target).await.items.is_empty());
    assert!(
        f.persistence
            .open_notification(
                f.company.id,
                other,
                PrincipalId::new(other_target),
                &[],
                item.id
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn direct_thread_notification_account_deletion_and_verified_replacement_never_transfer_receipts()
 {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (user, name) = account(&f.persistence, "receipt_deleted").await;
    let target = f.join(user, &name).await;
    let old_email = email_of(f.pool(), user).await;
    let (_, old_source) = received(&f, target, f.owner).await;
    f.persistence.delete_user_account(user).await.unwrap();
    let username = format!("receipt_replacement_{}", Uuid::new_v4().simple());
    let replacement = f
        .persistence
        .create_user(&username, &old_email, "hash")
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at=CURRENT_TIMESTAMP WHERE id=$1")
        .bind(replacement.id)
        .execute(f.pool())
        .await
        .unwrap();
    let replacement_target = f.join(replacement.id, &username).await;
    assert_ne!(replacement_target, target);
    assert_eq!(projected(&f).await, 1);
    assert!(
        list(&f, replacement.id, replacement_target)
            .await
            .items
            .is_empty()
    );
    let target_pin: Uuid = sqlx::query_scalar(
        "SELECT principal_id FROM thread_messages WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company.id)
    .bind(old_source)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(target_pin, target);
    received(&f, replacement_target, f.owner).await;
    assert_eq!(projected(&f).await, 1);
    let current = list(&f, replacement.id, replacement_target).await;
    assert_eq!(current.items.len(), 1);
    assert_ne!(current.items[0].source_id, old_source);
    assert_eq!(
        current.items[0].inbox,
        Some(AddressTarget::Principal(PrincipalId::new(
            replacement_target
        )))
    );
}

#[tokio::test]
async fn direct_thread_notification_content_clearance_loss_withdraws_before_list_and_open() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (user, name) = account(&f.persistence, "receipt_classified").await;
    let target = f.join(user, &name).await;
    sqlx::query(
        "UPDATE department_members SET access_level=5 WHERE company_id=$1 AND root_user_id=$2",
    )
    .bind(f.company.id)
    .bind(user)
    .execute(f.pool())
    .await
    .unwrap();
    let thread = crate::application::ports::thread::ThreadPersistence::create_thread(
        &f.persistence,
        f.company.id,
        AddressTarget::Principal(PrincipalId::new(target)),
        "Classified private subject",
        &[],
    )
    .await
    .unwrap();
    let message = Uuid::new_v4();
    // Valid retained stamped-row stand-in. This proves current notification authority, not the
    // workflow producer's classification; production stamping has its own knowledge suites.
    sqlx::query("INSERT INTO messages(id,company_id,author_principal_id,subject,clean_text_body,attachments,direction,role,correlation_id,content_hash,audience,access_level,content_exact) \
        VALUES($1,$2,$3,'Classified private subject','Classified private body','{\"version\":\"1\",\"items\":[]}', \
        'inbound','system',$4,decode(repeat('ab',32),'hex'),'external_conversation',5,true)")
        .bind(message).bind(f.company.id).bind(f.principal_of(f.owner).await).bind(Uuid::new_v4())
        .execute(f.pool()).await.unwrap();
    crate::application::ports::thread::ThreadPersistence::associate_message(
        &f.persistence,
        thread.id,
        crate::entities::message::CanonicalMessageId::new(message),
        crate::entities::message::ThreadEntryKind::Conversation,
    )
    .await
    .unwrap();
    assert_eq!(projected(&f).await, 1);
    let item = list(&f, user, target).await.items.remove(0);
    sqlx::query(
        "UPDATE department_members SET access_level=0 WHERE company_id=$1 AND root_user_id=$2",
    )
    .bind(f.company.id)
    .bind(user)
    .execute(f.pool())
    .await
    .unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM notifications WHERE id=$1")
        .bind(item.id.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(state, "withdrawn");
    let hidden = list(&f, user, target).await;
    assert!(hidden.items.is_empty() && hidden.unread_count == 0);
    assert!(
        f.persistence
            .notification_href(f.company.id, user, PrincipalId::new(target), &[], item.id)
            .await
            .is_err()
    );
    assert!(
        f.persistence
            .open_notification(f.company.id, user, PrincipalId::new(target), &[], item.id)
            .await
            .is_err()
    );
}
