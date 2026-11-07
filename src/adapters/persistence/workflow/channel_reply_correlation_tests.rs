use super::super::channel_reply_tests::{select, shape};
use super::*;

#[tokio::test]
async fn channel_reply_binding_matched_human_reply_retains_only_passive_snapshot() {
    let s = Intents::new().await;
    let responder = s.binding(MessageTrigger::Any, None).await;
    select(&s, responder).await;
    let asked = Box::pin(asked(&s, Then::AwaitReply, true)).await;
    let answer = reply(&s, &asked, By::Member, "to").await;
    assert_eq!(shape(&s, answer).await, (1, 0));
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(matched(&s, answer).await, is_matched(&asked));
    assert_eq!(shape(&s, answer).await, (1, 0));
    assert!(s.runs(responder).await.is_empty());
    let ordinary = reply(&s, &asked, By::Member, "to").await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(shape(&s, ordinary).await, (1, 1));
    assert_eq!(s.runs(responder).await.len(), 1);
}

#[tokio::test]
async fn channel_reply_binding_deferred_human_reply_has_no_response_authority() {
    let s = Intents::new().await;
    let responder = s.binding(MessageTrigger::Any, None).await;
    select(&s, responder).await;
    let asked = Box::pin(asked(&s, Then::AwaitReply, false)).await;
    let answer = reply(&s, &asked, By::Member, "to").await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(s.intent_state(answer).await, ("pending".into(), None, 0));
    assert_eq!(shape(&s, answer).await, (1, 0));
    s.make_due(answer).await;
    s.p().park_wait(asked.answer).await.unwrap().unwrap();
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(matched(&s, answer).await, is_matched(&asked));
    assert_eq!(shape(&s, answer).await, (1, 0));
}
