use super::*;
use crate::adapters::persistence::workflow::mcp::McpPublicationSelection;
use crate::application::agent::ActionEffect;
use crate::application::services::mcp_client::McpClient;
use crate::application::workflow::run_views::{RunReadScope, RunView, RunViews};
use crate::infra::workflow::WorkflowEngine;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogRecipe {
    operations: Vec<OperationRecipe>,
    editing_agent: Vec<TypeName>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationRecipe {
    tool: TypeName,
    effect: ActionEffect,
    approval_required: bool,
}

struct Verification {
    fixture: Fixture,
    server: McpServer,
    client: Arc<HttpMcpClient>,
    engine: WorkflowEngine,
    connection: CompanyMcpConnection,
    version: VersionId,
    target: BindingTarget,
}

fn captured(name: &str) -> Value {
    serde_json::from_str(match name {
        "tools" => include_str!("../../mcp/fixtures/doop/tools.json"),
        "profile" => include_str!("../../mcp/fixtures/doop/profile.json"),
        "guide" => include_str!("../../mcp/fixtures/doop/guide.json"),
        "canvas" => include_str!("../../mcp/fixtures/doop/get-canvas.json"),
        "denied" => include_str!("../../mcp/fixtures/doop/foreign-canvas-error.json"),
        _ => panic!("unknown capture"),
    })
    .unwrap()
}

fn recipe() -> CatalogRecipe {
    serde_json::from_str(include_str!("../../../../integrations/doop/catalog.json")).unwrap()
}

fn tools() -> Vec<McpDiscoveredTool> {
    captured("tools")["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| McpDiscoveredTool {
            name: tool["name"]
                .as_str()
                .unwrap()
                .to_owned()
                .try_into()
                .unwrap(),
            description: tool["description"].as_str().unwrap_or_default().to_owned(),
            input_schema: tool["inputSchema"].clone(),
            output_schema: tool.get("outputSchema").cloned(),
        })
        .collect()
}

async fn register(f: &Fixture, url: &str) -> CompanyMcpConnection {
    let catalog = tools();
    let recipe = recipe();
    let p = &f.persistence;
    let company = f.target.company;
    let current = p
        .create_mcp_connection(
            company.as_uuid(),
            McpConnectionWrite {
                slug: "doop".into(),
                endpoint: url.to_owned().try_into().unwrap(),
                enabled: true,
                auth: McpAuth::Bearer,
                discovered_tools: catalog.clone(),
                tool_grants: recipe
                    .operations
                    .iter()
                    .map(|op| op.tool.as_str().to_owned().try_into().unwrap())
                    .collect(),
            },
        )
        .await
        .unwrap();
    p.replace_mcp_token(
        company.as_uuid(),
        current.id,
        current.revision,
        Some(SecretString::from(TOKEN)),
    )
    .await
    .unwrap();
    let current = connection_on(p, company.as_uuid(), current.id).await;
    for selected in recipe.operations {
        let tool = catalog
            .iter()
            .find(|tool| tool.name.as_str() == selected.tool.as_str())
            .unwrap();
        p.register_mcp_operation(
            company,
            f.target.actor,
            McpOperationWrite {
                connection: current.id,
                catalog_revision: current.revision,
                expected_version: None,
                operation: McpOperation::review(
                    company,
                    tool,
                    selected.effect,
                    selected.approval_required,
                    1,
                )
                .unwrap(),
            },
        )
        .await
        .unwrap();
    }
    current
}

async fn publish(f: &Fixture, engine: &WorkflowEngine, connection: Uuid) -> VersionId {
    let selections = ["get_profile", "get_canvas"].map(|tool| McpPublicationSelection {
        connection,
        slot: ResourceName::parse("service").unwrap(),
        tool: TypeName::parse(tool).unwrap(),
    });
    let definitions = engine.services().definitions(vec![], selections.into());
    let mut source: Value = serde_json::from_str(include_str!(
        "../../../../integrations/doop/verify-company.workflow.json"
    ))
    .unwrap();
    source["workflow_id"] = json!(f.target.workflow.as_uuid());
    let mut draft = f.save(None);
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let publication = f.publish();
    definitions.publish(publication.clone()).await.unwrap();
    publication.version
}

async fn bind(
    f: &Fixture,
    engine: &WorkflowEngine,
    connection: Uuid,
    version: VersionId,
) -> BindingTarget {
    let target = BindingTarget {
        company: f.target.company,
        actor: f.target.actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let bindings = engine.services().bindings();
    let state = bindings.configure(ConfigureBinding {
        target, expected: None, association: RelatedAssociation::Company, version,
        params: json!({"expected_bot_id":"user-bot-a","expected_workspace_id":"workspace-a","verification_canvas_id":"canvas-a"}),
        resources: BTreeMap::from([(ResourceName::parse("service").unwrap(), RuntimeResourceId::new(connection))]),
        run_as: None, message_trigger: None,
    }).await.unwrap();
    bindings
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    target
}

impl Verification {
    async fn new() -> Self {
        let server = mcp_server(Call::Json).await;
        server.set_tools(captured("tools")["tools"].clone());
        server.set_tool_result(
            "get_profile".to_owned().try_into().unwrap(),
            captured("profile"),
        );
        server.set_tool_result(
            "get_canvas".to_owned().try_into().unwrap(),
            captured("canvas"),
        );
        let mut fixture = Fixture::new().await;
        fixture.persistence = PostgresPersistence::with_credential_cipher(
            fixture.persistence.pool().clone(),
            CredentialCipher::for_test(),
        );
        let client = guarded_client(&server.url);
        let engine =
            WorkflowEngine::for_tests_with_mcp(fixture.persistence.clone(), client.clone());
        let connection = register(&fixture, &server.url).await;
        let version = publish(&fixture, &engine, connection.id).await;
        let target = bind(&fixture, &engine, connection.id, version).await;
        Self {
            fixture,
            server,
            client,
            engine,
            connection,
            version,
            target,
        }
    }

    async fn run(&self, input: Value) -> AppResult<RunView> {
        let company = self.target.company;
        let admitted = self
            .engine
            .services()
            .service()
            .admit(AdmitWorkflowRequest {
                history_session: None,
                company_id: company,
                actor: self.target.actor,
                association: RelatedAssociation::Company,
                trigger: TriggerRef::new(
                    company,
                    TriggerId::new(Uuid::new_v4()),
                    TriggerSource::Manual,
                )
                .unwrap(),
                correlation_id: CorrelationId::new(),
                binding_id: self.target.binding,
                idempotency_key: IdempotencyKey::parse(Uuid::new_v4().to_string()).unwrap(),
                input,
                rerun_of: None,
                delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
                expected_binding_revision: None,
                admission_claim: None,
            })
            .await?;
        let AdmissionResult::Created(run) = admitted else {
            panic!("new run")
        };
        let scope = RunReadScope {
            company,
            run,
            viewer: self.target.actor,
        };
        let shutdown = CancellationToken::new();
        let _cancel_on_drop = shutdown.clone().drop_guard();
        let workers = self.engine.spawn(&shutdown);
        let result = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let view = self.fixture.persistence.run_view(scope).await?.unwrap();
                if matches!(view.detail.summary.state.as_str(), "succeeded" | "failed") {
                    return Ok(view);
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
        self.client.shutdown().await.unwrap();
        result.expect("verification run must terminate")
    }
}

#[path = "doop_setup_scope_tests.rs"]
mod scope_tests;

#[tokio::test]
async fn doop_connection_verification_publishes_binds_and_runs_through_production_workers() {
    let setup = Box::pin(Verification::new()).await;
    let view = setup.run(json!({})).await.unwrap();
    assert_eq!(view.detail.summary.state, "succeeded");
    assert_eq!(setup.server.call_names(), ["get_profile", "get_canvas"]);
    assert_eq!(
        setup.server.arguments(),
        [json!({}), json!({"canvas_id":"canvas-a"})]
    );
    let verified = view
        .detail
        .executions
        .iter()
        .find(|step| step.step == "verified")
        .unwrap();
    let step = setup
        .fixture
        .persistence
        .step_view(
            RunReadScope {
                company: setup.target.company,
                run: view.detail.summary.run,
                viewer: setup.target.actor,
            },
            verified.execution,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        step.committed_output,
        Some(
            json!({"verified":true,"bot_id":"user-bot-a","workspace_id":"workspace-a","canvas_id":"canvas-a"})
        )
    );
    assert_eq!(setup.connection.slug.as_str(), "doop");
}

#[test]
fn doop_catalog_shipped_recipe_has_explicit_write_approval_and_only_six_editor_tools() {
    let recipe = recipe();
    assert_eq!(recipe.operations.len(), 9);
    assert_eq!(recipe.editing_agent.len(), 6);
    for op in recipe.operations {
        assert_eq!(op.approval_required, op.effect == ActionEffect::Write);
    }
    for excluded in [
        "create_canvas",
        "get_profile",
        "list_canvases",
        "delete_frame",
        "set_frame_html",
        "export_frame",
    ] {
        assert!(
            !recipe
                .editing_agent
                .iter()
                .any(|tool| tool.as_str() == excluded)
        );
    }
}

enum IdentityFault {
    WrongBot,
    MissingWorkspace,
    WrongWorkspace,
    WrongCanvas,
    WrongCanvasWorkspace,
    ForeignCanvas,
}

impl IdentityFault {
    fn response(&self) -> (&'static str, Value) {
        let mut profile = captured("profile");
        let mut canvas = captured("canvas");
        match self {
            Self::WrongBot => profile["structuredContent"]["id"] = json!("other-bot"),
            Self::MissingWorkspace => {
                profile["structuredContent"]
                    .as_object_mut()
                    .unwrap()
                    .remove("managed_workspace_id");
            }
            Self::WrongWorkspace => {
                profile["structuredContent"]["managed_workspace_id"] = json!("workspace-b")
            }
            Self::WrongCanvas => canvas["structuredContent"]["canvas"]["id"] = json!("canvas-b"),
            Self::WrongCanvasWorkspace => {
                canvas["structuredContent"]["workspace_id"] = json!("workspace-b")
            }
            Self::ForeignCanvas => canvas = captured("denied"),
        }
        if matches!(
            self,
            Self::WrongBot | Self::MissingWorkspace | Self::WrongWorkspace
        ) {
            ("get_profile", profile)
        } else {
            ("get_canvas", canvas)
        }
    }

    fn failed_step(&self) -> &'static str {
        match self {
            Self::WrongBot => "verify_bot",
            Self::MissingWorkspace => "verify_workspace_present",
            Self::WrongWorkspace => "verify_workspace",
            Self::WrongCanvas => "verify_canvas_id",
            Self::WrongCanvasWorkspace => "verify_canvas_workspace",
            Self::ForeignCanvas => "canvas",
        }
    }
}

#[tokio::test]
async fn doop_connection_verification_identity_failures_stop_at_distinct_steps_without_writes() {
    for fault in [
        IdentityFault::WrongBot,
        IdentityFault::MissingWorkspace,
        IdentityFault::WrongWorkspace,
        IdentityFault::WrongCanvas,
        IdentityFault::WrongCanvasWorkspace,
        IdentityFault::ForeignCanvas,
    ] {
        let setup = Box::pin(Verification::new()).await;
        let (tool, response) = fault.response();
        setup
            .server
            .set_tool_result(tool.to_owned().try_into().unwrap(), response);
        let view = setup.run(json!({})).await.unwrap();
        assert_eq!(
            view.detail.summary.state,
            "failed",
            "{}",
            fault.failed_step()
        );
        assert_eq!(
            view.detail.executions.first().unwrap().step,
            fault.failed_step()
        );
        assert!(
            !view
                .detail
                .executions
                .iter()
                .any(|step| step.step == "verified")
        );
        assert_eq!(
            setup.server.calls(),
            if tool == "get_profile" { 1 } else { 2 }
        );
        assert!(
            setup
                .server
                .call_names()
                .iter()
                .all(|name| ["get_profile", "get_canvas"].contains(&name.as_str()))
        );
    }
}

#[tokio::test]
async fn doop_connection_verification_user_input_cannot_override_trusted_binding() {
    let setup = Box::pin(Verification::new()).await;
    assert!(
        setup
            .run(json!({"expected_workspace_id":"workspace-b","verification_canvas_id":"canvas-b"}))
            .await
            .is_err()
    );
    assert_eq!(setup.server.calls(), 0);
    setup.client.shutdown().await.unwrap();
}

#[path = "doop_document_tests.rs"]
mod document_tests;
