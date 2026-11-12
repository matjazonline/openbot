use super::*;

pub(super) async fn new_agent(
    f: &AdmissionFixture,
    bindings: Vec<AgentToolBinding>,
) -> crate::entities::agent::Agent {
    AgentPersistence::create(
        f.persistence(),
        f.binding.target.company.as_uuid(),
        AgentWrite {
            name: "Reusable agent".into(),
            slug: format!("agent-{}", Uuid::new_v4().simple()),
            provider: Some("openai".into()),
            model: Some("fixture-model".into()),
            system_prompt: Some("Retain the configured facts.".into()),
            tool_bindings: Some(bindings),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

pub(super) async fn bindings(f: &AdmissionFixture, agent: Uuid) -> Vec<AgentToolBinding> {
    crate::adapters::persistence::agent_capability::bindings_on(
        &mut f.persistence().pool().acquire().await.unwrap(),
        Some(f.binding.target.company.as_uuid()),
        agent,
    )
    .await
    .unwrap()
}

pub(super) fn definition(
    f: &AdmissionFixture,
    agent: Uuid,
    bindings: Vec<AgentToolBinding>,
) -> ResolvedAgentDefinition {
    let company = f.binding.target.company;
    ResolvedAgentDefinition {
        native_permissions: Default::default(),
        company_id: company,
        agent_id: AgentId::new(agent),
        key: AgentKey::parse("assistant").unwrap(),
        instructions: "Retain this immutable instruction.".into(),
        model: Some(ModelSettings {
            provider: TypeName::parse("openai").unwrap(),
            model: "fixture-model".into(),
            max_output_tokens: 1000,
        }),
        tools: bindings
            .into_iter()
            .map(|binding| {
                let snapshot = match binding.source {
                    AgentBindingSource::Builtin {
                        tool: AgentBuiltinTool::SearchKnowledge,
                    } => {
                        crate::application::workflow::knowledge::builtin_knowledge_snapshot(company)
                            .unwrap()
                    }
                    AgentBindingSource::Builtin {
                        tool: AgentBuiltinTool::MessageReply,
                    } => crate::application::workflow::message::builtin_message_snapshot(
                        company,
                        crate::application::workflow::message::MessageKind::Reply,
                    )
                    .unwrap(),
                    _ => panic!("Storage revision fixture expects an explicitly trusted built-in"),
                };
                ResolvedAgentTool {
                    binding,
                    input_schema: snapshot.contract.input_schema,
                    output_schema: snapshot.contract.output_schema,
                    policy: snapshot.policy,
                    approval_required: false,
                }
            })
            .collect(),
        skills: Vec::new(),
    }
}

pub(super) async fn insert_revision(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    value: &ResolvedAgentDefinition,
    revision: Uuid,
) {
    let company = value.company_id.as_uuid();
    let agent = value.agent_id.as_uuid();
    let content = String::from_utf8(value.canonical_content().unwrap()).unwrap();
    sqlx::query("INSERT INTO agent_revisions(company_id,agent_id,id,draft_revision,content,digest) VALUES($1,$2,$3,1,$4,$5)")
        .bind(company).bind(agent).bind(revision).bind(content).bind(value.content_hash().unwrap().to_vec())
        .execute(&mut **tx).await.unwrap();
    for (position, tool) in value.tools.iter().enumerate() {
        sqlx::query("INSERT INTO agent_revision_bindings(company_id,agent_id,revision_id,binding_id,position) VALUES($1,$2,$3,$4,$5)")
            .bind(company).bind(agent).bind(revision).bind(tool.binding.id.as_uuid()).bind(position as i16)
            .execute(&mut **tx).await.unwrap();
    }
    for (position, skill) in value.skills.iter().enumerate() {
        sqlx::query("INSERT INTO agent_revision_skills(company_id,agent_id,revision_id,assignment_id,skill_id,position) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(company).bind(agent).bind(revision).bind(skill.assignment_id.as_uuid()).bind(skill.skill.skill_id)
            .bind(position as i16).execute(&mut **tx).await.unwrap();
    }
}

pub(super) async fn save_revision(f: &AdmissionFixture, value: &ResolvedAgentDefinition) -> Uuid {
    let revision = Uuid::new_v4();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    insert_revision(&mut tx, value, revision).await;
    tx.commit().await.unwrap();
    revision
}

pub(super) async fn save_manifest(f: &AdmissionFixture, value: &DependencyManifest) -> Uuid {
    let manifest = Uuid::new_v4();
    let company = value.company_id.as_uuid();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO agent_dependency_manifests(company_id,id,root_run_id,content,digest) VALUES($1,$2,$3,$4,$5)")
        .bind(company).bind(manifest).bind(value.root_run_id.as_uuid())
        .bind(String::from_utf8(value.canonical_content().unwrap()).unwrap()).bind(value.content_hash().unwrap().to_vec())
        .execute(&mut *tx).await.unwrap();
    for selected in &value.agents {
        sqlx::query("INSERT INTO agent_manifest_revisions(company_id,manifest_id,agent_id,revision_id) VALUES($1,$2,$3,$4)")
            .bind(company).bind(manifest).bind(selected.agent_id.as_uuid()).bind(selected.revision_id.as_uuid())
            .execute(&mut *tx).await.unwrap();
    }
    for selected in &value.workflows {
        sqlx::query("INSERT INTO agent_manifest_workflows(company_id,manifest_id,workflow_id,version_id) VALUES($1,$2,$3,$4)")
            .bind(company).bind(manifest).bind(selected.workflow_id.as_uuid()).bind(selected.version_id.as_uuid())
            .execute(&mut *tx).await.unwrap();
    }
    sqlx::query("INSERT INTO run_dependency_manifests(company_id,run_id,root_run_id,manifest_id) VALUES($1,$2,$2,$3)")
        .bind(company).bind(value.root_run_id.as_uuid()).bind(manifest).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    manifest
}

pub(super) async fn manifest_for(
    f: &AdmissionFixture,
    run: RunId,
    agent: Uuid,
    revision: Uuid,
) -> DependencyManifest {
    #[derive(sqlx::FromRow)]
    struct SelectedVersion {
        workflow_id: Uuid,
        version_id: Uuid,
    }
    let selected: SelectedVersion = sqlx::query_as(
        "SELECT workflow_id,version_id FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    DependencyManifest {
        company_id: f.binding.target.company,
        root_run_id: run,
        agents: vec![AgentRevisionSelection {
            agent_id: AgentId::new(agent),
            revision_id: AgentRevisionId::new(revision),
        }],
        workflows: vec![WorkflowVersionSelection {
            workflow_id: WorkflowId::new(selected.workflow_id),
            version_id: VersionId::new(selected.version_id),
        }],
    }
}

pub(super) fn native_rejection(error: sqlx::Error) {
    let database = error
        .as_database_error()
        .expect("native database rejection");
    assert!(
        matches!(
            database.code().as_deref(),
            Some("23503" | "23514" | "23505")
        ),
        "{error}"
    );
}

pub(super) async fn add_skill(f: &AdmissionFixture) -> crate::entities::skill::Skill {
    use crate::use_cases::skill::{SkillManagementPersistence, SkillWrite};
    f.persistence()
        .create_company(
            f.binding.target.company.as_uuid(),
            SkillWrite {
                files: None,
                slug: format!("skill-{}", Uuid::new_v4().simple()),
                name: "Instructions".into(),
                description: "Test instructions".into(),
                instructions: "Original shared skill text.".into(),
                required_tools: Some(Vec::new()),
                created_by: None,
            },
        )
        .await
        .unwrap()
}

pub(super) async fn attach_run(
    f: &AdmissionFixture,
    run: RunId,
    root: RunId,
    manifest: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO run_dependency_manifests(company_id,run_id,root_run_id,manifest_id) VALUES($1,$2,$3,$4)")
        .bind(f.binding.target.company.as_uuid()).bind(run.as_uuid()).bind(root.as_uuid()).bind(manifest)
        .execute(f.persistence().pool()).await.map(|_| ())
}

pub(super) async fn native_http(f: &AdmissionFixture, connection: Uuid) {
    sqlx::query("INSERT INTO workflow_http_connections(company_id,id,origin,bearer) VALUES($1,$2,'https://fixture.test',false)")
        .bind(f.binding.target.company.as_uuid()).bind(connection).execute(f.persistence().pool()).await.unwrap();
    sqlx::query("INSERT INTO workflow_http_operations(company_id,connection_id,name,snapshot,approval_required) VALUES($1,$2,'lookup',$3,false)")
        .bind(f.binding.target.company.as_uuid()).bind(connection).bind(operation_snapshot(f)).execute(f.persistence().pool()).await.unwrap();
}

pub(super) async fn native_mcp(f: &AdmissionFixture, connection: Uuid) -> Uuid {
    let version = Uuid::new_v4();
    let discovery = vec![crate::entities::mcp::McpDiscoveredTool {
        name: "lookup".to_owned().try_into().unwrap(),
        description: "Look up a record".into(),
        input_schema: json!({"type":"object"}),
        output_schema: Some(json!({"type":"object"})),
    }];
    sqlx::query("INSERT INTO company_mcp_connections(company_id,id,slug,endpoint_url,enabled,auth_type,discovery_json) VALUES($1,$2,'crm','https://fixture.test/rpc',true,'none',$3)")
        .bind(f.binding.target.company.as_uuid()).bind(connection).bind(serde_json::to_value(discovery).unwrap()).execute(f.persistence().pool()).await.unwrap();
    sqlx::query("INSERT INTO company_mcp_tool_grants(company_id,connection_id,tool_name) VALUES($1,$2,'lookup')")
        .bind(f.binding.target.company.as_uuid()).bind(connection).execute(f.persistence().pool()).await.unwrap();
    sqlx::query("INSERT INTO workflow_mcp_operations(company_id,connection_id,name,snapshot,approval_required,version) VALUES($1,$2,'lookup',$3,false,$4)")
        .bind(f.binding.target.company.as_uuid()).bind(connection).bind(operation_snapshot(f)).bind(version).execute(f.persistence().pool()).await.unwrap();
    version
}

pub(super) fn operation_snapshot(f: &AdmissionFixture) -> Value {
    json!({"company_id":f.binding.target.company,"contract":{"name":"lookup","connection":null,
        "input_schema":{"type":"object"},"output_schema":{"type":"object"}},
        "policy":{"capability":"lookup","policy_revision":1,"effect":"read","recovery":"reconcile"}})
}

pub(super) async fn active_grant(f: &AdmissionFixture, binding: AgentBindingId) -> bool {
    sqlx::query_scalar(
        "SELECT revoked_at IS NULL FROM agent_binding_grants WHERE company_id=$1 AND binding_id=$2",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(binding.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap()
}

pub(super) fn mcp_write(
    current: &crate::entities::mcp::CompanyMcpConnection,
) -> crate::use_cases::mcp::McpConnectionWrite {
    crate::use_cases::mcp::McpConnectionWrite {
        slug: current.slug.clone(),
        endpoint: current.endpoint.clone(),
        enabled: current.enabled,
        auth: current.auth.clone(),
        discovered_tools: current.discovered_tools.clone(),
        tool_grants: current.tool_grants.clone(),
    }
}
