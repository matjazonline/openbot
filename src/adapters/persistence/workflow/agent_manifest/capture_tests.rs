use super::*;

#[tokio::test]
async fn autonomous_agent_admission_capacity_refuses_before_lookup_or_retaining_an_extra_member() {
    let Some(pool) = crate::adapters::persistence::test_support::test_pool().await else {
        return;
    };
    let agent = AgentId::new(Uuid::new_v4());
    let workflow = WorkflowId::new(Uuid::new_v4());
    let mut closure = Closure {
        company: CompanyId::new(Uuid::new_v4()),
        actor: WorkflowActor::authenticated(Uuid::new_v4()).unwrap(),
        resources: BTreeMap::new(),
        agents: BTreeMap::from([(agent, AgentRevisionId::new(Uuid::new_v4()))]),
        workflows: (1..MAX_AGENT_DEPENDENCIES)
            .map(|_| (VersionId::new(Uuid::new_v4()), workflow))
            .collect(),
        deadline: Instant::now() + Duration::from_secs(5),
    };
    let new_workflow = Node::Workflow(workflow, VersionId::new(Uuid::new_v4()));
    let new_agent = Node::Agent(AgentId::new(Uuid::new_v4()), AgentUseMode::Tools);
    let mut tx = pool.begin().await.unwrap();
    // There are deliberately no backing rows for these new identities. A lookup
    // would return "unavailable", while the capacity guard must reject them first.
    for node in [new_workflow, new_agent] {
        let error = closure
            .edges_on(&mut tx, node)
            .await
            .err()
            .expect("capacity must be refused before looking up an extra identity");
        assert!(error.to_string().contains("at most 256"), "{error}");
        assert_eq!(closure.agents.len() + closure.workflows.len(), 256);
    }
    tx.rollback().await.unwrap();

    // The unique-selection bound is independent of traversal use views.
    for mode in [AgentUseMode::Tools, AgentUseMode::ModelOnly] {
        closure
            .check_node_capacity(Node::Agent(agent, mode))
            .unwrap();
    }
    let version = *closure.workflows.keys().next().unwrap();
    closure
        .check_node_capacity(Node::Workflow(workflow, version))
        .unwrap();
    closure.workflows.remove(&version);
    closure.check_node_capacity(new_workflow).unwrap();
    closure.check_node_capacity(new_agent).unwrap();
}
