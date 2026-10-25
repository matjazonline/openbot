//! p08 D1.5 against PostgreSQL: a reply resumes a `message.reply` wait only while that wait is
//! open, only from the responder the question addressed, and only once; every other reply is
//! ordinary ingress and is offered to the channel's bindings.
use super::*;
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::completion::WorkflowCompletion;
use crate::application::workflow::lease::FencedWorkflowResult;
use crate::application::workflow::polling::WorkflowStepKind;
use crate::application::workflow::triggers::MessageTriggerInput;
use crate::application::workflow::waits::{
    IngressReply, ReplyIngress, WaitProgress, WorkflowEventId, WorkflowWaits,
};
use chrono::Utc;

/// What the asking run waits for after its question.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Then {
    /// A `message.reply` wait on the question.
    AwaitReply,
    /// Some other event, so the run stays live without awaiting the question.
    AwaitOther,
}

/// Whom the asking run's question goes to, and so whom it expects an answer from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// A `user` send: the member answers, from the thread the send opened.
    Member,
    /// A `message.reply` to a customer's message: its author answers, in the customer thread.
    Customer,
    /// A `channel` post: a member answers in-app.
    Channel,
}

fn question_source(workflow: WorkflowId, start: Value, then: Then) -> Value {
    let wait = match then {
        Then::AwaitReply => json!({
            "event":{"literal":"message.reply"},
            "correlation":{"ref":"/steps/start/output/messages/0/message_id"},
            "deadline":{"literal":(Utc::now() + chrono::Duration::minutes(30)).to_rfc3339()},
            "payload_schema":{"literal":MessageTriggerInput::schema()}}),
        Then::AwaitOther => json!({
            "event":{"literal":"review.completed"},
            "correlation":{"literal":"request-1"},
            "deadline":{"literal":(Utc::now() + chrono::Duration::minutes(30)).to_rfc3339()},
            "payload_schema":{"literal":{"type":"object"}}}),
    };
    let mut source: Value =
        serde_json::from_str(&registry::example("message.send").unwrap().source).unwrap();
    source["workflow_id"] = json!(workflow.to_string());
    source["steps"]["start"] = start;
    source["steps"]["start"]["routes"] = json!({"success":"answer"});
    source["steps"]["answer"] =
        json!({"type":"wait.event","with":wait,"routes":{"success":"$end"}});
    source
}

/// A run that asked a question and then waits.
struct Asked {
    run: RunId,
    /// The wait step's activation.
    answer: ActivationRequest,
    /// The outbound question as stored, and the thread and channel it was published in.
    question: Uuid,
    thread: Uuid,
    channel: Uuid,
    /// The principal the question expects an answer from: the member of a `user` send, the
    /// author of the message a `reply` answered; none for a `channel` post.
    expected: Option<Uuid>,
}

async fn asked(s: &Intents, then: Then, park: bool) -> Asked {
    asked_via(s, Ask::Member, then, park).await
}

/// Publish, bind and admit a run asking through `ask`, and drive its question to completion.
async fn asked_via(s: &Intents, ask: Ask, then: Then, park: bool) -> Asked {
    let p = s.p();
    let workflow = WorkflowId::new(Uuid::new_v4());
    let (start, association) = match ask {
        Ask::Member => {
            let user = member(&s.fixture, "member").await;
            (
                json!({"type":"message.send","with":{
                    "content":{"literal":{"subject":"Question","body":"Shall we proceed?"}},
                    "destinations":{"literal":[{"kind":"user","id":user.to_string()}]}}}),
                RelatedAssociation::Channel(RelatedChannelId::new(s.channel)),
            )
        }
        Ask::Customer => {
            let source = s.message().await;
            (
                json!({"type":"message.reply","with":{
                    "content":{"literal":{"body":"Shall we proceed?"}},
                    "source_message":{"literal":source.as_uuid().to_string()}}}),
                RelatedAssociation::Thread {
                    channel_id: RelatedChannelId::new(s.channel),
                    thread_id: RelatedThreadId::new(s.thread),
                },
            )
        }
        Ask::Channel => {
            let notices = new_channel(
                &s.fixture,
                &format!("notices-{}", workflow.as_uuid().simple()),
            )
            .await;
            (
                json!({"type":"message.send","with":{
                    "content":{"literal":{"subject":"Question","body":"Shall we proceed?"}},
                    "destinations":{"literal":[{"kind":"channel","id":notices.to_string()}]}}}),
                RelatedAssociation::Channel(RelatedChannelId::new(s.channel)),
            )
        }
    };
    let version = VersionId::new(Uuid::new_v4());
    let definitions = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        message::MessagePublicationDirectory {
            persistence: p.clone(),
        },
    );
    s.fixture
        .publish_source(
            &definitions,
            workflow,
            question_source(workflow, start, then).to_string(),
            version,
        )
        .await
        .unwrap();
    let target = Box::pin(bound(&s.fixture, version)).await;
    // Each asking run under its own key: one company may hold several.
    let key = format!("question-{}", workflow.as_uuid().simple());
    let scope = Box::pin(admitted_as(&s.fixture, target, association, &key)).await;
    let claim = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let kind = if ask == Ask::Customer {
        "message.reply"
    } else {
        "message.send"
    };
    let outcome = runtime(p)
        .execute(&WorkflowStepKind(kind.into()), &claim)
        .await;
    let answer = match outcome {
        // A post is published in-app: its receipt is local, and the step completes at once.
        WorkflowHandlerOutcome::Completed(result) => p
            .complete_io(FencedWorkflowResult {
                fence: claim.fence,
                output: result.unwrap(),
                policy: lease(),
            })
            .await
            .unwrap()
            .expect("the post completes its step"),
        WorkflowHandlerOutcome::Suspended(_) => {
            deliver(
                p.pool(),
                delivery(p.pool(), scope.run, 0).await,
                "delivered",
            )
            .await;
            assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
            let WaitProgress::Completed(sent) = p.resume_wait(scope).await.unwrap() else {
                panic!("a delivered question completes its step");
            };
            *sent
        }
    }
    .successor
    .expect("the run goes on to wait");
    if park {
        p.park_wait(answer).await.unwrap().unwrap();
    }
    let (question, thread, channel, kind, principal, answered): (
        Uuid,
        Uuid,
        Uuid,
        String,
        Option<Uuid>,
        Uuid,
    ) = sqlx::query_as(
        "SELECT target.message_id, target.thread_id, thread.channel_id, target.kind, \
           target.principal_id, target.destination_id \
         FROM workflow_message_destinations AS target \
         JOIN threads AS thread ON thread.id = target.thread_id WHERE target.run_id = $1",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let expected = match kind.as_str() {
        "user" => principal,
        "reply" => sqlx::query_scalar("SELECT author_principal_id FROM messages WHERE id = $1")
            .bind(answered)
            .fetch_one(p.pool())
            .await
            .unwrap(),
        _ => None,
    };
    Asked {
        run: scope.run,
        answer,
        question,
        thread,
        channel,
        expected,
    }
}

/// Who answers the question.
#[derive(Clone, Copy)]
enum By {
    /// The responder the question expects.
    Member,
    /// Whoever wrote the stored reply, here the customer.
    Author,
    /// Someone else entirely.
    Principal(Uuid),
}

/// A reply received by mail into the question's thread, answering the question, as its intent.
async fn reply(s: &Intents, asked: &Asked, by: By, role: &str) -> Uuid {
    reply_from(s, asked, by, role, "external").await
}

/// A reply into the question's thread that reached ingress by `origin`, as its intent.
async fn reply_from(s: &Intents, asked: &Asked, by: By, role: &str, origin: &str) -> Uuid {
    let n = s.messages.fetch_add(1, Ordering::SeqCst);
    let message = s
        .p()
        .create_message(&inbound(
            asked.thread,
            &format!("<reply-{n}@partner.test>"),
            &[],
            "Yes, go ahead.",
        ))
        .await
        .unwrap()
        .canonical_id;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workflow_message_intents (company_id, id, message_id, channel_id, thread_id, \
           role, mentioned, admissible, origin, reply_to_message_id, responder_principal_id, \
           hop_count, trace_channels, delivery_policy, correlation_id) \
         SELECT $1, $2, message.id, $3, $4, $5, false, true, $9, $6, \
           COALESCE($7, message.author_principal_id), 0, '{}', 'deliver', $2 \
         FROM messages AS message WHERE message.company_id = $1 AND message.id = $8",
    )
    .bind(s.fixture.target.company.as_uuid())
    .bind(id)
    .bind(asked.channel)
    .bind(asked.thread)
    .bind(role)
    .bind(asked.question)
    .bind(match by {
        By::Member => Some(asked.expected.expect("this question expects a responder")),
        By::Author => None,
        By::Principal(principal) => Some(principal),
    })
    .bind(message.as_uuid())
    .bind(origin)
    .execute(s.pool())
    .await
    .unwrap();
    id
}

/// A member's principal: a party to the company who is not whom a question addressed.
async fn principal_of(s: &Intents, user: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM principals WHERE user_id = $1")
        .bind(user)
        .fetch_one(s.pool())
        .await
        .unwrap()
}

/// `(state, outcome, matched run)` of an intent.
async fn matched(s: &Intents, intent: Uuid) -> (String, Option<String>, Option<Uuid>) {
    sqlx::query_as(
        "SELECT state, outcome, matched_run_id FROM workflow_message_intents WHERE id = $1",
    )
    .bind(intent)
    .fetch_one(s.pool())
    .await
    .unwrap()
}

/// `(state, consumed event)` of the run's reply wait.
async fn wait(s: &Intents, asked: &Asked) -> (String, Option<Uuid>) {
    sqlx::query_as(
        "SELECT state, consumed_event_id FROM workflow_waits WHERE run_id = $1 AND reason = 'event'",
    )
    .bind(asked.run.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap()
}

fn is_matched(asked: &Asked) -> (String, Option<String>, Option<Uuid>) {
    (
        "done".to_string(),
        Some("matched".to_string()),
        Some(asked.run.as_uuid()),
    )
}

/// D1.5.3/.5: the addressed member's reply resumes the open wait and admits nothing; a second
/// answer to the same question, while the run still waits, is ordinary ingress.
#[tokio::test]
async fn message_reply_resumes_its_wait_once_and_a_second_reply_is_ordinary() {
    let s = Intents::new().await;
    let fallback = s.binding(MessageTrigger::Any, None).await;
    let asked = Box::pin(asked(&s, Then::AwaitReply, true)).await;
    let first = reply(&s, &asked, By::Member, "to").await;

    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(matched(&s, first).await, is_matched(&asked));
    assert!(
        s.runs(fallback).await.is_empty(),
        "a matched reply admits nothing"
    );

    let second = reply(&s, &asked, By::Member, "to").await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(s.intent_state(second).await.1.as_deref(), Some("admitted"));
    assert_eq!(s.runs(fallback).await.len(), 1);

    let WaitProgress::Completed(_) = s.p().resume_wait(asked.answer).await.unwrap() else {
        panic!("the recorded reply completes the wait");
    };
    assert_eq!(
        wait(&s, &asked).await,
        ("completed".to_string(), Some(first))
    );
}

/// D1.5.3: a reply to a send no wait awaits is ordinary ingress even while its run is live, and
/// `mentioned_or_reply` admits it as a reply to a workflow message although it is only a copy.
#[tokio::test]
async fn message_reply_to_an_unawaited_send_is_ordinary_ingress() {
    let s = Intents::new().await;
    let replies = s.binding(MessageTrigger::MentionedOrReply, None).await;
    let direct = s.binding(MessageTrigger::Direct, None).await;
    let asked = Box::pin(asked(&s, Then::AwaitOther, true)).await;
    let intent = reply(&s, &asked, By::Member, "cc").await;

    s.drain(&s.admissions(admission(s.p()))).await;

    assert_eq!(s.admitted(intent).await, ids(&[replies]));
    assert!(s.runs(direct).await.is_empty());
}

/// D1.5.3/.4: an answer from anyone but the addressed member, and the member's answer after
/// the wait expired, are ordinary ingress; neither touches the run.
#[tokio::test]
async fn message_reply_from_the_wrong_responder_or_after_expiry_is_ordinary_ingress() {
    let s = Intents::new().await;
    let fallback = s.binding(MessageTrigger::Any, None).await;
    let asked = Box::pin(asked(&s, Then::AwaitReply, true)).await;

    let stranger = reply(&s, &asked, By::Author, "to").await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(
        s.intent_state(stranger).await.1.as_deref(),
        Some("admitted")
    );
    assert_eq!(wait(&s, &asked).await, ("waiting".to_string(), None));

    sqlx::query(
        "UPDATE workflow_waits SET deadline = clock_timestamp() WHERE run_id = $1 AND reason = 'event'",
    )
    .bind(asked.run.as_uuid())
    .execute(s.pool())
    .await
    .unwrap();
    s.p().resume_wait(asked.answer).await.unwrap();
    assert_eq!(wait(&s, &asked).await.0, "expired");
    let late = reply(&s, &asked, By::Member, "to").await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(s.intent_state(late).await.1.as_deref(), Some("admitted"));
    assert_eq!(s.runs(fallback).await.len(), 2);
}

/// D1.5.3 event-before-park: a reply that arrives before its wait parks is deferred without
/// spending attempts, however often it is rechecked, and resumes the wait once it parks; past
/// the park grace it falls through to ordinary admission instead.
#[tokio::test]
async fn message_reply_before_its_wait_parks_is_deferred_until_it_does() {
    let s = Intents::new().await;
    let fallback = s.binding(MessageTrigger::Any, None).await;
    let asked = Box::pin(asked(&s, Then::AwaitReply, false)).await;
    let early = reply(&s, &asked, By::Member, "to").await;

    for _ in 0..6 {
        s.drain(&s.admissions(admission(s.p()))).await;
        assert_eq!(
            s.intent_state(early).await,
            ("pending".to_string(), None, 0)
        );
        s.make_due(early).await;
    }
    s.p().park_wait(asked.answer).await.unwrap().unwrap();
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(matched(&s, early).await, is_matched(&asked));

    let unparked = Box::pin(self::asked(&s, Then::AwaitReply, false)).await;
    let late = reply(&s, &unparked, By::Member, "to").await;
    // Accepted six minutes ago, while the binding was already active.
    sqlx::query(
        "UPDATE workflow_bindings SET activated_at = activated_at - interval '1 hour' WHERE id = $1",
    )
    .bind(fallback.as_uuid())
    .execute(s.pool())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE workflow_message_intents SET created_at = clock_timestamp() - interval '6 minutes' \
         WHERE id = $1",
    )
    .bind(late)
    .execute(s.pool())
    .await
    .unwrap();
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(s.intent_state(late).await.1.as_deref(), Some("admitted"));
    assert_eq!(s.runs(fallback).await.len(), 1);
}

fn ingress_reply(intent: &ClaimedIntent, payload: Value) -> IngressReply {
    IngressReply {
        company: intent.claim.company,
        thread: intent.thread,
        answers: intent.reply_to.expect("a reply"),
        responder: intent.responder,
        in_app: intent.in_app,
        event: WorkflowEventId(intent.claim.intent),
        payload,
        park_grace_until: intent.accepted_at + chrono::Duration::minutes(5),
    }
}

/// Failure mode "crash after the reply event, before the outcome": reprocessing the intent finds
/// its own event and matches once, admitting nothing.
#[tokio::test]
async fn message_reply_crash_replay_matches_once() {
    let s = Intents::new().await;
    let fallback = s.binding(MessageTrigger::Any, None).await;
    let asked = Box::pin(asked(&s, Then::AwaitReply, true)).await;
    let intent = reply(&s, &asked, By::Member, "to").await;
    let claimed = s
        .p()
        .claim_intents(MessageIntentWorkerId(Uuid::new_v4()), MESSAGE_INTENT_PAGE)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let payload = s.p().trigger_input(&claimed).await.unwrap().to_value();
    assert_eq!(
        s.p()
            .deliver_reply(ingress_reply(&claimed, payload))
            .await
            .unwrap(),
        ReplyIngress::Recorded(asked.run)
    );

    s.admissions(admission(s.p()))
        .process(claimed)
        .await
        .unwrap();

    assert_eq!(matched(&s, intent).await, is_matched(&asked));
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_wait_events WHERE run_id = $1")
            .bind(asked.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(events, 1);
    assert!(s.runs(fallback).await.is_empty());
}

/// D1.5.6 (F016.2): a reply recorded before the wait's deadline is consumed by a resume that
/// runs after it; the wait completes rather than expiring.
#[tokio::test]
async fn message_reply_recorded_before_the_deadline_is_consumed_after_it() {
    let s = Intents::new().await;
    let asked = Box::pin(asked(&s, Then::AwaitReply, true)).await;
    let intent = reply(&s, &asked, By::Member, "to").await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(matched(&s, intent).await, is_matched(&asked));
    // The deadline falls exactly when the reply was recorded, which is now in the past.
    sqlx::query(
        "UPDATE workflow_waits AS wait SET deadline = event.created_at FROM workflow_wait_events AS event \
         WHERE wait.run_id = $1 AND wait.reason = 'event' AND event.company_id = wait.company_id \
           AND event.run_id = wait.run_id",
    )
    .bind(asked.run.as_uuid())
    .execute(s.pool())
    .await
    .unwrap();

    let WaitProgress::Completed(_) = s.p().resume_wait(asked.answer).await.unwrap() else {
        panic!("the reply recorded in time completes the wait");
    };
    assert_eq!(
        wait(&s, &asked).await,
        ("completed".to_string(), Some(intent))
    );
}

/// D1.5.5 (F019): a reply and the wait's expiry racing at its deadline never deadlock or time
/// out, and agree: a recorded reply is the one the wait consumes; otherwise the wait expired.
#[tokio::test]
async fn message_reply_racing_wait_expiry_agrees_without_deadlock() {
    let s = Intents::new().await;
    for lead in [0u64, 40, 80] {
        let asked = Box::pin(asked(&s, Then::AwaitReply, true)).await;
        let intent = reply(&s, &asked, By::Member, "to").await;
        let claimed = s
            .p()
            .claim_intents(MessageIntentWorkerId(Uuid::new_v4()), MESSAGE_INTENT_PAGE)
            .await
            .unwrap()
            .pop()
            .unwrap();
        let payload = s.p().trigger_input(&claimed).await.unwrap().to_value();
        sqlx::query(
            "UPDATE workflow_waits SET deadline = clock_timestamp() + interval '150 milliseconds' \
             WHERE run_id = $1 AND reason = 'event'",
        )
        .bind(asked.run.as_uuid())
        .execute(s.pool())
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(150 - lead)).await;

        let (delivered, resumed) = tokio::join!(
            s.p().deliver_reply(ingress_reply(&claimed, payload)),
            s.p().resume_wait(asked.answer)
        );
        let delivered = delivered.expect("the reply neither deadlocks nor times out");
        resumed.expect("the expiry neither deadlocks nor times out");
        // Settle whichever side has not finished: past the deadline the wait completes with a
        // reply recorded in time, or expires without one.
        tokio::time::sleep(Duration::from_millis(200)).await;
        s.p().resume_wait(asked.answer).await.unwrap();
        let settled = wait(&s, &asked).await;
        match delivered {
            ReplyIngress::Recorded(run) => {
                assert_eq!(run, asked.run);
                assert_eq!(
                    settled,
                    ("completed".to_string(), Some(intent)),
                    "lead {lead}"
                );
            }
            ReplyIngress::Unmatched => {
                assert_eq!(settled, ("expired".to_string(), None), "lead {lead}");
            }
            other => panic!("unexpected {other:?} at lead {lead}"),
        }
        // Give the intent back so the next iteration's claim takes only its own.
        sqlx::query("DELETE FROM workflow_message_intents WHERE id = $1")
            .bind(intent)
            .execute(s.pool())
            .await
            .unwrap();
    }
}

/// D1.5.4 (WF-P08-F021): an awaited `message.reply` to a customer is answered only by that
/// message's author; another party to the thread is ordinary ingress.
#[tokio::test]
async fn message_reply_to_a_workflow_reply_matches_only_the_customer_it_answered() {
    let s = Intents::new().await;
    let fallback = s.binding(MessageTrigger::Any, None).await;
    let asked = Box::pin(asked_via(&s, Ask::Customer, Then::AwaitReply, true)).await;
    let colleague = principal_of(&s, member(&s.fixture, "member").await).await;

    let other = reply(&s, &asked, By::Principal(colleague), "to").await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(s.intent_state(other).await.1.as_deref(), Some("admitted"));
    assert_eq!(wait(&s, &asked).await, ("waiting".to_string(), None));

    let customer = reply(&s, &asked, By::Author, "to").await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(matched(&s, customer).await, is_matched(&asked));
    assert_eq!(
        s.runs(fallback).await.len(),
        1,
        "the customer's answer started nothing"
    );
}

/// D1.5.4 (WF-P08-F021): an awaited `channel` post is answered only by a member posting in-app;
/// a reply that arrived by mail or by internal relay is ordinary ingress.
#[tokio::test]
async fn message_reply_to_a_channel_post_matches_only_an_in_app_answer() {
    let s = Intents::new().await;
    let asked = Box::pin(asked_via(&s, Ask::Channel, Then::AwaitReply, true)).await;
    let colleague = principal_of(&s, member(&s.fixture, "member").await).await;

    for origin in ["external", "internal"] {
        let intent = reply_from(&s, &asked, By::Principal(colleague), "to", origin).await;
        s.drain(&s.admissions(admission(s.p()))).await;
        assert_eq!(
            matched(&s, intent).await,
            ("done".to_string(), Some("none".to_string()), None),
            "{origin}"
        );
    }
    assert_eq!(wait(&s, &asked).await, ("waiting".to_string(), None));
    let in_app = reply_from(&s, &asked, By::Principal(colleague), "to", "application").await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(matched(&s, in_app).await, is_matched(&asked));
}

/// WF-P08-F024: once a reply fell through to admission it is ordinary ingress for good. A sibling
/// binding's transient failure retries the intent after the wait has parked; the retry admits
/// that binding and never answers the wait with a message that already started a run.
#[tokio::test]
async fn message_reply_that_fell_through_is_never_matched_on_a_retry() {
    let s = Intents::new().await;
    let steady = s.binding(MessageTrigger::Any, None).await;
    let flaky = s.binding(MessageTrigger::Any, None).await;
    let asked = Box::pin(asked(&s, Then::AwaitReply, false)).await;
    let intent = reply(&s, &asked, By::Member, "to").await;
    // Accepted past the park grace, while both bindings were already active.
    sqlx::query(
        "UPDATE workflow_bindings SET activated_at = activated_at - interval '1 hour' \
         WHERE id = ANY($1)",
    )
    .bind(vec![steady.as_uuid(), flaky.as_uuid()])
    .execute(s.pool())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE workflow_message_intents SET created_at = clock_timestamp() - interval '6 minutes' \
         WHERE id = $1",
    )
    .bind(intent)
    .execute(s.pool())
    .await
    .unwrap();
    let failing = Scripted::new(
        s.p(),
        Box::new(move |_, binding| {
            Box::pin(async move {
                if binding == flaky {
                    Script::Fail
                } else {
                    Script::Delegate
                }
            })
        }),
    );

    s.drain(&s.admissions(failing)).await;
    assert_eq!(s.admitted(intent).await, ids(&[steady]));
    assert_eq!(s.intent_state(intent).await.0, "pending");
    s.p().park_wait(asked.answer).await.unwrap().unwrap();
    s.make_due(intent).await;
    s.drain(&s.admissions(admission(s.p()))).await;

    assert_eq!(
        matched(&s, intent).await,
        ("done".to_string(), Some("admitted".to_string()), None)
    );
    assert_eq!(s.admitted(intent).await, ids(&[steady, flaky]));
    assert_eq!(wait(&s, &asked).await, ("waiting".to_string(), None));
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_wait_events WHERE run_id = $1")
            .bind(asked.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(events, 0, "the wait was never answered");
}

/// WF-P08-F023: a reply whose delivery keeps failing ends its intent on the last attempt, logged
/// and counted apart from binding refusals.
#[tokio::test]
async fn message_reply_delivery_failing_on_the_last_attempt_fails_the_intent_observably() {
    let s = Intents::new().await;
    let asked = Box::pin(asked(&s, Then::AwaitReply, true)).await;
    let intent = reply(&s, &asked, By::Member, "to").await;
    sqlx::query("UPDATE workflow_message_intents SET attempts = $2 - 1 WHERE id = $1")
        .bind(intent)
        .bind(MAX_INTENT_ATTEMPTS)
        .execute(s.pool())
        .await
        .unwrap();
    // Hold the run row, so the reply's delivery times out on its lock.
    let mut blocker = s.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id = $1 FOR UPDATE")
        .bind(asked.run.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let monitor = Arc::new(InMemoryMonitor::new());

    s.drain(
        &s.admissions(admission(s.p()))
            .with_monitoring(monitor.clone()),
    )
    .await;
    blocker.rollback().await.unwrap();

    assert_eq!(
        s.intent_state(intent).await,
        ("failed".to_string(), None, MAX_INTENT_ATTEMPTS)
    );
    assert_eq!(
        monitor.get_stats_json()["custom_counters"]["workflow_message_intents_failed_total{cause=transient}"],
        1
    );
    assert_eq!(wait(&s, &asked).await, ("waiting".to_string(), None));
}

/// Hold the run's event-producer lock, as a concurrent producer would.
async fn hold_producer_lock(
    s: &Intents,
    asked: &Asked,
) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut holder = s.pool().begin().await.unwrap();
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('workflow_wait_events:' || $1::text || ':' || $2::text, 0))",
    )
    .bind(s.fixture.target.company.as_uuid())
    .bind(asked.run.as_uuid())
    .execute(&mut *holder)
    .await
    .unwrap();
    holder
}

async fn reply_settled(s: &Intents, intent: Uuid) -> bool {
    sqlx::query_scalar("SELECT reply_settled FROM workflow_message_intents WHERE id = $1")
        .bind(intent)
        .fetch_one(s.pool())
        .await
        .unwrap()
}

/// WF-P08-F028: a reply that runs out of time is retried, never settled as ordinary ingress.
/// Exceeding the delivery budget while no single statement times out is a `DatabaseTimeout`, and
/// the worker defers on a timeout and matches the reply once it gets through.
#[tokio::test]
async fn message_reply_delivery_out_of_time_is_retried_not_settled() {
    let s = Intents::new().await;
    let fallback = s.binding(MessageTrigger::Any, None).await;
    let asked = Box::pin(asked(&s, Then::AwaitReply, true)).await;
    let intent = reply(&s, &asked, By::Member, "to").await;
    let claimed = s
        .p()
        .claim_intents(MessageIntentWorkerId(Uuid::new_v4()), MESSAGE_INTENT_PAGE)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let payload = s.p().trigger_input(&claimed).await.unwrap().to_value();

    // The lock is held past a 200 ms budget but well inside the 1 s lock timeout.
    let holder = hold_producer_lock(&s, &asked).await;
    let out_of_time = crate::adapters::persistence::workflow::reply_waits::deliver_within(
        s.p(),
        ingress_reply(&claimed, payload),
        Duration::from_millis(200),
    )
    .await;
    holder.rollback().await.unwrap();
    assert!(
        matches!(out_of_time, Err(AppError::DatabaseTimeout(_))),
        "{out_of_time:?}"
    );

    // Through the worker, a lock held past the statement's lock timeout defers the intent.
    let holder = hold_producer_lock(&s, &asked).await;
    s.admissions(admission(s.p()))
        .process(claimed)
        .await
        .unwrap();
    holder.rollback().await.unwrap();
    assert_eq!(
        s.intent_state(intent).await,
        ("pending".to_string(), None, 1)
    );
    assert!(
        !reply_settled(&s, intent).await,
        "a timeout is not a verdict"
    );

    s.make_due(intent).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(matched(&s, intent).await, is_matched(&asked));
    assert!(s.runs(fallback).await.is_empty());
}

/// D1.5.5: a reply its run can never take (here its event id already names other content) is
/// logged and becomes ordinary ingress, settled for good.
#[tokio::test]
async fn message_reply_the_run_can_never_take_is_ordinary_ingress() {
    let s = Intents::new().await;
    let fallback = s.binding(MessageTrigger::Any, None).await;
    let asked = Box::pin(asked(&s, Then::AwaitReply, true)).await;
    let intent = reply(&s, &asked, By::Member, "to").await;
    // The intent's event id is already taken by a different answer.
    sqlx::query(
        "INSERT INTO workflow_wait_events (company_id, run_id, id, event_name, correlation, payload) \
         VALUES ($1, $2, $3, 'message.reply', $4, '{}')",
    )
    .bind(s.fixture.target.company.as_uuid())
    .bind(asked.run.as_uuid())
    .bind(intent)
    .bind(asked.question.to_string())
    .execute(s.pool())
    .await
    .unwrap();

    s.drain(&s.admissions(admission(s.p()))).await;

    assert_eq!(s.intent_state(intent).await.1.as_deref(), Some("admitted"));
    assert!(reply_settled(&s, intent).await);
    assert_eq!(s.runs(fallback).await.len(), 1);
}
