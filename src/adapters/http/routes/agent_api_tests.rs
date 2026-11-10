use super::*;
use crate::adapters::persistence::{PostgresPersistence, test_support::test_pool};
use crate::use_cases::{
    company::{CompanyPersistence, CompanyWrite},
    user::UserPersistence,
};
use serde_json::{Value, json};

async fn response_json(response: impl IntoResponse) -> Value {
    let response = response.into_response();
    assert!(response.status().is_success());
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

struct Fixture {
    persistence: Arc<PostgresPersistence>,
    agents: Arc<AgentUseCases>,
    skills: Arc<SkillUseCases>,
    user: Uuid,
    company: Uuid,
}
impl Fixture {
    async fn new() -> Option<Self> {
        let persistence = Arc::new(PostgresPersistence::new(test_pool().await?));
        let suffix = Uuid::new_v4().simple().to_string();
        let user = persistence
            .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
            .await
            .unwrap()
            .id;
        let company = CompanyPersistence::create(
            persistence.as_ref(),
            user,
            CompanyWrite {
                name: "Runtime API".into(),
                slug: suffix,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        // Every company agent is created with a desk, which answers on a keyed model.
        CompanyPersistence::replace_model_connections_for_user(
            persistence.as_ref(),
            user,
            company,
            vec![crate::use_cases::company::CompanyModelConnectionWrite {
                provider: "openai".into(),
                api_key: Some("test-key".into()),
                models: vec!["gpt-4o-mini".into()],
                is_default: true,
            }],
        )
        .await
        .unwrap();
        let skills = Arc::new(SkillUseCases::new(
            persistence.clone(),
            persistence.clone(),
            persistence.clone(),
        ));
        let agents = Arc::new(
            AgentUseCases::new(
                persistence.clone(),
                persistence.clone(),
                persistence.clone(),
            )
            .with_company_agents(
                crate::adapters::persistence::test_support::company_agents(persistence.as_ref()),
            ),
        );
        Some(Self {
            persistence,
            agents,
            skills,
            user,
            company,
        })
    }
    async fn create(&self, body: Value) -> Value {
        response_json(
            create_agent_json(
                State(self.agents.clone()),
                State(
                    crate::adapters::persistence::test_support::department_use_cases(
                        &self.persistence,
                    ),
                ),
                AuthenticatedUser { id: self.user },
                Path(self.company),
                Json(serde_json::from_value(body).unwrap()),
            )
            .await
            .unwrap(),
        )
        .await
    }
    async fn update(&self, id: Uuid, patch: Value) -> AppResult<Value> {
        let mut body =
            json!({"name":"Structured", "slug":"structured", "description":"Updated description"});
        body.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        let current = self
            .agents
            .get_company_agent(self.user, self.company, id)
            .await?
            .expect("created agent");
        let body = json!({"expected_revision":current.draft_revision, "agent":body});
        Ok(response_json(
            update_agent_json(
                State(self.agents.clone()),
                State(self.skills.clone()),
                State(
                    crate::adapters::persistence::test_support::department_use_cases(
                        &self.persistence,
                    ),
                ),
                AuthenticatedUser { id: self.user },
                Path((self.company, id)),
                Json(serde_json::from_value(body).unwrap()),
            )
            .await?,
        )
        .await)
    }
    async fn cleanup(&self) {
        CompanyPersistence::delete(self.persistence.as_ref(), self.company)
            .await
            .unwrap();
    }
}

/// The JSON API answers with an agent's own fields only; the retired runtime settings are
/// neither returned nor accepted (p08 B4c).
#[tokio::test]
async fn api_agents_carry_no_retired_runtime_fields() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let saved = f
        .create(json!({"name":"Default", "slug":"default-agent"}))
        .await;
    let id = serde_json::from_value(saved["agent"]["id"].clone()).unwrap();
    let edited = f.update(id, json!({})).await.unwrap();
    for retired in [
        "harness_kind",
        "granted_tool_ids",
        "native_tool_policy",
        "config_json",
        "run_timeout_secs",
        "response_contract",
    ] {
        assert!(saved["agent"].get(retired).is_none(), "{retired}");
        assert!(edited["agent"].get(retired).is_none(), "{retired}");
    }
    assert!(saved.get("sub_agent_ids").is_none());
    assert_eq!(edited["agent"]["description"], "Updated description");
    // An agent in no department lists none (01-departments 04C.4).
    assert_eq!(saved["departments"], json!([]));
    assert_eq!(edited["departments"], json!([]));
    f.cleanup().await;
}

/// p08 D2.6, F044: the library Agent Builder's company copy keeps its provisioning grants and,
/// as workflows need, an explicit model (the company default); "Create assistant channel" then
/// gives it a working channel, its grants captured as the trusted built-ins.
#[tokio::test]
async fn library_builder_copy_keeps_its_grants_on_an_explicit_model() {
    use crate::adapters::persistence::workflow::provisioning::{
        AssistantChannelService, ProvisioningDeployment,
    };
    use crate::application::workflow::provisioning::{
        AssistantChannelRequest, AssistantChannels, CREATE_AGENT_CHANNEL, LIST_COMPANY_AGENTS,
        ReplyMode,
    };
    use crate::use_cases::company::CompanyModelConnectionWrite;
    let Some(f) = Fixture::new().await else {
        return;
    };
    CompanyPersistence::replace_model_connections_for_user(
        f.persistence.as_ref(),
        f.user,
        f.company,
        vec![CompanyModelConnectionWrite {
            provider: "openai".into(),
            api_key: Some("test-key".into()),
            models: vec!["gpt-4o-mini".into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let mut builder = crate::use_cases::builtin_agent_library::agent_builder_definition().write;
    builder.slug = format!("builder-{}", Uuid::new_v4().simple());
    let definition = f
        .agents
        .create_library_agent(&crate::entities::user::Caller::operator(f.user), builder)
        .await
        .unwrap();
    let company_channels = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM channels WHERE company_id = $1")
            .bind(f.company)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap()
    };
    let channels_before = company_channels().await;
    let copied = f
        .agents
        .create_agent_from_library(f.user, f.company, definition.id, None)
        .await
        .unwrap();
    // 01DEP-P03-D1: the copy comes with its default desk at its slug; "Create additional desk"
    // adds more.
    assert_eq!(company_channels().await, channels_before + 1);
    assert_eq!(
        (
            copied.agent.provider.as_deref(),
            copied.agent.model.as_deref()
        ),
        (Some("openai"), Some("gpt-4o-mini"))
    );
    assert!(
        copied
            .warnings
            .iter()
            .any(|warning| warning.code == "library_model_pinned")
    );
    let grants: Vec<String> = sqlx::query_scalar("SELECT ARRAY(SELECT alias FROM agent_draft_bindings WHERE agent_id=agents.id ORDER BY position) FROM agents WHERE id = $1")
        .bind(copied.agent.id)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(grants, [LIST_COMPANY_AGENTS, CREATE_AGENT_CHANNEL]);

    let templates = ProvisioningDeployment::new(
        Arc::new(
            crate::application::workflow::templates::TemplateCatalogue::shipped(
                &crate::adapters::workflow_source::WorkflowSourceDecoder,
            )
            .unwrap(),
        ),
        &[],
        "agents.test",
    );
    let created = AssistantChannelService::new(f.persistence.as_ref().clone(), templates)
        .create_assistant_channel(
            crate::domain::workflow::CompanyId::new(f.company),
            crate::application::workflow::WorkflowActor::authenticated(f.user).unwrap(),
            AssistantChannelRequest {
                agent_slug: copied.agent.slug.clone(),
                channel_slug: "builder-desk".into(),
                channel_name: "Builder".into(),
                reply_mode: ReplyMode::Autonomous,
                command_key: "builder-desk".into(),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(created["channel"]["slug"], "builder-desk");
    let company: String = sqlx::query_scalar("SELECT slug::text FROM companies WHERE id = $1")
        .bind(f.company)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(
        created["channel"]["address"],
        format!("builder-desk@{company}.agents.test")
    );
    f.agents
        .delete_library_agent(
            &crate::entities::user::Caller::operator(f.user),
            definition.id,
        )
        .await
        .unwrap();
    f.cleanup().await;
}

/// 03C.0 over the JSON API: `department_id` and `role` beside the agent's fields place the new
/// agent; a role without a department, a bad role or an unknown field is a 400 with nothing
/// written.
#[tokio::test]
async fn agent_default_desk_json_create_places_the_agent() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let department = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO departments (id, company_id, name, slug, created_by) \
         VALUES ($1, $2, 'Support', 'support', \
                 '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Test\"}')",
    )
    .bind(department)
    .bind(f.company)
    .execute(f.persistence.pool())
    .await
    .unwrap();
    let saved = f
        .create(
            json!({"name": "Lead", "slug": "lead", "department_id": department,
            "role": "lead"}),
        )
        .await;
    let agent: Uuid = serde_json::from_value(saved["agent"]["id"].clone()).unwrap();
    let placed: (bool, Option<String>) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM department_members AS member \
                         WHERE member.department_id = $1 AND member.principal_id = principal.id), \
                (SELECT role.role FROM department_roles AS role \
                  WHERE role.department_id = $1 AND role.principal_id = principal.id) \
           FROM principals AS principal WHERE principal.agent_id = $2",
    )
    .bind(department)
    .bind(agent)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(placed, (true, Some("lead".into())));
    // 01-departments 04C.4: the response names the department and the role the agent holds there.
    assert_eq!(
        saved["departments"],
        json!([{"id": department, "name": "Support", "slug": "support", "roles": ["lead"]}])
    );
    assert_eq!(saved["more_departments"], false);
    let edited = f
        .update(agent, json!({"name": "Lead", "slug": "lead"}))
        .await
        .unwrap();
    assert_eq!(edited["departments"], saved["departments"]);

    for body in [
        json!({"name": "Stray", "slug": "stray", "role": "lead"}),
        json!({"name": "Stray", "slug": "stray", "department_id": department, "role": "boss"}),
        json!({"name": "Stray", "slug": "stray", "department_id": "not-a-uuid"}),
        json!({"name": "Stray", "slug": "stray", "harness_kind": "rig"}),
    ] {
        let refused = create_agent_json(
            State(f.agents.clone()),
            State(crate::adapters::persistence::test_support::department_use_cases(&f.persistence)),
            AuthenticatedUser { id: f.user },
            Path(f.company),
            Json(body.clone()),
        )
        .await;
        assert!(
            matches!(refused, Err(AppError::BadRequest(_))),
            "{body}: {:?}",
            refused.map(|_| ())
        );
    }
    let agents: i64 = sqlx::query_scalar("SELECT count(*) FROM agents WHERE company_id = $1")
        .bind(f.company)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(agents, 1);
    f.cleanup().await;
}
