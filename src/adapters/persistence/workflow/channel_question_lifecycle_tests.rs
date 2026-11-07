use super::resume::ingress;
use super::*;
use crate::adapters::persistence::workflow::tests::message_tests::support::claim;
use crate::entities::transport::DeliveryStatus;
use crate::transport::{DeliveryOutcome, DeliveryQueue};

#[tokio::test]
async fn channel_reply_hitl_resume_late_response_races_timeout_and_stops_unsent_work_before_wake() {
    let s = Box::pin(Intents::new()).await;
    let pending = Box::pin(pending_with(
        &s,
        1,
        |source| {
            source["steps"]["start"]["with"]["deadline"] = json!({"literal":
            (Utc::now()+chrono::Duration::seconds(5)).to_rfc3339()});
        },
        crate::application::workflow::DeliveryPolicy::Deliver,
    ))
    .await;
    let asked = Box::pin(published(&s, &pending)).await;
    let id = reply(&s, &asked, By::Author, "to").await;
    let response = ingress(&s, &asked, id).await;
    let delivery = delivery(s.pool(), asked.run, 0).await;
    let claimed = claim(s.p(), delivery).await;
    let deadline: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT deadline FROM workflow_channel_questions WHERE run_id=$1")
            .bind(asked.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    tokio::time::sleep(
        (deadline - Utc::now()).to_std().unwrap_or_default() + Duration::from_millis(20),
    )
    .await;
    // The transport gate independently stops the send before the wait sweeper runs.
    assert_eq!(
        s.p()
            .begin_part(&claimed.lease, claimed.parts[0].id)
            .await
            .unwrap(),
        DeliveryOutcome::Cancelled(DeliveryStatus::DeadLetter)
    );
    let (reply, timeout) = tokio::join!(
        s.p().deliver_reply(response),
        s.p().resume_wait(asked.answer)
    );
    assert_eq!(reply.unwrap(), ReplyIngress::Unmatched);
    assert!(matches!(timeout.unwrap(), WaitProgress::Expired(_)));
    let run: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(asked.run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(run, "failed");
}

#[tokio::test]
async fn channel_reply_hitl_resume_response_racing_cancellation_cannot_reopen_the_run() {
    let s = Box::pin(Intents::new()).await;
    let asked = Box::pin(managed(&s, 1)).await;
    let id = reply(&s, &asked, By::Author, "to").await;
    let response = ingress(&s, &asked, id).await;
    let head = s
        .p()
        .head(asked.answer.company, asked.run)
        .await
        .unwrap()
        .unwrap();
    let cancel = CancelCommand {
        company_id: asked.answer.company,
        run_id: asked.run,
        actor: s.owner(),
        command_key: IdempotencyKey::parse("question.cancel").unwrap(),
        expected_revision: head.revision,
    };
    let (reply, cancel) = tokio::join!(s.p().deliver_reply(response), s.p().cancel(cancel));
    assert!(matches!(
        reply.unwrap(),
        ReplyIngress::Recorded(_) | ReplyIngress::Unmatched
    ));
    cancel.unwrap();
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Refused
    ));
    let run: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(asked.run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(run, "cancelled");
}

#[tokio::test]
async fn channel_reply_hitl_prompt_unknown_send_stays_unknown_after_a_human_response() {
    let s = Box::pin(Intents::new()).await;
    let asked = Box::pin(managed(&s, 1)).await;
    let delivery = delivery(s.pool(), asked.run, 0).await;
    deliver(s.pool(), delivery, "unknown").await;
    s.p()
        .consume_message_receipts(asked.answer, lease())
        .await
        .unwrap();
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Pending
    ));
    let id = reply(&s, &asked, By::Author, "to").await;
    assert_eq!(
        s.p()
            .deliver_reply(ingress(&s, &asked, id).await)
            .await
            .unwrap(),
        ReplyIngress::Recorded(asked.run)
    );
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Completed(_)
    ));
    let status: String = sqlx::query_scalar("SELECT status FROM message_deliveries WHERE id=$1")
        .bind(delivery)
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(status, "outcome_unknown");
    let cancellation: Option<String> =
        sqlx::query_scalar("SELECT cancellation_reason FROM message_deliveries WHERE id=$1")
            .bind(delivery)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(cancellation.as_deref(), Some("question_closed"));
}

async fn prompt_identity(s: &Intents, asked: &Asked) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('wait',question.wait_id,'deadline',question.deadline,'invocation',question.invocation_id,'message',question.message_id,'continuations',job.continuation_count) FROM workflow_channel_questions AS question JOIN background_tasks AS job ON job.company_id=question.company_id AND job.workflow_execution_id=question.execution_id WHERE question.company_id=$1 AND question.run_id=$2")
        .bind(asked.answer.company.as_uuid()).bind(asked.run.as_uuid()).fetch_one(s.pool()).await.unwrap()
}

#[tokio::test]
async fn channel_reply_hitl_prompt_safe_retry_keeps_message_wait_deadline_and_continuation_budget()
{
    use crate::transport::{FailureDetail, PartResult, ProviderSendOutcome};
    let s = Box::pin(Intents::new()).await;
    let asked = Box::pin(managed(&s, 1)).await;
    let before = prompt_identity(&s, &asked).await;
    let delivery = delivery(s.pool(), asked.run, 0).await;
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
        detail: FailureDetail::parse("Provider explicitly refused before acceptance").unwrap(),
    };
    assert_eq!(
        s.p()
            .complete_part(PartResult {
                fence: &first.lease,
                part_id: part,
                outcome: &retry
            })
            .await
            .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Retryable)
    );
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Pending
    ));
    let second = claim(s.p(), delivery).await;
    assert_eq!(second.parts[0].id, part);
    assert!(
        s.p()
            .begin_part(&second.lease, part)
            .await
            .unwrap()
            .lease_held()
    );
    let accepted = ProviderSendOutcome::Delivered { provider_key: None };
    assert_eq!(
        s.p()
            .complete_part(PartResult {
                fence: &second.lease,
                part_id: part,
                outcome: &accepted
            })
            .await
            .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Delivered)
    );
    assert_eq!(prompt_identity(&s, &asked).await, before);
    let slots: i64 = sqlx::query_scalar("SELECT workflow_message_slots($1,$2)")
        .bind(asked.answer.company.as_uuid())
        .bind(asked.answer.execution.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(slots, 1);
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Pending
    ));
}

#[tokio::test]
async fn channel_reply_hitl_resume_recorded_in_time_survives_delayed_wakeup() {
    let s = Box::pin(Intents::new()).await;
    let pending = Box::pin(pending_with(
        &s,
        1,
        |source| {
            source["steps"]["start"]["with"]["deadline"] = json!({"literal":
            (Utc::now()+chrono::Duration::seconds(5)).to_rfc3339()});
        },
        crate::application::workflow::DeliveryPolicy::Deliver,
    ))
    .await;
    let asked = Box::pin(published(&s, &pending)).await;
    let id = reply(&s, &asked, By::Author, "to").await;
    assert_eq!(
        s.p()
            .deliver_reply(ingress(&s, &asked, id).await)
            .await
            .unwrap(),
        ReplyIngress::Recorded(asked.run)
    );
    let deadline: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT deadline FROM workflow_channel_questions WHERE run_id=$1")
            .bind(asked.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    tokio::time::sleep(
        (deadline - Utc::now()).to_std().unwrap_or_default() + Duration::from_millis(20),
    )
    .await;
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Completed(_)
    ));
}

#[tokio::test]
async fn channel_reply_hitl_prompt_provider_entry_rechecks_deadline_after_competing_parent_lock() {
    let s = Box::pin(Intents::new()).await;
    let pending = Box::pin(pending_with(
        &s,
        1,
        |source| {
            source["steps"]["start"]["with"]["deadline"] = json!({"literal":
            (Utc::now()+chrono::Duration::seconds(5)).to_rfc3339()});
        },
        crate::application::workflow::DeliveryPolicy::Deliver,
    ))
    .await;
    let asked = Box::pin(published(&s, &pending)).await;
    let delivery = delivery(s.pool(), asked.run, 0).await;
    let claimed = claim(s.p(), delivery).await;
    let mut blocker = s.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM message_deliveries WHERE id=$1 FOR UPDATE")
        .bind(delivery)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let p = s.p().clone();
    let fence = claimed.lease;
    let part = claimed.parts[0].id;
    let entering = tokio::spawn(async move { p.begin_part(&fence, part).await });
    crate::adapters::persistence::test_support::wait_until_a_backend_is_blocked(s.pool()).await;
    let deadline: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT deadline FROM workflow_channel_questions WHERE run_id=$1")
            .bind(asked.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    tokio::time::sleep(
        (deadline - Utc::now()).to_std().unwrap_or_default() + Duration::from_millis(20),
    )
    .await;
    blocker.rollback().await.unwrap();
    let error = entering.await.unwrap().unwrap_err();
    assert!(
        format!("{error:?}").contains("workflow_question_provider_entry_guard"),
        "{error:?}"
    );
    let started: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT request_started_at FROM message_delivery_parts WHERE id=$1")
            .bind(part.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert!(
        started.is_none(),
        "the rolled-back entry granted no provider call"
    );
}
