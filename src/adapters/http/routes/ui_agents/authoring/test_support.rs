use super::*;
use crate::adapters::http::routes::authorization_test_support::Fixture as ScopeFixture;
use crate::use_cases::agent::AgentPersistence;

pub(super) struct Fixture {
    pub f: ScopeFixture,
    pub workflow: Arc<crate::infra::workflow::WorkflowServices>,
    pub agent: Agent,
}
impl Fixture {
    pub async fn new() -> Option<Self> {
        let f = ScopeFixture::new().await?;
        sqlx::query("INSERT INTO company_model_connections(company_id,provider,api_key,models) VALUES ($1,'openai','authoring-secret-never-return','{gpt-test}')")
            .bind(f.company.id).execute(f.persistence.pool()).await.unwrap();
        let agent = AgentPersistence::create(
            f.persistence.as_ref(),
            f.company.id,
            AgentWrite {
                name: "Authoring".into(),
                slug: "authoring".into(),
                provider: Some("openai".into()),
                model: Some("gpt-test".into()),
                system_prompt: Some("private immutable instruction marker".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let workflow =
            crate::infra::workflow::WorkflowEngine::for_tests(f.persistence.as_ref().clone())
                .services();
        Some(Self { f, workflow, agent })
    }
    pub fn workspace(&self, user: Uuid) -> Workspace {
        Workspace {
            workflow: self.workflow.clone(),
            company_use_cases: self.f.companies.clone(),
            agent_use_cases: self.f.agents.clone(),
            user_use_cases: self.f.users.clone(),
            skill_use_cases: Arc::new(SkillUseCases::new(
                self.f.persistence.clone(),
                self.f.persistence.clone(),
                self.f.persistence.clone(),
            )),
            config: self.f.config.clone(),
            user_id: user,
        }
    }
    pub fn scope(&self) -> AgentPublicationScope {
        scope(self.f.owner.id, self.f.company.id, self.agent.id).unwrap()
    }
    pub fn query(&self) -> Query<CompanyQuery> {
        Query(CompanyQuery {
            company_id: self.f.company.id,
        })
    }
    pub async fn state(&self) -> AgentAuthoringState {
        self.workflow
            .agent_authoring_reads()
            .authoring_state(self.scope())
            .await
            .unwrap()
    }

    pub async fn skill(
        &self,
        name: &str,
        required: Vec<crate::domain::workflow::TypeName>,
    ) -> crate::entities::skill::Skill {
        self.workspace(self.f.owner.id)
            .skill_use_cases
            .create_company(
                self.f.owner.id,
                self.f.company.id,
                crate::use_cases::skill::SkillWrite {
                    files: None,
                    slug: name.into(),
                    name: name.into(),
                    description: format!("Use {name}"),
                    instructions: format!("Private instructions for {name}"),
                    required_tools: Some(required),
                    created_by: None,
                },
            )
            .await
            .unwrap()
    }
}

pub(super) async fn text(response: impl IntoResponse) -> String {
    let response = response.into_response();
    assert!(response.status().is_success(), "{}", response.status());
    String::from_utf8(
        axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

pub(super) async fn form(fields: &[(&str, String)]) -> AgentForm {
    use axum::extract::FromRequest;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().map(|(key, value)| (*key, value.as_str())))
        .finish();
    let request = axum::http::Request::post("/ui/agents")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(axum::body::Body::from(body))
        .unwrap();
    Form::<AgentForm>::from_request(request, &())
        .await
        .unwrap()
        .0
}

pub(super) fn fields(revision: AgentDraftRevision, tools: &str) -> Vec<(&'static str, String)> {
    vec![
        ("name", "Edited <agent>".into()),
        ("slug", "authoring".into()),
        ("native_permissions", "full".into()),
        ("provider", "openai".into()),
        ("model", "gpt-test".into()),
        (
            "system_prompt",
            "unsaved </textarea><img src=x onerror=alert(1)>".into(),
        ),
        ("expected_revision", revision.get().to_string()),
        ("skill_ids", String::new()),
        ("tool_assignments", tools.into()),
    ]
}
