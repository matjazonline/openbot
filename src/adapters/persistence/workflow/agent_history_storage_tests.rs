use super::*;
use crate::application::workflow::{agent::AgentPreparation, agent_history::*};

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct Captured {
    id: Uuid,
    execution_id: Uuid,
    position: i64,
    state: String,
    input: Value,
    output: Option<Value>,
    checkpoint_id: Option<Uuid>,
}
async fn captured(setup: &AgentSetup) -> Vec<Captured> {
    sqlx::query_as(
        "SELECT exchange.id,exchange.execution_id,exchange.position,exchange.state,exchange.output,exchange.checkpoint_id, \
         execution.frozen_inputs->'context' AS input FROM agent_history_exchanges AS exchange \
         JOIN workflow_executions AS execution ON execution.company_id=exchange.company_id AND execution.id=exchange.execution_id \
         WHERE exchange.company_id=$1 ORDER BY exchange.position",
    ).bind(setup.fixture.target.company.as_uuid()).fetch_all(setup.fixture.persistence.pool()).await.unwrap()
}

#[tokio::test]
async fn agent_history_storage_capture_precedes_checkpoint_and_replays_one_original_input() {
    let setup = Box::pin(agent_setup_source(
        "http://127.0.0.1:1",
        None,
        false,
        |source, _| {
            source["steps"]["start"]["with"]["context"] =
                json!({"literal":[null,{"input":"original"},1.5]});
        },
    ))
    .await;
    let p = &setup.fixture.persistence;
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    let first = p
        .prepare_agent(&setup.claim, policy)
        .await
        .unwrap()
        .expect_ready();
    let expected = AgentExchangeId::for_execution(setup.claim.fence.scope.execution);
    assert_eq!(first.prepared.history_exchange(), Some(expected));
    let rows = captured(&setup).await;
    assert_eq!(
        rows,
        vec![Captured {
            id: expected.as_uuid(),
            execution_id: expected.as_uuid(),
            position: 1,
            state: "queued".into(),
            input: json!([null,{"input":"original"},1.5]),
            output: None,
            checkpoint_id: None,
        }]
    );
    let checkpoint_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_harness_runs WHERE company_id=$1")
            .bind(setup.fixture.target.company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(
        checkpoint_count, 0,
        "input is durable even before a checkpoint exists"
    );
    let second = p
        .prepare_agent(&setup.claim, policy)
        .await
        .unwrap()
        .expect_ready();
    assert_eq!(
        first.prepared.spec(setup.claim.fence).unwrap().prepared,
        second.prepared.spec(setup.claim.fence).unwrap().prepared
    );
    assert_eq!(captured(&setup).await, rows);
    let last: i64 = sqlx::query_scalar(
        "SELECT last_exchange_order FROM agent_history_conversations WHERE company_id=$1",
    )
    .bind(setup.fixture.target.company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(last, 1, "replay cannot consume another queue position");
}

#[tokio::test]
async fn agent_history_completion_driver_keeps_exact_json_and_replays_without_provider() {
    let setup = Box::pin(agent_setup_source(
        "http://127.0.0.1:1",
        None,
        false,
        |source, _| {
            source["steps"]["start"]["with"]["output_schema"] = json!({"literal":{}});
        },
    ))
    .await;
    let value = json!({"answer":[null,3.5,{"quoted":"<data>"}]});
    let llm = scripted_llm(vec![LlmTurn::text(value.to_string())]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let kind = WorkflowStepKind("agent.run".into());
    let first = Box::pin(runtime.execute(&kind, &setup.claim)).await;
    assert_eq!(first.completed_result().unwrap(), value);
    let rows = captured(&setup).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "completed");
    assert_eq!(rows[0].output, Some(value.clone()));
    assert!(rows[0].checkpoint_id.is_some());
    let replay = Box::pin(runtime.execute(&kind, &setup.claim)).await;
    assert_eq!(replay.completed_result().unwrap(), value);
    assert_eq!(captured(&setup).await, rows);
    assert_eq!(
        usage_rows(&setup.fixture.persistence, setup.claim.fence.scope.run).await,
        1
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn agent_history_completion_repair_accepts_only_the_repaired_value() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let llm = scripted_llm(vec![
        LlmTurn::text("not json"),
        LlmTurn::text("\"repaired\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let outcome = Box::pin(
        agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim),
    )
    .await;
    assert_eq!(outcome.completed_result().unwrap(), json!("repaired"));
    let rows = captured(&setup).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].position, 1);
    assert_eq!(rows[0].state, "completed");
    assert_eq!(rows[0].output, Some(json!("repaired")));
    assert_eq!(
        usage_rows(&setup.fixture.persistence, setup.claim.fence.scope.run).await,
        2
    );
    assert_eq!(llm.finish().await.unwrap(), 2);
}

#[tokio::test]
async fn agent_history_completion_invalid_candidates_never_become_an_answer() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let llm = scripted_llm(vec![
        LlmTurn::text("not json"),
        LlmTurn::text("still not json"),
        LlmTurn::text("last invalid repair"),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let outcome = Box::pin(
        agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim),
    )
    .await;
    let outcome = outcome.completed_result();
    assert!(outcome.is_err());
    let rows = captured(&setup).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "failed", "{outcome:?}");
    assert_eq!(rows[0].output, None);
    assert_eq!(
        usage_rows(&setup.fixture.persistence, setup.claim.fence.scope.run).await,
        3
    );
    assert_eq!(llm.finish().await.unwrap(), 3);
}

#[tokio::test]
async fn agent_history_storage_competing_preparations_share_position_and_reject_changed_replay() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    let (left, right) = tokio::join!(
        Box::pin(p.prepare_agent(&setup.claim, policy)),
        Box::pin(p.prepare_agent(&setup.claim, policy))
    );
    assert_eq!(
        left.unwrap()
            .expect_ready()
            .prepared
            .spec(setup.claim.fence)
            .unwrap()
            .prepared,
        right
            .unwrap()
            .expect_ready()
            .prepared
            .spec(setup.claim.fence)
            .unwrap()
            .prepared
    );
    let before = captured(&setup).await;
    let mut changed = setup.claim.clone();
    changed.activation.inputs["context"] = json!({"changed":true});
    assert!(p.prepare_agent(&changed, policy).await.is_err());
    let mut stale = setup.claim.clone();
    stale.fence.attempt.0 += 1;
    assert!(p.prepare_agent(&stale, policy).await.is_err());
    assert_eq!(captured(&setup).await, before);
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].position, 1);
}

#[tokio::test]
async fn agent_history_storage_native_identity_order_and_source_pins_are_enforced() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    p.prepare_agent(
        &setup.claim,
        LeasePolicy::new(Duration::from_secs(60)).unwrap(),
    )
    .await
    .unwrap()
    .expect_ready();
    let scope = setup.claim.fence.scope;
    let before = captured(&setup).await;
    for assignment in [
        "position=2",
        "conversation_id=gen_random_uuid()",
        "input_digest=sha256('changed'::bytea)",
        "agent_id=gen_random_uuid()",
        "access_level=10",
    ] {
        let sql = format!(
            "UPDATE agent_history_exchanges SET {assignment} WHERE company_id=$1 AND execution_id=$2"
        );
        assert!(
            sqlx::query(&sql)
                .bind(scope.company.as_uuid())
                .bind(scope.execution.as_uuid())
                .execute(p.pool())
                .await
                .is_err(),
            "{assignment}"
        );
    }
    assert!(sqlx::query("UPDATE agent_history_conversations SET last_exchange_order=last_exchange_order+1 WHERE company_id=$1")
        .bind(scope.company.as_uuid()).execute(p.pool()).await.is_err());
    assert!(
        sqlx::query("DELETE FROM agent_history_exchanges WHERE company_id=$1")
            .bind(scope.company.as_uuid())
            .execute(p.pool())
            .await
            .is_err()
    );
    let deletion = sqlx::query("DELETE FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .execute(p.pool())
        .await
        .unwrap_err();
    assert_eq!(
        deletion.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    assert_eq!(captured(&setup).await, before);
    let indexes:Vec<String>=sqlx::query_scalar("SELECT indexname FROM pg_indexes WHERE schemaname='public' AND tablename='agent_history_exchanges'")
        .fetch_all(p.pool()).await.unwrap();
    for name in [
        "agent_history_exchanges_pending",
        "agent_history_exchanges_recent",
        "agent_history_exchanges_source",
        "agent_history_exchanges_checkpoint",
    ] {
        assert!(
            indexes.iter().any(|index| index == name),
            "missing {name}: {indexes:?}"
        );
    }
}

#[tokio::test]
async fn agent_history_level_enqueue_uses_current_content_and_refuses_lower_or_out_of_domain() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut p.pool().acquire().await.unwrap(),
        scope.company,
        scope.run,
        crate::entities::access_level::AccessLevel::L5,
    )
    .await
    .unwrap();
    p.prepare_agent(
        &setup.claim,
        LeasePolicy::new(Duration::from_secs(60)).unwrap(),
    )
    .await
    .unwrap()
    .expect_ready();
    let level: i16 =
        sqlx::query_scalar("SELECT access_level FROM agent_history_exchanges WHERE company_id=$1")
            .bind(scope.company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(level, 5);
    for lower in [0_i16, 10] {
        assert!(
            sqlx::query("UPDATE agent_history_exchanges SET access_level=$2 WHERE company_id=$1")
                .bind(scope.company.as_uuid())
                .bind(lower)
                .execute(p.pool())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn agent_history_storage_checkpoint_requires_exact_server_owned_exchange_reference() {
    use crate::application::workflow::checkpoints::WorkflowCheckpointStore;
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    let prepared = p
        .prepare_agent(&setup.claim, policy)
        .await
        .unwrap()
        .expect_ready()
        .prepared;
    let spec = prepared.spec(setup.claim.fence).unwrap();
    let before = captured(&setup).await;
    for proof in [json!(null), json!(Uuid::new_v4())] {
        let mut forged = prepared.spec(setup.claim.fence).unwrap();
        forged.prepared["history_exchange"] = proof;
        assert!(
            p.open_workflow_checkpoint(setup.claim.fence, &forged, policy)
                .await
                .is_err()
        );
        assert_eq!(captured(&setup).await, before);
    }
    p.open_workflow_checkpoint(setup.claim.fence, &spec, policy)
        .await
        .unwrap();
    assert_eq!(captured(&setup).await[0].state, "active");
}

#[tokio::test]
async fn agent_history_completion_json_null_is_an_accepted_value_in_storage() {
    let setup = Box::pin(agent_setup_source(
        "http://127.0.0.1:1",
        None,
        false,
        |source, _| {
            source["steps"]["start"]["with"]["output_schema"] = json!({"literal":{}});
        },
    ))
    .await;
    let llm = scripted_llm(vec![LlmTurn::text("null")]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let output = Box::pin(
        agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim),
    )
    .await
    .completed_result()
    .unwrap();
    assert_eq!(output, Value::Null);
    let rows = captured(&setup).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "completed");
    assert_eq!(rows[0].output, Some(Value::Null));
    assert_eq!(llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn agent_history_completion_cancel_before_checkpoint_keeps_the_captured_input() {
    use crate::application::workflow::{
        CancelCommand, CancelResult, WorkflowInspection, WorkflowRunTransitions,
    };
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    p.prepare_agent(
        &setup.claim,
        LeasePolicy::new(Duration::from_secs(60)).unwrap(),
    )
    .await
    .unwrap()
    .expect_ready();
    let before = captured(&setup).await;
    assert!(before[0].checkpoint_id.is_none());
    let command = CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: setup.fixture.target.actor,
        command_key: IdempotencyKey::parse("cancel-queued").unwrap(),
        expected_revision: p
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    assert!(matches!(
        p.cancel(command).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    let rows = captured(&setup).await;
    assert_eq!(rows[0].id, before[0].id);
    assert_eq!(rows[0].input, before[0].input);
    assert_eq!(rows[0].state, "cancelled");
    assert_eq!(rows[0].output, None);
    assert_eq!(rows[0].checkpoint_id, None);
}

#[path = "agent_history_commit_ack_tests.rs"]
mod acknowledgement;
