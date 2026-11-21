use super::*;
use crate::application::agent::{AgentKey, AgentPublicationScope, AgentPublications};
use crate::application::workflow::{
    agent::AgentPreparation,
    registry::ModelStep,
    tools::{WorkflowToolWrite, WorkflowTools},
};
use crate::entities::agent_capability::{
    AgentBindingId, AgentBindingSource, AgentId, AgentToolAlias, AgentToolBinding,
};

#[path = "autonomous_agent_child_selection_tests.rs"]
mod descendants;

#[path = "autonomous_agent_sample_closure_tests.rs"]
mod samples;

#[path = "autonomous_memory_scope_tests.rs"]
mod memory_scopes;

struct GraphWorkflow {
    workflow: WorkflowId,
    version: VersionId,
    binding: BindingTarget,
}

fn model_source(agent: &AgentKey, kind: ModelStep) -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example(kind.kind()).unwrap().source).unwrap();
    source["steps"]["start"]["with"]["agent"] = json!({"literal":agent.as_str()});
    source
}

async fn publish_graph_definition(setup: &AgentSetup, mut source: Value) -> GraphWorkflow {
    let p = &setup.fixture.persistence;
    let fixture = Fixture {
        _db: setup.fixture._db.clone(),
        persistence: p.clone(),
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..setup.fixture.target
        },
    };
    let service = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        crate::adapters::persistence::workflow::child_publication::ChildPublicationDirectory {
            persistence: p.clone(),
            inner: AgentPublicationDirectory {
                persistence: p.clone(),
                inner: Preflight,
            },
        },
    );
    source["workflow_id"] = json!(fixture.target.workflow.to_string());
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    service.save(draft).await.unwrap();
    let mut publication = fixture.publish();
    publication.version = VersionId::new(Uuid::new_v4());
    publication.key =
        IdempotencyKey::parse(format!("graph:{}", publication.version.as_uuid())).unwrap();
    let version = publication.version;
    service.publish(publication).await.unwrap();
    let target = BindingTarget {
        company: fixture.target.company,
        actor: fixture.target.actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    GraphWorkflow {
        workflow: fixture.target.workflow,
        version,
        binding: target,
    }
}

async fn publish_graph(setup: &AgentSetup, source: Value, params: Value) -> GraphWorkflow {
    let graph = Box::pin(publish_graph_definition(setup, source)).await;
    Box::pin(activate_graph(
        setup,
        &graph,
        params,
        RelatedAssociation::Company,
    ))
    .await;
    graph
}

async fn activate_graph(
    setup: &AgentSetup,
    graph: &GraphWorkflow,
    params: Value,
    association: RelatedAssociation,
) {
    let p = &setup.fixture.persistence;
    let target = graph.binding;
    let version = graph.version;
    let bindings = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let state = bindings
        .configure(ConfigureBinding {
            escalation: None,
            target,
            expected: None,
            association,
            version,
            params,
            resources: BTreeMap::new(),
            run_as: None,
            message_trigger: None,
        })
        .await
        .unwrap();
    bindings
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
}

async fn register_child(setup: &AgentSetup, child: &GraphWorkflow, name: &str) -> AgentToolBinding {
    let registration = TypeName::parse(name).unwrap();
    WorkflowTools::register_workflow_tool(
        &setup.fixture.persistence,
        setup.fixture.target.company,
        setup.fixture.target.actor,
        WorkflowToolWrite {
            name: registration.clone(),
            workflow: child.workflow,
            version: child.version,
            params: json!({}),
            expected: None,
        },
    )
    .await
    .unwrap();
    AgentToolBinding {
        id: AgentBindingId::new(Uuid::new_v4()),
        alias: AgentToolAlias::parse("child").unwrap(),
        source: AgentBindingSource::Workflow {
            registration,
            workflow: child.workflow,
            version: child.version,
            params: json!({}),
        },
    }
}

async fn admit_graph(
    setup: &AgentSetup,
    graph: &GraphWorkflow,
    key: &str,
) -> Result<AdmissionResult, Box<dyn std::error::Error + Send + Sync>> {
    let p = &setup.fixture.persistence;
    let mut request = super::autonomous_admission::request(setup, key, Uuid::new_v4()).await;
    request.binding_id = graph.binding.binding;
    Ok(
        WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
            .admit(request)
            .await?,
    )
}

#[tokio::test]
async fn autonomous_agent_admission_cycle_refuses_atomically_but_model_only_is_a_leaf() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let graph = root_graph(&setup).await;
    let scope = AgentPublicationScope {
        company,
        actor: setup.fixture.target.actor,
        agent: AgentId::new(setup.agent),
    };
    let original = AgentPublications::active_agent_revision(p, scope)
        .await
        .unwrap()
        .unwrap();
    let mut bindings: Vec<_> = original
        .definition
        .tools
        .iter()
        .map(|tool| tool.binding.clone())
        .collect();
    bindings.push(register_child(&setup, &graph, "self_child").await);
    AgentPersistence::update(
        p,
        setup.agent,
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            tool_bindings: Some(bindings),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    setup.fixture.publish_agent(setup.agent).await;
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    let error = admit_graph(&setup, &graph, "cyclic-root")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("cycle"), "{error}");
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(before, after);
    let old = p
        .prepare_agent(
            &setup.claim,
            crate::application::workflow::lease::LeasePolicy::new(std::time::Duration::from_secs(
                60,
            ))
            .unwrap(),
        )
        .await
        .unwrap()
        .expect_ready();
    assert_eq!(
        old.prepared.tools().len(),
        1,
        "new binding cannot expand the old selected revision"
    );
    assert_model_only_leaf(&setup).await;
}

async fn root_graph(setup: &AgentSetup) -> GraphWorkflow {
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    #[derive(sqlx::FromRow)]
    struct Version {
        workflow: Uuid,
        version: Uuid,
        binding: Uuid,
    }
    let root: Version = sqlx::query_as("SELECT workflow_id AS workflow,version_id AS version,binding_id AS binding FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(company.as_uuid()).bind(setup.claim.fence.scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    GraphWorkflow {
        workflow: WorkflowId::new(root.workflow),
        version: VersionId::new(root.version),
        binding: BindingTarget {
            company,
            actor: setup.fixture.target.actor,
            binding: WorkflowBindingId::new(root.binding),
        },
    }
}

async fn assert_model_only_leaf(setup: &AgentSetup) {
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let classifier = publish_graph(
        setup,
        model_source(&AgentKey::parse("assistant").unwrap(), ModelStep::Classify),
        json!({}),
    )
    .await;
    p.set_http_enabled(
        company,
        setup.fixture.target.actor,
        setup.connection.id,
        setup.connection.revision,
        false,
    )
    .await
    .unwrap();
    let AdmissionResult::Created(run) = admit_graph(setup, &classifier, "model-only-leaf")
        .await
        .unwrap()
    else {
        panic!("fresh model-only run");
    };
    #[derive(sqlx::FromRow)]
    struct Job {
        execution: Uuid,
        job: Uuid,
    }
    let job: Job = sqlx::query_as("SELECT execution.id AS execution,job.id AS job FROM workflow_executions AS execution JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=$1 AND execution.run_id=$2")
        .bind(company.as_uuid()).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let claim = p
        .claim_io(
            crate::application::workflow::activation::ActivationRequest {
                company,
                run,
                execution: ExecutionId::new(job.execution),
                job: crate::application::workflow::activation::WorkflowJobId(job.job),
            },
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap();
    let launch = p
        .prepare_agent(
            &claim,
            crate::application::workflow::lease::LeasePolicy::new(std::time::Duration::from_secs(
                60,
            ))
            .unwrap(),
        )
        .await
        .unwrap()
        .expect_ready();
    assert!(launch.prepared.tools().is_empty());
    assert!(launch.prepared.selection().skills().is_empty());
    assert_eq!(
        launch.prepared.selection().agent().tools.len(),
        2,
        "canonical revision stays intact behind model-only projection"
    );
}

#[tokio::test]
async fn autonomous_agent_admission_combined_depth_is_bounded_across_agent_workflow_edges() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let mut graph = publish_graph(
        &setup,
        model_source(&AgentKey::parse("assistant").unwrap(), ModelStep::AgentRun),
        json!({}),
    )
    .await;
    // Root workflow + original agent is depth2. Fifteen alternating pairs reach32;
    // a sixteenth reaches34 even though every individual workflow has no child bundle.
    for level in 1..=16 {
        let binding = register_child(&setup, &graph, &format!("child_{level}")).await;
        let key = AgentKey::parse(format!("depth-{level}")).unwrap();
        let agent = AgentPersistence::create(
            p,
            setup.fixture.target.company.as_uuid(),
            AgentWrite {
                name: format!("Depth {level}"),
                slug: key.as_str().to_owned(),
                provider: Some(SCRIPTED_PROVIDER.into()),
                model: Some(SCRIPTED_MODEL.into()),
                tool_bindings: Some(vec![binding]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        setup.fixture.publish_agent(agent.id).await;
        graph = publish_graph(&setup, model_source(&key, ModelStep::AgentRun), json!({})).await;
        if level == 15 {
            assert!(matches!(
                admit_graph(&setup, &graph, "depth-32").await.unwrap(),
                AdmissionResult::Created(_)
            ));
        }
    }
    let error = admit_graph(&setup, &graph, "depth-34").await.unwrap_err();
    assert!(error.to_string().contains("depth exceeds 32"), "{error}");
}

#[tokio::test]
async fn autonomous_agent_admission_node_limit_covers_the_complete_dynamic_universe() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let mut candidates = vec!["assistant".to_owned()];
    for index in 1..256 {
        let key = format!("candidate-{index}");
        let agent = AgentPersistence::create(
            p,
            company.as_uuid(),
            AgentWrite {
                name: format!("Candidate {index}"),
                slug: key.clone(),
                provider: Some(SCRIPTED_PROVIDER.into()),
                model: Some(SCRIPTED_MODEL.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        setup.fixture.publish_agent(agent.id).await;
        candidates.push(key);
    }
    let mut source = model_source(&AgentKey::parse("assistant").unwrap(), ModelStep::AgentRun);
    source["parameter_schema"] = json!({"type":"object","properties":{"selected":{"type":"string","enum":&candidates[..255]}},"required":["selected"],"additionalProperties":false});
    source["steps"]["start"]["with"]["agent"] = json!({"ref":"/params/selected"});
    let boundary = publish_graph(&setup, source.clone(), json!({"selected":"assistant"})).await;
    assert!(matches!(
        admit_graph(&setup, &boundary, "exactly-256-selected-nodes")
            .await
            .unwrap(),
        AdmissionResult::Created(_)
    ));
    source["parameter_schema"]["properties"]["selected"]["enum"] = json!(candidates);
    let graph = publish_graph(&setup, source, json!({"selected":"assistant"})).await;
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    let error = admit_graph(&setup, &graph, "too-many-selected-nodes")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("at most 256"), "{error}");
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(
        before, after,
        "all256 declared agents plus the root workflow exceed the limit, despite one runtime selector"
    );
}

/// Plan07 D9a (B2-F4): an agent's workflow tool whose bundle uses an unslotted operation is
/// checked against the run's reach, not the agent's: with the connection moved into Billing and
/// the agent placed there (its own tool stays in its reach), the company-wide run is refused.
#[tokio::test]
async fn unit_resource_selection_admission_workflow_agent_tool_bundle_out_of_reach_is_refused() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let child_fixture = Fixture {
        _db: setup.fixture._db.clone(),
        persistence: p.clone(),
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..setup.fixture.target
        },
    };
    let mut source: Value =
        serde_json::from_str(&registry::example("tool.call").unwrap().source).unwrap();
    source["workflow_id"] = json!(child_fixture.target.workflow.to_string());
    source["steps"]["start"]["with"] = json!({"tool":{"literal":"registered.request"},
        "arguments":{"literal":{"method":"POST","path":"/messages","body":{"message":"hello"}}}});
    let service = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        http_directory(p, &setup.connection),
    );
    let mut draft = child_fixture.save(None);
    draft.content.source = source.to_string();
    service.save(draft).await.unwrap();
    let mut publication = child_fixture.publish();
    publication.version = VersionId::new(Uuid::new_v4());
    publication.key =
        IdempotencyKey::parse(format!("graph:{}", publication.version.as_uuid())).unwrap();
    service.publish(publication.clone()).await.unwrap();
    let child = GraphWorkflow {
        workflow: child_fixture.target.workflow,
        version: publication.version,
        binding: BindingTarget {
            company,
            actor: setup.fixture.target.actor,
            binding: WorkflowBindingId::new(Uuid::new_v4()),
        },
    };
    let scope = AgentPublicationScope {
        company,
        actor: setup.fixture.target.actor,
        agent: AgentId::new(setup.agent),
    };
    let original = AgentPublications::active_agent_revision(p, scope)
        .await
        .unwrap()
        .unwrap();
    let mut bindings: Vec<_> = original
        .definition
        .tools
        .iter()
        .map(|tool| tool.binding.clone())
        .collect();
    bindings.push(register_child(&setup, &child, "lookup_child").await);
    AgentPersistence::update(
        p,
        setup.agent,
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            tool_bindings: Some(bindings),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    setup.fixture.publish_agent(setup.agent).await;
    let root = root_graph(&setup).await;
    assert!(matches!(
        admit_graph(&setup, &root, "agent-tool-in-reach")
            .await
            .unwrap(),
        AdmissionResult::Created(_)
    ));
    let billing = p.test_unit(company.as_uuid(), "billing").await;
    sqlx::query(
        "INSERT INTO department_roles (company_id, department_id, role, principal_id) \
         SELECT $1, $2, 'lead', id FROM principals WHERE company_id=$1 AND agent_id=$3 \
         AND kind='agent'",
    )
    .bind(company.as_uuid())
    .bind(billing)
    .bind(setup.agent)
    .execute(p.pool())
    .await
    .unwrap();
    p.move_http(
        company,
        setup.fixture.target.actor,
        setup.connection.id,
        crate::entities::department::DepartmentId::new(billing),
    )
    .await
    .unwrap();
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert!(
        admit_graph(&setup, &root, "agent-tool-out-of-reach")
            .await
            .is_err()
    );
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(after, runs, "refused before any run row");
}
