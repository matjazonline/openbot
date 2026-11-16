use super::*;
use crate::application::workflow::agent::AgentToolObservation;
use crate::application::workflow::agent_contract::agent_action_snapshot;

#[tokio::test]
async fn autonomous_agent_workflow_reference_keeps_the_child_contract_in_the_agent_revision() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let company = setup.fixture.target.company;
    let published = setup.fixture.publish_agent(setup.agent).await;
    let child = p
        .selectable_version(company, setup.child)
        .await
        .unwrap()
        .unwrap();
    let definition = child.compiled().graph().definition();
    let tool = agent_action_snapshot(company, &published.definition.tools[0]).unwrap();
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
        json!({"reviewer":setup.fixture.principal(setup.reviewer).await.to_string()})
    );
    let bundle = p
        .selectable_version(company, setup.parent)
        .await
        .unwrap()
        .unwrap();
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
    let claim = setup.claim("canonical-child").await;
    let call = setup
        .call(setup.initial_pin(), json!({"question":"q"}), &claim)
        .await;
    assert!(matches!(
        setup.dispatch(&claim, &call).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let exact:bool=sqlx::query_scalar("SELECT child.bundle_digest=sha256(version.bundle) FROM workflow_runs AS child JOIN workflow_versions AS version ON version.company_id=child.company_id AND version.workflow_id=child.workflow_id AND version.id=child.version_id WHERE child.company_id=$1 AND child.parent_run_id=$2")
        .bind(company.as_uuid()).bind(claim.fence.scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(
        exact,
        "the actual child runs the canonical immutable bundle"
    );
}

#[tokio::test]
async fn autonomous_agent_workflow_reference_step_http_alias_cannot_redirect_agent_child() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let company = setup.fixture.target.company;
    let admin = setup.fixture.target.actor;
    let connection = p
        .create_http(
            company,
            admin,
            HttpConnectionWrite {
                origin: HttpOrigin::parse("https://tools.fixture.test").unwrap(),
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
    let mut invalid = http_operation(company, "pinned.request");
    invalid.tool.workflow = Some(setup.initial_pin());
    assert!(matches!(
        p.register_http_operation(company, admin, connection.id, connection.revision, invalid)
            .await,
        Err(AppError::BadRequest(_))
    ));
    publish_parent(&setup.fixture, "http-same-name")
        .await
        .unwrap();
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["resources"] = json!([{"slot":"service","kind":"http"}]);
    let directory = HttpPublicationDirectory {
        persistence: p.clone(),
        selections: vec![HttpPublicationSelection {
            connection: connection.id,
            slot: Some(ResourceName::parse("service").unwrap()),
            operation: name(TOOL),
        }],
    };
    let version = publish_parent_with(
        &setup.fixture,
        "step-same-name",
        source.to_string(),
        directory,
    )
    .await
    .unwrap();
    let bundle = p
        .selectable_version(company, version)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bundle.snapshots().agents[0].agent_id.as_uuid(), setup.agent);
    assert_eq!(bundle.snapshots().tools.len(), 1);
    assert_eq!(
        bundle.snapshots().tools[0].contract.connection,
        Some(ResourceName::parse("service").unwrap())
    );
    assert!(bundle.snapshots().tools[0].workflow.is_none());
    let claim = setup.claim("own-binding-after-http-alias").await;
    let call = setup
        .call(setup.initial_pin(), json!({"question":"q"}), &claim)
        .await;
    assert!(matches!(
        setup.dispatch(&claim, &call).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    assert_eq!(child_version(&setup, &claim).await, setup.child);
}

#[tokio::test]
async fn autonomous_agent_admission_rejects_a_newly_published_self_workflow_binding() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let company = setup.fixture.target.company;
    p.register_workflow_tool(
        company,
        setup.fixture.target.actor,
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
    setup.fixture.publish_agent(setup.agent).await;
    publish_parent(&setup.fixture, "recursive-reference")
        .await
        .unwrap();
    let error = setup
        .admit(
            setup.target,
            TriggerId::new(Uuid::new_v4()),
            "cyclic-admission",
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error,AppError::BadRequest(ref message) if message.contains("cycle")),
        "{error:?}"
    );
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(runs, 0);
}

async fn child_version(setup: &ToolSetup, claim: &ClaimedWorkflow) -> VersionId {
    let version = sqlx::query_scalar("SELECT version_id FROM workflow_runs WHERE parent_run_id=$1")
        .bind(claim.fence.scope.run.as_uuid())
        .fetch_one(setup.persistence().pool())
        .await
        .unwrap();
    VersionId::new(version)
}
