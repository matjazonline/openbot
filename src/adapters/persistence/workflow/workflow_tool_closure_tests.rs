use super::*;
use std::collections::BTreeMap;

#[tokio::test]
async fn autonomous_agent_admission_checks_resources_of_agent_owned_child_workflows() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let company = setup.fixture.target.company;
    let mut child = echo_child();
    child["resources"] = json!([{"slot":"child_service","kind":"http"}]);
    let child_workflow = WorkflowId::new(Uuid::new_v4());
    let child_version = setup.fixture.publish_workflow(child_workflow, child).await;
    p.register_workflow_tool(
        company,
        setup.fixture.target.actor,
        create(
            "slotted.review",
            child_workflow,
            child_version,
            json!({"reviewer":"x"}),
        ),
    )
    .await
    .unwrap();
    grant(&setup, &["slotted.review"]).await;
    setup.fixture.publish_agent(setup.agent).await;
    // The workflow still contains just the reusable agent reference. Root binding
    // configuration cannot know the active revision's new child until admission.
    publish_parent(&setup.fixture, "agent-child-reference")
        .await
        .unwrap();
    let before = root_count(p, company).await;
    assert!(
        setup
            .admit(
                setup.target,
                TriggerId::new(Uuid::new_v4()),
                "missing-child-resource"
            )
            .await
            .is_err()
    );
    let after = root_count(p, company).await;
    assert_eq!(
        before, after,
        "a missing reachable resource must refuse before a root exists"
    );
    let connection = p
        .create_http(
            company,
            setup.fixture.target.actor,
            HttpConnectionWrite {
                origin: HttpOrigin::parse("https://child.fixture.test").unwrap(),
                bearer: false,
            },
        )
        .await
        .unwrap();
    let target = bind_child_resource(&setup, RuntimeResourceId::new(connection.id)).await;
    assert!(matches!(
        setup
            .admit(
                target,
                TriggerId::new(Uuid::new_v4()),
                "ready-child-resource"
            )
            .await
            .unwrap(),
        AdmissionResult::Created(_)
    ));
    let calls: i64 =
        sqlx::query_scalar("SELECT count(*) FROM model_usage_calls WHERE company_id=$1")
            .bind(company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(
        calls, 0,
        "admission validates the complete closure without provider work"
    );
}

async fn bind_child_resource(setup: &ToolSetup, resource: RuntimeResourceId) -> BindingTarget {
    let p = setup.persistence();
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["resources"] = json!([{"slot":"child_service","kind":"http"}]);
    let version = publish_parent_with(
        &setup.fixture,
        "with-child-resource",
        source.to_string(),
        message::MessagePublicationDirectory {
            persistence: p.clone(),
        },
    )
    .await
    .unwrap();
    let target = BindingTarget {
        company: setup.fixture.target.company,
        actor: setup.fixture.target.actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let service = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let state = service
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Company,
            version,
            params: json!({}),
            resources: BTreeMap::from([(ResourceName::parse("child_service").unwrap(), resource)]),
            run_as: None,
            message_trigger: None,
        })
        .await
        .unwrap();
    service
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    target
}

async fn root_count(p: &PostgresPersistence, company: CompanyId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap()
}
