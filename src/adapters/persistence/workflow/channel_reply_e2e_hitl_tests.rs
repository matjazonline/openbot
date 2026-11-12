use super::*;
use crate::application::workflow::decisions::*;

#[tokio::test]
async fn channel_reply_e2e_grandchild_human_wait_returns_to_the_only_response_owner() {
    let d = Box::pin(Deployment::new(vec![])).await;
    let mut leaf = example(3).source;
    leaf["steps"]["start"]["type"] = json!("channel.no_reply");
    let middle = Box::pin(call_child(&d, leaf)).await;
    let root = Box::pin(call_child(&d, middle)).await;
    let mut selected = example(0);
    selected.source = root;
    Box::pin(bind(&d, selected)).await;
    d.email(customer_email(&d, "grandchild", None)).await;
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(d.run_until("grandchild actual prompt", async || {
        waiting(&d, 1).await && delivered(&d, 1).await
    }))
    .await;
    let (_, rfc) = prompt(&d, 0).await;
    assert_eq!(
        d.count(
            "SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND waiting_reason='child_run'"
        )
        .await,
        2
    );
    assert_eq!(
        d.count("SELECT count(*) FROM channel_reply_runs WHERE company_id=$1")
            .await,
        1
    );
    answer(&d, "grandchild-answer", &rfc, "Leaf business result").await;
    Box::pin(
        d.run_until("leaf silence remains local, root replies", async || {
            responses_published(&d, 1).await && delivered(&d, 2).await
        }),
    )
    .await;
    let provider = delivery.stop().await;
    let rows = finals(&d).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].output, json!({"body":"Leaf business result"}));
    let child_decisions:i64=sqlx::query_scalar("SELECT count(*) FROM channel_reply_decisions AS decision JOIN workflow_runs AS run ON run.company_id=decision.company_id AND run.id=decision.run_id WHERE run.company_id=$1 AND run.parent_run_id IS NOT NULL")
        .bind(d.fixture.target.company.as_uuid()).fetch_one(d.p().pool()).await.unwrap();
    assert_eq!(child_decisions, 0);
    assert_eq!(provider.sent.lock().unwrap().len(), 2);
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}

async fn final_review(d: &Deployment) -> DecisionView {
    let id: Uuid = sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1")
        .bind(d.fixture.target.company.as_uuid())
        .fetch_one(d.p().pool())
        .await
        .unwrap();
    d.p()
        .read_decision(
            d.fixture.target.company,
            d.fixture.target.actor,
            DecisionId(id),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn channel_reply_e2e_protected_final_requires_exact_current_approval() {
    for choice in ["approve", "reject"] {
        let mut d = Box::pin(Deployment::new(vec![LlmTurn::text(
            r#"{"body":"Exact saved draft"}"#,
        )]))
        .await;
        let mut selected = example(4);
        model(&mut selected);
        selected.approval_policy = Some(
            crate::application::workflow::channel_reply::ReplyApprovalPolicy::new(
                vec![d.fixture.principal(d.fixture.target.actor.user_id()).await],
                300,
            )
            .unwrap(),
        );
        Box::pin(bind(&d, selected)).await;
        d.email(customer_email(&d, "protected", None)).await;
        let delivery = Delivery::start(&d, ProviderCase::Accept);
        Box::pin(d.run_until("private final response review",async || {
            d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND waiting_reason='decision'").await==1
        })).await;
        let view = final_review(&d).await;
        assert!(view.proposal.to_string().contains("Exact saved draft"));
        d.p()
            .add_decision_comment(AddDecisionComment {
                company: d.fixture.target.company,
                actor: d.fixture.target.actor,
                decision: view.decision,
                command_key: DecisionCommandKey::parse("comment-only").unwrap(),
                body: "Looks good".into(),
            })
            .await
            .unwrap();
        assert_eq!(final_review(&d).await.state, "waiting");
        assert!(finals(&d).await.is_empty());
        assert!(delivery.provider.sent.lock().unwrap().is_empty());
        submit_final_review(&d, view, choice).await;
        // The decision committed while workers were stopped. New worker/wakeup
        // objects must continue the saved execution without regenerating its draft.
        super::recovery::reconstruct(&mut d);
        Box::pin(d.run_until("accepted final review continuation", async || {
            if choice == "approve" {
                responses_published(&d, 1).await && delivered(&d, 1).await
            } else {
                d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND state='failed'")
                    .await
                    == 1
            }
        }))
        .await;
        let provider = delivery.stop().await;
        let sent = provider.messages();
        assert_eq!(sent.len(), usize::from(choice == "approve"));
        if choice == "approve" {
            assert_eq!(sent[0].email.body_text, "Exact saved draft");
            assert_eq!(
                finals(&d).await[0].output,
                json!({"body":"Exact saved draft"})
            );
        } else {
            assert!(finals(&d).await.is_empty());
        }
        assert_eq!(
            d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
                .await,
            1
        );
        assert_eq!(d.llm.finish().await.unwrap(), 1);
    }
}

#[tokio::test]
async fn channel_reply_e2e_two_waits_in_one_thread_route_answers_without_new_runs() {
    let d = Box::pin(Deployment::new(vec![])).await;
    Box::pin(bind(&d, example(3))).await;
    let first = d.email(customer_email(&d, "two-first", None)).await;
    let second = d
        .email(customer_email(&d, "two-second", Some("two-first")))
        .await;
    assert_eq!(
        first.thread.as_ref().unwrap().id,
        second.thread.as_ref().unwrap().id
    );
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(d.run_until("two independent human waits", async || {
        waiting(&d, 2).await && delivered(&d, 2).await
    }))
    .await;
    let (_, first_prompt) = prompt(&d, 0).await;
    let (_, second_prompt) = prompt(&d, 1).await;
    let (a, b) = tokio::join!(
        answer(&d, "concurrent-answer-a", &first_prompt, "First result"),
        answer(&d, "concurrent-answer-b", &second_prompt, "Second result")
    );
    assert_ne!(
        a.inbound_message.as_ref().unwrap().canonical_id,
        b.inbound_message.as_ref().unwrap().canonical_id
    );
    Box::pin(
        d.run_until("both original runs completed and replied", async || {
            responses_published(&d, 2).await && delivered(&d, 4).await
        }),
    )
    .await;
    // Duplicate ingress reuses the canonical input. It cannot admit a new reply run.
    answer(&d, "concurrent-answer-a", &first_prompt, "First result").await;
    Box::pin(d.run_until("duplicate input stable", async || waiting(&d, 0).await)).await;
    let provider = delivery.stop().await;
    assert_eq!(provider.sent.lock().unwrap().len(), 4);
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        2
    );
    let rows = finals(&d).await;
    let mut bodies: Vec<_> = rows
        .iter()
        .map(|row| row.output["body"].as_str().unwrap())
        .collect();
    bodies.sort();
    assert_eq!(bodies, vec!["First result", "Second result"]);
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}

#[tokio::test]
async fn channel_reply_e2e_question_rounds_resume_same_owner_with_frozen_contract() {
    let d = Box::pin(Deployment::new(vec![])).await;
    let selected = two_rounds();
    let original_contract = selected.contract.clone();
    let binding = Box::pin(bind(&d, selected)).await;
    let incoming = d.email(customer_email(&d, "human", None)).await;
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(
        d.run_until("first human wait and prompt delivered", async || {
            waiting(&d, 1).await && delivered(&d, 1).await
        }),
    )
    .await;
    let (first, rfc) = prompt(&d, 0).await;
    let root: Uuid =
        sqlx::query_scalar("SELECT run_id FROM channel_reply_runs WHERE company_id=$1")
            .bind(d.fixture.target.company.as_uuid())
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    let answer1 = answer(&d, "human-answer-1", &rfc, "First answer").await;
    let mut replacement = original_contract;
    replacement.body = crate::application::workflow::channel_reply::ReplyText::Literal {
        literal: "Changed while waiting".into(),
    };
    let next_binding = Box::pin(d.bind(example(0).source, Some(MessageTrigger::Any))).await;
    d.configure_response(next_binding, replacement, None).await;
    Box::pin(d.run_until("second human wait", async || {
        d.count("SELECT count(*) FROM workflow_channel_questions WHERE company_id=$1")
            .await
            == 2
            && waiting(&d, 1).await
            && delivered(&d, 2).await
    }))
    .await;
    let (second, rfc2) = prompt(&d, 1).await;
    assert_ne!(first, second);
    answer(&d, "human-answer-2", &rfc2, "Final human answer").await;
    Box::pin(
        d.run_until("same owner final response delivered", async || {
            responses_published(&d, 1).await && delivered(&d, 3).await
        }),
    )
    .await;
    let provider = delivery.stop().await;
    let final_rows = finals(&d).await;
    assert_eq!(final_rows[0].run, root);
    assert_response_binding(&d, root, binding.binding.as_uuid()).await;
    assert_eq!(
        final_rows[0].source,
        incoming.inbound_message.unwrap().canonical_id.as_uuid()
    );
    assert_eq!(final_rows[0].output, json!({"body":"Final human answer"}));
    assert_question_source(
        &d,
        second,
        answer1.inbound_message.unwrap().canonical_id.as_uuid(),
    )
    .await;
    let sent = provider.messages();
    assert_eq!(sent.len(), 3);
    let final_sent = sent
        .iter()
        .find(|sent| Some(sent.delivery.as_uuid()) == final_rows[0].delivery)
        .unwrap();
    assert_eq!(final_sent.email.body_text, "Final human answer");
    assert_eq!(
        final_sent.email.in_reply_to.as_deref(),
        Some("<human@partner.test>")
    );
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        1
    );
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}

#[tokio::test]
async fn channel_reply_e2e_current_transport_revocation_blocks_final_after_human_wait() {
    let d = Box::pin(Deployment::new(vec![])).await;
    Box::pin(bind(&d, example(3))).await;
    d.email(customer_email(&d, "revoke-waiting", None)).await;
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(
        d.run_until("actual human question before revocation", async || {
            waiting(&d, 1).await && delivered(&d, 1).await
        }),
    )
    .await;
    let (_, rfc) = prompt(&d, 0).await;
    answer(&d, "revoke-answer", &rfc, "Do not send after revocation").await;
    let paused = sqlx::query(
        "UPDATE channel_bindings SET status='paused' WHERE company_id=$1 AND channel_id=$2",
    )
    .bind(d.fixture.target.company.as_uuid())
    .bind(d.channel)
    .execute(d.p().pool())
    .await
    .unwrap();
    assert_eq!(
        paused.rows_affected(),
        1,
        "The actual source transport binding is revoked"
    );
    Box::pin(d.run_until("current revocation stops the original response owner",async || {
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND state IN ('failed','cancelled')").await==1
            || d.count("SELECT count(*) FROM channel_reply_decisions WHERE company_id=$1 AND state='failed'").await==1
    })).await;
    let provider = delivery.stop().await;
    assert_eq!(
        provider.messages().len(),
        1,
        "Only the prompt accepted before revocation exists remotely"
    );
    assert_eq!(
        d.count(
            "SELECT count(*) FROM channel_reply_decisions WHERE company_id=$1 AND state='published'"
        )
        .await,
        0
    );
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        1
    );
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}

async fn submit_final_review(d: &Deployment, view: DecisionView, choice: &str) {
    let command = SubmitDecision {
        company: d.fixture.target.company,
        actor: d.fixture.target.actor,
        decision: view.decision,
        command_key: DecisionCommandKey::parse(choice).unwrap(),
        expected_revision: view.revision,
        expected_artifact: view.artifact_digest.clone(),
        choice: choice.into(),
        feedback: String::new(),
        data: json!({}),
    };
    let mut stale = command.clone();
    stale.expected_artifact = "0".repeat(64);
    assert!(d.p().submit_decision(stale).await.is_err());
    d.p().submit_decision(command.clone()).await.unwrap();
    assert_eq!(
        d.p().submit_decision(command).await.unwrap().disposition,
        DecisionDisposition::Replayed
    );
}

async fn assert_question_source(d: &Deployment, prompt: Uuid, expected: Uuid) {
    let source: Uuid = sqlx::query_scalar(
        "SELECT source_message_id FROM workflow_channel_questions WHERE message_id=$1",
    )
    .bind(prompt)
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(source, expected);
}

fn two_rounds() -> ResponseExample {
    let mut selected = example(3);
    selected.source["steps"]["ask"]["routes"] = json!({"success":"again"});
    selected.source["steps"]["again"] = selected.source["steps"]["ask"].clone();
    selected.source["steps"]["again"]["with"]["source_message"] =
        json!({"ref":"/steps/ask/output/source_message"});
    selected.source["steps"]["again"]["routes"] = json!({"success":"start"});
    selected.source["steps"]["start"]["with"]["value"] =
        json!({"object":{"body":{"ref":"/steps/again/output/message"}}});
    selected
}

async fn assert_response_binding(d: &Deployment, run: Uuid, expected: Uuid) {
    let saved: Uuid =
        sqlx::query_scalar("SELECT binding_id FROM channel_reply_runs WHERE run_id=$1")
            .bind(run)
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    assert_eq!(saved, expected);
}
