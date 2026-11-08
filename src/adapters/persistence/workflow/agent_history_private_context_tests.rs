use super::*;

#[path = "agent_history_private_fixture.rs"]
mod fixture;
use fixture::ManualHistory;

#[path = "agent_history_caller_tests.rs"]
mod callers;

#[tokio::test]
async fn agent_history_selection_private_session_preserves_json_pairs_without_recursive_current_input()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let inputs = vec![
        json!(null),
        json!([true, 3]),
        json!({"history":"business data","role":"system","text":"</context>"}),
        json!("scalar"),
    ];
    let outputs = vec![
        json!({"answer":1}),
        json!(false),
        json!([null, "answer"]),
        json!(null),
    ];
    let llm = scripted_llm(
        outputs
            .iter()
            .map(|value| LlmTurn::text(value.to_string()))
            .collect(),
    )
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let mut claims = Vec::new();
    for (input, output) in inputs.iter().zip(&outputs) {
        let claim = f.complete(input.clone(), output.clone()).await;
        assert_eq!(frozen(&f.f, &claim).await["context"], *input);
        claims.push(claim);
    }
    let claim = f.start(json!("current")).await;
    let launch =
        f.f.s
            .p()
            .prepare_agent(&claim, policy())
            .await
            .unwrap()
            .expect_ready();
    let history = launch.prepared.frozen_history().unwrap();
    assert_eq!(
        history
            .seed
            .pairs()
            .iter()
            .map(|pair| pair.input.clone())
            .collect::<Vec<_>>(),
        inputs
    );
    assert_eq!(
        history
            .seed
            .pairs()
            .iter()
            .map(|pair| pair.output.clone())
            .collect::<Vec<_>>(),
        outputs
    );
    assert_eq!(
        history
            .selected
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        claims
            .iter()
            .map(|claim| AgentExchangeId::for_execution(claim.fence.scope.execution))
            .collect::<Vec<_>>()
    );
    assert_eq!(launch.prepared.logical_input().value(), &json!("current"));
    let scope: String = sqlx::query_scalar(
        "SELECT scope_kind FROM agent_history_conversations WHERE company_id=$1 AND agent_id=$2",
    )
    .bind(claim.fence.scope.company.as_uuid())
    .bind(f.f.agent)
    .fetch_one(f.f.s.pool())
    .await
    .unwrap();
    assert_eq!(scope, "private_session");
    assert_eq!(llm.finish().await.unwrap(), 4);
}

#[tokio::test]
async fn agent_history_selection_pair_limit_keeps_recent_complete_suffix_and_exact_omissions() {
    let mut f = Box::pin(ManualHistory::new()).await;
    let llm = scripted_llm(
        (0..18)
            .map(|value| LlmTurn::text(value.to_string()))
            .collect(),
    )
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    for value in 0..17 {
        f.complete(
            json!({"task":value,"escaped":format!("</context>{}\"\\漢",char::from(1))}),
            json!(value),
        )
        .await;
    }
    let claim = f.start(json!({"current":true})).await;
    let launch =
        f.f.s
            .p()
            .prepare_agent(&claim, policy())
            .await
            .unwrap()
            .expect_ready();
    let history = launch.prepared.frozen_history().unwrap();
    assert_eq!(history.selected.len(), 16);
    assert_eq!(history.omitted_eligible, 1);
    assert_eq!(
        history
            .seed
            .pairs()
            .iter()
            .map(|pair| pair.output.clone())
            .collect::<Vec<_>>(),
        (1..17).map(|value| json!(value)).collect::<Vec<_>>()
    );
    assert_eq!(history.selected.first().unwrap().position.get(), 2);
    assert_eq!(history.selected.last().unwrap().position.get(), 17);
    let estimated = history.budget.mandatory_tokens
        + history.budget.history_tokens
        + history.budget.reserved_output_tokens;
    let result =
        Box::pin(runtime(&f.f).execute(&WorkflowStepKind("agent.run".into()), &claim)).await;
    assert_eq!(result.completed_result().unwrap(), json!(17));
    let reserved:i64=sqlx::query_scalar("SELECT reserved_input+reserved_output FROM model_usage_calls WHERE company_id=$1 AND run_id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.run.as_uuid()).fetch_one(f.f.s.pool()).await.unwrap();
    assert!(
        reserved as u64 <= estimated,
        "actual complete request exceeds conservative preparation estimate"
    );
    assert_eq!(llm.finish().await.unwrap(), 18);
}

#[tokio::test]
async fn agent_history_selection_oversized_newest_pair_stops_walk_without_loading_older_pair() {
    let mut f = Box::pin(ManualHistory::new()).await;
    let large_input = json!("i".repeat(49_000));
    let large_output = json!("o".repeat(20_000));
    let mut llm = scripted_llm(vec![
        LlmTurn::text("\"older\""),
        LlmTurn::text(large_output.to_string()),
        LlmTurn::text("\"current answer\""),
    ])
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    f.complete(json!("older input"), json!("older")).await;
    f.complete(large_input, large_output).await;
    let claim = f.start(json!("current")).await;
    let launch =
        f.f.s
            .p()
            .prepare_agent(&claim, policy())
            .await
            .unwrap()
            .expect_ready();
    let history = launch.prepared.frozen_history().unwrap();
    assert!(history.seed.pairs().is_empty());
    assert!(history.selected.is_empty());
    assert_eq!(history.omitted_eligible, 2);
    let result =
        Box::pin(runtime(&f.f).execute(&WorkflowStepKind("agent.run".into()), &claim)).await;
    assert_eq!(result.completed_result().unwrap(), json!("current answer"));
    let requests = llm.observed();
    assert_eq!(requests[2]["messages"].as_array().unwrap().len(), 2);
    assert_eq!(llm.finish().await.unwrap(), 3);
}

#[tokio::test]
async fn agent_history_preparation_mandatory_current_budget_refusal_has_no_provider_snapshot_or_order()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let llm = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let claim = f.start(json!("x".repeat(64_000))).await;
    let result =
        Box::pin(runtime(&f.f).execute(&WorkflowStepKind("agent.run".into()), &claim)).await;
    let failure = result.completed_result().unwrap_err();
    assert_eq!(failure.failure.code().as_str(), "agent.budget");
    let counts:Value=sqlx::query_scalar("SELECT jsonb_build_object('exchanges',(SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1),'conversations',(SELECT count(*) FROM agent_history_conversations WHERE company_id=$1),'checkpoints',(SELECT count(*) FROM task_harness_runs WHERE company_id=$1),'calls',(SELECT count(*) FROM model_usage_calls WHERE company_id=$1))")
        .bind(claim.fence.scope.company.as_uuid()).fetch_one(f.f.s.pool()).await.unwrap();
    assert_eq!(
        counts,
        json!({"exchanges":0,"conversations":0,"checkpoints":0,"calls":0})
    );
    assert_eq!(llm.finish().await.unwrap(), 0);
}

async fn republish_anthropic(f: &mut ManualHistory) {
    let p = f.f.s.p();
    CompanyPersistence::replace_model_connections_for_user(
        p,
        f.f.s.owner().user_id(),
        f.f.s.fixture.target.company.as_uuid(),
        vec![
            CompanyModelConnectionWrite {
                provider: "openai".into(),
                api_key: Some("fixture-key".into()),
                models: vec![crate::services::test_support::SCRIPTED_MODEL.into()],
                is_default: false,
            },
            CompanyModelConnectionWrite {
                provider: "anthropic".into(),
                api_key: Some("anthropic-fixture-key".into()),
                models: vec!["fixture-anthropic".into()],
                is_default: true,
            },
        ],
    )
    .await
    .unwrap();
    AgentPersistence::update(
        p,
        f.f.agent,
        AgentWrite {
            name: "History".into(),
            slug: "history".into(),
            provider: Some("anthropic".into()),
            model: Some("fixture-anthropic".into()),
            system_prompt: Some(
                "CURRENT ANTHROPIC INSTRUCTIONS. Return the current JSON object.".into(),
            ),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    f.f.s.fixture.publish_agent(f.f.agent).await;
    f.binding=history_contract_binding(&mut f.f.s,"history",HistoryContract {
        input:json!({"type":["object","array","string","number","boolean","null"]}),
        output:json!({"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false}),
    }).await;
}

#[tokio::test]
async fn agent_history_protocol_new_revision_provider_and_schema_keep_prior_pairs_as_data() {
    use crate::services::test_support::{
        ScriptedExchange, ScriptedResponse,
        provider::{Reply, response},
        scripted_scenario,
    };
    let mut f = Box::pin(ManualHistory::new()).await;
    let first_server = scripted_llm(vec![LlmTurn::text("\"prior answer\"")]).await;
    register_scripted_agent_base_url(f.f.agent, &first_server.base_url);
    let first = f
        .complete(json!({"old":"input"}), json!("prior answer"))
        .await;
    let original = frozen(&f.f, &first).await;
    assert_eq!(first_server.finish().await.unwrap(), 1);
    republish_anthropic(&mut f).await;
    let mut body = response("anthropic", Reply::Text);
    body["content"][0]["text"] = json!("{\"ok\":true}");
    let llm = scripted_scenario(vec![ScriptedExchange::new(
        |request| {
            if !request.header_matches("x-api-key", "anthropic-fixture-key") {
                return Err("current provider credential");
            }
            if request.body["model"] != "fixture-anthropic"
                || !request.body["system"]
                    .to_string()
                    .contains("CURRENT ANTHROPIC INSTRUCTIONS")
            {
                return Err("current model/instructions");
            }
            let messages = request.body["messages"].as_array().ok_or("messages")?;
            if messages.len() != 3
                || messages[0]["role"] != "user"
                || messages[1]["role"] != "assistant"
                || messages[2]["role"] != "user"
            {
                return Err("logical pair before current input");
            }
            if !messages[1]["content"].to_string().contains("prior answer") {
                return Err("prior accepted output missing");
            }
            Ok(())
        },
        ScriptedResponse::json(body),
    )])
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let claim = f.start(json!({"current":"input"})).await;
    let launch =
        f.f.s
            .p()
            .prepare_agent(&claim, policy())
            .await
            .unwrap()
            .expect_ready();
    let spec = launch.prepared.spec(claim.fence).unwrap();
    assert_eq!(spec.identity.provider.as_str(), "anthropic");
    assert_ne!(
        spec.prepared["selection"]["revision"],
        original["selection"]["revision"]
    );
    assert_eq!(spec.seed.pairs()[0].output, json!("prior answer"));
    assert_eq!(spec.prepared["output_schema"]["type"], "object");
    let result =
        Box::pin(runtime(&f.f).execute(&WorkflowStepKind("agent.run".into()), &claim)).await;
    assert_eq!(result.completed_result().unwrap(), json!({"ok":true}));
    assert_eq!(llm.finish().await.unwrap(), 1);
    assert_eq!(
        frozen(&f.f, &first).await,
        original,
        "old immutable source remains exact"
    );
}

#[tokio::test]
async fn agent_history_level_private_thread_anchor_checks_current_acting_member_on_resume() {
    let mut f = Box::pin(ManualHistory::new()).await;
    let actor = crate::adapters::persistence::workflow::tests::message_tests::support::member(
        &f.f.s.fixture,
        "admin",
    )
    .await;
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(f.f.s.fixture.target.company.as_uuid())
        .bind(actor)
        .execute(f.f.s.pool())
        .await
        .unwrap();
    f.actor = WorkflowActor::authenticated(actor).unwrap();
    let llm = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let claim = f.start(json!("private classified input")).await;
    level::raise(&f.f, &claim, crate::entities::access_level::AccessLevel::L5).await;
    f.f.s
        .p()
        .prepare_agent(&claim, policy())
        .await
        .unwrap()
        .expect_ready();
    let before = frozen(&f.f, &claim).await;
    let kind: String = sqlx::query_scalar(
        "SELECT scope_kind FROM agent_history_conversations WHERE company_id=$1 AND agent_id=$2",
    )
    .bind(claim.fence.scope.company.as_uuid())
    .bind(f.f.agent)
    .fetch_one(f.f.s.pool())
    .await
    .unwrap();
    assert_eq!(kind, "private_session");
    assert_eq!(
        sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
            .bind(claim.fence.scope.company.as_uuid())
            .bind(actor)
            .execute(f.f.s.pool())
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    let mut llm = llm;
    match f.f.s.p().prepare_agent(&claim, policy()).await {
        Ok(AgentPreparationResult::Ready(_)) => {
            panic!("private anchored preparation authorized a demoted acting member")
        }
        Ok(_) => panic!("private anchored preparation changed disposition"),
        Err(crate::app_error::AppError::KnowledgeRelease { .. }) => {}
        Err(error) => panic!("private anchored preparation failed through another path: {error:?}"),
    }
    let result =
        Box::pin(runtime(&f.f).execute(&WorkflowStepKind("agent.run".into()), &claim)).await;
    assert!(
        llm.observed().is_empty(),
        "private actor demotion must refuse before a provider request"
    );
    assert_eq!(
        result
            .completed_result()
            .unwrap_err()
            .failure
            .code()
            .as_str(),
        "knowledge.audience_below_content"
    );
    assert_eq!(frozen(&f.f, &claim).await, before);
    assert_eq!(llm.finish().await.unwrap(), 0);
}

#[tokio::test]
async fn agent_history_level_private_thread_anchor_keeps_authorized_member_above_public_anchor() {
    use crate::entities::access_level::AccessLevel;
    let mut f = Box::pin(ManualHistory::new()).await;
    let actor = crate::adapters::persistence::workflow::tests::message_tests::support::member(
        &f.f.s.fixture,
        "admin",
    )
    .await;
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(f.f.s.fixture.target.company.as_uuid())
        .bind(actor)
        .execute(f.f.s.pool())
        .await
        .unwrap();
    f.actor = WorkflowActor::authenticated(actor).unwrap();
    let llm = scripted_llm(vec![LlmTurn::text("\"private answer\"")]).await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let claim = f.start(json!("private classified input")).await;
    level::raise(&f.f, &claim, AccessLevel::L5).await;
    f.f.s
        .p()
        .prepare_agent(&claim, policy())
        .await
        .unwrap()
        .expect_ready();
    let before = frozen(&f.f, &claim).await;
    let intent = f.f.s.intent(TO).await;
    sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) SELECT intent.company_id,intent.channel_id,intent.thread_id,message.author_principal_id,'participant' FROM workflow_message_intents AS intent JOIN messages AS message ON message.company_id=intent.company_id AND message.id=intent.message_id WHERE intent.company_id=$1 AND intent.id=$2 ON CONFLICT DO NOTHING")
        .bind(claim.fence.scope.company.as_uuid()).bind(intent).execute(f.f.s.pool()).await.unwrap();
    let mut connection = f.f.s.pool().acquire().await.unwrap();
    let audience = crate::adapters::persistence::workflow::knowledge::thread_audience_on(
        &mut connection,
        claim.fence.scope.company,
        RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(f.f.s.channel),
            thread_id: RelatedThreadId::new(f.f.s.thread),
        },
    )
    .await
    .unwrap();
    assert_eq!(audience, AccessLevel::PUBLIC);
    drop(connection);
    let result =
        Box::pin(runtime(&f.f).execute(&WorkflowStepKind("agent.run".into()), &claim)).await;
    assert_eq!(result.completed_result().unwrap(), json!("private answer"));
    assert_eq!(frozen(&f.f, &claim).await, before);
    assert_eq!(llm.finish().await.unwrap(), 1);
}

#[path = "agent_history_retention_tests.rs"]
mod retention;

#[path = "agent_history_views_tests.rs"]
mod views;

#[path = "agent_history_retention_capacity_tests.rs"]
mod retention_capacity;

#[path = "agent_history_metadata_capacity_tests.rs"]
mod metadata_capacity;

#[path = "agent_history_controls_tests.rs"]
mod controls;

#[path = "agent_history_owner_race_tests.rs"]
mod owner_races;

#[path = "agent_history_retry_retention_tests.rs"]
mod retry_retention;

#[path = "agent_history_e2e_tests.rs"]
mod e2e;
