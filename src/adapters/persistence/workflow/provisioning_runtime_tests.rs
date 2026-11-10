//! p08 D2 end to end through the production composition: an admin asks the Agent Builder, in
//! the app, for an assistant; the Builder lists the company's agents and creates the agent and
//! its team channel on the chosen reply mode; and the new channel then answers through its own
//! workflow. Members and emailed requests are refused (F037, F039).
use super::*;
use crate::application::use_cases::thread::IngressOrigin;
use crate::application::workflow::provisioning::*;

#[path = "provisioning_requester_lock_tests.rs"]
mod requester_locks;

#[path = "channel_reply_native_runtime_tests.rs"]
mod channel_reply_native_runtime_tests;

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
            tool_bindings: Some(vec![
                crate::entities::agent_capability::AgentToolBinding::builtin(
                    crate::entities::agent_capability::AgentBuiltinTool::ListCompanyAgents,
                ),
                crate::entities::agent_capability::AgentToolBinding::builtin(
                    crate::entities::agent_capability::AgentBuiltinTool::CreateAgentChannel,
                ),
            ]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    register_scripted_agent_base_url(agent.id, &d.llm.base_url);
    let created = d
        .engine
        .services()
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
    let headers = format!("Message-ID: <{}@example.test>\n", Uuid::new_v4());
    write_with(d, from, to, text, origin, headers).await;
}

async fn write_with(
    d: &Deployment,
    from: &str,
    to: &str,
    text: &str,
    origin: IngressOrigin,
    headers: String,
) {
    let ingested = d
        .threads
        .ingest_test_email_as(
            RawInboundPayload {
                headers: Some(headers),
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
         AND (EXISTS (SELECT 1 FROM workflow_message_destinations AS reply \
           WHERE reply.company_id = run.company_id AND reply.run_id = run.id AND reply.kind = 'reply') \
           OR EXISTS (SELECT 1 FROM channel_reply_decisions AS final \
           WHERE final.company_id=run.company_id AND final.run_id=run.id AND final.state='published'))"
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

/// An agent answering the desk at `slug`. The agent's own address is its autonomous default desk
/// (01DEP-P03-D5), so a reviewed desk at `slug` belongs to the agent `{slug}-agent`.
fn spec(slug: &str, mode: ReplyMode) -> Value {
    let mut spec = json!({
        "name": "Helper", "slug": slug, "system_prompt": "Answer the team briefly.",
        "provider": SCRIPTED_PROVIDER, "model": SCRIPTED_MODEL, "reply_mode": mode.as_str()
    });
    if mode == ReplyMode::Reviewed {
        spec["slug"] = json!(format!("{slug}-agent"));
        spec["channel_slug"] = json!(slug);
    }
    spec
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
        replied(&d, builder).await == 1
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
        replied(&d, binding).await == 1
    })
    .await;
    assert_eq!(replied(&d, binding).await, 1, "the helper's run replies");
    assert_eq!(
        reply_count(&d).await,
        2,
        "the Builder's reply and the helper's"
    );
    assert_eq!(d.llm.finish().await.unwrap(), 4);
}

/// The Builder holds a conversation: a follow-up in the same thread reaches the agent with the
/// thread so far, so the earlier message is still in front of it.
#[tokio::test]
async fn builder_sees_the_conversation_so_far() {
    let mut d = Deployment::new(vec![
        LlmTurn::text(r#"{"body": "What should the new agent handle?"}"#),
        LlmTurn::text(r#"{"body": "A billing agent, then."}"#),
    ])
    .await;
    let builder = builder(&d).await;
    let (_, email) = person(&d, "admin").await;
    let first = format!("{}@example.test", Uuid::new_v4());
    write_with(
        &d,
        &email,
        "builder",
        "Make me an agent for the finance team.",
        IngressOrigin::TrustedApplication,
        format!("Message-ID: <{first}>\n"),
    )
    .await;
    d.run_until("the first answer", async || replied(&d, builder).await == 1)
        .await;
    write_with(
        &d,
        &email,
        "builder",
        "Billing questions.",
        IngressOrigin::TrustedApplication,
        format!(
            "Message-ID: <{}@example.test>\nIn-Reply-To: <{first}>\nReferences: <{first}>\n",
            Uuid::new_v4()
        ),
    )
    .await;
    d.run_until("the follow-up answer", async || {
        replied(&d, builder).await == 2
    })
    .await;

    let requests = d.llm.observed();
    let follow_up = requests[1].to_string();
    assert!(
        follow_up.contains(r#"\"untrusted_message\":\"Billing questions.\""#),
        "{follow_up}"
    );
    assert!(
        follow_up.contains("untrusted_history")
            && follow_up.contains("Make me an agent for the finance team."),
        "the follow-up sees the first message: {follow_up}"
    );
    assert!(
        !requests[0].to_string().contains("Billing questions."),
        "history is frozen when a run starts"
    );
    assert_eq!(d.llm.finish().await.unwrap(), 2);
}

/// The Builder's spec adds its own steps to the reply mode's: a classifier before the draft,
/// whose label the agent reads, and a rule before the reply that lets only billing questions
/// through. The new desk -- beside the agent's default one, which takes no steps -- runs them in
/// that order.
#[tokio::test]
async fn builder_adds_custom_steps_to_the_new_agents_workflow() {
    let mut spec = spec("helper", ReplyMode::Autonomous);
    spec["channel_slug"] = json!("billing");
    let labels = json!({"literal": {"type": "string", "enum": ["billing", "other"]}});
    spec["steps"] = json!([
        {"id": "topic", "slot": "before_draft", "type": "ai.classify", "with": {
            "agent": {"literal": "helper"},
            "context": {"object": {"untrusted_message": {"ref": "/input/message"}}},
            "output_schema": labels}},
        {"id": "billing_only", "slot": "before_reply", "type": "decision.rule",
            "with": {"data": {"ref": "/steps/topic/output"}, "data_schema": labels},
            "rule": {"cases": [{"when": {"eq": [{"ref": "/steps/topic/output"},
                {"literal": "billing"}]}, "choice": "send"}], "default": "hold"},
            "routes": {"choices": {"send": "$next", "hold": "$next"}}}
    ]);
    let mut d = Deployment::new(vec![
        LlmTurn::tool_call(CREATE_AGENT_CHANNEL, spec),
        LlmTurn::text(r#"{"body": "Created helper."}"#),
        LlmTurn::text(r#""billing""#),
        LlmTurn::text(r#"{"body": "Your invoice is attached."}"#),
    ])
    .await;
    let builder = builder(&d).await;
    let (_, email) = person(&d, "admin").await;
    write(
        &d,
        &email,
        "builder",
        "Make me a billing helper.",
        IngressOrigin::TrustedApplication,
    )
    .await;
    d.run_until("the Builder's run finished", async || {
        replied(&d, builder).await == 1
    })
    .await;
    let created = receipts(&d, builder).await.remove(0);
    assert_eq!(created["agent"]["slug"], "helper", "{created}");
    let binding = Uuid::parse_str(created["binding_id"].as_str().unwrap()).unwrap();

    register_scripted_agent_base_url(
        Uuid::parse_str(created["agent"]["id"].as_str().unwrap()).unwrap(),
        &d.llm.base_url,
    );
    write(
        &d,
        &email,
        "billing",
        "Where is my invoice?",
        IngressOrigin::TrustedApplication,
    )
    .await;
    d.run_until("the helper answered", async || {
        replied(&d, binding).await == 1
    })
    .await;
    assert_eq!(replied(&d, binding).await, 1, "billing passes the rule");
    let steps: Vec<String> = sqlx::query_scalar(
        "SELECT execution.step_id FROM workflow_executions AS execution \
         JOIN workflow_runs AS run \
           ON run.company_id = execution.company_id AND run.id = execution.run_id \
         WHERE run.binding_id = $1 ORDER BY execution.activation",
    )
    .bind(binding)
    .fetch_all(d.p().pool())
    .await
    .unwrap();
    assert_eq!(
        steps,
        ["context", "topic", "draft", "billing_only", "reply"],
        "the custom steps run in their slots"
    );
    let draft = d.llm.observed().remove(3).to_string();
    assert!(
        draft.contains(r#"\"untrusted_topic\":\"billing\""#),
        "{draft}"
    );
    assert_eq!(d.llm.finish().await.unwrap(), 4);
}

/// An admin's message the Builder cannot act on yet, and a model that keeps listing: the listing
/// runs twice, the third identical request is refused before it runs, and the step fails as a
/// repeated call after three model calls instead of spending its whole allowance. The model saw
/// the message's subject as well as its body.
#[tokio::test]
async fn builder_stops_a_listing_loop_once_its_result_repeats() {
    let mut d = Deployment::new(vec![
        LlmTurn::tool_call(LIST_COMPANY_AGENTS, json!({})),
        LlmTurn::tool_call(LIST_COMPANY_AGENTS, json!({})),
        LlmTurn::tool_call(LIST_COMPANY_AGENTS, json!({})),
    ])
    .await;
    let builder = builder(&d).await;
    let (_, email) = person(&d, "admin").await;
    write(
        &d,
        &email,
        "builder",
        "we are in CET",
        IngressOrigin::TrustedApplication,
    )
    .await;
    d.run_until("the Builder's run finished", async || {
        finished_runs(&d, builder).await == 1
    })
    .await;

    let failure: ListingLoopFailure = sqlx::query_as(
        "SELECT run.state, attempt.workflow_failure_code FROM workflow_runs AS run \
         JOIN workflow_executions AS execution \
           ON execution.company_id = run.company_id AND execution.run_id = run.id \
           AND execution.id = run.terminal_execution_id \
         JOIN background_tasks AS job \
           ON job.company_id = execution.company_id AND job.workflow_execution_id = execution.id \
         JOIN task_attempts AS attempt ON attempt.task_id = job.id \
         WHERE run.binding_id = $1",
    )
    .bind(builder)
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(failure.state, "failed");
    assert_eq!(failure.workflow_failure_code, "agent.repeated_tool_call");
    let work: ListingLoopWork = sqlx::query_as(
        "SELECT jsonb_array_length(harness.checkpoint->'turns') AS turns, \
           jsonb_array_length(harness.checkpoint->'invocations') AS invocations \
         FROM task_harness_runs AS harness JOIN workflow_runs AS run \
           ON run.company_id = harness.company_id AND run.id = harness.workflow_run_id \
         WHERE run.binding_id = $1",
    )
    .bind(builder)
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(work.turns, 3);
    assert_eq!(work.invocations, 2, "the third listing never ran");
    let first = d.llm.observed().remove(0).to_string();
    assert!(
        first.contains(r#"\"untrusted_subject\":\"Assistant\""#),
        "{first}"
    );
    assert_eq!(d.llm.finish().await.unwrap(), 3);
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
        replied(&d, builder).await == 1
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
        replied(&d, builder).await == 2
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
        replied(&d, builder).await == 1
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

#[derive(sqlx::FromRow)]
struct ListingLoopFailure {
    state: String,
    workflow_failure_code: String,
}
#[derive(sqlx::FromRow)]
struct ListingLoopWork {
    turns: i32,
    invocations: i32,
}

async fn reply_count(d: &Deployment) -> usize {
    replies(d).await.len() + d.count(
        "SELECT count(*) FROM channel_reply_decisions WHERE company_id=$1 AND state='published'"
    ).await as usize
}

/// 01DEP-P03-D8/D9 end to end: a department led by a person has a plain desk; when an agent takes
/// the lead the desk answers through it, and after the lead moves to another agent the same desk
/// and binding answer through the new lead.
#[tokio::test]
async fn department_lead_desk_answers_through_its_current_lead() {
    use crate::adapters::persistence::department::PostgresDepartments;
    use crate::application::ports::department::{
        DepartmentPersistence, DepartmentScope, DepartmentWrite,
    };
    use crate::entities::department::DepartmentRole;
    let d = Deployment::new(vec![
        LlmTurn::text(r#"{"body": "From the first lead."}"#),
        LlmTurn::text(r#"{"body": "From the second lead."}"#),
    ])
    .await;
    let p = d.p();
    let company = d.fixture.target.company;
    let owner = d.fixture.target.actor;
    let agents = crate::adapters::persistence::test_support::company_agents(p);
    let mut leads = Vec::new();
    for slug in ["first-lead", "second-lead"] {
        let created = agents
            .create(
                owner,
                company,
                AgentWrite {
                    name: slug.into(),
                    slug: slug.into(),
                    system_prompt: Some("Answer briefly.".into()),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        register_scripted_agent_base_url(created.agent.id, &d.llm.base_url);
        let principal: Uuid =
            sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2")
                .bind(company.as_uuid())
                .bind(created.agent.id)
                .fetch_one(p.pool())
                .await
                .unwrap();
        leads.push((
            created.agent.id,
            crate::entities::transport::PrincipalId::new(principal),
        ));
    }
    let departments = PostgresDepartments::new(
        p.clone(),
        crate::adapters::persistence::test_support::provisioning_templates(),
    );
    let scope = DepartmentScope {
        actor: owner,
        company,
    };
    let owner_principal: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2 AND kind = 'person'",
    )
    .bind(company.as_uuid())
    .bind(owner.user_id())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let department = departments
        .create(
            scope,
            DepartmentWrite {
                name: "Ops".into(),
                slug: "ops".into(),
                ..DepartmentWrite::default()
            },
            Some(crate::entities::transport::PrincipalId::new(
                owner_principal,
            )),
        )
        .await
        .unwrap();
    departments
        .assign_role(scope, department.id, DepartmentRole::Lead, leads[0].1)
        .await
        .unwrap();
    let binding: Uuid = sqlx::query_scalar(
        "SELECT (result->>'binding_id')::uuid FROM workflow_provisioning_commands \
         WHERE company_id = $1 AND command_key = $2",
    )
    .bind(company.as_uuid())
    .bind(format!("department:{}", department.id))
    .fetch_one(p.pool())
    .await
    .unwrap();
    let (_, email) = person(&d, "admin").await;
    write(
        &d,
        &email,
        "ops",
        "Who answers?",
        IngressOrigin::TrustedApplication,
    )
    .await;
    Box::pin(d.run_until("the first lead answered", async || {
        replied(&d, binding).await == 1
    }))
    .await;
    departments
        .assign_role(scope, department.id, DepartmentRole::Lead, leads[1].1)
        .await
        .unwrap();
    write(
        &d,
        &email,
        "ops",
        "And now?",
        IngressOrigin::TrustedApplication,
    )
    .await;
    Box::pin(d.run_until("the second lead answered", async || {
        replied(&d, binding).await == 2
    }))
    .await;
    let answered_by: Vec<Uuid> = sqlx::query_scalar(
        "SELECT exchange.agent_id FROM agent_history_exchanges AS exchange \
         JOIN workflow_runs AS run ON run.company_id = exchange.company_id \
          AND run.id = exchange.run_id \
         WHERE run.binding_id = $1 ORDER BY run.created_at",
    )
    .bind(binding)
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(answered_by, [leads[0].0, leads[1].0]);
    assert_eq!(d.llm.finish().await.unwrap(), 2);
}
