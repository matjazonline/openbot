use super::*;
use crate::adapters::persistence::doop_live_setup_tests;
use crate::entities::mcp::McpEndpoint;
use crate::use_cases::mcp::{McpConnectionWrite, McpUseCases};

pub(super) struct Environment {
    database: Arc<crate::adapters::persistence::test_support::OwnDatabase>,
    pub(super) persistence: Arc<PostgresPersistence>,
    pub(super) client: Arc<client::LiveClient>,
    pub(super) engine: WorkflowEngine,
    pub(super) service: McpUseCases,
    endpoint: McpEndpoint,
}

pub(super) struct CompanyDocument {
    pub(super) fixture: Fixture,
    pub(super) connection: CompanyMcpConnection,
    pub(super) target: BindingTarget,
    pub(super) version: VersionId,
    pub(super) agent: Uuid,
    pub(super) channel: crate::application::workflow::RelatedChannelId,
    parameters: Value,
}
impl CompanyDocument {
    pub(super) fn runtime<'a>(
        &'a self,
        engine: &'a WorkflowEngine,
    ) -> run_support::DocumentRun<'a> {
        run_support::DocumentRun {
            persistence: &self.fixture.persistence,
            engine,
            target: self.target,
            channel: self.channel,
        }
    }
}

impl Environment {
    pub(super) async fn new(input: &Input) -> Self {
        let database = Arc::new(
            crate::adapters::persistence::test_support::own_database()
                .await
                .expect("owned PostgreSQL is required"),
        );
        let persistence = Arc::new(PostgresPersistence::with_credential_cipher(
            database.pool.clone(),
            CredentialCipher::for_test(),
        ));
        immutable_guard(&persistence).await;
        let (endpoint, guarded) = doop_live_setup_tests::guarded_client(input);
        let client = Arc::new(client::LiveClient::new(guarded, input));
        let engine =
            WorkflowEngine::for_tests_with_mcp(persistence.as_ref().clone(), client.clone());
        let service = McpUseCases::new(
            persistence.clone(),
            persistence.clone(),
            persistence.clone(),
            client.clone(),
            Arc::new(crate::use_cases::mcp::UnavailableMcpDiscovery),
        );
        Self {
            database,
            persistence,
            client,
            engine,
            service,
            endpoint,
        }
    }

    pub(super) async fn company(&self, input: &Company, origin: &str) -> CompanyDocument {
        let provisioned = Box::pin(doop_live_setup_tests::register_company(
            &self.persistence,
            &self.service,
            &self.endpoint,
            input,
        ))
        .await;
        let connection =
            Box::pin(self.review_operations(provisioned.actor, &provisioned.connection)).await;
        // Unlike Fixture::in_database, this scope never disables snapshot immutability.
        let fixture = Fixture {
            _db: self.database.clone(),
            persistence: self.persistence.as_ref().clone(),
            target: DefinitionTarget {
                company: CompanyId::new(connection.company_id),
                actor: WorkflowActor::authenticated(provisioned.actor).unwrap(),
                workflow: WorkflowId::new(Uuid::new_v4()),
            },
        };
        let agent = Box::pin(editor(&fixture)).await;
        let channel = Box::pin(delivery_support::prepare_delivery(&fixture, &connection)).await;
        let version = Box::pin(publish_document(
            &fixture,
            &self.engine,
            connection.id,
            McpErrorPolicy::FailStep,
        ))
        .await;
        let mut parameters = params(&fixture);
        parameters["expected_bot_id"] = json!(input.bot);
        parameters["expected_workspace_id"] = json!(input.workspace);
        parameters["doop_origin"] = json!(origin);
        let target = BindingTarget {
            company: fixture.target.company,
            actor: fixture.target.actor,
            binding: WorkflowBindingId::new(Uuid::new_v4()),
        };
        bind(
            &self.engine,
            target,
            version,
            channel,
            parameters.clone(),
            connection.id,
        )
        .await
        .unwrap();
        CompanyDocument {
            fixture,
            connection,
            target,
            version,
            agent,
            channel,
            parameters,
        }
    }

    async fn review_operations(
        &self,
        actor: Uuid,
        current: &CompanyMcpConnection,
    ) -> CompanyMcpConnection {
        let operations = recipe().operations;
        let connection = self
            .service
            .update(
                actor,
                current,
                McpConnectionWrite {
                    slug: current.slug.clone(),
                    endpoint: current.endpoint.clone(),
                    enabled: current.enabled,
                    auth: current.auth.clone(),
                    discovered_tools: current.discovered_tools.clone(),
                    tool_grants: operations
                        .iter()
                        .map(|operation| tool_name(operation.tool.as_str()))
                        .collect(),
                },
            )
            .await
            .unwrap();
        let company = CompanyId::new(connection.company_id);
        for selected in operations {
            let tool = connection
                .discovered_tools
                .iter()
                .find(|tool| tool.name.as_str() == selected.tool.as_str())
                .unwrap();
            self.engine
                .services()
                .mcp_operations()
                .register_mcp_operation(
                    company,
                    WorkflowActor::authenticated(actor).unwrap(),
                    McpOperationWrite {
                        connection: connection.id,
                        catalog_revision: connection.revision,
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
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_mcp_operations WHERE company_id=$1 AND connection_id=$2 AND revoked_at IS NULL")
            .bind(connection.company_id).bind(connection.id).fetch_one(self.persistence.pool()).await.unwrap();
        assert_eq!(count, 9);
        connection
    }

    pub(super) async fn assert_foreign(&self, own: &CompanyDocument, foreign: &CompanyDocument) {
        assert!(
            self.service
                .get(
                    own.target.actor.user_id(),
                    foreign.connection.company_id,
                    foreign.connection.id
                )
                .await
                .is_err()
        );
        assert!(
            crate::use_cases::mcp::McpCredentialPersistence::mcp_token(
                self.persistence.as_ref(),
                own.connection.company_id,
                foreign.connection.id,
                foreign.connection.revision
            )
            .await
            .is_err()
        );
        let target = BindingTarget {
            binding: WorkflowBindingId::new(Uuid::new_v4()),
            ..own.target
        };
        assert!(
            bind(
                &self.engine,
                target,
                own.version,
                own.channel,
                own.parameters.clone(),
                foreign.connection.id
            )
            .await
            .is_err(),
            "same-endpoint foreign connection cannot bind"
        );
    }

    async fn publish_graph(
        &self,
        document: &CompanyDocument,
        mut source: Value,
        selections: Vec<crate::adapters::persistence::workflow::mcp::McpPublicationSelection>,
    ) -> VersionId {
        let workflow = WorkflowId::new(Uuid::new_v4());
        let version = VersionId::new(Uuid::new_v4());
        source["workflow_id"] = json!(workflow);
        let definitions = self.engine.services().definitions(vec![], selections);
        let mut draft = document.fixture.save(None);
        draft.target.workflow = workflow;
        draft.content.source = source.to_string();
        definitions.save(draft).await.unwrap();
        let mut publication = document.fixture.publish();
        publication.target.workflow = workflow;
        publication.version = version;
        publication.key = IdempotencyKey::parse(Uuid::new_v4().to_string()).unwrap();
        definitions.publish(publication).await.unwrap();
        version
    }

    pub(super) async fn uncertain(&self, document: &CompanyDocument) -> RunReadScope {
        let mut source = document_source(&document.fixture);
        let create = source["steps"]["create"].clone();
        source["steps"] = json!({"create":create});
        source["entry"] = json!("create");
        source["steps"]["create"]["routes"]["success"] = json!("$end");
        source["output_schema"] = json!(true);
        source["limits"]["max_steps"] = json!(1);
        let version = Box::pin(self.publish_graph(
            document,
            source,
            vec![
                crate::adapters::persistence::workflow::mcp::McpPublicationSelection {
                    connection: document.connection.id,
                    slot: ResourceName::parse("service").unwrap(),
                    tool: TypeName::parse("create_canvas").unwrap(),
                },
            ],
        ))
        .await;
        let target = BindingTarget {
            binding: WorkflowBindingId::new(Uuid::new_v4()),
            ..document.target
        };
        bind(
            &self.engine,
            target,
            version,
            document.channel,
            document.parameters.clone(),
            document.connection.id,
        )
        .await
        .unwrap();
        run_support::DocumentRun {
            persistence: &document.fixture.persistence,
            engine: &self.engine,
            target,
            channel: document.channel,
        }
        .admit(json!({"title":"Deliberately uncertain partial draft",
                "brief":"The fixture loses one actual accepted create receipt; do not retry it."}))
        .await
        .unwrap()
    }

    pub(super) async fn health(&self, document: &CompanyDocument) -> RunReadScope {
        let mut source: Value =
            serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
        source["steps"]["start"]["with"]["value"] = json!({"literal":{"health":"passed"}});
        source["steps"]["start"]["with"]["output_schema"] = json!({"literal":{"type":"object"}});
        source["limits"]["max_steps"] = json!(2);
        let version = Box::pin(self.publish_graph(document, source, vec![])).await;
        let target = BindingTarget {
            binding: WorkflowBindingId::new(Uuid::new_v4()),
            ..document.target
        };
        let bindings = self.engine.services().bindings();
        let configured = bindings
            .configure(ConfigureBinding {
                target,
                expected: None,
                association: RelatedAssociation::Channel(document.channel),
                version,
                params: json!({}),
                resources: BTreeMap::new(),
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
        run_support::DocumentRun {
            persistence: &document.fixture.persistence,
            engine: &self.engine,
            target,
            channel: document.channel,
        }
        .admit(json!({}))
        .await
        .unwrap()
    }
}

async fn bind(
    engine: &WorkflowEngine,
    target: BindingTarget,
    version: VersionId,
    channel: crate::application::workflow::RelatedChannelId,
    params: Value,
    connection: Uuid,
) -> LifecycleResult<()> {
    let bindings = engine.services().bindings();
    let state = bindings
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Channel(channel),
            version,
            params,
            resources: BTreeMap::from([(
                ResourceName::parse("service").unwrap(),
                RuntimeResourceId::new(connection),
            )]),
            run_as: None,
            message_trigger: None,
        })
        .await?;
    bindings
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await?;
    Ok(())
}

pub(super) async fn immutable_guard(persistence: &PostgresPersistence) {
    let enabled: String = sqlx::query_scalar("SELECT tgenabled::text FROM pg_trigger WHERE tgrelid='workflow_runs'::regclass AND tgname='workflow_run_snapshot_immutable'")
        .fetch_one(persistence.pool()).await.unwrap();
    assert_eq!(
        enabled, "O",
        "live proof must retain the real immutable snapshot guard"
    );
}
