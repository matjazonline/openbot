use super::*;
use crate::application::workflow::{
    activation::ActivationRequest,
    waits::{WaitProgress, WorkflowWaits},
};
use crate::entities::access_level::AccessLevel;

async fn graph(setup: &AgentSetup) -> GraphWorkflow {
    let key = AgentKey::parse("assistant").unwrap();
    let mut child = model_source(&key, ModelStep::AgentRun);
    child["steps"]["start"]["with"]["context"] = json!({"literal":"CHILD PRIVATE INPUT"});
    let child = publish_graph(setup, child, json!({})).await;
    let mut parent = model_source(&key, ModelStep::AgentRun);
    let mut again = parent["steps"]["start"].clone();
    again["with"]["context"] = json!({"literal":"parent next task"});
    parent["steps"]["again"] = again;
    parent["steps"]["start"]["routes"]["success"] = json!("child");
    let call: Value =
        serde_json::from_str(&registry::example("workflow.call").unwrap().source).unwrap();
    parent["steps"]["child"] = call["steps"]["start"].clone();
    parent["steps"]["child"]["child_workflow_id"] = json!(child.workflow);
    parent["steps"]["child"]["child_version_id"] = json!(child.version);
    parent["steps"]["child"]["with"]["input"] = json!({"literal":{}});
    parent["steps"]["child"]["with"]["params"] = json!({"literal":{}});
    parent["steps"]["child"]["routes"]["success"] = json!("again");
    publish_graph(setup, parent, json!({})).await
}

async fn stamp(setup: &AgentSetup, claim: &ClaimedWorkflow, level: AccessLevel) {
    let mut tx = setup.fixture.persistence.pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut tx,
        claim.fence.scope.company,
        claim.fence.scope.run,
        level,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

struct Branch {
    parent: ActivationRequest,
    child: ClaimedWorkflow,
}

async fn begin(setup: &AgentSetup, graph: &GraphWorkflow, level: AccessLevel) -> Branch {
    let p = &setup.fixture.persistence;
    let AdmissionResult::Created(run) = admit_graph(setup, graph, "history-child-journey")
        .await
        .unwrap()
    else {
        panic!("new root")
    };
    let first = claim_run(setup, run).await;
    stamp(setup, &first, level).await;
    let output =
        Box::pin(agent_runtime(setup).execute(&WorkflowStepKind("agent.run".into()), &first))
            .await
            .completed_result()
            .unwrap();
    assert_eq!(output, json!("parent accepted"));
    let parent = p
        .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
            fence: first.fence,
            output,
            policy: lease(),
        })
        .await
        .unwrap()
        .unwrap()
        .successor
        .unwrap();
    p.park_child(parent).await.unwrap().unwrap();
    let child: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND parent_run_id=$2")
            .bind(parent.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    Branch {
        parent,
        child: claim_run(setup, RunId::new(child)).await,
    }
}

async fn finish_child(setup: &AgentSetup, branch: Branch) -> ClaimedWorkflow {
    let p = &setup.fixture.persistence;
    let output = Box::pin(
        agent_runtime(setup).execute(&WorkflowStepKind("agent.run".into()), &branch.child),
    )
    .await
    .completed_result()
    .unwrap();
    assert_eq!(output, json!("CHILD PRIVATE ANSWER"));
    p.complete_io(crate::application::workflow::lease::FencedWorkflowResult {
        fence: branch.child.fence,
        output,
        policy: lease(),
    })
    .await
    .unwrap()
    .unwrap();
    let WaitProgress::Completed(done) = p.resume_wait(branch.parent).await.unwrap() else {
        panic!("actual child result completes parent call")
    };
    p.claim_io(
        done.successor.unwrap(),
        crate::application::workflow::lease::WorkflowWorkerId(Uuid::new_v4()),
        lease(),
    )
    .await
    .unwrap()
    .unwrap()
}

async fn journey(initial: AccessLevel) {
    journey_with(initial, false).await;
}

/// Phase 05 E8 stand-in rows: an enabled tenant allowing generation, a ready user namespace for
/// the run actor, and both capture switches on for the agent (acceptance reads only these).
async fn seed_capture(setup: &AgentSetup) {
    let pool = setup.fixture.persistence.pool();
    let company = setup.fixture.target.company.as_uuid();
    let actor = setup.fixture.target.actor.user_id();
    sqlx::query(
        "INSERT INTO memory_tenants (company_id, state, revision, disable_epoch, \
           provisioner_credential_id, generation_cap, changed_by, changed_at) \
         VALUES ($1, 'enabled', 1, 0, gen_random_uuid(), 100000, $2, clock_timestamp())",
    )
    .bind(company)
    .bind(actor)
    .execute(pool)
    .await
    .unwrap();
    for (kind, agent, user) in [
        ("user", None, Some(actor)),
        ("agent", Some(setup.agent), None),
    ] {
        sqlx::query(
            "INSERT INTO memory_namespaces (company_id, id, scope_kind, agent_id, user_id, state, \
               credential_id, credential_envelope) \
             VALUES ($1, gen_random_uuid(), $2, $3, $4, 'ready', gen_random_uuid(), 'stand-in')",
        )
        .bind(company)
        .bind(kind)
        .bind(agent)
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO memory_agent_capture (company_id, agent_id, kind, enabled, allowance_tokens, \
           max_output_tokens, expiry_hours, revision, changed_by, reason, changed_at) \
         SELECT $1, $2, kind, true, 4000, 500, 12, 1, $3, 'Capture pilot', clock_timestamp() \
         FROM unnest(ARRAY['private_sessions', 'threads']) AS kind",
    )
    .bind(company)
    .bind(setup.agent)
    .bind(actor)
    .execute(pool)
    .await
    .unwrap();
}

async fn journey_with(initial: AccessLevel, capture: bool) {
    let mut setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    if capture {
        seed_capture(&setup).await;
        // A runtime with the memory service configured arms capture (U7). This agent has no
        // memory tools and its namespaces are ready, so the service is never contacted.
        let config = crate::infra::memory_config::MemoryServiceConfig {
            base_url: "http://127.0.0.1:9".into(),
            provisioner: crate::infra::memory_config::ProvisionerToken::parse(
                "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                    .into(),
            )
            .unwrap(),
            request_timeout: Duration::from_secs(1),
            recall_deadline: Duration::from_millis(500),
            interactive_in_flight: 1,
            background_in_flight: 1,
        };
        setup.recall = Some(Arc::new(
            crate::application::workflow::memory_recall::MemoryRecall::new(
                Arc::new(setup.fixture.persistence.clone()),
                Arc::new(crate::adapters::memory_service::MemoryServiceHttp::new(&config).unwrap()),
            ),
        ));
    }
    let graph = graph(&setup).await;
    let mut llm = scripted_llm(vec![
        LlmTurn::text("\"parent accepted\""),
        LlmTurn::text("\"CHILD PRIVATE ANSWER\""),
        LlmTurn::text("\"parent done\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let branch = begin(&setup, &graph, initial).await;
    let root_run = branch.parent.run;
    let child_run = branch.child.fence.scope.run;
    let p = &setup.fixture.persistence;
    let inherited:Value=sqlx::query_scalar("SELECT jsonb_build_object('ceiling',knowledge_ceiling,'content',content_level,'actor',actor_id) FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(branch.child.fence.scope.company.as_uuid()).bind(branch.child.fence.scope.run.as_uuid())
        .fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        inherited,
        json!({"ceiling":9,"content":initial.as_i16(),"actor":setup.fixture.target.actor.user_id()})
    );
    let child = p
        .prepare_agent(&branch.child, lease())
        .await
        .unwrap()
        .expect_ready();
    assert!(
        child
            .prepared
            .frozen_history()
            .unwrap()
            .seed
            .pairs()
            .is_empty()
    );
    assert_eq!(
        child.prepared.frozen_history().unwrap().content_level,
        initial
    );
    stamp(&setup, &branch.child, AccessLevel::L5).await;
    let last = finish_child(&setup, branch).await;
    // Prepared here before the runtime runs it; with capture, as a configured runtime prepares
    // (U7), so the frozen preparation is armed.
    let prepared = p
        .prepare_agent_with_recall(
            &last,
            lease(),
            crate::application::workflow::memory_recall::MemoryPreparation {
                recall: None,
                capture,
            },
        )
        .await
        .unwrap()
        .expect_ready();
    let history = prepared.prepared.frozen_history().unwrap();
    assert_eq!(history.content_level, AccessLevel::L5);
    assert_eq!(history.seed.pairs().len(), 1);
    assert_eq!(history.seed.pairs()[0].output, json!("parent accepted"));
    assert_eq!(
        Box::pin(agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &last))
            .await
            .completed_result()
            .unwrap(),
        json!("parent done")
    );
    assert_branches(&setup, root_run, child_run).await;
    let requests = llm.observed();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1]["messages"].as_array().unwrap().len(), 2);
    assert!(!requests[1].to_string().contains("parent accepted"));
    assert!(requests[2].to_string().contains("parent accepted"));
    assert!(!requests[2].to_string().contains("CHILD PRIVATE"));
    assert_eq!(llm.finish().await.unwrap(), 3);
    if capture {
        // Both exchanges of the creator's own manual root are captured; the child branch never.
        let captured: Vec<(String, String)> = sqlx::query_as(
            "SELECT conversation.scope_kind, namespace.scope_kind FROM memory_deliveries AS delivery \
             JOIN agent_history_exchanges AS exchange ON exchange.company_id = delivery.company_id \
               AND exchange.id = delivery.exchange_id \
             JOIN agent_history_conversations AS conversation \
               ON conversation.company_id = exchange.company_id AND conversation.id = exchange.conversation_id \
             JOIN memory_namespaces AS namespace ON namespace.company_id = delivery.company_id \
               AND namespace.id = delivery.namespace_id \
             WHERE delivery.company_id = $1 AND delivery.kind = 'generation'",
        )
        .bind(setup.fixture.target.company.as_uuid())
        .fetch_all(p.pool())
        .await
        .unwrap();
        assert_eq!(
            captured,
            vec![
                ("private_session".to_owned(), "user".to_owned()),
                ("private_session".to_owned(), "user".to_owned())
            ]
        );
        let texts: String = sqlx::query_scalar(
            "SELECT string_agg(request::text, ' ') FROM memory_deliveries WHERE company_id = $1",
        )
        .bind(setup.fixture.target.company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
        assert!(
            !texts.contains("CHILD PRIVATE"),
            "the child branch is never captured"
        );
    }
}

async fn assert_branches(setup: &AgentSetup, root: RunId, child_run: RunId) {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('scope',conversation.scope_kind,'conversation',exchange.conversation_id,'state',exchange.state,'level',exchange.access_level,'agent',conversation.agent_id) FROM agent_history_exchanges AS exchange JOIN agent_history_conversations AS conversation ON conversation.company_id=exchange.company_id AND conversation.id=exchange.conversation_id WHERE exchange.company_id=$1 AND exchange.run_id=ANY($2) ORDER BY exchange.position,exchange.id")
        .bind(setup.fixture.target.company.as_uuid()).bind(vec![root.as_uuid(),child_run.as_uuid()])
        .fetch_all(setup.fixture.persistence.pool()).await.unwrap();
    assert_eq!(rows.len(), 3);
    let child = rows.iter().find(|r| r["scope"] == "child_branch").unwrap();
    let roots: Vec<_> = rows
        .iter()
        .filter(|r| r["scope"] == "private_session")
        .collect();
    assert_eq!(roots.len(), 2);
    assert_eq!(roots[0]["conversation"], roots[1]["conversation"]);
    assert_ne!(roots[0]["conversation"], child["conversation"]);
    assert!(
        rows.iter()
            .all(|r| r["agent"] == json!(setup.agent) && r["state"] == "completed")
    );
    assert_eq!(child["level"], 5);
}

#[tokio::test]
async fn agent_history_children_same_agent_private_branch_completes_without_parent_transcript() {
    journey(AccessLevel::PUBLIC).await;
}

#[tokio::test]
async fn agent_history_level_child_branch_inherits_content_and_result_taints_parent() {
    journey(AccessLevel::L5).await;
}

/// Phase 05 E8: with both switches on, a manual root's private-session exchanges are captured
/// into the creator's memory and its child branch's never are.
#[tokio::test]
async fn memory_capture_a_child_branch_is_never_captured() {
    journey_with(AccessLevel::PUBLIC, true).await;
}

#[tokio::test]
async fn agent_history_callers_repeated_activation_replay_keeps_outputs_after_later_workflow_failure()
 {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let run = repeat_run(&setup).await;
    let mut llm = scripted_llm(vec![
        LlmTurn::text("\"first repeat\""),
        LlmTurn::text("\"second repeat\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let p = &setup.fixture.persistence;
    let first = claim_run(&setup, run).await;
    let kind = WorkflowStepKind("agent.run".into());
    let output = Box::pin(agent_runtime(&setup).execute(&kind, &first))
        .await
        .completed_result()
        .unwrap();
    // Box fixture progression to keep the repeated-call test on the stock debug stack.
    let second = Box::pin(claim_after_first(&setup, &first, output)).await;
    assert_ne!(first.fence.scope.execution, second.fence.scope.execution);
    assert_ne!(first.activation.step, second.activation.step);
    let prepared = p
        .prepare_agent(&second, lease())
        .await
        .unwrap()
        .expect_ready();
    assert_eq!(
        prepared.prepared.frozen_history().unwrap().seed.pairs()[0].output,
        "first repeat"
    );
    let output = Box::pin(agent_runtime(&setup).execute(&kind, &second))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(output, json!("second repeat"));
    assert_eq!(
        Box::pin(agent_runtime(&setup).execute(&kind, &second))
            .await
            .completed_result()
            .unwrap(),
        output
    );
    let terminal = p
        .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
            fence: second.fence,
            output,
            policy: lease(),
        })
        .await
        .unwrap();
    assert!(
        terminal.is_none(),
        "the third activation hits the existing step budget"
    );
    assert_retained_repeats(
        &setup,
        run,
        RepeatedCalls {
            first: first.fence.scope.execution,
            second: second.fence.scope.execution,
        },
    )
    .await;
    assert_eq!(llm.observed().len(), 2);
    assert_eq!(llm.finish().await.unwrap(), 2);
}

struct RepeatedCalls {
    first: ExecutionId,
    second: ExecutionId,
}

async fn assert_retained_repeats(setup: &AgentSetup, run: RunId, calls: RepeatedCalls) {
    let p = &setup.fixture.persistence;
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(setup.fixture.target.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(
        state, "failed",
        "next activation exceeds original max_steps2 after accepted output"
    );
    let exchanges:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('execution',execution_id,'position',position,'state',state,'output',output) FROM agent_history_exchanges WHERE company_id=$1 AND run_id=$2 ORDER BY position")
        .bind(setup.fixture.target.company.as_uuid()).bind(run.as_uuid()).fetch_all(p.pool()).await.unwrap();
    assert_eq!(
        exchanges,
        json!([
                {"execution":calls.first,"position":1,"state":"completed","output":"first repeat"},
                {"execution":calls.second,"position":2,"state":"completed","output":"second repeat"}
        ])
        .as_array()
        .unwrap()
        .clone()
    );
}

async fn repeat_run(setup: &AgentSetup) -> RunId {
    let mut source = model_source(&AgentKey::parse("assistant").unwrap(), ModelStep::AgentRun);
    source["steps"]["repeat"] = source["steps"]["start"].clone();
    source["steps"]["start"]["routes"]["success"] = json!("repeat");
    source["steps"]["repeat"]["routes"]["success"] = json!("after");
    let after: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    source["steps"]["after"] = after["steps"]["start"].clone();
    source["limits"]["max_steps"] = json!(2);
    let graph = publish_graph(setup, source, json!({})).await;
    let AdmissionResult::Created(run) = admit_graph(setup, &graph, "history-repeat").await.unwrap()
    else {
        panic!("new root")
    };
    run
}

async fn claim_after_first(
    setup: &AgentSetup,
    first: &ClaimedWorkflow,
    output: Value,
) -> ClaimedWorkflow {
    let p = &setup.fixture.persistence;
    let next = p
        .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
            fence: first.fence,
            output,
            policy: lease(),
        })
        .await
        .unwrap()
        .unwrap()
        .successor
        .unwrap();
    p.claim_io(
        next,
        crate::application::workflow::lease::WorkflowWorkerId(Uuid::new_v4()),
        lease(),
    )
    .await
    .unwrap()
    .unwrap()
}

/// E2 with U4 on real runs: a manual root's child, run as the root's actor through the production
/// child path, inherits the root's user scope; the parent walk reaches the root's session and
/// admission.
#[tokio::test]
async fn memory_scope_a_manual_roots_child_inherits_the_user_scope() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let graph = graph(&setup).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"parent accepted\"")]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let branch = Box::pin(begin(&setup, &graph, AccessLevel::PUBLIC)).await;
    assert_eq!(llm.finish().await.unwrap(), 1);
    let p = &setup.fixture.persistence;
    let mut conn = p.pool().acquire().await.unwrap();
    let actor: Uuid =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(branch.parent.company.as_uuid())
            .bind(branch.parent.run.as_uuid())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    for run in [branch.parent.run, branch.child.fence.scope.run] {
        let owner = super::checkpoints::memory_tools::user_scope_owner_on(
            &mut conn,
            branch.parent.company,
            run,
        )
        .await
        .unwrap();
        assert_eq!(owner, Some(actor), "{run:?}");
    }
    let parent: Option<Uuid> =
        sqlx::query_scalar("SELECT parent_run_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(branch.parent.company.as_uuid())
            .bind(branch.child.fence.scope.run.as_uuid())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        parent,
        Some(branch.parent.run.as_uuid()),
        "a real child run"
    );
}
