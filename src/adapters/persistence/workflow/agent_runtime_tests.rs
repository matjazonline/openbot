//! Native `agent.run` end to end: real Rig protocol against a scripted provider, saved
//! tool calls through ActionService, checkpoint replay and current revocation.
use super::*;
use crate::adapters::{
    harness::rig::{WorkflowAgentRuntime, providers::ProviderRegistry},
    mcp::{HttpMcpClient, policy::EndpointPolicy},
    persistence::workflow::{
        agent_publication::AgentPublicationDirectory,
        tests::approval_test_support::{composer, notices},
    },
    workflow_agent::AgentActionTools,
    workflow_mcp::McpRuntime,
    workflow_message::MessageRuntime,
};
use crate::application::{
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
        company::{CompanyModelConnectionWrite, CompanyPersistence},
    },
    workflow::{
        lifecycle::PublicationDependencies, publication::AgentSnapshot,
        templates::CompanyWorkflowDraft, worker::WorkflowHandler,
    },
};
use crate::services::test_support::{
    LlmTurn, SCRIPTED_MODEL, SCRIPTED_PROVIDER, register_scripted_agent_base_url, scripted_llm,
};

const HTTP_OK: &str = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 11\r\nconnection: close\r\n\r\n{\"ok\":true}";

/// A caller-supplied (forged) agent snapshot: the SQL owner must refuse it at publish.
struct ForgedAgents {
    http: http::HttpPublicationDirectory,
    agent: AgentSnapshot,
}
#[async_trait]
impl PublicationDirectory for ForgedAgents {
    async fn capture(
        &self,
        actor: WorkflowActor,
        draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        let mut captured = self.http.capture(actor, draft).await?;
        captured.snapshots.agents.push(self.agent.clone());
        Ok(captured)
    }
}

struct AgentSetup {
    fixture: Fixture,
    connection: HttpConnection,
    agent: Uuid,
    claim: ClaimedWorkflow,
}

fn http_directory(
    p: &PostgresPersistence,
    connection: &HttpConnection,
) -> http::HttpPublicationDirectory {
    http::HttpPublicationDirectory {
        persistence: p.clone(),
        selections: vec![http::HttpPublicationSelection {
            connection: connection.id,
            slot: None,
            operation: TypeName::parse("registered.request").unwrap(),
        }],
    }
}

async fn agent_setup(origin: &str) -> AgentSetup {
    agent_setup_with(origin, None).await
}

async fn agent_setup_with(origin: &str, skill: Option<Value>) -> AgentSetup {
    agent_setup_full(origin, skill, false).await
}

async fn agent_setup_full(origin: &str, skill: Option<Value>, protected: bool) -> AgentSetup {
    agent_setup_source(origin, skill, protected, |_, _| {}).await
}

/// `skill` (instructions JSON) is created and linked to the agent before publication;
/// `protected` registers the tool as approval-required and gives the step an approval policy;
/// `customize` edits the source (given the owner actor id) before publication.
async fn agent_setup_source(
    origin: &str,
    skill: Option<Value>,
    protected: bool,
    customize: impl FnOnce(&mut Value, Uuid),
) -> AgentSetup {
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let connection = if protected {
        Box::pin(protected_connection(&fixture, origin)).await
    } else {
        Box::pin(configured(&fixture, origin, Entrance::Tool)).await
    };
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let agent = AgentPersistence::create(
        p,
        company.as_uuid(),
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some("Answer with the requested JSON string.".into()),
            workflow_tools: Some(vec![TypeName::parse("registered.request").unwrap()]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    CompanyPersistence::replace_model_connections_for_user(
        p,
        fixture.target.actor.user_id(),
        company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    if let Some(instructions) = skill {
        let skill_id = Uuid::new_v4();
        sqlx::query("INSERT INTO skills(id,company_id,slug,name,description,trigger,instructions,created_by,required_tools) VALUES($1,$2,'support','Support','Support policy','Support questions',$3,'{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Fixture\"}','{registered.request}')")
            .bind(skill_id).bind(company.as_uuid()).bind(instructions).execute(p.pool()).await.unwrap();
        sqlx::query(
            "INSERT INTO agent_skills(company_id,agent_id,skill_id,position) VALUES($1,$2,$3,0)",
        )
        .bind(company.as_uuid())
        .bind(agent.id)
        .bind(skill_id)
        .execute(p.pool())
        .await
        .unwrap();
    }
    let directory = AgentPublicationDirectory {
        persistence: p.clone(),
        inner: http_directory(p, &connection),
    };
    let definitions =
        DefinitionService::new(Preflight, p.clone(), WorkflowSourceDecoder, directory);
    let mut draft = fixture.save(None);
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    if protected {
        source["steps"]["start"]["with"]["approval"] = json!({"literal":{
            "reviewers":[{"kind":"user","id":fixture.target.actor.user_id()}],
            "deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()}});
    }
    customize(&mut source, fixture.target.actor.user_id());
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let publication = fixture.publish();
    definitions.publish(publication.clone()).await.unwrap();
    let target = Box::pin(bound(
        &fixture,
        &connection,
        publication.version,
        Entrance::Tool,
    ))
    .await;
    let claim = Box::pin(claimed(&fixture, target)).await;
    AgentSetup {
        fixture,
        connection,
        agent: agent.id,
        claim,
    }
}

fn agent_runtime(
    setup: &AgentSetup,
) -> WorkflowAgentRuntime<PostgresPersistence, AgentActionTools> {
    let p = &setup.fixture.persistence;
    let http = HttpRuntime::with_approvals(
        p.clone(),
        HttpDestinationPolicy::new([setup.connection.origin.as_str().to_owned()]).unwrap(),
        lease(),
        Some(notices(p)),
    );
    let mcp = McpRuntime::new(
        p.clone(),
        Arc::new(HttpMcpClient::new(EndpointPolicy::default())),
        lease(),
    );
    let message = MessageRuntime::new(p.clone(), composer(p), "agents.test".into(), lease());
    WorkflowAgentRuntime::new(
        ProviderRegistry::standard().unwrap(),
        p.clone(),
        AgentActionTools::new(Arc::new(http), Arc::new(mcp), Arc::new(message)),
        Some(notices(p)),
        lease(),
    )
}

async fn usage_rows(p: &PostgresPersistence, run: RunId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM model_usage_calls WHERE run_id=$1")
        .bind(run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn workflow_agent_run_saves_tool_call_dispatches_once_and_replays_final_output() {
    let (origin, server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call(
            "registered__request",
            json!({"method":"POST","path":"/messages","body":{"message":"hello"}}),
        ),
        LlmTurn::text("\"done\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let kind = WorkflowStepKind("agent.run".into());
    let outcome = Box::pin(runtime.execute(&kind, &setup.claim)).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("done"));
    let request = server.await.unwrap();
    assert!(request.starts_with("POST /messages"), "{request}");
    let requests = llm.observed();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0]["messages"]
            .to_string()
            .contains("untrusted input"),
        "context is delimited as data"
    );
    let p = &setup.fixture.persistence;
    let run = setup.claim.fence.scope.run;
    assert_eq!(usage_rows(p, run).await, 2);
    // Replay of the same attempt returns the saved output; no provider or effect call.
    let replay = Box::pin(runtime.execute(&kind, &setup.claim)).await;
    assert_eq!(replay.completed_result().unwrap(), json!("done"));
    assert_eq!(usage_rows(p, run).await, 2);
    assert_eq!(llm.finish().await.unwrap(), 2);
    let dispatches: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_dispatches")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(dispatches, 1);
}

#[tokio::test]
async fn workflow_agent_run_refuses_revoked_model_connection_before_provider_entry() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let llm = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let p = &setup.fixture.persistence;
    sqlx::query("UPDATE company_model_connections SET models='{other-model}' WHERE company_id=$1")
        .bind(setup.fixture.target.company.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    let outcome = Box::pin(
        agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim),
    )
    .await;
    let failure = outcome.completed_result().unwrap_err();
    assert_eq!(failure.failure.code().as_str(), "agent.rejected");
    assert_eq!(usage_rows(p, setup.claim.fence.scope.run).await, 0);
    assert_eq!(llm.finish().await.unwrap(), 0);
}

#[tokio::test]
async fn workflow_agent_publication_refuses_forged_snapshot_and_keeps_frozen_version() {
    let (origin, server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let p = &setup.fixture.persistence;
    // A caller-supplied snapshot granting an extra tool never becomes a grant.
    let mut forged = setup_agent_snapshot(&setup).await;
    forged.tools.push(TypeName::parse("message.send").unwrap());
    let definitions = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        ForgedAgents {
            http: http_directory(p, &setup.connection),
            agent: forged,
        },
    );
    let mut request = setup.fixture.publish();
    request.expected = DraftRevision::new(1).unwrap();
    request.key = IdempotencyKey::parse("forged-agent").unwrap();
    assert!(definitions.publish(request).await.is_err());
    // Editing the agent after publication neither blocks nor changes the frozen version.
    sqlx::query("UPDATE agents SET system_prompt='Edited later', workflow_tools=array_append(workflow_tools,'message.send') WHERE id=$1")
        .bind(setup.agent)
        .execute(p.pool())
        .await
        .unwrap();
    let mut llm = scripted_llm(vec![LlmTurn::text("\"frozen\"")]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let outcome = Box::pin(
        agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim),
    )
    .await;
    assert_eq!(outcome.completed_result().unwrap(), json!("frozen"));
    let requests = llm.observed();
    assert!(
        requests[0]["messages"]
            .to_string()
            .contains("Answer with the requested JSON string.")
    );
    assert!(!requests[0]["messages"].to_string().contains("Edited later"));
    assert!(!requests[0]["tools"].to_string().contains("message__send"));
    assert_eq!(llm.finish().await.unwrap(), 1);
    drop(server);
}

#[tokio::test]
async fn workflow_agent_run_refuses_removed_tool_grant_before_provider_entry() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let llm = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let p = &setup.fixture.persistence;
    sqlx::query("UPDATE agents SET workflow_tools='{}' WHERE id=$1")
        .bind(setup.agent)
        .execute(p.pool())
        .await
        .unwrap();
    let outcome = Box::pin(
        agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim),
    )
    .await;
    assert_eq!(
        outcome
            .completed_result()
            .unwrap_err()
            .failure
            .code()
            .as_str(),
        "agent.rejected"
    );
    assert_eq!(usage_rows(p, setup.claim.fence.scope.run).await, 0);
    assert_eq!(llm.finish().await.unwrap(), 0);
}

/// The snapshot the production capture derives for the fixture agent.
async fn setup_agent_snapshot(setup: &AgentSetup) -> AgentSnapshot {
    let bundle_json: Vec<u8> =
        sqlx::query_scalar("SELECT bundle FROM workflow_versions WHERE company_id=$1 LIMIT 1")
            .bind(setup.fixture.target.company.as_uuid())
            .fetch_one(setup.fixture.persistence.pool())
            .await
            .unwrap();
    let bundle = crate::application::workflow::publication::restore_bundle(
        &bundle_json,
        &WorkflowSourceDecoder,
    )
    .unwrap();
    bundle.snapshots().agents[0].clone()
}

/// 06-D5 F011/F014: unrelated saves keep workflow grants and required tools; Some replaces.
#[tokio::test]
async fn workflow_agent_capability_writers_preserve_unsupplied_lists() {
    use crate::application::use_cases::skill::{
        SkillManagementPersistence as SkillPersistence, SkillWrite,
    };
    use crate::entities::skill::SkillInstruction;
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company.as_uuid();
    let stored = |id: Uuid| async move {
        sqlx::query_scalar::<_, Vec<String>>("SELECT workflow_tools FROM agents WHERE id=$1")
            .bind(id)
            .fetch_one(p.pool())
            .await
            .unwrap()
    };
    let unrelated = AgentWrite {
        name: "Assistant renamed".into(),
        slug: "assistant".into(),
        provider: Some(SCRIPTED_PROVIDER.into()),
        model: Some(SCRIPTED_MODEL.into()),
        ..Default::default()
    };
    AgentPersistence::update(p, setup.agent, unrelated.clone())
        .await
        .unwrap();
    assert_eq!(
        stored(setup.agent).await,
        vec!["registered.request".to_string()]
    );
    let mut cleared = unrelated;
    cleared.workflow_tools = Some(vec![]);
    AgentPersistence::update(p, setup.agent, cleared)
        .await
        .unwrap();
    assert!(stored(setup.agent).await.is_empty());
    let write = |required: Option<Vec<TypeName>>| SkillWrite {
        slug: "support".into(),
        name: "Support".into(),
        description: "Support policy".into(),
        trigger: "Support questions".into(),
        instructions: vec![SkillInstruction::Prompt {
            text: "Be precise.".into(),
        }],
        required_tools: required,
        created_by: None,
    };
    let skill = SkillPersistence::create_company(
        p,
        company,
        write(Some(vec![TypeName::parse("registered.request").unwrap()])),
    )
    .await
    .unwrap();
    SkillPersistence::update_company(p, company, skill.id, write(None))
        .await
        .unwrap();
    let required: Vec<String> = sqlx::query_scalar("SELECT required_tools FROM skills WHERE id=$1")
        .bind(skill.id)
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(required, vec!["registered.request".to_string()]);
}

/// 06-D5 F014: the SQL shape check accepts exactly the Rust `TypeName` grammar.
#[tokio::test]
async fn workflow_agent_capability_sql_shape_matches_type_names() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let pool = setup.fixture.persistence.pool();
    let long = format!("a{}", "b".repeat(127));
    let too_long = format!("a{}", "b".repeat(128));
    for name in [
        "a",
        "a.b",
        "_x-1.y_2",
        "crm.lookup",
        "1a",
        "a..b",
        "a.",
        ".a",
        "a b",
        "a.-b",
        "ä",
        "",
        long.as_str(),
        too_long.as_str(),
    ] {
        let sql: bool =
            sqlx::query_scalar("SELECT public.valid_workflow_type_names(ARRAY[$1::text])")
                .bind(name)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(sql, TypeName::parse(name).is_ok(), "{name:?}");
    }
    let duplicate: bool =
        sqlx::query_scalar("SELECT public.valid_workflow_type_names(ARRAY['a','a'])")
            .fetch_one(pool)
            .await
            .unwrap();
    assert!(!duplicate);
    let many: Vec<String> = (0..33).map(|i| format!("t{i}")).collect();
    let bounded: bool = sqlx::query_scalar("SELECT public.valid_workflow_type_names($1)")
        .bind(&many)
        .fetch_one(pool)
        .await
        .unwrap();
    assert!(!bounded);
}

/// 06-D5 F014: a revocation committed while admission waits on the agent row is observed;
/// the provider is never entered.
#[tokio::test]
async fn workflow_agent_run_competing_revocation_refuses_admission() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let llm = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let p = &setup.fixture.persistence;
    let mut revoke = p.pool().begin().await.unwrap();
    sqlx::query("UPDATE agents SET workflow_tools='{}' WHERE id=$1")
        .bind(setup.agent)
        .execute(&mut *revoke)
        .await
        .unwrap();
    let runtime = agent_runtime(&setup);
    let kind = WorkflowStepKind("agent.run".into());
    let run = runtime.execute(&kind, &setup.claim);
    let commit = async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        revoke.commit().await.unwrap();
    };
    let (outcome, ()) = tokio::join!(Box::pin(run), commit);
    assert_eq!(
        outcome
            .completed_result()
            .unwrap_err()
            .failure
            .code()
            .as_str(),
        "agent.rejected"
    );
    assert_eq!(usage_rows(p, setup.claim.fence.scope.run).await, 0);
    assert_eq!(llm.finish().await.unwrap(), 0);
}

/// 06-D5 F014: capture refuses agents a workflow cannot run.
#[tokio::test]
async fn workflow_agent_capture_refuses_unrunnable_agents() {
    let (origin, _server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let p = &setup.fixture.persistence;
    let publish = |key: &'static str| {
        let fixture = &setup.fixture;
        let connection = &setup.connection;
        async move {
            let definitions = DefinitionService::new(
                Preflight,
                p.clone(),
                WorkflowSourceDecoder,
                AgentPublicationDirectory {
                    persistence: p.clone(),
                    inner: http_directory(p, connection),
                },
            );
            let mut request = fixture.publish();
            request.expected = DraftRevision::new(1).unwrap();
            request.key = IdempotencyKey::parse(key).unwrap();
            definitions.publish(request).await
        }
    };
    for (key, change) in [
        ("no-model", "UPDATE agents SET model=NULL WHERE id=$1"),
        (
            "unregistered",
            "UPDATE agents SET model='gpt-4o-mini', workflow_tools='{missing.tool}' WHERE id=$1",
        ),
    ] {
        sqlx::query(change)
            .bind(setup.agent)
            .execute(p.pool())
            .await
            .unwrap();
        assert!(publish(key).await.is_err(), "{key}");
    }
}

/// 06-F017: a slot-bound step HTTP selection is not an agent tool. Granting the same
/// operation to the agent refuses publication with an actionable error instead of
/// preparing the grant as an MCP slot.
#[tokio::test]
async fn workflow_agent_capture_refuses_slot_bound_http_grant() {
    let (origin, _server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let p = &setup.fixture.persistence;
    let definitions = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        AgentPublicationDirectory {
            persistence: p.clone(),
            inner: http::HttpPublicationDirectory {
                persistence: p.clone(),
                selections: vec![http::HttpPublicationSelection {
                    connection: setup.connection.id,
                    slot: Some(ResourceName::parse("service").unwrap()),
                    operation: TypeName::parse("registered.request").unwrap(),
                }],
            },
        },
    );
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["resources"] = json!([{"slot":"service","kind":"http"}]);
    let mut draft = setup.fixture.save(Some(DraftRevision::new(1).unwrap()));
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let mut request = setup.fixture.publish();
    request.expected = DraftRevision::new(2).unwrap();
    request.key = IdempotencyKey::parse("slot-bound-grant").unwrap();
    let Err(crate::application::workflow::lifecycle::LifecycleError::Application(
        AppError::BadRequest(message),
    )) = definitions.publish(request).await
    else {
        panic!("a slot-bound HTTP grant must refuse publication");
    };
    assert!(
        message.contains("conflicts with another tool of the same name"),
        "{message}"
    );
}

/// 06-D5: unlinking a frozen skill revokes it before the next model admission.
#[tokio::test]
async fn workflow_agent_run_refuses_unlinked_skill_before_provider_entry() {
    let prompt = json!([{"kind":"prompt","text":"Use the registered request for facts."}]);
    let setup = Box::pin(agent_setup_with("http://127.0.0.1:1", Some(prompt))).await;
    let llm = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let p = &setup.fixture.persistence;
    sqlx::query("DELETE FROM agent_skills WHERE agent_id=$1")
        .bind(setup.agent)
        .execute(p.pool())
        .await
        .unwrap();
    let outcome = Box::pin(
        agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim),
    )
    .await;
    assert_eq!(
        outcome
            .completed_result()
            .unwrap_err()
            .failure
            .code()
            .as_str(),
        "agent.rejected"
    );
    assert_eq!(llm.finish().await.unwrap(), 0);
}

/// 06-D5: a skill with legacy executable tool steps cannot be used by a workflow agent.
#[tokio::test]
async fn workflow_agent_capture_refuses_legacy_tool_step_skill() {
    let steps = json!([{"kind":"tool","tool":"datetime"}]);
    let result = tokio::spawn(async move {
        Box::pin(agent_setup_with("http://127.0.0.1:1", Some(steps))).await;
    })
    .await;
    assert!(
        result.is_err(),
        "publication of a legacy tool-step skill must fail"
    );
}

/// 06-D5 F014: an agent edit committed while publish waits on the agent row fails publish.
#[tokio::test]
async fn workflow_agent_publish_competing_agent_edit_fails_publication() {
    let (origin, _server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let p = &setup.fixture.persistence;
    let definitions = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        AgentPublicationDirectory {
            persistence: p.clone(),
            inner: http_directory(p, &setup.connection),
        },
    );
    let mut request = setup.fixture.publish();
    request.expected = DraftRevision::new(1).unwrap();
    request.key = IdempotencyKey::parse("racing-edit").unwrap();
    // Capture reads the current agent; the edit then holds the row until publish waits on it.
    let mut edit = p.pool().begin().await.unwrap();
    let publish = async {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        definitions.publish(request).await
    };
    let commit = async {
        sqlx::query("UPDATE agents SET system_prompt='Edited concurrently' WHERE id=$1")
            .bind(setup.agent)
            .execute(&mut *edit)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        edit.commit().await.unwrap();
    };
    let (published, ()) = tokio::join!(Box::pin(publish), commit);
    assert!(
        published.is_err(),
        "publish must not freeze a concurrently edited agent"
    );
}

async fn protected_connection(fixture: &Fixture, origin: &str) -> HttpConnection {
    let p = &fixture.persistence;
    let (company, actor) = (fixture.target.company, fixture.target.actor);
    let connection = p
        .create_http(
            company,
            actor,
            HttpConnectionWrite {
                origin: HttpOrigin::parse(origin).unwrap(),
                bearer: true,
            },
        )
        .await
        .unwrap();
    let connection = p
        .replace_http_secret(
            company,
            actor,
            connection.id,
            connection.revision,
            Some(SecretString::from("fixture-http-secret")),
        )
        .await
        .unwrap();
    let mut op = operation(company, "registered.request");
    op.approval_required = true;
    p.register_http_operation(company, actor, connection.id, connection.revision, op)
        .await
        .unwrap()
}

/// Approves the one open decision of the execution (decision views carry no command key reuse).
async fn approve_open(setup: &AgentSetup, key: &str) {
    use crate::application::workflow::decisions::{
        DecisionCommandKey, DecisionId, SubmitDecision, WorkflowDecisions,
    };
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    let id: Uuid = sqlx::query_scalar(
        "SELECT decision.id FROM workflow_decisions AS decision JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id WHERE decision.company_id=$1 AND decision.execution_id=$2 AND wait.state='waiting'",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let actor = setup.fixture.target.actor;
    let view = p
        .read_decision(scope.company, actor, DecisionId(id))
        .await
        .unwrap();
    p.submit_decision(SubmitDecision {
        company: scope.company,
        actor,
        decision: view.decision,
        command_key: DecisionCommandKey::parse(key).unwrap(),
        expected_revision: view.revision,
        expected_artifact: view.artifact_digest,
        choice: "approve".into(),
        feedback: String::new(),
        data: json!({}),
    })
    .await
    .unwrap();
}

async fn reclaim(setup: &AgentSetup) -> ClaimedWorkflow {
    setup
        .fixture
        .persistence
        .claim_io(
            setup.claim.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .expect("the continued agent job is claimable")
}

/// Serves `count` sequential HTTP requests, each answered with `HTTP_OK`.
async fn servers(count: usize) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut seen = Vec::new();
        for _ in 0..count {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 8192];
            let read = socket.read(&mut bytes).await.unwrap();
            socket.write_all(HTTP_OK.as_bytes()).await.unwrap();
            seen.push(String::from_utf8(bytes[..read].to_vec()).unwrap());
        }
        seen
    });
    (origin, task)
}

#[path = "agent_driver_tests.rs"]
mod driver;

#[path = "agent_recovery_tests.rs"]
mod recovery;

#[path = "agent_model_step_tests.rs"]
mod model_steps;
