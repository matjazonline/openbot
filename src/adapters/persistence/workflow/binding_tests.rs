use super::*;
use crate::application::use_cases::mcp::{McpConnectionWrite, McpPersistence};
use crate::application::workflow::binding::{ResourceDirectory, ResourceReadiness};
use crate::entities::mcp::{McpAuth, McpDiscoveredTool};
use serde_json::json;
use std::sync::Mutex;

#[derive(Clone)]
struct Capture {
    persistence: PostgresPersistence,
    command: Arc<Mutex<Option<PreparedBinding>>>,
}
#[async_trait]
impl BindingLifecycle for Capture {
    async fn binding(
        &self,
        company: CompanyId,
        id: WorkflowBindingId,
    ) -> AppResult<Option<BindingState>> {
        self.persistence.binding(company, id).await
    }
    async fn save_binding(&self, command: PreparedBinding) -> AppResult<()> {
        *self.command.lock().unwrap() = Some(command);
        Ok(())
    }
}
impl Capture {
    pub(super) fn new(persistence: PostgresPersistence) -> Self {
        Self {
            persistence,
            command: Arc::new(Mutex::new(None)),
        }
    }
    fn take(&self) -> PreparedBinding {
        self.command.lock().unwrap().take().unwrap()
    }
    pub(super) fn service(
        &self,
    ) -> BindingService<Preflight, Self, PostgresPersistence, PostgresPersistence> {
        BindingService::new(
            Preflight,
            self.clone(),
            self.persistence.clone(),
            self.persistence.clone(),
        )
    }
}

pub(super) struct BindingFixture {
    pub(super) fixture: Fixture,
    pub(super) target: BindingTarget,
    version: VersionId,
    resources: std::collections::BTreeMap<ResourceName, RuntimeResourceId>,
}
impl BindingFixture {
    pub(super) async fn new(requirements: serde_json::Value) -> Self {
        let mut source: serde_json::Value =
            serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
        source["resources"] = requirements;
        Self::from_source(source).await
    }
    pub(super) async fn from_source(source: serde_json::Value) -> Self {
        let fixture = Fixture::new().await;
        Self::from_fixture(fixture, source).await
    }
    pub(super) async fn from_fixture(fixture: Fixture, source: serde_json::Value) -> Self {
        let mut draft = fixture.save(None);
        draft.content.source = source.to_string();
        fixture.service().save(draft).await.unwrap();
        let publication = fixture.publish();
        fixture
            .service()
            .publish(publication.clone())
            .await
            .unwrap();
        Self {
            target: BindingTarget {
                company: fixture.target.company,
                actor: fixture.target.actor,
                binding: WorkflowBindingId::new(Uuid::new_v4()),
            },
            version: publication.version,
            fixture,
            resources: Default::default(),
        }
    }
    pub(super) fn request(&self, expected: Option<BindingStateRevision>) -> ConfigureBinding {
        ConfigureBinding {
            target: self.target,
            expected,
            association: RelatedAssociation::Company,
            version: self.version,
            params: json!({}),
            resources: self.resources.clone(),
        }
    }
    pub(super) fn service(
        &self,
    ) -> BindingService<Preflight, PostgresPersistence, PostgresPersistence, PostgresPersistence>
    {
        let p = &self.fixture.persistence;
        BindingService::new(Preflight, p.clone(), p.clone(), p.clone())
    }
    pub(super) async fn prepare(&self, request: ConfigureBinding) -> PreparedBinding {
        let capture = Capture::new(self.fixture.persistence.clone());
        capture.service().configure(request).await.unwrap();
        capture.take()
    }
    pub(super) async fn activity(&self, expected: BindingStateRevision) -> PreparedBinding {
        let capture = Capture::new(self.fixture.persistence.clone());
        capture
            .service()
            .activate(SetBindingActivity {
                target: self.target,
                expected,
            })
            .await
            .unwrap();
        capture.take()
    }
    pub(super) async fn state(&self) -> BindingState {
        self.fixture
            .persistence
            .binding(self.target.company, self.target.binding)
            .await
            .unwrap()
            .unwrap()
    }
    pub(super) async fn mcp(&mut self) -> Uuid {
        let connection = self
            .fixture
            .persistence
            .create_mcp_connection(self.target.company.as_uuid(), mcp_write())
            .await
            .unwrap();
        self.resources.insert(
            ResourceName::parse("service").unwrap(),
            RuntimeResourceId::new(connection.id),
        );
        connection.id
    }
}
fn mcp_write() -> McpConnectionWrite {
    McpConnectionWrite {
        slug: "service".into(),
        endpoint: "https://example.test/mcp".to_owned().try_into().unwrap(),
        enabled: true,
        auth: McpAuth::None,
        discovered_tools: vec![McpDiscoveredTool {
            name: "lookup".to_owned().try_into().unwrap(),
            description: String::new(),
            input_schema: json!({"type":"object"}),
        }],
        tool_grants: vec!["lookup".to_owned().try_into().unwrap()],
    }
}
async fn block_company(f: &BindingFixture) -> Transaction<'_, Postgres> {
    let mut tx = f.fixture.persistence.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR UPDATE")
        .bind(f.target.company.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}

#[tokio::test]
async fn workflow_binding_sql_competing_creation_edit_activity_and_aba() {
    let f = BindingFixture::new(json!([])).await;
    let p = &f.fixture.persistence;
    let first = f.prepare(f.request(None)).await;
    let second = f.prepare(f.request(None)).await;
    let blocker = block_company(&f).await;
    let (first, second, ()) = tokio::join!(p.save_binding(first), p.save_binding(second), async {
        crate::adapters::persistence::test_support::wait_until_backends_are_blocked(p.pool(), 2)
            .await;
        blocker.commit().await.unwrap();
    });
    assert_ne!(first.is_ok(), second.is_ok());
    let original = f.state().await;
    let activate = f.activity(original.revision).await;
    let edit = f.prepare(f.request(Some(original.revision))).await;
    let blocker = block_company(&f).await;
    let (activate, edit, ()) =
        tokio::join!(p.save_binding(activate), p.save_binding(edit), async {
            crate::adapters::persistence::test_support::wait_until_backends_are_blocked(
                p.pool(),
                2,
            )
            .await;
            blocker.commit().await.unwrap();
        });
    assert_ne!(activate.is_ok(), edit.is_ok());
    let current = f.state().await;
    assert_eq!(current.revision.get(), 2);
    let stale = f.activity(current.revision).await;
    let active = f
        .service()
        .activate(SetBindingActivity {
            target: f.target,
            expected: current.revision,
        })
        .await
        .unwrap();
    let stopped = f
        .service()
        .deactivate(SetBindingActivity {
            target: f.target,
            expected: active.revision,
        })
        .await
        .unwrap();
    assert!(p.save_binding(stale).await.is_err());
    assert_eq!(f.state().await.revision, stopped.revision);
    assert_eq!(original.configuration.params(), &json!({}));
    assert_eq!(original.configuration.revision().get(), 1);
}

#[tokio::test]
async fn workflow_binding_sql_final_event_failure_rolls_back_head_and_history() {
    let f = BindingFixture::new(json!([])).await;
    let p = &f.fixture.persistence;
    sqlx::query(
        "ALTER TABLE workflow_binding_events ADD CONSTRAINT reject_binding_event CHECK (false)",
    )
    .execute(p.pool())
    .await
    .unwrap();
    assert!(f.service().configure(f.request(None)).await.is_err());
    assert!(
        p.binding(f.target.company, f.target.binding)
            .await
            .unwrap()
            .is_none()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_binding_revisions")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::query("ALTER TABLE workflow_binding_events DROP CONSTRAINT reject_binding_event")
        .execute(p.pool())
        .await
        .unwrap();
    let configured = f.service().configure(f.request(None)).await.unwrap();
    sqlx::query("ALTER TABLE workflow_binding_events ADD CONSTRAINT reject_binding_event CHECK (state_revision = 1)").execute(p.pool()).await.unwrap();
    assert!(
        f.service()
            .configure(f.request(Some(configured.revision)))
            .await
            .is_err()
    );
    assert!(
        f.service()
            .activate(SetBindingActivity {
                target: f.target,
                expected: configured.revision
            })
            .await
            .is_err()
    );
    let stored = f.state().await;
    assert_eq!(stored.revision, configured.revision);
    assert!(!stored.active);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_binding_revisions")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn workflow_binding_sql_archive_and_actor_revocation_after_preflight() {
    let f = BindingFixture::new(json!([])).await;
    let p = &f.fixture.persistence;
    let configured = f.service().configure(f.request(None)).await.unwrap();
    let activation = f.activity(configured.revision).await;
    let mut blocker = block_company(&f).await;
    sqlx::query("UPDATE workflow_definitions SET archived = true WHERE company_id = $1")
        .bind(f.target.company.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (result, ()) = tokio::join!(p.save_binding(activation), async {
        crate::adapters::persistence::test_support::wait_until_backends_are_blocked(p.pool(), 1)
            .await;
        blocker.commit().await.unwrap();
    });
    assert!(matches!(result, Err(AppError::NotFound(_))));
    let stopped = f
        .service()
        .deactivate(SetBindingActivity {
            target: f.target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    let capture = Capture::new(p.clone());
    capture
        .service()
        .deactivate(SetBindingActivity {
            target: f.target,
            expected: stopped.revision,
        })
        .await
        .unwrap();
    let mut blocker = block_company(&f).await;
    sqlx::query("DELETE FROM company_members WHERE company_id = $1 AND user_id = $2")
        .bind(f.target.company.as_uuid())
        .bind(f.target.actor.user_id())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (result, ()) = tokio::join!(p.save_binding(capture.take()), async {
        crate::adapters::persistence::test_support::wait_until_backends_are_blocked(p.pool(), 1)
            .await;
        blocker.commit().await.unwrap();
    });
    assert!(matches!(result, Err(AppError::NotFound(_))));
    assert_eq!(f.state().await.revision, stopped.revision);
}

#[tokio::test]
async fn workflow_binding_sql_mcp_revocation_readiness_and_contract_scope() {
    let mut f = BindingFixture::new(json!([{"slot":"service","kind":"mcp"}])).await;
    let id = f.mcp().await;
    let p = &f.fixture.persistence;
    let configured = f.service().configure(f.request(None)).await.unwrap();
    let activation = f.activity(configured.revision).await;
    let edit = f.prepare(f.request(Some(configured.revision))).await;
    let mut blocker = block_company(&f).await;
    sqlx::query("DELETE FROM company_mcp_tool_grants WHERE company_id = $1 AND connection_id = $2")
        .bind(f.target.company.as_uuid())
        .bind(id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (result, ()) = tokio::join!(p.save_binding(activation), async {
        crate::adapters::persistence::test_support::wait_until_backends_are_blocked(p.pool(), 1)
            .await;
        blocker.commit().await.unwrap();
    });
    assert!(matches!(result, Err(AppError::NotFound(_))));
    assert!(
        p.save_binding(edit).await.is_err(),
        "inactive reconfiguration must recheck grants"
    );
    let stopped = f
        .service()
        .deactivate(SetBindingActivity {
            target: f.target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    assert!(!stopped.active);
    let status = p
        .inspect(f.target.company, f.target.actor, RuntimeResourceId::new(id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.readiness, ResourceReadiness::Revoked);
    assert!(status.supported_contracts.is_empty());
    let mut write = mcp_write();
    write.auth = McpAuth::Bearer;
    p.update_mcp_connection(f.target.company.as_uuid(), id, 1, write)
        .await
        .unwrap();
    assert_eq!(
        p.inspect(f.target.company, f.target.actor, RuntimeResourceId::new(id))
            .await
            .unwrap()
            .unwrap()
            .readiness,
        ResourceReadiness::Unavailable
    );
    assert!(
        f.service()
            .activate(SetBindingActivity {
                target: f.target,
                expected: stopped.revision
            })
            .await
            .is_err()
    );
    assert!(
        p.inspect(
            CompanyId::new(Uuid::new_v4()),
            f.target.actor,
            RuntimeResourceId::new(id)
        )
        .await
        .is_err()
    );
    let mut contract =
        BindingFixture::new(json!([{"slot":"service","kind":"mcp","contract":"lookup"}])).await;
    contract.mcp().await;
    assert!(
        contract
            .service()
            .configure(contract.request(None))
            .await
            .is_err()
    );
    let mut unsupported = BindingFixture::new(json!([{"slot":"service","kind":"http"}])).await;
    unsupported.mcp().await;
    assert!(
        unsupported
            .service()
            .configure(unsupported.request(None))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn workflow_binding_sql_changed_configuration_roundtrips_without_mutating_history() {
    let mut f = BindingFixture::new(json!([{"slot":"service","kind":"mcp"}])).await;
    let old_resource = f.mcp().await;
    let configured = f.service().configure(f.request(None)).await.unwrap();
    let active = f
        .service()
        .activate(SetBindingActivity {
            target: f.target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    let p = &f.fixture.persistence;
    let mut write = mcp_write();
    write.slug = "replacement".into();
    let replacement = p
        .create_mcp_connection(f.target.company.as_uuid(), write)
        .await
        .unwrap();
    let mut request = f.request(Some(active.revision));
    request.params = json!({"changed": "new configuration"});
    request.resources.insert(
        ResourceName::parse("service").unwrap(),
        RuntimeResourceId::new(replacement.id),
    );
    let expected_params = request.params.clone();
    let expected_resources = request.resources.clone();
    let changed = f.service().configure(request).await.unwrap();
    let loaded = f.state().await;
    assert!(loaded.active);
    assert_eq!(loaded.revision.get(), 3);
    assert_eq!(loaded.configuration.revision().get(), 2);
    assert_eq!(loaded.configuration.params(), &expected_params);
    assert_eq!(loaded.configuration.resources(), &expected_resources);
    assert_eq!(
        store_bundle(loaded.configuration.bundle()).unwrap(),
        store_bundle(changed.configuration.bundle()).unwrap()
    );
    #[derive(sqlx::FromRow)]
    struct History {
        revision: i64,
        params: serde_json::Value,
        resources: serde_json::Value,
    }
    let history: Vec<History> = sqlx::query_as("SELECT revision, params, resources FROM workflow_binding_revisions WHERE company_id = $1 AND binding_id = $2 ORDER BY revision")
        .bind(f.target.company.as_uuid()).bind(f.target.binding.as_uuid()).fetch_all(p.pool()).await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].revision, 1);
    assert_eq!(history[0].params, json!({}));
    assert_eq!(history[0].resources, json!({"service": old_resource}));
    assert_eq!(history[1].revision, 2);
    assert_eq!(history[1].params, expected_params);
    assert_eq!(
        history[1].resources,
        serde_json::to_value(expected_resources).unwrap()
    );
    assert_eq!(active.configuration.params(), &json!({}));
    assert_eq!(
        active.configuration.resources()[&ResourceName::parse("service").unwrap()].as_uuid(),
        old_resource
    );
}
