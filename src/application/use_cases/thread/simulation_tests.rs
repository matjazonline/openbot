//! The product simulation ingress, durable dispatch, real Rig tools, and escaped HTML.
use super::*;
use crate::{
    adapters::http::{pages, routes::channel::compose_and_ingest},
    entities::harness::HarnessKind,
    services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario},
};
use serde_json::{Value, json};

fn echo_call() -> Value {
    json!({"id":"sim-tool", "object":"chat.completion", "created":0, "model":SCRIPTED_MODEL,
        "choices":[{"index":0,"finish_reason":"tool_calls","message":{"role":"assistant","content":null,
            "tool_calls":[{"id":"sim-echo","type":"function","function":{"name":"echo","arguments":"{\"message\":\"simulation result\"}"}}]}}],
        "usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}})
}

#[tokio::test]
async fn queued_simulation_continues_after_a_tool_and_renders_validated_json() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let expected = r#"{"status":"<script>**done**</script>"}"#;
    let llm = scripted_scenario(vec![
        ScriptedExchange::new(|_| Ok(()), ScriptedResponse::json(echo_call())),
        ScriptedExchange::new(
            |request| {
                let body = request.body.to_string();
                if !body.contains("simulation result") || !body.contains("sim-echo") {
                    return Err("missing tool continuation");
                }
                Ok(())
            },
            ScriptedResponse::turn(LlmTurn::text(expected), 1),
        ),
    ])
    .await;
    let fx = fixture_for_harness(pool.clone(), Some(&llm.base_url), HarnessKind::Rig).await;
    let contract = json!({"version":1,"format":"json_schema","schema":{"type":"object","properties":{"status":{"type":"string"}},"required":["status"]}});
    sqlx::query("UPDATE agents SET response_contract=$1 WHERE company_id=$2")
        .bind(&contract)
        .bind(fx.company.id)
        .execute(&pool)
        .await
        .unwrap();
    let ingest = compose_and_ingest(
        &fx.threads,
        inbound(
            &fx.owner_email,
            &fx.address(&fx.channel_a),
            "<simulation@test.example>",
            "Return status",
        ),
        ReplyDelivery::InAppOnly,
    )
    .await
    .unwrap();
    let task = ingest.task_id.unwrap();
    let lease = fx.claim(task).await;
    assert!(matches!(
        fx.threads
            .execute_claimed_agent_task_and_dispatch(
                &ingest,
                ReplyDelivery::InAppOnly,
                lease,
                ingest.correlation_id().unwrap()
            )
            .await
            .unwrap(),
        DispatchOutcome::Replied(_)
    ));
    assert_eq!(llm.finish().await, Ok(2));
    let thread = ingest.thread.as_ref().unwrap();
    let messages = fx.threads.get_thread_history(thread.id).await.unwrap();
    let reply = messages.iter().find(|message| message.is_agent()).unwrap();
    assert_eq!(reply.body, expected);
    assert_eq!(
        serde_json::to_value(&reply.response_contract).unwrap(),
        contract
    );
    let tasks = fx
        .threads
        .list_company_tasks(fx.company.id, Some(fx.channel_a.id), None, true)
        .await
        .unwrap();
    let html = pages::channel_simulation_loaded_thread_fragment(&pages::SimulationThreadView {
        company: &fx.company,
        channel: &fx.channel_a,
        app_domain_name: "example.test",
        thread,
        messages: &messages,
        reply_context: None,
        tasks: &tasks,
        include_oob: false,
    });
    assert!(html.contains("Validated JSON"));
    assert!(html.contains("Runtime: Rig"));
    assert!(html.contains("&lt;script&gt;**done**&lt;/script&gt;"));
    assert!(!html.contains("<script>**done**"));
    assert!(!html.contains("scripted-key"));
    let deliveries: i64 =
        sqlx::query_scalar("SELECT count(*) FROM message_deliveries WHERE company_id=$1")
            .bind(fx.company.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(deliveries, 0);
    CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
        .await
        .unwrap();
}
