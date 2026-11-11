use super::*;

pub(super) const V1: &str = "Published mixed agent revision one.";
pub(super) const V2: &str = "Published mixed agent revision two.";
pub(super) const SKILL_V1: &str = "Frozen support policy revision one.";
pub(super) const SKILL_V2: &str = "Frozen support policy revision two.";

pub(super) struct Journey {
    pub f: Fixture,
    pub scope: AgentPublicationScope,
    pub http: crate::application::workflow::http::HttpConnection,
    pub skill: crate::entities::skill::Skill,
}

pub(super) struct Workflow {
    pub target: BindingTarget,
    pub version: VersionId,
}

pub(super) struct AdmissionIdentity {
    key: IdempotencyKey,
    trigger: TriggerRef,
    correlation: CorrelationId,
}

impl Journey {
    pub async fn new(server: &McpServer, origin: &str) -> Self {
        let (f, agent) = published_mcp_agent(server).await;
        // The broad runtime fixture permits corruption tests; this journey exercises real guards.
        sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
            .execute(f.persistence.pool())
            .await
            .unwrap();
        let http = crate::adapters::persistence::workflow::tests::http_tests::support::configured(
            &f,
            origin,
            crate::adapters::persistence::workflow::tests::http_tests::support::Entrance::Tool,
        )
        .await;
        let skill = SkillManagementPersistence::create_company(
            &f.persistence,
            f.target.company.as_uuid(),
            SkillWrite {
                files: None,
                slug: "mixed-support".into(),
                name: "Mixed support".into(),
                description: "Both tools required".into(),
                instructions: SKILL_V1.into(),
                required_tools: Some(vec![
                    TypeName::parse("registered.request").unwrap(),
                    TypeName::parse(TOOL).unwrap(),
                ]),
                created_by: None,
            },
        )
        .await
        .unwrap();
        let scope = AgentPublicationScope {
            company: f.target.company,
            actor: f.target.actor,
            agent,
        };
        let state = f.persistence.authoring_state(scope).await.unwrap();
        let mut tools: Vec<_> = state.tools.into_iter().map(|t| t.assignment).collect();
        tools.push(AgentToolAssignment {
            binding: None,
            alias: AgentToolAlias::parse("registered.request").unwrap(),
            choice: AgentToolChoice::Http {
                connection: http.id,
                operation: TypeName::parse("registered.request").unwrap(),
            },
        });
        f.persistence
            .save_company_agent(SaveCompanyAgent {
                scope,
                expected_revision: state.agent.draft_revision,
                write: AgentWrite {
                    name: state.agent.name,
                    slug: state.agent.slug,
                    provider: state.agent.provider,
                    model: state.agent.model,
                    system_prompt: Some(V1.into()),
                    skill_ids: vec![skill.id],
                    ..Default::default()
                },
                tools: Some(tools),
            })
            .await
            .unwrap();
        Self {
            f,
            scope,
            http,
            skill,
        }
    }

    pub async fn publish(&self) -> PublishedAgentRevision {
        self.f.publish_agent(self.scope.agent.as_uuid()).await
    }

    pub fn edit(&self, instructions: &str) -> impl std::future::Future<Output = ()> + '_ {
        let instructions = instructions.to_owned();
        self.save_draft(move |request| request.write.system_prompt = Some(instructions))
    }

    pub async fn save_draft(&self, edit: impl FnOnce(&mut SaveCompanyAgent)) {
        let state = self
            .f
            .persistence
            .authoring_state(self.scope)
            .await
            .unwrap();
        let mut request = SaveCompanyAgent {
            scope: self.scope,
            expected_revision: state.agent.draft_revision,
            write: AgentWrite {
                name: state.agent.name,
                slug: state.agent.slug,
                provider: state.agent.provider,
                model: state.agent.model,
                system_prompt: state.agent.system_prompt,
                description: state.agent.description,
                skill_ids: state.skills.iter().map(|s| s.id).collect(),
                ..Default::default()
            },
            tools: Some(state.tools.into_iter().map(|t| t.assignment).collect()),
        };
        edit(&mut request);
        self.f
            .persistence
            .save_company_agent(request)
            .await
            .unwrap();
    }

    pub async fn edit_skill(&self, instructions: &str) {
        SkillManagementPersistence::update_company(
            &self.f.persistence,
            self.scope.company.as_uuid(),
            self.skill.id,
            SkillWrite {
                files: None,
                slug: self.skill.slug.as_str().into(),
                name: self.skill.name.clone(),
                description: self.skill.description.clone(),
                instructions: instructions.into(),
                required_tools: Some(vec![
                    TypeName::parse("registered.request").unwrap(),
                    TypeName::parse(TOOL).unwrap(),
                ]),
                created_by: None,
            },
        )
        .await
        .unwrap();
    }

    pub async fn workflow(&self) -> Workflow {
        let f = Fixture {
            _db: self.f._db.clone(),
            persistence: self.f.persistence.clone(),
            target: DefinitionTarget {
                workflow: WorkflowId::new(Uuid::new_v4()),
                ..self.f.target
            },
        };
        let p = &f.persistence;
        let definitions = DefinitionService::new(
            Preflight,
            p.clone(),
            WorkflowSourceDecoder,
            AgentPublicationDirectory {
                persistence: p.clone(),
                inner: Preflight,
            },
        );
        let mut source: Value =
            serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
        source["workflow_id"] = json!(f.target.workflow.to_string());
        source["resources"] = json!([]);
        let mut draft = f.save(None);
        draft.content.source = source.to_string();
        definitions.save(draft).await.unwrap();
        let mut publication = f.publish();
        publication.key =
            IdempotencyKey::parse(format!("e2e-publication:{}", f.target.workflow)).unwrap();
        definitions.publish(publication.clone()).await.unwrap();
        Workflow {
            target: bind_without_slots(&f, publication.version).await,
            version: publication.version,
        }
    }

    pub fn admission_identity(&self, key: IdempotencyKey) -> AdmissionIdentity {
        AdmissionIdentity {
            key,
            trigger: TriggerRef::new(
                self.scope.company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation: CorrelationId::new(),
        }
    }

    pub fn admission_request(
        &self,
        workflow: &Workflow,
        identity: &AdmissionIdentity,
    ) -> AdmitWorkflowRequest {
        AdmitWorkflowRequest {
            history_session: None,
            company_id: self.scope.company,
            actor: self.scope.actor,
            association: RelatedAssociation::Company,
            trigger: identity.trigger.clone(),
            correlation_id: identity.correlation,
            binding_id: workflow.target.binding,
            idempotency_key: identity.key.clone(),
            input: json!({}),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        }
    }

    pub async fn admit(&self, workflow: &Workflow) -> RunId {
        let p = &self.f.persistence;
        let identity =
            self.admission_identity(IdempotencyKey::parse(Uuid::new_v4().to_string()).unwrap());
        let request = self.admission_request(workflow, &identity);
        let result = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
            .admit(request)
            .await
            .unwrap();
        let AdmissionResult::Created(run) = result else {
            panic!("new admission")
        };
        run
    }

    pub async fn publication_request(&self, key: &str) -> PublishAgent {
        let state = self
            .f
            .persistence
            .authoring_state(self.scope)
            .await
            .unwrap();
        PublishAgent {
            scope: self.scope,
            expected_draft_revision: state.agent.draft_revision,
            expected_active_revision: state.active.map(|a| a.id),
            key: AgentPublicationKey::parse(key).unwrap(),
        }
    }

    pub fn runtime(
        &self,
        server: &McpServer,
    ) -> WorkflowAgentRuntime<PostgresPersistence, AgentActionTools> {
        let p = &self.f.persistence;
        let http = HttpRuntime::new(
            p.clone(),
            HttpDestinationPolicy::new([self.http.origin.as_str().to_owned()]).unwrap(),
            lease(),
        );
        let mcp = McpRuntime::new(p.clone(), guarded_client(&server.url), lease());
        WorkflowAgentRuntime::new(
            ProviderRegistry::standard().unwrap(),
            p.clone(),
            AgentActionTools::new(
                Arc::new(http),
                Arc::new(mcp),
                Arc::new(MessageRuntime::new(
                    p.clone(),
                    composer(p),
                    "agents.test".into(),
                    lease(),
                )),
            )
            .with_workflow_tools(Arc::new(p.clone())),
            Some(notices(p)),
            lease(),
        )
    }

    pub async fn process_pending(&self, server: &McpServer, runs: &[RunId]) {
        let p = &self.f.persistence;
        for _ in 0..16 {
            let candidates = p.poll_work(None, 16).await.unwrap().candidates;
            if candidates.is_empty() {
                break;
            }
            // Reconstruct the real handler/worker each poll; all correctness lives in native rows.
            let runtime = self.runtime(server);
            let worker = WorkflowWorker {
                port: p,
                handler: &runtime,
                notices: None,
                worker: WorkflowWorkerId(Uuid::new_v4()),
                lease: lease(),
                poll: PollPolicy::new(Duration::from_secs(1), 16).unwrap(),
                active_jobs: None,
                monitoring: None,
            };
            for candidate in candidates {
                assert!(
                    runs.contains(&candidate.scope.run),
                    "only this isolated journey's roots"
                );
                Box::pin(worker.process(candidate, &CancellationToken::new()))
                    .await
                    .unwrap();
            }
        }
    }

    pub async fn drive(&self, server: &McpServer, runs: &[RunId]) {
        // Box the shared worker loop so the fixture retains the stock-stack bound.
        Box::pin(self.process_pending(server, runs)).await;
        for run in runs {
            assert_eq!(self.run_state(*run).await, "succeeded");
        }
    }

    pub async fn run_state(&self, run: RunId) -> String {
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(self.scope.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(self.f.persistence.pool())
            .await
            .unwrap()
    }

    pub async fn assert_selection(
        &self,
        run: RunId,
        workflow: &Workflow,
        revision: &PublishedAgentRevision,
    ) {
        let view = self
            .f
            .persistence
            .run_view(RunReadScope {
                company: self.scope.company,
                run,
                viewer: self.scope.actor,
            })
            .await
            .unwrap()
            .unwrap();
        let dependencies = view.dependencies.as_ref().unwrap();
        assert_eq!(dependencies.agents.len(), 1);
        let selected = &dependencies.agents[0];
        assert_eq!(selected.agent_id, self.scope.agent);
        assert_eq!(selected.revision_id, revision.id);
        assert_eq!(
            selected.model,
            Some(format!("{SCRIPTED_PROVIDER} / {SCRIPTED_MODEL}"))
        );
        assert_eq!(selected.bindings.len(), 2);
        assert_eq!(selected.assignments.len(), 1);
        assert_eq!(selected.assignments[0].skill, self.skill.id);
        assert!(
            dependencies
                .workflows
                .iter()
                .any(|w| w.version_id == workflow.version)
        );
        assert_eq!(view.calls.items.len(), 2);
        for call in &view.calls.items {
            assert_eq!(call.origin.revision, revision.id);
            assert!(
                revision
                    .definition
                    .tools
                    .iter()
                    .any(|t| t.binding.id == call.origin.binding)
            );
        }
    }
}

pub(super) fn turns(runs: usize) -> Vec<LlmTurn> {
    (0..runs)
        .flat_map(|_| {
            [
                LlmTurn::tool_call(
                    "registered__request",
                    json!({"method":"POST","path":"/record"}),
                ),
                LlmTurn::tool_call(TOOL, json!({"customer_id":"c-42"})),
                LlmTurn::text("\"complete\""),
            ]
        })
        .collect()
}
