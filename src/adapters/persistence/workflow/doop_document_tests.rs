use super::*;
use crate::application::use_cases::{
    agent::{AgentPersistence, AgentWrite},
    company::{CompanyModelConnectionWrite, CompanyPersistence},
    skill::{SkillManagementPersistence, SkillWrite},
};
use crate::application::workflow::agent::McpErrorPolicy;
use crate::application::workflow::decisions::{
    DecisionCommandKey, DecisionId, SubmitDecision, WorkflowDecisions,
};
use crate::services::test_support::{SCRIPTED_MODEL, SCRIPTED_PROVIDER};

struct Document {
    f: Fixture,
    server: McpServer,
    client: Arc<HttpMcpClient>,
    engine: WorkflowEngine,
    target: BindingTarget,
    agent: Uuid,
    connection: Uuid,
    channel: crate::application::workflow::RelatedChannelId,
    delivery: delivery_support::DeliveryFixture,
}

fn envelope(value: Value) -> Value {
    json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false})
}

fn canvas() -> Value {
    envelope(
        json!({"canvas":{"id":"canvas-a","name":"Synthetic document","url":"/c/canvas-a"},
        "workspace_id":"workspace-a","frames":[]}),
    )
}

fn final_canvas() -> Value {
    let mut value = canvas();
    value["structuredContent"]["frames"] = json!([{
        "id":"frame-a","name":"Draft","width":800,"height":1000,
        "image_url":"/frame-a.png","url":"/c/canvas-a","empty":false
    }]);
    value
}

fn params(f: &Fixture) -> Value {
    json!({"expected_bot_id":"user-bot-a","expected_workspace_id":"workspace-a",
        "doop_origin":"https://doop.example.test",
        "result_destinations":[{"kind":"user","id":f.target.actor.user_id()}],
        "action_approval":{"reviewers":[{"kind":"user","id":f.target.actor.user_id()}],
            "deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()}})
}

fn input() -> Value {
    json!({"title":"Synthetic document","brief":"Write a short synthetic project brief."})
}

fn tool_name(name: &str) -> crate::entities::mcp::McpToolName {
    name.to_owned().try_into().unwrap()
}

const EDITOR: &str = include_str!("../../../../integrations/doop/document-editor.md");
const GUIDES: [&str; 3] = [
    include_str!("../../../../integrations/doop/document-guide-1.md"),
    include_str!("../../../../integrations/doop/document-guide-2.md"),
    include_str!("../../../../integrations/doop/document-guide-3.md"),
];

#[path = "doop_document_guide_tests.rs"]
mod guide_tests;

fn editor_tools() -> Vec<TypeName> {
    recipe()
        .editing_agent
        .into_iter()
        .filter(|name| name.as_str() != "get_guide")
        .collect()
}

async fn guide_skills(f: &Fixture) -> Vec<Uuid> {
    let mut ids = Vec::new();
    for (index, instructions) in GUIDES.iter().enumerate() {
        let skill = SkillManagementPersistence::create_company(
            &f.persistence, f.target.company.as_uuid(), SkillWrite {
                files: None,
                slug: format!("doop-document-guide-{}", index + 1),
                name: format!("Doop document guide {}/3", index + 1),
                description: "Pinned Doop reference; editor instructions restrict capabilities and review claims.".into(),
                instructions: (*instructions).into(),
                required_tools: Some(vec![]), created_by: None,
            },
        ).await.unwrap();
        ids.push(skill.id);
    }
    ids
}

#[path = "doop_document_live_tests.rs"]
mod live_tests;
#[path = "doop_document_runtime_support.rs"]
mod run_support;

#[path = "doop_document_agent_tests.rs"]
mod agent_tests;
#[path = "doop_document_create_tests.rs"]
mod create_tests;
#[path = "doop_document_delivery_support.rs"]
mod delivery_support;
#[path = "doop_document_failure_tests.rs"]
mod failure_tests;
#[path = "doop_document_lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "doop_document_policy_tests.rs"]
mod policy_tests;
#[path = "doop_document_recovery_tests.rs"]
mod recovery_tests;
#[path = "doop_document_result_tests.rs"]
mod result_tests;
#[path = "doop_document_sample_tests.rs"]
mod sample_tests;

fn document_source(f: &Fixture) -> Value {
    let mut source: Value = serde_json::from_str(include_str!(
        "../../../../integrations/doop/create-document.workflow.json"
    ))
    .unwrap();
    source["workflow_id"] = json!(f.target.workflow);
    source
}

async fn editor(f: &Fixture) -> Uuid {
    let p = &f.persistence;
    let selected = editor_tools();
    let skill_ids = guide_skills(f).await;
    let bindings = crate::adapters::persistence::agent_binding_selection::resolve_bindings_on(
        &mut p.pool().acquire().await.unwrap(),
        f.target.company.as_uuid(),
        None,
        &selected,
    )
    .await
    .unwrap();
    let agent = AgentPersistence::create(
        p,
        f.target.company.as_uuid(),
        AgentWrite {
            name: "Doop document editor".into(),
            slug: "doop-document-editor".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some(EDITOR.into()),
            tool_bindings: Some(bindings),
            skill_ids,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    CompanyPersistence::replace_model_connections_for_user(
        p,
        f.target.actor.user_id(),
        f.target.company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    f.publish_agent(agent.id).await;
    agent.id
}

async fn publish_document(
    f: &Fixture,
    e: &WorkflowEngine,
    connection: Uuid,
    policy: McpErrorPolicy,
) -> VersionId {
    let selections =
        ["get_profile", "create_canvas", "get_canvas"].map(|tool| McpPublicationSelection {
            connection,
            slot: ResourceName::parse("service").unwrap(),
            tool: TypeName::parse(tool).unwrap(),
        });
    let definitions = e.services().definitions(vec![], selections.into());
    let mut draft = f.save(None);
    let mut source = document_source(f);
    source["steps"]["edit"]["with"]["mcp_error_policy"] = json!({"literal":policy});
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let publication = f.publish();
    definitions.publish(publication.clone()).await.unwrap();
    publication.version
}

impl Document {
    async fn new(policy: McpErrorPolicy) -> Self {
        let server = mcp_server(Call::Json).await;
        server.set_tools(captured("tools")["tools"].clone());
        server.set_tool_result(tool_name("get_guide"), captured("guide"));
        server.set_tool_result(tool_name("get_profile"), captured("profile"));
        server.set_tool_result(tool_name("create_canvas"), envelope(json!({"id":"canvas-a","name":"Synthetic document","url":"/c/canvas-a","workspace_id":"workspace-a"})));
        server.set_tool_result(tool_name("get_canvas"), final_canvas());
        let mut f = Fixture::new().await;
        f.persistence = PostgresPersistence::with_credential_cipher(
            f.persistence.pool().clone(),
            CredentialCipher::for_test(),
        );
        let connection = register(&f, &server.url).await;
        // Box the real-PG delivery setup seam to keep the fixture's async poll frames shallow.
        let delivery = Box::pin(delivery_support::delivery(&f, &connection)).await;
        let channel = delivery.channel;
        let agent = editor(&f).await;
        let client = guarded_client(&server.url);
        let engine = WorkflowEngine::for_tests_with_mcp(f.persistence.clone(), client.clone());
        let version = publish_document(&f, &engine, connection.id, policy).await;
        let target = BindingTarget {
            company: f.target.company,
            actor: f.target.actor,
            binding: WorkflowBindingId::new(Uuid::new_v4()),
        };
        let bindings = engine.services().bindings();
        let configured = bindings
            .configure(ConfigureBinding {
                target,
                expected: None,
                association: RelatedAssociation::Channel(channel),
                version,
                params: params(&f),
                resources: BTreeMap::from([(
                    ResourceName::parse("service").unwrap(),
                    RuntimeResourceId::new(connection.id),
                )]),
                run_as: None,
                message_trigger: None,
            })
            .await
            .unwrap();
        bindings
            .activate(SetBindingActivity {
                target,
                expected: configured.revision,
            })
            .await
            .unwrap();
        Self {
            f,
            server,
            client,
            engine,
            target,
            agent,
            connection: connection.id,
            channel,
            delivery,
        }
    }

    fn runtime(&self) -> run_support::DocumentRun<'_> {
        run_support::DocumentRun {
            persistence: &self.f.persistence,
            engine: &self.engine,
            target: self.target,
            channel: self.channel,
        }
    }

    async fn report_failure(&self, scope: RunReadScope, view: &RunView) {
        if view.detail.summary.state != "failed" {
            return;
        }
        for execution in &view.detail.executions {
            let step = self
                .f
                .persistence
                .step_view(scope, execution.execution)
                .await
                .unwrap()
                .unwrap();
            for attempt in step.attempts {
                if let Some(code) = attempt.failure_code {
                    eprintln!("Doop step {} failed: {code}", execution.step);
                }
            }
        }
    }

    async fn drive(
        &self,
        scope: RunReadScope,
        stop: impl Fn(&RunView, &[DecisionId]) -> bool,
    ) -> RunView {
        assert_eq!(self.engine.slots(), 1, "one slot per test worker group");
        let shutdown = CancellationToken::new();
        let _guard = shutdown.clone().drop_guard();
        let mut workers = self.engine.spawn(&shutdown);
        workers.extend(self.engine.spawn(&shutdown));
        let (_delivery_shutdown, mut delivery_receiver) = tokio::sync::broadcast::channel(1);
        // Production workers poll once per second; budget three polls per graph step.
        // Keep this bounded assertion in CI instead of raising any runtime limit.
        let steps = document_source(&self.f)["steps"].as_object().unwrap().len() as u64;
        let result = tokio::time::timeout(Duration::from_secs(5 + 3 * steps), async {
            loop {
                self.delivery
                    .worker
                    .drain_once(&mut delivery_receiver)
                    .await
                    .unwrap();
                let view = self.f.persistence.run_view(scope).await.unwrap().unwrap();
                let decisions = self.runtime().approvals(scope).await;
                if terminal(&view) {
                    // Box the diagnostics seam so failure reporting adds no large poll frame.
                    Box::pin(self.report_failure(scope, &view)).await;
                    return view;
                }
                // The decision and run view are separate reads. Wait for suspension
                // before stopping workers or submitting an approval continuation.
                if !decisions.is_empty() && view.detail.summary.state != "waiting" {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    continue;
                }
                if stop(&view, &decisions) {
                    return view;
                }
                for decision in decisions {
                    self.runtime().approve(scope, decision).await;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        shutdown.cancel();
        for (_, worker) in workers {
            tokio::time::timeout(Duration::from_secs(10), worker)
                .await
                .unwrap()
                .unwrap();
        }
        match result {
            Ok(view) => view,
            Err(_) => panic!(
                "document timed out: {:?}; calls={:?}",
                self.f.persistence.run_view(scope).await.unwrap().map(|v| (
                    v.detail.summary.state,
                    v.detail.summary.waiting_reason,
                    v.detail
                        .executions
                        .into_iter()
                        .map(|e| e.step)
                        .collect::<Vec<_>>()
                )),
                self.server.call_names()
            ),
        }
    }
}

fn terminal(view: &RunView) -> bool {
    matches!(
        view.detail.summary.state.as_str(),
        "succeeded" | "failed" | "cancelled"
    )
}

#[tokio::test]
async fn doop_create_shipped_document_publishes_with_actual_catalog_and_editor() {
    let server = mcp_server(Call::Json).await;
    let mut f = Fixture::new().await;
    f.persistence = PostgresPersistence::with_credential_cipher(
        f.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let connection = register(&f, &server.url).await;
    editor(&f).await;
    let client = guarded_client(&server.url);
    let e = WorkflowEngine::for_tests_with_mcp(f.persistence.clone(), client.clone());
    publish_document(&f, &e, connection.id, McpErrorPolicy::FailStep).await;
    assert_eq!(server.calls(), 0, "publication never dispatches tools");
    client.shutdown().await.unwrap();
}
