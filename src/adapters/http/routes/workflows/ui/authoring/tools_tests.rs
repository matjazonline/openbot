//! Workflow tools and the agent pane's cards: register, re-pin, re-point and revoke with what
//! each does to published versions said first; grants against the list seen; "used by" through
//! a diamond of child workflows, which the version's `agent_ids` index stores once (gate
//! P09-B1b-D1).
use super::*;

impl Fixture {
    pub(super) async fn register(&self, workflow: Uuid, fields: &[(&str, &str)]) -> String {
        register_tool(
            self.ui(self.owner),
            Path(workflow),
            self.query(),
            form(fields),
        )
        .await
        .unwrap()
        .0
    }

    pub(super) async fn publish_with_workflow_tool(
        &self,
        agent: Uuid,
        name: &str,
        workflow: Uuid,
        version: Uuid,
    ) {
        use crate::application::agent::*;
        use crate::domain::workflow::TypeName;
        use crate::entities::agent_capability::*;
        let cases = self.ui(self.owner).agents;
        let stored = cases
            .get_company_agent(self.owner, self.company, agent)
            .await
            .unwrap()
            .unwrap();
        let saved = cases
            .update_agent(
                self.owner,
                self.company,
                CompanyAgentUpdate {
                    agent: AgentId::new(agent),
                    expected_revision: stored.draft_revision,
                    write: AgentWrite {
                        name: stored.name,
                        slug: stored.slug,
                        provider: stored.provider,
                        model: stored.model,
                        ..Default::default()
                    },
                    tools: Some(vec![AgentToolAssignment {
                        binding: None,
                        alias: AgentToolAlias::parse(name).unwrap(),
                        choice: AgentToolChoice::Workflow {
                            registration: TypeName::parse(name).unwrap(),
                            workflow: WorkflowId::new(workflow),
                            version: VersionId::new(version),
                            params_digest: AgentParameterDigest::of(&json!({})).unwrap(),
                        },
                    }]),
                },
            )
            .await
            .unwrap();
        self.engine
            .agent_publications()
            .publish_agent(PublishAgent {
                scope: AgentPublicationScope {
                    company: CompanyId::new(self.company),
                    actor: WorkflowActor::authenticated(self.owner).unwrap(),
                    agent: AgentId::new(agent),
                },
                expected_draft_revision: saved.draft_revision,
                expected_active_revision: self
                    .engine
                    .agent_authoring_reads()
                    .authoring_state(AgentPublicationScope {
                        company: CompanyId::new(self.company),
                        actor: WorkflowActor::authenticated(self.owner).unwrap(),
                        agent: AgentId::new(agent),
                    })
                    .await
                    .unwrap()
                    .active
                    .map(|active| active.id),
                key: AgentPublicationKey::parse(Uuid::new_v4().to_string()).unwrap(),
            })
            .await
            .unwrap();
    }

    async fn agent_ids(&self, version: Uuid) -> Vec<Uuid> {
        sqlx::query_scalar("SELECT agent_ids FROM workflow_versions WHERE id = $1")
            .bind(version)
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }
}

/// A workflow whose input and output a tool call can carry.
pub(super) fn tool_source(workflow: Uuid) -> String {
    let mut source = example("data.map", workflow);
    source["input_schema"] = json!({"type": "object", "properties": {"key": {"type": "string"}}});
    source["output_schema"] = json!(true);
    serde_json::to_string_pretty(&source).unwrap()
}

#[tokio::test]
async fn a_tool_name_is_registered_re_pinned_re_pointed_and_revoked_saying_what_each_does() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let a1 = f.published(a, "Orders", &tool_source(a)).await.to_string();
    let a2 = f.published(a, "Orders", &tool_source(a)).await.to_string();
    let b1 = f.published(b, "Refunds", &tool_source(b)).await.to_string();
    let versions = f.pane(f.owner, a, "versions").await.unwrap();
    assert!(versions.contains("Use as a workflow tool"), "{versions}");

    let registered = f
        .register(
            a,
            &[
                ("name", "lookup"),
                ("version_id", &a1),
                ("params_json", "{}"),
            ],
        )
        .await;
    assert!(registered.contains("Registered lookup"), "{registered}");
    let listed = tools_pane(f.ui(f.owner), f.query()).await.unwrap();
    let listed = text(listed).await;
    assert!(
        listed.contains("lookup") && listed.contains("Orders"),
        "{listed}"
    );
    assert!(
        listed.contains("Revoking refuses new runs whose selected agents need this workflow tool")
    );

    // The name is taken: the form says what re-pinning does before anything is written.
    let asked = f
        .register(
            a,
            &[
                ("name", "lookup"),
                ("version_id", &a2),
                ("params_json", "{}"),
            ],
        )
        .await;
    assert!(
        asked.contains("registered for this workflow already"),
        "{asked}"
    );
    assert!(
        asked.contains("Re-pin tool") && asked.contains(r#"name="expected_revision" value="1""#)
    );
    let repinned = f
        .register(
            a,
            &[
                ("name", "lookup"),
                ("version_id", &a2),
                ("params_json", "{}"),
                ("expected_revision", "1"),
            ],
        )
        .await;
    assert!(repinned.contains("Re-pinned lookup"), "{repinned}");
    assert!(repinned.contains("Published agents keep their pinned workflow version"));

    // From another workflow: re-pointing refuses the old pins, said first.
    let asked = f
        .register(
            b,
            &[
                ("name", "lookup"),
                ("version_id", &b1),
                ("params_json", "{}"),
            ],
        )
        .await;
    assert!(asked.contains("registered for another workflow"), "{asked}");
    assert!(asked.contains("Point tool here"));
    let stale = f
        .register(
            b,
            &[
                ("name", "lookup"),
                ("version_id", &b1),
                ("params_json", "{}"),
                ("expected_revision", "1"),
            ],
        )
        .await;
    assert!(
        stale.contains("changed since the form was shown"),
        "{stale}"
    );
    let pointed = f
        .register(
            b,
            &[
                ("name", "lookup"),
                ("version_id", &b1),
                ("params_json", "{}"),
                ("expected_revision", "2"),
            ],
        )
        .await;
    assert!(
        pointed.contains("Pointed lookup at this workflow"),
        "{pointed}"
    );

    // Refusals stay in the form.
    let reserved = f
        .register(
            b,
            &[
                ("name", "message.send"),
                ("version_id", &b1),
                ("params_json", "{}"),
            ],
        )
        .await;
    assert!(
        reserved.contains("Not registered:") && reserved.contains("reserved"),
        "{reserved}"
    );
    let invalid = f
        .register(
            b,
            &[
                ("name", "Bad Name"),
                ("version_id", &b1),
                ("params_json", "{}"),
            ],
        )
        .await;
    assert!(invalid.contains("is not a valid tool name"), "{invalid}");
    let foreign = f
        .register(
            b,
            &[
                ("name", "other"),
                ("version_id", &a1),
                ("params_json", "{}"),
            ],
        )
        .await;
    assert!(
        foreign.contains("Choose a published version of this workflow"),
        "{foreign}"
    );

    let revoke = |revision: &'static str| {
        revoke_tool(
            f.ui(f.owner),
            f.query(),
            form(&[("name", "lookup"), ("expected_revision", revision)]),
        )
    };
    let stale = revoke("2").await.unwrap().0;
    assert!(stale.contains("changed or was revoked"), "{stale}");
    let revoked = revoke("3").await.unwrap().0;
    assert!(revoked.contains("Revoked lookup"), "{revoked}");
    assert!(revoked.contains("No workflow tools yet"));
}

#[tokio::test]
async fn autonomous_agent_authoring_usage_refuses_members_and_foreign_agents() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let agent = f.agent("Assistant", "assistant").await;
    for (user, company) in [(f.stranger, f.other), (f.member, f.company)] {
        let error = agent_usage(
            f.ui(user),
            Path(agent),
            Query(CompanyQuery {
                company_id: company,
            }),
        )
        .await
        .err();
        assert!(matches!(error, Some(AppError::NotFound(_))), "{error:?}");
    }
}

#[tokio::test]
async fn used_by_lists_a_binding_that_reaches_the_agent_only_through_a_child_diamond() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let agent = f.agent("Assistant", "assistant").await;
    f.publish_usage_agent(agent).await;
    // The grandchild runs the agent; two children each call it; the parent calls both.
    let grandchild = Uuid::new_v4();
    let mut source = example("agent.run", grandchild);
    source["input_schema"] = json!({"type": "object", "properties": {"text": {"type": "string", "maxLength": 128}}, "required": ["text"]});
    source["output_schema"] = json!(true);
    let grandchild_version = f
        .published(
            grandchild,
            "Drafter",
            &serde_json::to_string_pretty(&source).unwrap(),
        )
        .await;
    assert_eq!(f.agent_ids(grandchild_version).await, [agent]);
    let caller = |workflow: Uuid, child: Uuid, version: Uuid| {
        let mut source = example("workflow.call", workflow);
        source["input_schema"] = json!({"type": "object", "properties": {"text": {"type": "string", "maxLength": 128}}, "required": ["text"]});
        source["output_schema"] = json!(true);
        source["steps"]["start"]["child_workflow_id"] = json!(child.to_string());
        source["steps"]["start"]["child_version_id"] = json!(version.to_string());
        source
    };
    let (left, right) = (Uuid::new_v4(), Uuid::new_v4());
    let mut children = Vec::new();
    for (workflow, title) in [(left, "Left"), (right, "Right")] {
        let source = caller(workflow, grandchild, grandchild_version);
        children.push(
            f.published(
                workflow,
                title,
                &serde_json::to_string_pretty(&source).unwrap(),
            )
            .await,
        );
    }
    let parent = Uuid::new_v4();
    let mut source = caller(parent, left, children[0]);
    let mut second = source["steps"]["start"].clone();
    second["child_workflow_id"] = json!(right.to_string());
    second["child_version_id"] = json!(children[1].to_string());
    source["steps"]["start"]["routes"] = json!({"success": "second"});
    source["steps"]["second"] = second;
    let parent_version = f
        .published(
            parent,
            "Router",
            &serde_json::to_string_pretty(&source).unwrap(),
        )
        .await;
    // Stored once, though two paths reach it.
    assert_eq!(f.agent_ids(parent_version).await, [agent]);
    let plain = Uuid::new_v4();
    let plain_version = f
        .published(plain, "Plain", &source_of("data.map", plain))
        .await;
    assert!(f.agent_ids(plain_version).await.is_empty());

    let usage = async || {
        agent_usage(f.ui(f.owner), Path(agent), f.query())
            .await
            .unwrap()
            .0
    };
    let none = usage().await;
    assert!(
        none.contains("No binding's version froze this agent"),
        "{none}"
    );
    let configure = async |binding: Uuid, expected: Option<u64>, version: Uuid| {
        crate::adapters::http::routes::workflows::bindings::configure_binding(
            &f.engine,
            f.company,
            WorkflowActor::authenticated(f.owner).unwrap(),
            binding,
            crate::adapters::http::routes::workflows::bindings::BindingRequest {
                expected_state_revision: expected,
                association: crate::application::workflow::RelatedAssociation::Company,
                version,
                params: json!({}),
                resources: BTreeMap::new(),
                run_as: None,
                message_trigger: None,
            },
        )
        .await
        .unwrap()
    };
    let binding = Uuid::new_v4();
    configure(binding, None, parent_version).await;
    let listed = usage().await;
    assert!(
        listed.contains("Router") && listed.contains("Company-wide"),
        "{listed}"
    );
    // Configured off that version, the binding no longer runs the agent.
    configure(binding, Some(1), plain_version).await;
    let gone = usage().await;
    assert!(
        gone.contains("No binding's version froze this agent"),
        "{gone}"
    );
}

impl Fixture {
    async fn publish_usage_agent(&self, agent: Uuid) {
        use crate::application::agent::{AgentPublicationKey, AgentPublicationScope, PublishAgent};
        let scope = AgentPublicationScope {
            company: CompanyId::new(self.company),
            actor: WorkflowActor::authenticated(self.owner).unwrap(),
            agent: crate::entities::agent_capability::AgentId::new(agent),
        };
        let publications = self.engine.agent_publications();
        let preview = publications.preview_agent(scope).await.unwrap();
        publications
            .publish_agent(PublishAgent {
                scope,
                expected_draft_revision: preview.draft_revision,
                expected_active_revision: preview.active_revision,
                key: AgentPublicationKey::parse(Uuid::new_v4().to_string()).unwrap(),
            })
            .await
            .unwrap();
    }
}
