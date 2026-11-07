//! CHS-P05-B1-F1: release authority is current when a queued question may enter transport.
use super::*;
use crate::adapters::persistence::workflow::knowledge;
use crate::adapters::persistence::workflow::tests::message_tests::support::claim;
use crate::entities::access_level::AccessLevel;
use crate::entities::transport::DeliveryStatus;
use crate::transport::{DeliveryOutcome, DeliveryQueue, PartResult, ProviderSendOutcome};

#[derive(sqlx::FromRow)]
struct Sender {
    principal_id: Uuid,
    email: String,
}
struct ClassifiedQuestion {
    asked: Asked,
    user: Uuid,
    email: EmailAddress,
}

async fn classified(s: &Intents) -> ClassifiedQuestion {
    let pending = Box::pin(pending(s, 1)).await;
    let sender:Sender=sqlx::query_as("SELECT source.author_principal_id AS principal_id,identity.subject AS email FROM workflow_executions AS execution JOIN messages AS source ON source.company_id=execution.company_id AND source.id=(execution.frozen_inputs->>'source_message')::uuid JOIN message_participants AS party ON party.company_id=source.company_id AND party.message_id=source.id AND party.kind='sender' JOIN participant_identities AS identity ON identity.company_id=party.company_id AND identity.id=party.participant_identity_id WHERE execution.company_id=$1 AND execution.id=$2 ORDER BY party.position LIMIT 1")
        .bind(pending.scope.company.as_uuid()).bind(pending.scope.execution.as_uuid()).fetch_one(s.pool()).await.unwrap();
    let user = s
        .p()
        .create_user(&Uuid::new_v4().simple().to_string(), &sender.email, "hash")
        .await
        .unwrap()
        .id;
    let company = pending.scope.company.as_uuid();
    let mut tx = s.pool().begin().await.unwrap();
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(user)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES($1,$2,$3,'member',5)")
        .bind(Uuid::new_v4()).bind(company).bind(user).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE principals SET kind='person',user_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(company)
        .bind(sender.principal_id)
        .bind(user)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE participant_identities SET status='verified',provenance='account',claim_metadata='{\"version\":1,\"kind\":\"account\"}'::jsonb WHERE company_id=$1 AND principal_id=$2 AND subject=$3")
        .bind(company).bind(sender.principal_id).bind(&sender.email).execute(&mut *tx).await.unwrap();
    knowledge::raise_content_level_on(
        &mut tx,
        pending.scope.company,
        pending.scope.run,
        AccessLevel::L5,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let asked = Box::pin(published(s, &pending)).await;
    let level: i16 = sqlx::query_scalar(
        "SELECT access_level::smallint FROM messages WHERE company_id=$1 AND id=$2",
    )
    .bind(company)
    .bind(asked.question)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(level, 5);
    ClassifiedQuestion {
        asked,
        user,
        email: EmailAddress::from(sender.email),
    }
}

async fn demote(s: &Intents, question: &ClassifiedQuestion) {
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(question.asked.answer.company.as_uuid())
        .bind(question.user)
        .execute(s.pool())
        .await
        .unwrap();
}

async fn refused(s: &Intents, question: &ClassifiedQuestion) {
    let delivery = delivery(s.pool(), question.asked.run, 0).await;
    let claimed = claim(s.p(), delivery).await;
    assert_eq!(
        s.p()
            .begin_part(&claimed.lease, claimed.parts[0].id)
            .await
            .unwrap(),
        DeliveryOutcome::Cancelled(DeliveryStatus::DeadLetter)
    );
    let started: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
        "SELECT request_started_at FROM message_delivery_parts WHERE delivery_id=$1",
    )
    .bind(delivery)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert!(started.is_none());
    let reason: Option<String> =
        sqlx::query_scalar("SELECT cancellation_reason FROM message_deliveries WHERE id=$1")
            .bind(delivery)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(reason.as_deref(), Some("recipient_revoked"));
    assert!(matches!(
        s.p().resume_wait(question.asked.answer).await.unwrap(),
        WaitProgress::Pending
    ));
}

#[tokio::test]
async fn channel_reply_hitl_prompt_release_clearance_revoked_before_entry() {
    let s = Box::pin(Intents::new()).await;
    let question = Box::pin(classified(&s)).await;
    demote(&s, &question).await;
    refused(&s, &question).await;
}

#[tokio::test]
async fn channel_reply_hitl_prompt_release_verification_revoked_before_entry() {
    let s = Box::pin(Intents::new()).await;
    let question = Box::pin(classified(&s)).await;
    sqlx::query("UPDATE users SET email_verified_at=NULL WHERE id=$1")
        .bind(question.user)
        .execute(s.pool())
        .await
        .unwrap();
    refused(&s, &question).await;
}

#[tokio::test]
async fn channel_reply_hitl_prompt_release_retired_frozen_address_is_refused() {
    let s = Box::pin(Intents::new()).await;
    let question = Box::pin(classified(&s)).await;
    let replacement = format!("replacement-{}@example.test", Uuid::new_v4().simple());
    let mut tx = s.pool().begin().await.unwrap();
    sqlx::query("UPDATE users SET email=$2,email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(question.user)
        .bind(&replacement)
        .execute(&mut *tx)
        .await
        .unwrap();
    crate::adapters::persistence::account_identity::rekey_account_identity_on(
        &mut tx,
        question.user,
        question.email.as_str(),
        &replacement,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    refused(&s, &question).await;
}

#[tokio::test]
async fn channel_reply_hitl_prompt_release_thread_audience_change_is_refused() {
    let s = Box::pin(Intents::new()).await;
    let question = Box::pin(classified(&s)).await;
    let principal = Uuid::new_v4();
    let mut tx = s.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO principals(id,company_id,kind,display_label) VALUES($1,$2,'external','New participant')")
        .bind(principal).bind(question.asked.answer.company.as_uuid()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) VALUES($1,$2,$3,$4,'participant')")
        .bind(question.asked.answer.company.as_uuid()).bind(question.asked.channel).bind(question.asked.thread).bind(principal)
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    refused(&s, &question).await;
}

#[tokio::test]
async fn channel_reply_hitl_prompt_release_safe_retry_rechecks_without_changing_the_human_wait() {
    let s = Box::pin(Intents::new()).await;
    let question = Box::pin(classified(&s)).await;
    let delivery = delivery(s.pool(), question.asked.run, 0).await;
    let first = claim(s.p(), delivery).await;
    let part = first.parts[0].id;
    assert!(
        s.p()
            .begin_part(&first.lease, part)
            .await
            .unwrap()
            .lease_held()
    );
    let retry = ProviderSendOutcome::RetryAfter {
        retry_after: Duration::ZERO,
        class: crate::entities::transport::FailureClass::Network,
        detail: crate::transport::FailureDetail::parse("Definitely not accepted").unwrap(),
    };
    s.p()
        .complete_part(PartResult {
            fence: &first.lease,
            part_id: part,
            outcome: &retry,
        })
        .await
        .unwrap();
    let before: Value = sqlx::query_scalar(
        "SELECT to_jsonb(question) FROM workflow_channel_questions AS question WHERE run_id=$1",
    )
    .bind(question.asked.run.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    demote(&s, &question).await;
    let second = claim(s.p(), delivery).await;
    assert_eq!(
        s.p().begin_part(&second.lease, part).await.unwrap(),
        DeliveryOutcome::Cancelled(DeliveryStatus::DeadLetter)
    );
    let attempts: i32 =
        sqlx::query_scalar("SELECT attempt_count FROM message_delivery_parts WHERE id=$1")
            .bind(part.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(attempts, 1, "the refused retry admitted no second request");
    let after: Value = sqlx::query_scalar(
        "SELECT to_jsonb(question) FROM workflow_channel_questions AS question WHERE run_id=$1",
    )
    .bind(question.asked.run.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(before, after);
}

#[tokio::test]
async fn channel_reply_hitl_prompt_release_committed_demotion_wins_while_entry_is_blocked() {
    let s = Box::pin(Intents::new()).await;
    let question = Box::pin(classified(&s)).await;
    let delivery = delivery(s.pool(), question.asked.run, 0).await;
    let claimed = claim(s.p(), delivery).await;
    let mut blocker = s.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(question.asked.answer.company.as_uuid())
        .bind(question.asked.run.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let p = s.p().clone();
    let fence = claimed.lease;
    let part = claimed.parts[0].id;
    let entering = tokio::spawn(async move { p.begin_part(&fence, part).await });
    crate::adapters::persistence::test_support::wait_until_a_backend_is_blocked(s.pool()).await;
    tokio::time::timeout(Duration::from_secs(2), demote(&s, &question))
        .await
        .expect("revoker commits independently");
    blocker.rollback().await.unwrap();
    assert_eq!(
        entering.await.unwrap().unwrap(),
        DeliveryOutcome::Cancelled(DeliveryStatus::DeadLetter)
    );
}

#[tokio::test]
async fn channel_reply_hitl_prompt_release_authorized_entry_retains_accepted_truth_after_demotion()
{
    let s = Box::pin(Intents::new()).await;
    let question = Box::pin(classified(&s)).await;
    let delivery = delivery(s.pool(), question.asked.run, 0).await;
    let claimed = claim(s.p(), delivery).await;
    let part = claimed.parts[0].id;
    assert!(
        s.p()
            .begin_part(&claimed.lease, part)
            .await
            .unwrap()
            .lease_held()
    );
    demote(&s, &question).await;
    let accepted = ProviderSendOutcome::Delivered { provider_key: None };
    assert_eq!(
        s.p()
            .complete_part(PartResult {
                fence: &claimed.lease,
                part_id: part,
                outcome: &accepted
            })
            .await
            .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Delivered)
    );
    assert!(matches!(
        s.p().resume_wait(question.asked.answer).await.unwrap(),
        WaitProgress::Pending
    ));
}
