//! Workflow tools and the agent pane's cards: register, re-pin, re-point and revoke with what
//! each does to published versions said first; grants against the list seen; "used by" through
//! a diamond of child workflows, which the version's `agent_ids` index stores once (gate
//! P09-B1b-D1).
use super::*;

impl Fixture {
    async fn register(&self, workflow: Uuid, fields: &[(&str, &str)]) -> String {
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

    async fn grant(&self, user: Uuid, agent: Uuid, fields: &[(&str, &str)]) -> AppResult<String> {
        grant_agent_tools(self.ui(user), Path(agent), self.query(), form(fields))
            .await
            .map(|html| html.0)
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
fn tool_source(workflow: Uuid) -> String {
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
    assert!(listed.contains("Revoking stops every published version that froze this tool"));

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
    assert!(repinned.contains("Published versions keep calling the version they froze"));

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
async fn an_agents_grants_save_against_the_list_seen_and_say_what_each_name_is() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let agent = f.agent("Assistant", "assistant").await;
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, "Orders", &tool_source(workflow))
        .await;
    f.register(
        workflow,
        &[
            ("name", "lookup"),
            ("version_id", &version.to_string()),
            ("params_json", "{}"),
        ],
    )
    .await;
    let card = agent_tools(f.ui(f.owner), Path(agent), f.query())
        .await
        .unwrap()
        .0;
    assert!(
        card.starts_with(r#"<section id="agent-workflow-tools""#),
        "{card}"
    );
    assert!(card.contains("Registered workflow tools not granted") && card.contains("lookup"));

    let saved = f
        .grant(
            f.owner,
            agent,
            &[
                ("expected", ""),
                ("tools", "lookup\n message.send \n\nhttp_lookup"),
            ],
        )
        .await
        .unwrap();
    assert!(saved.contains("Saved 3 granted tools"), "{saved}");
    assert!(saved.contains("workflow tool: registered for Orders"));
    assert!(saved.contains("built-in message tool"));
    assert!(saved.contains("resolved at publication"));
    assert_eq!(
        field_value(&saved, "expected"),
        "lookup\nmessage.send\nhttp_lookup"
    );

    // The card was opened before that save: nothing changes and the typed text stays.
    let stale = f
        .grant(f.owner, agent, &[("expected", ""), ("tools", "lookup")])
        .await
        .unwrap();
    assert!(
        stale.contains("The grants changed since this card was shown"),
        "{stale}"
    );
    assert!(stale.contains("\nlookup</textarea>"));
    let provisioning = f
        .grant(
            f.owner,
            agent,
            &[
                ("expected", "lookup\nmessage.send\nhttp_lookup"),
                ("tools", "lookup\nlist_company_agents"),
            ],
        )
        .await
        .unwrap();
    assert!(
        provisioning.contains("Not saved: Provisioning tools"),
        "{provisioning}"
    );
    let invalid = f
        .grant(f.owner, agent, &[("expected", ""), ("tools", "Not A Name")])
        .await
        .unwrap();
    assert!(invalid.contains("is not a valid tool name"), "{invalid}");
    let granted = f
        .engine
        .authoring_reads()
        .agent_tool_grants(CompanyId::new(f.company), agent)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(granted, ["lookup", "message.send", "http_lookup"]);

    // Another company's admin, naming its own company with this agent, finds no agent.
    let f = &f;
    let foreign = |user: Uuid, company: Uuid| {
        let query = move || {
            Query(CompanyQuery {
                company_id: company,
            })
        };
        async move {
            (
                agent_tools(f.ui(user), Path(agent), query()).await.err(),
                grant_agent_tools(
                    f.ui(user),
                    Path(agent),
                    query(),
                    form(&[
                        ("expected", "lookup\nmessage.send\nhttp_lookup"),
                        ("tools", ""),
                    ]),
                )
                .await
                .err(),
                agent_usage(f.ui(user), Path(agent), query()).await.err(),
            )
        }
    };
    for (user, company) in [(f.stranger, f.other), (f.member, f.company)] {
        let (read, write, usage) = foreign(user, company).await;
        for error in [read, write, usage] {
            assert!(matches!(error, Some(AppError::NotFound(_))), "{error:?}");
        }
    }
    assert_eq!(
        f.engine
            .authoring_reads()
            .agent_tool_grants(CompanyId::new(f.company), agent)
            .await
            .unwrap()
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn used_by_lists_a_binding_that_reaches_the_agent_only_through_a_child_diamond() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let agent = f.agent("Assistant", "assistant").await;
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

/// What the revoke and re-point warnings say is what happens to a version that froze the tool
/// (P07-D2 rev 4 §D2.2, §D2.7; F020): its binding can no longer be activated after a revoke,
/// re-registering the name for the same workflow restores it, and pointing the name at another
/// workflow stops it again.
#[tokio::test]
async fn revoking_or_re_pointing_a_frozen_tool_stops_its_versions_bindings_as_the_warning_says() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for fact in [
        "their new runs are refused",
        "their bindings can no longer be configured or activated",
        "their agents' next calls to it are refused",
        "A call already in progress still returns its result",
        "Registering the name again for the same workflow restores them",
    ] {
        assert!(pages::REVOKE_WARNING.contains(fact), "{fact}");
    }
    for fact in [
        "their new runs are refused",
        "their bindings can no longer be configured or activated",
        "A call already in progress still returns its result",
    ] {
        assert!(pages::REPOINT_WARNING.contains(fact), "{fact}");
    }
    assert!(!pages::REVOKE_WARNING.contains("already waiting"));
    assert!(pages::REPIN_NOTE.contains("published versions keep running"));

    let agent = f.agent("Assistant", "assistant").await;
    let (tool, other) = (Uuid::new_v4(), Uuid::new_v4());
    let tool_version = f
        .published(tool, "Orders", &tool_source(tool))
        .await
        .to_string();
    let other_version = f
        .published(other, "Refunds", &tool_source(other))
        .await
        .to_string();
    let registered = f
        .register(
            tool,
            &[
                ("name", "lookup"),
                ("version_id", &tool_version),
                ("params_json", "{}"),
            ],
        )
        .await;
    assert!(registered.contains("Registered lookup"), "{registered}");
    f.grant(f.owner, agent, &[("expected", ""), ("tools", "lookup")])
        .await
        .unwrap();
    // A version whose agent froze the tool, bound and active.
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, "Answer", &source_of("agent.run", workflow))
        .await;
    let binding = Uuid::new_v4();
    let actor = WorkflowActor::authenticated(f.owner).unwrap();
    use crate::adapters::http::routes::workflows::bindings as json_bindings;
    json_bindings::configure_binding(
        &f.engine,
        f.company,
        actor,
        binding,
        json_bindings::BindingRequest {
            expected_state_revision: None,
            association: crate::application::workflow::RelatedAssociation::Company,
            version,
            params: json!({}),
            resources: BTreeMap::new(),
            run_as: None,
            message_trigger: None,
        },
    )
    .await
    .unwrap();
    let state = |revision: u64, active: bool| {
        json_bindings::set_binding_activity(&f.engine, f.company, actor, binding, revision, active)
    };
    assert!(state(1, true).await.unwrap().active);
    let activate_in_page = |revision: u64| {
        activate_binding(
            f.ui(f.owner),
            Path((workflow, binding)),
            f.query(),
            axum::Form(ActivityForm {
                expected_state_revision: revision,
            }),
        )
    };

    let revoked = revoke_tool(
        f.ui(f.owner),
        f.query(),
        form(&[("name", "lookup"), ("expected_revision", "1")]),
    )
    .await
    .unwrap()
    .0;
    assert!(
        revoked.contains("can no longer start runs or call it"),
        "{revoked}"
    );
    state(2, false).await.unwrap();
    let refused = activate_in_page(3).await.unwrap().0;
    assert!(refused.contains("Not activated:"), "{refused}");
    assert!(!state(3, true).await.is_ok_and(|state| state.active));

    // The same name for the same workflow restores the version that froze it.
    f.register(
        tool,
        &[
            ("name", "lookup"),
            ("version_id", &tool_version),
            ("params_json", "{}"),
        ],
    )
    .await;
    let restored = activate_in_page(3).await.unwrap().0;
    assert!(restored.contains("Activated binding"), "{restored}");

    // Pointed at another workflow, it stops again.
    let pointed = f
        .register(
            other,
            &[
                ("name", "lookup"),
                ("version_id", &other_version),
                ("params_json", "{}"),
                ("expected_revision", "1"),
            ],
        )
        .await;
    assert!(
        pointed.contains("Pointed lookup at this workflow"),
        "{pointed}"
    );
    state(4, false).await.unwrap();
    let refused = activate_in_page(5).await.unwrap().0;
    assert!(refused.contains("Not activated:"), "{refused}");
}
