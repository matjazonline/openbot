//! Workflow tool registrations (07-D2 §D2.1–§D2.2, P07-Q1): admin commands with revisions,
//! bounds and authority; pins and grants confined to the agent's company (user requirement); and
//! the publication rules a granted workflow tool is held to.
use super::*;
use crate::adapters::persistence::workflow::http::{
    HttpPublicationDirectory, HttpPublicationSelection,
};
use crate::application::agent::{ActionEffect, ActionRecovery, ApprovedActionPolicy};
use crate::application::workflow::{
    http::{HttpConnectionWrite, HttpConnections, HttpOperation, HttpOrigin},
    publication::WorkflowToolPin,
    tools::{MAX_WORKFLOW_TOOLS, WorkflowToolWrite, WorkflowTools},
};

use super::workflow_tool_support::*;

fn name(value: &str) -> TypeName {
    TypeName::parse(value).unwrap()
}

fn child_workflow() -> WorkflowId {
    WorkflowId::new(Uuid::from_u128(CHILD))
}

fn create(
    tool: &str,
    workflow: WorkflowId,
    version: VersionId,
    params: Value,
) -> WorkflowToolWrite {
    WorkflowToolWrite {
        name: name(tool),
        workflow,
        version,
        params,
        expected: None,
    }
}

async fn registrations(p: &PostgresPersistence, company: CompanyId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_tool_registrations WHERE company_id=$1")
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap()
}

/// Change only the mutable draft; execution changes require explicit publication.
async fn grant(setup: &ToolSetup, tools: &[&str]) {
    crate::adapters::persistence::agent_capability::assign_test_tool_names(
        setup.persistence(),
        setup.agent,
        tools,
    )
    .await
    .unwrap();
}

#[path = "workflow_tool_closure_tests.rs"]
mod closure;

#[tokio::test]
async fn workflow_tool_registration_commands_keep_revisions_authority_and_bounds() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let (company, admin) = (setup.fixture.target.company, setup.fixture.target.actor);
    let first = setup.registration().await;
    assert_eq!(
        (first.workflow, first.version, first.revision),
        (child_workflow(), setup.child, 1)
    );
    let params = |reviewer: &str| json!({"reviewer": reviewer});
    // A second create of the name and a stale replacement conflict.
    let duplicate = create(TOOL, child_workflow(), setup.child, params("other"));
    assert!(matches!(
        p.register_workflow_tool(company, admin, duplicate.clone())
            .await,
        Err(AppError::Conflict(_))
    ));
    let stale = WorkflowToolWrite {
        expected: Some(2),
        ..duplicate.clone()
    };
    assert!(matches!(
        p.register_workflow_tool(company, admin, stale).await,
        Err(AppError::Conflict(_))
    ));
    let replaced = p
        .register_workflow_tool(
            company,
            admin,
            WorkflowToolWrite {
                expected: Some(1),
                ..duplicate
            },
        )
        .await
        .unwrap();
    assert_eq!((replaced.revision, replaced.params), (2, params("other")));
    assert!(matches!(
        p.revoke_workflow_tool(company, admin, name(TOOL), 1).await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(setup.registration().await.revision, 2);

    // Only the owner or an admin member manages registrations; others see nothing.
    let plain = WorkflowActor::authenticated(member(&setup.fixture, "member").await).unwrap();
    let outsider = WorkflowActor::authenticated(Uuid::new_v4()).unwrap();
    for actor in [plain, outsider] {
        let write = create("other.review", child_workflow(), setup.child, params("x"));
        assert!(matches!(
            p.register_workflow_tool(company, actor, write).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            p.revoke_workflow_tool(company, actor, name(TOOL), 2).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            p.workflow_tools(company, actor).await,
            Err(AppError::NotFound(_))
        ));
    }
    let admin_member = WorkflowActor::authenticated(member(&setup.fixture, "admin").await).unwrap();
    assert_eq!(
        p.workflow_tools(company, admin_member).await.unwrap().len(),
        1
    );

    // Unknown versions are not found; refused names and params write nothing.
    let unknown = create(
        "other.review",
        child_workflow(),
        VersionId::new(Uuid::new_v4()),
        params("x"),
    );
    assert!(matches!(
        p.register_workflow_tool(company, admin, unknown).await,
        Err(AppError::NotFound(_))
    ));
    for refused in [
        create("message.send", child_workflow(), setup.child, params("x")),
        // p08 D2.1: the provisioning built-ins are reserved too.
        create(
            "create_agent_channel",
            child_workflow(),
            setup.child,
            params("x"),
        ),
        create(
            "list_company_agents",
            child_workflow(),
            setup.child,
            params("x"),
        ),
        create(
            "other.review",
            child_workflow(),
            setup.child,
            json!({"reviewer": 42}),
        ),
        create("other.review", child_workflow(), setup.child, json!(["x"])),
        create(
            "other.review",
            child_workflow(),
            setup.child,
            json!({"reviewer": "x".repeat(70_000)}),
        ),
    ] {
        assert!(matches!(
            p.register_workflow_tool(company, admin, refused).await,
            Err(AppError::BadRequest(_))
        ));
    }
    assert_eq!(registrations(p, company).await, 1);

    // At most 256 per company, checked under the company lock: of two competing creates at
    // 255, exactly one is written.
    sqlx::query(
        "INSERT INTO workflow_tool_registrations(company_id,name,workflow_id,version_id,params) \
         SELECT $1, 'bulk.tool_' || n, $2, $3, $4 FROM generate_series(1, $5) AS n",
    )
    .bind(company.as_uuid())
    .bind(child_workflow().as_uuid())
    .bind(setup.child.as_uuid())
    .bind(params("x"))
    .bind(MAX_WORKFLOW_TOOLS as i32 - 2)
    .execute(p.pool())
    .await
    .unwrap();
    let (one, two) = tokio::join!(
        p.register_workflow_tool(
            company,
            admin,
            create("race.one", child_workflow(), setup.child, params("x"))
        ),
        p.register_workflow_tool(
            company,
            admin,
            create("race.two", child_workflow(), setup.child, params("x"))
        ),
    );
    let outcomes = [one, two];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(
        outcomes
            .iter()
            .any(|result| matches!(result, Err(AppError::BadRequest(_))))
    );
    assert_eq!(
        p.workflow_tools(company, admin).await.unwrap().len(),
        MAX_WORKFLOW_TOOLS
    );

    // Two replacements of one revision: exactly one applies.
    let current = setup.registration().await;
    let replace = |reviewer: &str| WorkflowToolWrite {
        name: name(TOOL),
        workflow: child_workflow(),
        version: setup.child,
        params: params(reviewer),
        expected: Some(current.revision),
    };
    let (one, two) = tokio::join!(
        p.register_workflow_tool(company, admin, replace("one")),
        p.register_workflow_tool(company, admin, replace("two")),
    );
    let outcomes = [one, two];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(
        outcomes
            .iter()
            .any(|result| matches!(result, Err(AppError::Conflict(_))))
    );
    assert_eq!(setup.registration().await.revision, current.revision + 1);

    // Revoking deletes the name; a second revoke finds nothing.
    let current = setup.registration().await;
    p.revoke_workflow_tool(company, admin, name(TOOL), current.revision)
        .await
        .unwrap();
    assert!(matches!(
        p.revoke_workflow_tool(company, admin, name(TOOL), current.revision)
            .await,
        Err(AppError::NotFound(_))
    ));

    // An archived workflow's versions cannot be pinned.
    let child_target = DefinitionTarget {
        workflow: child_workflow(),
        ..setup.fixture.target
    };
    setup
        .fixture
        .service()
        .archive(child_target, DraftRevision::new(1).unwrap())
        .await
        .unwrap();
    assert!(matches!(
        p.register_workflow_tool(
            company,
            admin,
            create(TOOL, child_workflow(), setup.child, params("x"))
        )
        .await,
        Err(AppError::NotFound(_))
    ));
}

/// User requirement (P07-Q1): the workflows an agent lists as tools are its own company's. A
/// registration cannot pin another company's version, by the service or by SQL, and an agent
/// granted a name registered only in another company cannot publish.
#[tokio::test]
async fn workflow_tool_pins_and_grants_stay_within_the_agents_company() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let (company, admin) = (setup.fixture.target.company, setup.fixture.target.actor);
    // Company B, in the same database, publishes the same workflow id and registers both the
    // same tool name and a name company A does not have.
    let other = Fixture::in_database(setup.fixture._db.clone(), "-other").await;
    let foreign = other.publish_workflow(child_workflow(), echo_child()).await;
    for tool in [TOOL, "refund.foreign"] {
        other
            .persistence
            .register_workflow_tool(
                other.target.company,
                other.target.actor,
                create(tool, child_workflow(), foreign, json!({"reviewer": "b"})),
            )
            .await
            .unwrap();
    }

    // Company A cannot pin B's version: the same answer as an unknown version, nothing written.
    let before = registrations(p, company).await;
    assert!(matches!(
        p.register_workflow_tool(
            company,
            admin,
            create(
                "refund.foreign",
                child_workflow(),
                foreign,
                json!({"reviewer": "a"})
            )
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        p.register_workflow_tool(
            company,
            admin,
            WorkflowToolWrite {
                expected: Some(1),
                ..create(TOOL, child_workflow(), foreign, json!({"reviewer": "a"}))
            }
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(registrations(p, company).await, before);
    assert_eq!(setup.registration().await.version, setup.child);
    // The schema refuses it too.
    let error = sqlx::query(
        "INSERT INTO workflow_tool_registrations(company_id,name,workflow_id,version_id) \
         VALUES($1,'refund.foreign',$2,$3)",
    )
    .bind(company.as_uuid())
    .bind(child_workflow().as_uuid())
    .bind(foreign.as_uuid())
    .execute(p.pool())
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );

    // Concrete draft bindings reject B's registration before any publication can capture it.
    let refused = crate::adapters::persistence::agent_capability::assign_test_tool_names(
        setup.persistence(),
        setup.agent,
        &["refund.foreign"],
    )
    .await;
    assert!(matches!(refused, Err(AppError::BadRequest(_))));
    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT ARRAY(SELECT alias FROM agent_draft_bindings WHERE agent_id=$1 ORDER BY position)",
    )
    .bind(setup.agent)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(!stored.iter().any(|alias| alias == "refund.foreign"));

    // A's own registration freezes A's child, though B registered the same name.
    grant(&setup, &[TOOL]).await;
    let published = setup.fixture.publish_agent(setup.agent).await;
    let version = publish_parent(&setup.fixture, "own-grant").await.unwrap();
    let bundle = p
        .selectable_version(company, version)
        .await
        .unwrap()
        .unwrap();
    let tool = crate::application::workflow::agent_contract::agent_action_snapshot(
        company,
        &published.definition.tools[0],
    )
    .unwrap();
    assert_eq!(tool.company_id, company);
    assert_eq!(
        tool.workflow,
        Some(WorkflowToolPin {
            workflow_id: child_workflow(),
            version_id: setup.child,
            params: json!({"reviewer":setup.fixture.principal(setup.reviewer).await.to_string()})
        })
    );
    assert_eq!(bundle.snapshots().agents[0].agent_id.as_uuid(), setup.agent);
    assert!(
        bundle
            .snapshots()
            .tools
            .iter()
            .all(|tool| tool.contract.name.as_str() != TOOL),
        "agent child capabilities are not copied into STEP tools"
    );
    assert!(bundle.children().is_empty());
    let AdmissionResult::Created(run) = setup
        .admit(setup.target, TriggerId::new(Uuid::new_v4()), "scoped-pin")
        .await
        .unwrap()
    else {
        panic!("new admission")
    };
    let captured:Uuid = sqlx::query_scalar("SELECT member.company_id FROM run_dependency_manifests AS ownership JOIN agent_manifest_workflows AS member ON member.company_id=ownership.company_id AND member.manifest_id=ownership.manifest_id WHERE ownership.company_id=$1 AND ownership.run_id=$2 AND member.version_id=$3")
        .bind(company.as_uuid()).bind(run.as_uuid()).bind(setup.child.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(captured, company.as_uuid());
    // And B's registrations are not A's to list.
    assert!(
        p.workflow_tools(company, admin)
            .await
            .unwrap()
            .iter()
            .all(|tool| tool.version != foreign)
    );
}

fn http_operation(company: CompanyId, tool: &str) -> HttpOperation {
    HttpOperation {
        tool: ToolSnapshot {
            workflow: None,
            company_id: company,
            contract: registry::ToolContract {
                name: name(tool),
                connection: None,
                input_schema: json!({"type":"object","properties":{"method":{"type":"string"},"path":{"type":"string"}},"required":["method","path"],"additionalProperties":false}),
                output_schema: json!(true),
            },
            policy: ApprovedActionPolicy {
                capability: name("http.request"),
                policy_revision: 1,
                effect: ActionEffect::Write,
                recovery: ActionRecovery::Reconcile,
            },
        },
        approval_required: false,
    }
}

#[path = "workflow_tool_reference_tests.rs"]
mod references;
