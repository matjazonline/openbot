//! p08 D2 end to end through the production composition: an admin asks the Agent Builder, in
//! the app, for an assistant; the Builder lists the company's agents and creates the agent and
//! its team channel on the chosen reply mode; and the new channel then answers through its own
//! workflow. Members and emailed requests are refused (F037, F039).
use super::*;
use crate::application::use_cases::thread::IngressOrigin;
use crate::application::workflow::provisioning::*;

/// The Agent Builder on the scripted model, granted the provisioning tools, answering its team
/// channel `builder` through the shipped autonomous template ("Create assistant channel", run
/// by the owner). Returns the Builder channel's binding.
async fn builder(d: &Deployment) -> Uuid {
    let p = d.p();
    let company = d.fixture.target.company.as_uuid();
    let agent = AgentPersistence::create(
        p,
        company,
        AgentWrite {
            name: "Agent Builder".into(),
            slug: "agent-builder".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some("Design and create agents.".into()),
            workflow_tools: Some(vec![
                TypeName::parse(LIST_COMPANY_AGENTS).unwrap(),
                TypeName::parse(CREATE_AGENT_CHANNEL).unwrap(),
            ]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    register_scripted_agent_base_url(agent.id, &d.llm.base_url);
    let created = d
        .engine
        .assistant_channels()
        .create_assistant_channel(
            d.fixture.target.company,
            d.fixture.target.actor,
            AssistantChannelRequest {
                agent_slug: "agent-builder".into(),
                channel_slug: "builder".into(),
                channel_name: "Builder".into(),
                reply_mode: ReplyMode::Autonomous,
                command_key: "builder".into(),
            },
        )
        .await
        .unwrap()
        .unwrap();
    Uuid::parse_str(created["binding_id"].as_str().unwrap()).unwrap()
}

/// A verified company member with `role`, whose address is theirs.
async fn person(d: &Deployment, role: &str) -> (Uuid, String) {
    let (user, email) = verified_member(&d.fixture).await;
    sqlx::query("UPDATE company_members SET role = $2 WHERE user_id = $1")
        .bind(user)
        .bind(role)
        .execute(d.p().pool())
        .await
        .unwrap();
    (user, email)
}

/// `from` writes `text` to the channel `to`: signed in through the app, or by email.
async fn write(d: &Deployment, from: &str, to: &str, text: &str, origin: IngressOrigin) {
    let ingested = d
        .threads
        .ingest_test_email_as(
            RawInboundPayload {
                headers: Some(format!("Message-ID: <{}@example.test>\n", Uuid::new_v4())),
                to: d.address(to),
                from: from.into(),
                subject: Some("Assistant".into()),
                text: Some(text.into()),
                ..Default::default()
            },
            origin,
        )
        .await
        .unwrap();
    assert!(ingested.accepted, "{:?}", ingested.reason());
}

/// Runs of the binding that are done with their steps: finished, or parked on the delivery of
/// the reply they published (no delivery engine runs here).
async fn finished_runs(d: &Deployment, binding: Uuid) -> i64 {
    d.count(&format!(
        "SELECT count(*) FROM workflow_runs WHERE company_id = $1 AND binding_id = '{binding}' \
         AND (state IN ('succeeded', 'failed') OR waiting_reason = 'effect')"
    ))
    .await
}

/// Runs of the binding that published their reply and did not fail.
async fn replied(d: &Deployment, binding: Uuid) -> i64 {
    d.count(&format!(
        "SELECT count(*) FROM workflow_runs AS run WHERE run.company_id = $1 \
         AND run.binding_id = '{binding}' AND run.state <> 'failed' \
         AND EXISTS (SELECT 1 FROM workflow_message_destinations AS reply \
           WHERE reply.company_id = run.company_id AND reply.run_id = run.id AND reply.kind = 'reply')"
    ))
    .await
}

/// Each provisioning receipt of the binding's runs, oldest first.
async fn receipts(d: &Deployment, binding: Uuid) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT receipt.result FROM workflow_action_receipts AS receipt \
         JOIN workflow_runs AS run ON run.company_id = receipt.company_id AND run.id = receipt.run_id \
         WHERE run.binding_id = $1 ORDER BY receipt.created_at",
    )
    .bind(binding)
    .fetch_all(d.p().pool())
    .await
    .unwrap()
}

fn spec(slug: &str, mode: ReplyMode) -> Value {
    json!({
        "name": "Helper", "slug": slug, "system_prompt": "Answer the team briefly.",
        "provider": SCRIPTED_PROVIDER, "model": SCRIPTED_MODEL, "reply_mode": mode.as_str()
    })
}

/// D2 verification: a signed-in admin's request runs the Builder, which lists, then creates the
/// agent, a team channel and its active binding, all as the admin even though the Builder runs
/// as the owner (F036); the new channel then answers a message through its workflow.
#[tokio::test]
async fn builder_provisions_an_assistant_for_a_signed_in_admin() {
    let d = Deployment::new(vec![
        LlmTurn::tool_call(LIST_COMPANY_AGENTS, json!({})),
        LlmTurn::tool_call(CREATE_AGENT_CHANNEL, spec("helper", ReplyMode::Autonomous)),
        LlmTurn::text(r#"{"body": "Created helper."}"#),
        LlmTurn::text(r#"{"body": "Hello from helper."}"#),
    ])
    .await;
    let builder = builder(&d).await;
    let (admin, email) = person(&d, "admin").await;
    write(
        &d,
        &email,
        "builder",
        "Make me a helper.",
        IngressOrigin::TrustedApplication,
    )
    .await;
    d.run_until("the Builder's run finished", async || {
        finished_runs(&d, builder).await == 1
    })
    .await;

    assert_eq!(
        replied(&d, builder).await,
        1,
        "the Builder's run replies too"
    );
    let receipts = receipts(&d, builder).await;
    assert_eq!(receipts.len(), 2, "{receipts:?}");
    let listed: Vec<&str> = receipts[0]["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|agent| agent["slug"].as_str().unwrap())
        .collect();
    assert_eq!(listed, ["agent-builder", "assistant"]);
    assert_eq!(
        receipts[0]["models"],
        json!([{"provider": SCRIPTED_PROVIDER, "model": SCRIPTED_MODEL}])
    );
    let created = &receipts[1];
    assert_eq!(created["agent"]["slug"], "helper", "{created}");
    assert_eq!(created["channel"]["address"], d.address("helper"), "F051");
    let binding = Uuid::parse_str(created["binding_id"].as_str().unwrap()).unwrap();
    let (run_as, actor, active): (Uuid, Uuid, bool) = sqlx::query_as(
        "SELECT revision.run_as_user_id, revision.actor_id, binding.active \
         FROM workflow_bindings AS binding JOIN workflow_binding_revisions AS revision \
           ON revision.company_id = binding.company_id AND revision.binding_id = binding.id \
         WHERE binding.id = $1",
    )
    .bind(binding)
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!((run_as, actor, active), (admin, admin, true));
    let builder_run_as: Uuid = sqlx::query_scalar(
        "SELECT run_as_user_id FROM workflow_binding_revisions WHERE binding_id = $1",
    )
    .bind(builder)
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(builder_run_as, d.fixture.target.actor.user_id());
    let access: String = sqlx::query_scalar("SELECT access_mode FROM channels WHERE id = $1")
        .bind(Uuid::parse_str(created["channel"]["id"].as_str().unwrap()).unwrap())
        .fetch_one(d.p().pool())
        .await
        .unwrap();
    assert_eq!(access, "team");

    // The new assistant answers its channel through the provisioned workflow.
    register_scripted_agent_base_url(
        Uuid::parse_str(created["agent"]["id"].as_str().unwrap()).unwrap(),
        &d.llm.base_url,
    );
    write(
        &d,
        &email,
        "helper",
        "Hi there",
        IngressOrigin::TrustedApplication,
    )
    .await;
    d.run_until("the helper answered", async || {
        finished_runs(&d, binding).await == 1
    })
    .await;
    assert_eq!(replied(&d, binding).await, 1, "the helper's run replies");
    assert_eq!(
        replies(&d).await.len(),
        2,
        "the Builder's reply and the helper's"
    );
    assert_eq!(d.llm.finish().await.unwrap(), 4);
}

/// F037/F039: a plain member in the app, and an admin by email, get the forbidden refusal from
/// both tools; nothing is listed or provisioned.
#[tokio::test]
async fn builder_refuses_members_and_emailed_requests() {
    let d = Deployment::new(vec![
        LlmTurn::tool_call(LIST_COMPANY_AGENTS, json!({})),
        LlmTurn::tool_call(CREATE_AGENT_CHANNEL, spec("helper", ReplyMode::Autonomous)),
        LlmTurn::text(r#"{"body": "Not allowed."}"#),
        LlmTurn::tool_call(CREATE_AGENT_CHANNEL, spec("helper", ReplyMode::Reviewed)),
        LlmTurn::text(r#"{"body": "Not allowed."}"#),
    ])
    .await;
    let builder = builder(&d).await;
    let agents = d
        .count("SELECT count(*) FROM agents WHERE company_id = $1")
        .await;
    let channels = d
        .count("SELECT count(*) FROM channels WHERE company_id = $1")
        .await;

    let (_, member) = person(&d, "member").await;
    write(
        &d,
        &member,
        "builder",
        "Make me a helper.",
        IngressOrigin::TrustedApplication,
    )
    .await;
    d.run_until("the member's run finished", async || {
        finished_runs(&d, builder).await == 1
    })
    .await;
    let (_, admin) = person(&d, "admin").await;
    write(
        &d,
        &admin,
        "builder",
        "Make me a helper.",
        IngressOrigin::ExternalTransport,
    )
    .await;
    d.run_until("the emailed run finished", async || {
        finished_runs(&d, builder).await == 2
    })
    .await;

    let codes: Vec<Value> = receipts(&d, builder)
        .await
        .iter()
        .map(|receipt| receipt["error"]["code"].clone())
        .collect();
    assert_eq!(codes, vec![json!(PROVISIONING_FORBIDDEN); 3]);
    assert_eq!(
        replied(&d, builder).await,
        2,
        "the Builder explains, both times"
    );
    assert_eq!(
        d.count("SELECT count(*) FROM agents WHERE company_id = $1")
            .await,
        agents
    );
    assert_eq!(
        d.count("SELECT count(*) FROM channels WHERE company_id = $1")
            .await,
        channels
    );
    assert_eq!(d.llm.finish().await.unwrap(), 5);
}

/// D2.5: a reviewed assistant's answer waits for its reviewer, the requester, until the run's
/// own deadline.
#[tokio::test]
async fn reviewed_assistants_park_their_answer_until_the_run_deadline() {
    let d = Deployment::new(vec![
        LlmTurn::tool_call(CREATE_AGENT_CHANNEL, spec("reviewed", ReplyMode::Reviewed)),
        LlmTurn::text(r#"{"body": "Created."}"#),
        LlmTurn::text(r#"{"body": "A draft for review."}"#),
    ])
    .await;
    let builder = builder(&d).await;
    let (admin, email) = person(&d, "admin").await;
    write(
        &d,
        &email,
        "builder",
        "A reviewed helper.",
        IngressOrigin::TrustedApplication,
    )
    .await;
    d.run_until("the Builder's run finished", async || {
        finished_runs(&d, builder).await == 1
    })
    .await;
    let created = receipts(&d, builder).await.remove(0);
    assert_eq!(created["reply_mode"], "reviewed", "{created}");
    register_scripted_agent_base_url(
        Uuid::parse_str(created["agent"]["id"].as_str().unwrap()).unwrap(),
        &d.llm.base_url,
    );
    let binding = Uuid::parse_str(created["binding_id"].as_str().unwrap()).unwrap();
    write(
        &d,
        &email,
        "reviewed",
        "Question",
        IngressOrigin::TrustedApplication,
    )
    .await;
    d.run_until("the answer waits for review", async || {
        d.count(&format!(
            "SELECT count(*) FROM workflow_runs WHERE company_id = $1 \
             AND binding_id = '{binding}' AND waiting_reason = 'decision'"
        ))
        .await
            == 1
    })
    .await;
    let (expires_with_run, reviewer): (bool, Value) = sqlx::query_as(
        "SELECT wait.deadline = run.deadline, revision.params -> 'reviewer' \
         FROM workflow_waits AS wait \
         JOIN workflow_runs AS run ON run.company_id = wait.company_id AND run.id = wait.run_id \
         JOIN workflow_binding_revisions AS revision ON revision.company_id = run.company_id \
           AND revision.binding_id = run.binding_id AND revision.revision = run.binding_revision \
         WHERE run.binding_id = $1 AND wait.reason = 'decision'",
    )
    .bind(binding)
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert!(expires_with_run);
    assert_eq!(reviewer, json!(admin.to_string()));
    assert_eq!(d.llm.finish().await.unwrap(), 3);
}
