use super::*;

pub(super) async fn ingress(s: &Intents, asked: &Asked, intent: Uuid) -> IngressReply {
    let message: Uuid = sqlx::query_scalar(
        "SELECT message_id FROM workflow_message_intents WHERE company_id=$1 AND id=$2",
    )
    .bind(s.fixture.target.company.as_uuid())
    .bind(intent)
    .fetch_one(s.pool())
    .await
    .unwrap();
    IngressReply {
        company: s.fixture.target.company,
        thread: asked.thread,
        answers: crate::entities::message::CanonicalMessageId::new(asked.question),
        responder: asked.expected,
        in_app: false,
        event: WorkflowEventId(intent),
        payload: MessageTriggerInput::new(
            crate::entities::message::CanonicalMessageId::new(message),
            "Yes, go ahead.",
            "Question",
            "customer@partner.test",
        )
        .to_value(),
        park_grace_until: Utc::now() + chrono::Duration::minutes(5),
    }
}

#[tokio::test]
async fn channel_reply_hitl_resume_before_ack_and_reconstruction_consumes_once() {
    let s = Box::pin(Intents::new()).await;
    let asked = Box::pin(managed(&s, 1)).await;
    let id = reply(&s, &asked, By::Author, "to").await;
    let input = ingress(&s, &asked, id).await;
    assert_eq!(
        s.p().deliver_reply(input.clone()).await.unwrap(),
        ReplyIngress::Recorded(asked.run)
    );
    assert_eq!(
        s.p().deliver_reply(input).await.unwrap(),
        ReplyIngress::Duplicate(asked.run)
    );
    // A newly constructed adapter observes committed ingress without any worker notification.
    let reconstructed = PostgresPersistence::new(s.pool().clone());
    let WaitProgress::Completed(done) = reconstructed.resume_wait(asked.answer).await.unwrap()
    else {
        panic!("human response completes the step");
    };
    assert_eq!(done.output["message"], "Yes, go ahead.");
    assert!(done.successor.is_some());
    let WaitProgress::Completed(replay) = reconstructed.resume_wait(asked.answer).await.unwrap()
    else {
        panic!("saved result replays");
    };
    assert_eq!(done.output, replay.output);
    let delivery = delivery(s.pool(), asked.run, 0).await;
    let cancellation: Option<String> =
        sqlx::query_scalar("SELECT cancellation_reason FROM message_deliveries WHERE id=$1")
            .bind(delivery)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(cancellation.as_deref(), Some("question_closed"));
}

#[tokio::test]
async fn channel_reply_hitl_resume_competing_humans_have_one_winner() {
    let s = Box::pin(Intents::new()).await;
    let asked = Box::pin(managed(&s, 1)).await;
    let a = reply(&s, &asked, By::Author, "to").await;
    let b = reply(&s, &asked, By::Author, "to").await;
    let first = ingress(&s, &asked, a).await;
    let second = ingress(&s, &asked, b).await;
    let (a, b) = tokio::join!(s.p().deliver_reply(first), s.p().deliver_reply(second));
    let results = [a.unwrap(), b.unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, ReplyIngress::Recorded(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, ReplyIngress::Unmatched))
            .count(),
        1
    );
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_wait_events WHERE run_id=$1")
            .bind(asked.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(events, 1);
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Completed(_)
    ));
}

#[tokio::test]
async fn channel_reply_hitl_resume_wrong_respondent_and_prompt_do_not_consume_wait() {
    let s = Box::pin(Intents::new()).await;
    let asked = Box::pin(managed(&s, 1)).await;
    let id = reply(&s, &asked, By::Author, "to").await;
    let valid = ingress(&s, &asked, id).await;
    let mut wrong = valid.clone();
    wrong.responder = Some(Uuid::new_v4());
    assert_eq!(
        s.p().deliver_reply(wrong).await.unwrap(),
        ReplyIngress::Unmatched
    );
    let mut wrong = valid.clone();
    wrong.answers = crate::entities::message::CanonicalMessageId::new(Uuid::new_v4());
    assert_eq!(
        s.p().deliver_reply(wrong).await.unwrap(),
        ReplyIngress::Unmatched
    );
    let mut wrong = valid.clone();
    wrong.thread = Uuid::new_v4();
    assert_eq!(
        s.p().deliver_reply(wrong).await.unwrap(),
        ReplyIngress::Unmatched
    );
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Pending
    ));
    assert_eq!(
        s.p().deliver_reply(valid).await.unwrap(),
        ReplyIngress::Recorded(asked.run)
    );
}

#[tokio::test]
async fn channel_reply_hitl_resume_two_live_runs_in_one_thread_stay_independent() {
    let s = Box::pin(Intents::new()).await;
    let first = Box::pin(managed(&s, 1)).await;
    let second = Box::pin(managed(&s, 1)).await;
    assert_ne!(first.run, second.run);
    assert_ne!(first.question, second.question);
    let id = reply(&s, &first, By::Author, "to").await;
    let response = ingress(&s, &first, id).await;
    assert_eq!(
        s.p().deliver_reply(response.clone()).await.unwrap(),
        ReplyIngress::Recorded(first.run)
    );
    let mut reused = response;
    reused.answers = crate::entities::message::CanonicalMessageId::new(second.question);
    assert_eq!(
        s.p().deliver_reply(reused).await.unwrap(),
        ReplyIngress::Undeliverable("event_id_reused")
    );
    assert!(matches!(
        s.p().resume_wait(second.answer).await.unwrap(),
        WaitProgress::Pending
    ));
    assert!(matches!(
        s.p().resume_wait(first.answer).await.unwrap(),
        WaitProgress::Completed(_)
    ));
}

#[tokio::test]
async fn channel_reply_hitl_resume_repeated_questions_have_distinct_waits_and_no_reply_keeps_prompts()
 {
    use crate::application::workflow::batch::{BatchBudget, WorkflowBatch};
    let s = Box::pin(Intents::new()).await;
    let first = Box::pin(managed(&s, 2)).await;
    let id = reply(&s, &first, By::Author, "to").await;
    assert_eq!(
        s.p()
            .deliver_reply(ingress(&s, &first, id).await)
            .await
            .unwrap(),
        ReplyIngress::Recorded(first.run)
    );
    let WaitProgress::Completed(done) = s.p().resume_wait(first.answer).await.unwrap() else {
        panic!("first reply");
    };
    let scope = done.successor.unwrap();
    let claim = s
        .p()
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let second = Box::pin(published(&s, &PendingQuestion { scope, claim })).await;
    assert_ne!(first.question, second.question);
    assert_ne!(first.answer.execution, second.answer.execution);
    let stale = reply(&s, &first, By::Author, "to").await;
    assert_eq!(
        s.p()
            .deliver_reply(ingress(&s, &first, stale).await)
            .await
            .unwrap(),
        ReplyIngress::Unmatched
    );
    let id = reply(&s, &second, By::Author, "to").await;
    assert_eq!(
        s.p()
            .deliver_reply(ingress(&s, &second, id).await)
            .await
            .unwrap(),
        ReplyIngress::Recorded(second.run)
    );
    let WaitProgress::Completed(done) = s.p().resume_wait(second.answer).await.unwrap() else {
        panic!("second reply");
    };
    let final_step = s
        .p()
        .advance_pure(
            done.successor.unwrap(),
            BatchBudget::new(2, Duration::from_secs(1)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        final_step.response_disposition,
        Some(crate::domain::workflow::ResponseDisposition::Suppressed)
    );
    let c = counts(&s, first.run).await;
    assert_eq!(c.questions, 2);
    assert_eq!(c.destinations, 2);
}

#[tokio::test]
async fn channel_reply_hitl_resume_ingress_consumes_before_ordinary_admission() {
    let s = Box::pin(Intents::new()).await;
    let fallback = s.binding(MessageTrigger::Any, None).await;
    let asked = Box::pin(managed(&s, 1)).await;
    let id = reply(&s, &asked, By::Author, "to").await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(matched(&s, id).await, is_matched(&asked));
    assert!(s.runs(fallback).await.is_empty());
    let second = reply(&s, &asked, By::Author, "to").await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(s.intent_state(second).await.1.as_deref(), Some("admitted"));
    assert_eq!(s.runs(fallback).await.len(), 1);
}
