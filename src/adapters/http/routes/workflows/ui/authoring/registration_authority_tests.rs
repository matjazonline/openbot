//! Registration revocation retires agent binding authority; explicit publication creates a fresh incarnation.
use super::tools_tests::tool_source;
use super::*;

#[tokio::test]
async fn autonomous_agent_authoring_workflow_registration_revocation_refuses_admission_without_republishing_workflow()
 {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let agent = f.agent("Assistant", "assistant").await;
    let tool = Uuid::new_v4();
    let other = Uuid::new_v4();
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
    f.publish_with_workflow_tool(
        agent,
        "lookup",
        tool,
        Uuid::parse_str(&tool_version).unwrap(),
    )
    .await;
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

    let start = || {
        json_bindings::start_command(
            &f.engine,
            f.company,
            actor,
            binding,
            None,
            json!({"text":"Registration authority probe"}),
            Uuid::new_v4().to_string(),
            None,
        )
    };
    let original = start().await.unwrap().0;
    let revoked = revoke_tool(
        f.ui(f.owner),
        f.query(),
        form(&[("name", "lookup"), ("expected_revision", "1")]),
    )
    .await
    .unwrap()
    .0;
    assert!(revoked.contains("new calls to it are refused"), "{revoked}");
    state(2, false).await.unwrap();
    let configured = activate_in_page(3).await.unwrap().0;
    assert!(configured.contains("Activated binding"), "{configured}");
    let refusal = start()
        .await
        .map(|_| ())
        .expect_err("revoked registration refuses actual run admission");
    assert!(
        matches!(&refusal, crate::adapters::http::routes::workflows::WorkflowError::App(AppError::NotFound(message)) if message == "Workflow resource"),
        "{refusal:?}"
    );

    // Re-registering the target does not revive the revoked binding incarnation.
    f.register(
        tool,
        &[
            ("name", "lookup"),
            ("version_id", &tool_version),
            ("params_json", "{}"),
        ],
    )
    .await;
    let still_revoked = start()
        .await
        .map(|_| ())
        .expect_err("regrant cannot revive the previous agent binding");
    assert!(
        matches!(&still_revoked, crate::adapters::http::routes::workflows::WorkflowError::App(AppError::BadRequest(message)) if message == "Selected agent authority is no longer available"),
        "{still_revoked:?}"
    );
    // Box the newly published binding recovery phase under the stock test stack budget.
    let grants = Box::pin(republish_fresh_binding(
        &f,
        agent,
        tool,
        Uuid::parse_str(&tool_version).unwrap(),
        actor,
    ))
    .await;
    let restored = start().await.unwrap().0;
    assert_ne!(original, restored);
    for (run, revision) in [(original, grants.previous), (restored, grants.current)] {
        let selected: Uuid = sqlx::query_scalar("SELECT selection.revision_id FROM run_dependency_manifests AS ownership JOIN agent_manifest_revisions AS selection ON selection.company_id=ownership.company_id AND selection.manifest_id=ownership.manifest_id WHERE ownership.company_id=$1 AND ownership.run_id=$2 AND selection.agent_id=$3")
            .bind(f.company).bind(run.as_uuid()).bind(agent).fetch_one(f.persistence.pool()).await.unwrap();
        assert_eq!(selected, revision.as_uuid());
    }

    // Pointed at another workflow, it stops again.
    let expected_registration = f
        .engine
        .workflow_tools()
        .workflow_tools(CompanyId::new(f.company), actor)
        .await
        .unwrap()
        .into_iter()
        .find(|registration| registration.name.as_str() == "lookup")
        .unwrap()
        .revision
        .to_string();
    let pointed = f
        .register(
            other,
            &[
                ("name", "lookup"),
                ("version_id", &other_version),
                ("params_json", "{}"),
                ("expected_revision", &expected_registration),
            ],
        )
        .await;
    assert!(
        pointed.contains("Pointed lookup at this workflow"),
        "{pointed}"
    );
    assert!(
        start().await.is_err(),
        "repointing cannot silently retarget an immutable binding"
    );
    let versions: Vec<Uuid> = sqlx::query_scalar(
        "SELECT version_id FROM workflow_runs WHERE company_id=$1 AND binding_id=$2 ORDER BY id",
    )
    .bind(f.company)
    .bind(binding)
    .fetch_all(f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(
        versions,
        [version, version],
        "both admitted runs use the same workflow version"
    );
}

struct RepublishedGrants {
    previous: crate::entities::agent_capability::AgentRevisionId,
    current: crate::entities::agent_capability::AgentRevisionId,
}

async fn republish_fresh_binding(
    f: &Fixture,
    agent: Uuid,
    tool: Uuid,
    version: Uuid,
    actor: WorkflowActor,
) -> RepublishedGrants {
    let agent_scope = crate::application::agent::AgentPublicationScope {
        company: CompanyId::new(f.company),
        actor,
        agent: crate::entities::agent_capability::AgentId::new(agent),
    };
    let old = f
        .engine
        .agent_authoring_reads()
        .authoring_state(agent_scope)
        .await
        .unwrap()
        .active
        .unwrap();
    assert_eq!(
        old.tools[0].access,
        crate::application::agent::AgentGrantState::Revoked
    );
    f.publish_with_workflow_tool(agent, "lookup", tool, version)
        .await;
    let current = f
        .engine
        .agent_authoring_reads()
        .authoring_state(agent_scope)
        .await
        .unwrap()
        .active
        .unwrap();
    assert_ne!(current.id, old.id);
    assert_ne!(
        current.tools[0].assignment.binding,
        old.tools[0].assignment.binding
    );

    RepublishedGrants {
        previous: old.id,
        current: current.id,
    }
}
