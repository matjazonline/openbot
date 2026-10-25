//! Workflow tool registrations (07-D2 §D2.1–§D2.2, P07-Q1): admin commands with revisions,
//! bounds and authority; pins and grants confined to the agent's company (user requirement); and
//! the publication rules a granted workflow tool is held to.
use super::*;
use crate::adapters::persistence::workflow::http::{
    HttpPublicationDirectory, HttpPublicationSelection,
};
use crate::application::workflow::{
    http::{HttpConnectionWrite, HttpConnections, HttpOperation, HttpOrigin},
    lifecycle::LifecycleError,
    publication::{ActionEffect, ActionRecovery, ApprovedActionPolicy, WorkflowToolPin},
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

/// Grant the fixture's `assistant` exactly `tools`.
async fn grant(setup: &ToolSetup, tools: &[&str]) {
    sqlx::query("UPDATE agents SET workflow_tools=$2 WHERE id=$1")
        .bind(setup.agent)
        .bind(tools)
        .execute(setup.persistence().pool())
        .await
        .unwrap();
}

fn bad_request(result: LifecycleResult<VersionId>) -> String {
    match result {
        Err(LifecycleError::Application(AppError::BadRequest(message))) => message,
        Err(LifecycleError::Validation(diagnostic)) => panic!("{}", diagnostic.code),
        Err(LifecycleError::Application(error)) => panic!("{error:?}"),
        Ok(_) => panic!("publication must be refused"),
    }
}

fn diagnostic(result: LifecycleResult<VersionId>) -> &'static str {
    match result {
        Err(LifecycleError::Validation(diagnostic)) => diagnostic.code,
        Err(LifecycleError::Application(error)) => panic!("{error:?}"),
        Ok(_) => panic!("publication must be refused"),
    }
}

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

    // A's agent granted a name only B registered cannot publish.
    grant(&setup, &["refund.foreign"]).await;
    let message = bad_request(publish_parent(&setup.fixture, "foreign-grant").await);
    assert!(message.contains("refund.foreign"), "{message}");

    // A's own registration freezes A's child, though B registered the same name.
    grant(&setup, &[TOOL]).await;
    let version = publish_parent(&setup.fixture, "own-grant").await.unwrap();
    let bundle = p
        .selectable_version(company, version)
        .await
        .unwrap()
        .unwrap();
    let tool = bundle
        .snapshots()
        .tools
        .iter()
        .find(|tool| tool.contract.name.as_str() == TOOL)
        .unwrap();
    assert_eq!(tool.company_id, company);
    assert_eq!(
        tool.workflow,
        Some(WorkflowToolPin {
            workflow_id: child_workflow(),
            version_id: setup.child,
            params: json!({"reviewer": setup.reviewer.to_string()}),
        })
    );
    assert_eq!(bundle.children().len(), 1);
    assert_eq!(bundle.children()[&setup.child].company_id(), company);
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

/// A granted workflow tool freezes the child's own contract and its params; a same-name HTTP
/// operation or captured step tool is refused as ambiguous (F022); the child's resource slots
/// must be the parent's; a tool cannot reach the root workflow.
#[tokio::test]
async fn workflow_tool_publication_freezes_the_child_contract_and_refuses_conflicts() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let (company, admin) = (setup.fixture.target.company, setup.fixture.target.actor);
    let bundle = p
        .selectable_version(company, setup.parent)
        .await
        .unwrap()
        .unwrap();
    let child = p
        .selectable_version(company, setup.child)
        .await
        .unwrap()
        .unwrap();
    let definition = child.compiled().graph().definition();
    let tool = bundle
        .snapshots()
        .tools
        .iter()
        .find(|tool| tool.contract.name.as_str() == TOOL)
        .unwrap();
    assert_eq!(tool.contract.connection, None);
    assert_eq!(
        Some(&tool.contract.input_schema),
        definition.input_schema.as_ref()
    );
    assert_eq!(
        Some(&tool.contract.output_schema),
        definition.output_schema.as_ref()
    );
    assert_eq!(tool.policy.capability.as_str(), "workflow.tool");
    assert_eq!(tool.policy.recovery, ActionRecovery::SafeRepeat);
    assert_eq!(
        tool.workflow.as_ref().unwrap().params,
        json!({"reviewer": setup.reviewer.to_string()})
    );
    assert_eq!(
        bundle.children()[&setup.child].content_hash(),
        child.content_hash()
    );

    // A same-name HTTP operation makes the agent's grant ambiguous.
    let connection = p
        .create_http(
            company,
            admin,
            HttpConnectionWrite {
                origin: HttpOrigin::parse("https://tools.example.test").unwrap(),
                bearer: false,
            },
        )
        .await
        .unwrap();
    let connection = p
        .register_http_operation(
            company,
            admin,
            connection.id,
            connection.revision,
            http_operation(company, TOOL),
        )
        .await
        .unwrap();
    // An HTTP operation never carries a workflow pin.
    let mut pinned = http_operation(company, "pinned.request");
    pinned.tool.workflow = Some(setup.frozen_pin(setup.parent).await);
    assert!(matches!(
        p.register_http_operation(company, admin, connection.id, connection.revision, pinned)
            .await,
        Err(AppError::BadRequest(_))
    ));
    let message = bad_request(publish_parent(&setup.fixture, "http-same-name").await);
    assert!(
        message.contains("registered both as a workflow and as an HTTP operation"),
        "{message}"
    );
    // A step's captured tool of the same name is not the registration either (F022).
    let mut slotted: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    slotted["resources"] = json!([{"slot":"service","kind":"http"}]);
    let selection = HttpPublicationDirectory {
        persistence: p.clone(),
        selections: vec![HttpPublicationSelection {
            connection: connection.id,
            slot: Some(ResourceName::parse("service").unwrap()),
            operation: name(TOOL),
        }],
    };
    let message = bad_request(
        publish_parent_with(
            &setup.fixture,
            "step-same-name",
            slotted.to_string(),
            selection,
        )
        .await,
    );
    assert!(
        message.contains("conflicts with another tool of the same name"),
        "{message}"
    );
    p.revoke_http_operation(
        company,
        admin,
        connection.id,
        connection.revision,
        name(TOOL),
    )
    .await
    .unwrap();

    // The child's resource slots must be declared by the parent.
    let mut slotted_child = echo_child();
    slotted_child["resources"] = json!([{"slot":"service","kind":"mcp"}]);
    let slotted_workflow = WorkflowId::new(Uuid::from_u128(CHILD + 1));
    let slotted_version = setup
        .fixture
        .publish_workflow(slotted_workflow, slotted_child)
        .await;
    p.register_workflow_tool(
        company,
        admin,
        create(
            "slotted.review",
            slotted_workflow,
            slotted_version,
            json!({"reviewer": "x"}),
        ),
    )
    .await
    .unwrap();
    grant(&setup, &["slotted.review"]).await;
    assert_eq!(
        diagnostic(publish_parent(&setup.fixture, "undeclared-slot").await),
        "publication.workflow_tool"
    );
    let mut declared: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    declared["resources"] = json!([{"slot":"service","kind":"mcp"}]);
    publish_parent_with(
        &setup.fixture,
        "declared-slot",
        declared.to_string(),
        message::MessagePublicationDirectory {
            persistence: p.clone(),
        },
    )
    .await
    .unwrap();

    // A tool pinned to a version of the parent itself is recursion.
    p.register_workflow_tool(
        company,
        admin,
        create(
            "self.review",
            setup.fixture.target.workflow,
            setup.parent,
            json!({}),
        ),
    )
    .await
    .unwrap();
    grant(&setup, &["self.review"]).await;
    assert_eq!(
        diagnostic(publish_parent(&setup.fixture, "recursive").await),
        "publication.recursion"
    );
}
