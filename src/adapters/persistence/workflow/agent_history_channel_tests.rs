use super::*;

#[path = "agent_history_channel_release_tests.rs"]
mod release;

#[path = "agent_history_thread_continuation_tests.rs"]
mod continuation;

async fn exchanges(d: &Deployment) -> Vec<Value> {
    sqlx::query_scalar("SELECT jsonb_build_object('id',exchange.id,'run',exchange.run_id,'state',exchange.state,'output',exchange.output,'level',exchange.access_level,'preparation',exchange.frozen_preparation,'conversation',exchange.conversation_id,'scope',conversation.scope_kind,'thread',conversation.thread_id) FROM agent_history_exchanges AS exchange JOIN agent_history_conversations AS conversation ON conversation.company_id=exchange.company_id AND conversation.id=exchange.conversation_id WHERE exchange.company_id=$1 ORDER BY exchange.position,exchange.id")
        .bind(d.fixture.target.company.as_uuid()).fetch_all(d.p().pool()).await.unwrap()
}

fn prepend_agent(selected: &mut ResponseExample) {
    let mut agent = example(0);
    model(&mut agent);
    let mut step = agent.source["steps"]["start"].clone();
    step["with"]["output_schema"] = json!({"literal":selected.source["output_schema"]});
    step["routes"]["success"] = selected.source["entry"].clone();
    selected.source["entry"] = json!("history_agent");
    selected.source["steps"]["history_agent"] = step;
}

#[tokio::test]
async fn agent_history_channel_projection_is_not_the_saved_output_or_next_seed() {
    let mut d = Box::pin(Deployment::new(vec![
        LlmTurn::text(r#"{"body":"Original accepted agent value"}"#),
        LlmTurn::text(r#"{"body":"Second accepted agent value"}"#),
    ]))
    .await;
    let mut selected = example(0);
    model(&mut selected);
    selected.contract.body = crate::application::workflow::channel_reply::ReplyText::Literal {
        literal: "Projected publication".into(),
    };
    Box::pin(bind(&d, selected)).await;
    d.email(customer_email(&d, "history-project-first", None))
        .await;
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(d.run_until("first projected reply", async || {
        responses_published(&d, 1).await && delivered(&d, 1).await
    }))
    .await;
    let first = exchanges(&d).await;
    assert_eq!(first.len(), 1);
    assert_eq!(
        first[0]["output"],
        json!({"body":"Original accepted agent value"})
    );
    let sent = delivery.provider.messages()[0].email.message_id.clone();
    answer(&d, "history-project-second", &sent, "Second customer task").await;
    Box::pin(d.run_until("second projected reply", async || {
        responses_published(&d, 2).await && delivered(&d, 2).await
    }))
    .await;
    let provider = delivery.stop().await;
    let rows = exchanges(&d).await;
    assert_eq!(rows.len(), 2);
    let original = rows.iter().find(|row| row["id"] == first[0]["id"]).unwrap();
    assert_eq!(*original, first[0]);
    let next = rows.iter().find(|row| row["id"] != first[0]["id"]).unwrap();
    assert_eq!(
        original["conversation"], next["conversation"],
        "actual scope/thread facts: {rows:?}"
    );
    let seed = &next["preparation"]["history"]["seed"];
    assert_eq!(seed.as_array().unwrap().len(), 1);
    assert_eq!(
        seed[0]["output"],
        json!({"body":"Original accepted agent value"})
    );
    let sent = provider.messages();
    assert_eq!(sent.len(), 2);
    assert!(
        sent.iter()
            .all(|s| s.email.body_text == "Projected publication")
    );
    let requests = d.llm.observed();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]["messages"][2]["content"]
            .to_string()
            .contains("Original accepted agent value")
    );
    assert!(
        !requests[1]["messages"][2]["content"]
            .to_string()
            .contains("Projected publication")
    );
    assert_eq!(d.llm.finish().await.unwrap(), 2);
}

#[tokio::test]
async fn agent_history_channel_no_reply_and_explicit_send_retain_agent_output_independently() {
    for index in [1, 2] {
        let d = Box::pin(Deployment::new(vec![LlmTurn::text(
            r#"{"body":"Accepted agent output"}"#,
        )]))
        .await;
        let mut selected = example(index);
        prepend_agent(&mut selected);
        Box::pin(bind(&d, selected)).await;
        let mut incoming = customer_email(&d, "history-suppressed", None);
        incoming.text = Some("ignore".into());
        d.email(incoming).await;
        let delivery = Delivery::start(&d, ProviderCase::Accept);
        Box::pin(
            d.run_until("suppressed result retains agent output", async || {
                let rows = finals(&d).await;
                rows.len() == 1
                    && rows[0].state == "suppressed"
                    && delivered(&d, if index == 1 { 0 } else { 1 }).await
            }),
        )
        .await;
        let provider = delivery.stop().await;
        let rows = exchanges(&d).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["state"], "completed");
        assert_eq!(rows[0]["output"], json!({"body":"Accepted agent output"}));
        let final_rows = finals(&d).await;
        assert_ne!(final_rows[0].output, rows[0]["output"]);
        assert!(final_rows[0].message.is_none() && final_rows[0].delivery.is_none());
        let sent = provider.messages();
        assert_eq!(sent.len(), if index == 1 { 0 } else { 1 });
        if index == 2 {
            assert_eq!(sent[0].email.body_text, "Sent explicitly");
        }
        assert_eq!(d.llm.finish().await.unwrap(), 1);
    }
}

#[tokio::test]
async fn agent_history_channel_delivery_retry_keeps_one_exchange_output_and_model_call() {
    let d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"One accepted model effect"}"#,
    )]))
    .await;
    let mut selected = example(0);
    model(&mut selected);
    Box::pin(bind(&d, selected)).await;
    d.email(customer_email(&d, "history-retry", None)).await;
    let delivery = Delivery::start(&d, ProviderCase::Retry);
    Box::pin(d.run_until("retried delivery only", async || {
        responses_published(&d, 1).await && delivered(&d, 1).await
    }))
    .await;
    let provider = delivery.stop().await;
    let sent = provider.messages();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].delivery, sent[1].delivery);
    assert_eq!(sent[0].email.message_id, sent[1].email.message_id);
    let rows = exchanges(&d).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["state"], "completed");
    assert_eq!(
        rows[0]["output"],
        json!({"body":"One accepted model effect"})
    );
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn agent_history_channel_terminal_delivery_failure_preserves_business_success_and_accepted_exchange()
 {
    let d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"Accepted before terminal transport failure"}"#,
    )]))
    .await;
    let mut selected = example(0);
    model(&mut selected);
    Box::pin(bind(&d, selected)).await;
    d.email(customer_email(&d, "history-terminal-transport", None))
        .await;
    let delivery = Delivery::start(&d, ProviderCase::Terminal);
    Box::pin(d.run_until("terminal delivery after committed agent output",async|| {
        responses_published(&d,1).await && d.count("SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND status='dead_letter'").await==1
    })).await;
    let provider = delivery.stop().await;
    assert_eq!(provider.messages().len(), 1);
    let before = exchanges(&d).await;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0]["state"], "completed");
    assert_eq!(
        before[0]["output"],
        json!({"body":"Accepted before terminal transport failure"})
    );
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND state='succeeded'")
            .await,
        1
    );
    let replacement = Delivery::start(&d, ProviderCase::Accept);
    // Let a reconstructed worker perform a real queue poll past its normal interval.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(replacement.stop().await.messages().is_empty());
    assert_eq!(exchanges(&d).await, before);
    retention::inspect_dead_delivery(&d, &before[0]).await;
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}

#[path = "agent_history_channel_retention_tests.rs"]
mod retention;

/// Phase 05 E8 (U3, U7): with the memory service configured, a completed exchange of a customer
/// email thread, with the agent's `threads` capture on, becomes one generation request in the
/// agent's own memory, labelled for the thread's lowest reader: the external customer makes it
/// public (producing ceiling 0, consumed unknown because thread input is conversational). The
/// deployment's own delivery worker sends it to the fake service. Seeded rows stand in for an
/// enabled tenant and a ready agent namespace (no lifecycle worker run is needed for that).
#[tokio::test]
async fn memory_capture_a_thread_exchange_is_captured_into_the_agent_memory_at_the_thread_level() {
    use crate::adapters::memory_service::contract_fake::ContractFake;
    let fake = ContractFake::start(MEMORY_ADMIN).await;
    fake.state.lock().unwrap().generation_cap = 100_000;
    let d = Box::pin(Deployment::with_memory(
        vec![LlmTurn::text(r#"{"body":"Captured thread answer"}"#)],
        &fake,
    ))
    .await;
    let company = d.fixture.target.company.as_uuid();
    let actor = d.fixture.target.actor.user_id();
    let pool = d.p().pool();
    let provisioner = crate::infra::memory_config::ProvisionerToken::parse(MEMORY_ADMIN.into())
        .unwrap()
        .credential_id();
    sqlx::query(
        "INSERT INTO memory_tenants (company_id, state, revision, disable_epoch, \
           provisioner_credential_id, generation_cap, changed_by, changed_at) \
         VALUES ($1, 'enabled', 1, 0, $3, 100000, $2, clock_timestamp())",
    )
    .bind(company)
    .bind(actor)
    .bind(provisioner)
    .execute(pool)
    .await
    .unwrap();
    let mut selected = example(1);
    prepend_agent(&mut selected);
    Box::pin(bind(&d, selected)).await;
    let agents: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM agents WHERE company_id = $1")
        .bind(company)
        .fetch_all(pool)
        .await
        .unwrap();
    for agent in &agents {
        let namespace = Uuid::new_v4();
        let envelope = d
            .p()
            .credential_cipher()
            .unwrap()
            .seal_envelope(
                &crate::adapters::persistence::credentials::CredentialContext::memory_client_credential(
                    company, namespace,
                ),
                &secrecy::SecretString::from(fake.client_token(company, namespace)),
            )
            .unwrap();
        sqlx::query(
            "INSERT INTO memory_namespaces (company_id, id, scope_kind, agent_id, state, \
               credential_id, credential_envelope) \
             VALUES ($1, $2, 'agent', $3, 'ready', gen_random_uuid(), $4)",
        )
        .bind(company)
        .bind(namespace)
        .bind(agent)
        .bind(envelope)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memory_agent_capture (company_id, agent_id, kind, enabled, \
               allowance_tokens, max_output_tokens, expiry_hours, revision, changed_by, reason, \
               changed_at) \
             VALUES ($1, $2, 'threads', true, 4000, 500, 12, 1, $3, 'Thread capture pilot', \
               clock_timestamp())",
        )
        .bind(company)
        .bind(agent)
        .bind(actor)
        .execute(pool)
        .await
        .unwrap();
    }
    d.email(customer_email(&d, "history-captured", None)).await;
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    let sent = || fake.state.lock().unwrap().generations.len();
    Box::pin(d.run_until("captured thread reply", async || {
        responses_published(&d, 1).await && delivered(&d, 1).await && sent() == 1
    }))
    .await;
    delivery.stop().await;
    let rows = exchanges(&d).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["scope"], "thread");
    let exchange: Uuid = serde_json::from_value(rows[0]["id"].clone()).unwrap();
    assert_eq!(
        rows[0]["preparation"]["capture"],
        json!(true),
        "armed at preparation"
    );
    let (agent, namespace): (Uuid, Uuid) = sqlx::query_as(
        "SELECT exchange.agent_id, namespace.id FROM agent_history_exchanges AS exchange \
         JOIN memory_namespaces AS namespace ON namespace.company_id = exchange.company_id \
           AND namespace.agent_id = exchange.agent_id \
         WHERE exchange.company_id = $1 AND exchange.id = $2",
    )
    .bind(company)
    .bind(exchange)
    .fetch_one(pool)
    .await
    .unwrap();
    let state: String = sqlx::query_scalar(
        "SELECT state FROM memory_deliveries WHERE company_id = $1 AND kind = 'generation'",
    )
    .bind(company)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(state, "delivered");
    let (sent_to, body) = {
        let service = fake.state.lock().unwrap();
        let (sent_to, body, _) = &service.generations[&format!("ob.g.{}", exchange.simple())];
        (sent_to.clone(), body.clone())
    };
    assert_eq!(
        sent_to,
        namespace.hyphenated().to_string(),
        "agent {agent}'s own memory"
    );
    assert_eq!(
        body["producing_ceiling"],
        json!(0),
        "an external reader is public"
    );
    assert_eq!(body["consumed"], json!({"kind":"unknown"}));
    assert_eq!(
        (
            body["allowance_tokens"].clone(),
            body["max_output_tokens"].clone()
        ),
        (json!(4000), json!(500))
    );
    let events = body["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[1]["text"],
        json!(r#"{"body":"Captured thread answer"}"#)
    );
    assert!(events.iter().all(|event| event["access_level"] == json!(0)));
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}
