use super::*;
use crate::adapters::persistence::credentials::CredentialCipher;
use crate::application::agent::DependencyManifest;
use crate::application::ports::channel::{ChannelOwnershipWrite, ChannelPersistence, ChannelWrite};
use crate::application::ports::department::DepartmentWrite;
use crate::application::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::application::use_cases::company::CompanyModelConnectionWrite;
use crate::application::workflow::reads::WorkflowReads;
use crate::entities::department::{DepartmentId, DepartmentRole, RoleRefusalKind};
use crate::entities::transport::PrincipalId;
use crate::services::test_support::{SCRIPTED_MODEL, SCRIPTED_PROVIDER};

#[path = "role_snapshot_sql_tests.rs"]
mod sql_tests;

#[path = "role_snapshot_race_tests.rs"]
mod race_tests;

#[path = "escalation_refusal_tests.rs"]
mod escalation_refusals;

struct RoleFixture {
    admission: AdmissionFixture,
    department: DepartmentId,
    channel: Uuid,
    agent: Uuid,
    principal: PrincipalId,
    revision: crate::entities::agent_capability::AgentRevisionId,
}

impl RoleFixture {
    fn new() -> impl std::future::Future<Output = Self> {
        Self::with_role(DepartmentRole::Lead)
    }

    async fn with_role(role: DepartmentRole) -> Self {
        let mut source: Value =
            serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
        source["input_schema"] = json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"],"additionalProperties":false});
        source["steps"]["start"]["with"]
            .as_object_mut()
            .unwrap()
            .remove("agent");
        source["steps"]["start"]["with"]["role"] = json!({"literal":role.as_str()});
        Self::from_binding(BindingFixture::from_source(source).await).await
    }

    async fn from_binding(mut binding: BindingFixture) -> Self {
        binding.fixture.persistence = PostgresPersistence::with_credential_cipher(
            binding.fixture.persistence.pool().clone(),
            CredentialCipher::for_test(),
        );
        enable_model(&binding).await;
        let (agent, principal) = create_agent(&binding, "original").await;
        let revision = binding.fixture.publish_agent(agent).await.id;
        let departments = crate::adapters::persistence::test_support::department_use_cases(
            &binding.fixture.persistence,
        );
        let department = departments
            .create_department(
                binding.target.actor.user_id(),
                binding.target.company.as_uuid(),
                DepartmentWrite {
                    name: "Team".into(),
                    slug: "team".into(),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap()
            .id;
        departments
            .assign_role(
                binding.target.actor.user_id(),
                binding.target.company.as_uuid(),
                department,
                DepartmentRole::Lead,
                principal,
            )
            .await
            .unwrap();
        let channel = ChannelPersistence::create(
            &binding.fixture.persistence,
            binding.target.company.as_uuid(),
            ChannelWrite {
                name: "Role intake".into(),
                slug: "role-intake".into(),
                enabled: true,
                ownership: Some(ChannelOwnershipWrite {
                    department_id: Some(Some(department)),
                    members_only: Some(true),
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let mut configuration = binding.request(None);
        configuration.association = RelatedAssociation::Channel(RelatedChannelId::new(channel));
        let configured = binding.service().configure(configuration).await.unwrap();
        binding
            .service()
            .activate(SetBindingActivity {
                target: binding.target,
                expected: configured.revision,
            })
            .await
            .unwrap();
        Self {
            admission: AdmissionFixture { binding },
            department,
            channel,
            agent,
            principal,
            revision,
        }
    }

    fn p(&self) -> &PostgresPersistence {
        self.admission.persistence()
    }
    fn company(&self) -> CompanyId {
        self.admission.binding.target.company
    }

    async fn prepared(&self, key: &str) -> PreparedAdmission {
        let mut request = self.admission.request(key, self.admission.manual());
        request.association = RelatedAssociation::Channel(RelatedChannelId::new(self.channel));
        self.admission.prepare(request).await
    }

    async fn admitted(&self, key: &str) -> RunId {
        let command = self.prepared(key).await;
        match self.p().admit(&command).await.unwrap() {
            AdmissionResult::Created(run) => run,
            other => panic!("new role admission must create a run: {other:?}"),
        }
    }

    async fn manifest(&self, run: RunId) -> DependencyManifest {
        let content: String = sqlx::query_scalar(
            "SELECT content FROM agent_dependency_manifests WHERE company_id=$1 AND root_run_id=$2",
        )
        .bind(self.company().as_uuid())
        .bind(run.as_uuid())
        .fetch_one(self.p().pool())
        .await
        .unwrap();
        DependencyManifest::decode(content.as_bytes()).unwrap()
    }
}

async fn enable_model(binding: &BindingFixture) {
    CompanyPersistence::replace_model_connections_for_user(
        &binding.fixture.persistence,
        binding.target.actor.user_id(),
        binding.target.company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
}

async fn create_agent(binding: &BindingFixture, slug: &str) -> (Uuid, PrincipalId) {
    let agent = AgentPersistence::create(
        &binding.fixture.persistence,
        binding.target.company.as_uuid(),
        AgentWrite {
            name: slug.into(),
            slug: slug.into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some(format!("ROLE_INSTRUCTIONS_{slug}")),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
            .bind(binding.target.company.as_uuid())
            .bind(agent.id)
            .fetch_one(binding.fixture.persistence.pool())
            .await
            .unwrap();
    (agent.id, PrincipalId::new(principal))
}

#[tokio::test]
async fn workflow_role_admission_captures_exact_principal_revision_and_canonical_membership() {
    let f = RoleFixture::new().await;
    let run = f.admitted("capture").await;
    let manifest = f.manifest(run).await;
    assert_eq!(manifest.roles.len(), 1);
    let role = &manifest.roles[0];
    assert_eq!(role.step.as_str(), "start");
    assert_eq!(role.role, DepartmentRole::Lead);
    assert_eq!(role.department_id, f.department);
    assert_eq!(role.principals[0].principal_id, f.principal);
    assert_eq!(role.principals[0].agent_id.unwrap().as_uuid(), f.agent);
    assert_eq!(role.principals[0].revision_id, Some(f.revision));
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.company().as_uuid())
    .bind(run.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(rows, 1);
    let view = f.p().run_summary(f.company(), run).await.unwrap().unwrap();
    assert_eq!(view.role_holders.len(), 1);
    assert_eq!(view.role_holders[0].principal, f.principal);
    assert!(!view.role_holders_truncated);
}

#[tokio::test]
async fn workflow_role_admission_missing_holder_has_no_run_or_partial_capture() {
    let f = RoleFixture::new().await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.p());
    departments
        .clear_role(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            f.department,
            DepartmentRole::Lead,
            f.principal,
        )
        .await
        .unwrap();
    let command = f.prepared("missing").await;
    let error = f.p().admit(&command).await.unwrap_err();
    assert!(
        matches!(error, AppError::WorkflowRole(ref refusal) if refusal.kind==RoleRefusalKind::NoHolder && refusal.step.as_str()=="start" && refusal.role==DepartmentRole::Lead)
    );
    for table in [
        "workflow_runs",
        "workflow_executions",
        "workflow_run_role_bindings",
        "background_tasks",
    ] {
        let column = match table {
            "workflow_runs" => "id",
            "background_tasks" => "workflow_execution_id",
            _ => "run_id",
        };
        let record_id = if table == "background_tasks" {
            command.first_execution_id().as_uuid()
        } else {
            command.proposed_run_id().as_uuid()
        };
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {table} WHERE company_id=$1 AND {column}=$2"
        ))
        .bind(f.company().as_uuid())
        .bind(record_id)
        .fetch_one(f.p().pool())
        .await
        .unwrap();
        assert_eq!(count, 0, "a refused role leaves no {table}");
    }
}

#[tokio::test]
async fn workflow_role_admission_sealed_set_rejects_late_people_and_extra_requirements() {
    let f = RoleFixture::new().await;
    let run = f.admitted("sealed").await;
    let owner: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(f.company().as_uuid())
            .bind(f.admission.binding.target.actor.user_id())
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    for extra_step in [false, true] {
        let mut tx = f.p().pool().begin().await.unwrap();
        sqlx::query("INSERT INTO workflow_run_role_bindings(company_id,run_id,manifest_id,version_id,step,role,department_id,principal_id,agent_id,agent_revision_id) \
            SELECT company_id,run_id,manifest_id,version_id,CASE WHEN $3 THEN 'extra' ELSE step END,role,department_id, \
                CASE WHEN $3 THEN principal_id ELSE $4 END,CASE WHEN $3 THEN agent_id END,CASE WHEN $3 THEN agent_revision_id END \
            FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2")
            .bind(f.company().as_uuid()).bind(run.as_uuid()).bind(extra_step).bind(owner).execute(&mut *tx).await.unwrap();
        let error = tx.commit().await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some("workflow_guard:check_workflow_role_manifest")
        );
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.company().as_uuid())
    .bind(run.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(
        count, 1,
        "late insert attempts rolled back without reopening the captured set"
    );
}

#[path = "role_reviewer_tests.rs"]
mod reviewer_tests;

#[path = "role_context_tests.rs"]
mod context_tests;
