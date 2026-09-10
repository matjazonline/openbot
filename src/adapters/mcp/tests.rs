//! Real guarded transport fixtures; both external boundaries are local.
use super::*;
use crate::services::mcp_client::McpClient;
use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
use serde_json::{Value, json};
use std::time::Duration;

fn exchange(method: &'static str, id: Option<u64>, result: Value) -> ScriptedExchange {
    let response = if let Some(id) = id {
        ScriptedResponse::json(json!({"jsonrpc":"2.0","id":id,"result":result}))
    } else {
        let mut response = ScriptedResponse::json_body(String::new());
        response.status = 202;
        response
    };
    ScriptedExchange::new(
        move |request| {
            if request.method != "POST" || request.path != "/" || request.body["method"] != method {
                return Err("MCP method/path");
            }
            if !request.header_matches("authorization", "Bearer mcp-key") {
                return Err("MCP authentication (redacted)");
            }
            if let Some(id) = id
                && request.body["id"] != id
            {
                return Err("MCP request id");
            }
            if method == "tools/call"
                && (request.body["params"]["name"] != "lookup"
                    || request.body["params"]["arguments"] != json!({"key":"record"}))
            {
                return Err("MCP guarded arguments");
            }
            Ok(())
        },
        response,
    )
}

#[tokio::test]
async fn mcp_initializes_discovers_and_explicitly_calls_json_and_sse_then_shuts_down() {
    for sse in [false, true] {
        let mut call = exchange(
            "tools/call",
            Some(2),
            json!({"content":[{"type":"text","text":"record-value"}],"isError":false}),
        );
        if sse {
            call = sse_call();
        }
        let server = scripted_scenario(vec![
            exchange("initialize", Some(0), json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"scripted","version":"1"}})),
            exchange("notifications/initialized", None, Value::Null),
            exchange("tools/list", Some(1), json!({"tools":[{"name":"lookup","description":"Read a record","inputSchema":{"type":"object","properties":{"key":{"type":"string"}},"required":["key"]}}]})),
            call,
        ]).await;
        let adapter = HttpMcpClient::new(
            EndpointPolicy::new([format!("{}/", server.base_url.trim_end_matches('/'))]).unwrap(),
        );
        let endpoint = server.base_url.clone().try_into().unwrap();
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            let client = adapter
                .connect(&endpoint, Some(SecretString::from("mcp-key")))
                .await
                .unwrap();
            let tools = client.discover().await.unwrap();
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].name.as_str(), "lookup");
            // This is the guarded bridge seam. Discovery alone has executed nothing;
            // only a validated name and arguments are passed to rmcp, without Rig's rmcp helpers.
            let result = client
                .call(
                    &"lookup".to_string().try_into().unwrap(),
                    json!({"key":"record"}),
                )
                .await
                .unwrap();
            assert_eq!(result["isError"], false);
            assert!(
                serde_json::to_string(&result)
                    .unwrap()
                    .contains("record-value")
            );
            client.close().await.unwrap();
        })
        .await;
        let completion = server.finish().await;
        assert_eq!(completion, Ok(4));
        result.unwrap();
    }
}

fn sse_call() -> ScriptedExchange {
    let mut response = ScriptedResponse::json_body(format!(
        "event: message\ndata: {}\n\n",
        json!({"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"record-value"}],"isError":false}})
    ));
    response.headers = vec![("Content-Type".into(), "text/event-stream".into())];
    ScriptedExchange::new(
        |request| {
            if request.body["method"] != "tools/call" || request.body["params"]["name"] != "lookup"
            {
                return Err("SSE call");
            }
            Ok(())
        },
        response,
    )
}

fn initialize() -> Vec<ScriptedExchange> {
    vec![
        exchange(
            "initialize",
            Some(0),
            json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}),
        ),
        exchange("notifications/initialized", None, Value::Null),
    ]
}
fn adapter(url: &str) -> HttpMcpClient {
    HttpMcpClient::new(EndpointPolicy::new([format!("{}/", url.trim_end_matches('/'))]).unwrap())
}
#[tokio::test]
async fn mcp_discovery_rejects_duplicates_external_schemas_secret_echoes_and_oversized_data() {
    let tool = json!({"name":"lookup","description":"safe","inputSchema":{"type":"object"}});
    for tools in [
        json!([tool.clone(), tool.clone()]),
        json!([{"name":"lookup","inputSchema":{"$ref":"file:///etc/passwd"}}]),
        json!([{"name":"lookup","description":"mcp-key","inputSchema":{"type":"object"}}]),
        json!([{"name":"lookup","description":"x".repeat(MAX_MCP_DISCOVERY_BYTES),"inputSchema":{"type":"object"}}]),
        Value::Array(
            (0..101)
                .map(|i| json!({"name":format!("tool-{i}"),"inputSchema":{"type":"object"}}))
                .collect(),
        ),
    ] {
        let mut exchanges = initialize();
        exchanges.push(exchange("tools/list", Some(1), json!({"tools":tools})));
        let server = scripted_scenario(exchanges).await;
        let adapter = adapter(&server.base_url);
        let session = adapter
            .connect(
                &server.base_url.clone().try_into().unwrap(),
                Some(SecretString::from("mcp-key")),
            )
            .await
            .unwrap();
        assert!(session.discover().await.is_err());
        session.close().await.unwrap();
        adapter.shutdown().await.unwrap();
        assert_eq!(server.finish().await, Ok(3));
    }
}
#[tokio::test]
async fn mcp_repeated_pagination_cursor_fails_without_calling_a_tool() {
    let mut exchanges = initialize();
    exchanges.push(exchange(
        "tools/list",
        Some(1),
        json!({"tools":[],"nextCursor":"again"}),
    ));
    exchanges.push(exchange(
        "tools/list",
        Some(2),
        json!({"tools":[],"nextCursor":"again"}),
    ));
    let server = scripted_scenario(exchanges).await;
    let adapter = adapter(&server.base_url);
    let session = adapter
        .connect(
            &server.base_url.clone().try_into().unwrap(),
            Some(SecretString::from("mcp-key")),
        )
        .await
        .unwrap();
    assert!(session.discover().await.is_err());
    session.close().await.unwrap();
    adapter.shutdown().await.unwrap();
    assert_eq!(server.finish().await, Ok(4));
}
#[tokio::test]
async fn mcp_authentication_redirect_and_lost_effect_are_sanitized_and_never_retried() {
    for status in [401, 403, 302, 404] {
        let mut response = ScriptedResponse::json_body("secret-bearing-remote-error".into());
        response.status = status;
        response
            .headers
            .push(("Location".into(), "http://169.254.169.254/".into()));
        let server = scripted_scenario(vec![ScriptedExchange::new(|_| Ok(()), response)]).await;
        let adapter = adapter(&server.base_url);
        let result = adapter
            .connect(
                &server.base_url.clone().try_into().unwrap(),
                Some(SecretString::from("mcp-key")),
            )
            .await;
        let error = result.err().expect("refused response").to_string();
        assert!(!error.contains("secret-bearing") && !error.contains("mcp-key"));
        adapter.shutdown().await.unwrap();
        assert_eq!(server.finish().await, Ok(1));
    }
    let mut exchanges = initialize();
    let mut response = ScriptedResponse::json(Value::Null);
    response.disconnect = true;
    exchanges.push(ScriptedExchange::new(
        |r| {
            if r.body["method"] == "tools/call" {
                Ok(())
            } else {
                Err("call expected")
            }
        },
        response,
    ));
    let server = scripted_scenario(exchanges).await;
    let adapter = adapter(&server.base_url);
    let session = adapter
        .connect(
            &server.base_url.clone().try_into().unwrap(),
            Some(SecretString::from("mcp-key")),
        )
        .await
        .unwrap();
    let error = session
        .call(&"lookup".to_string().try_into().unwrap(), json!({}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("indeterminate"));
    session.close().await.unwrap();
    adapter.shutdown().await.unwrap();
    assert_eq!(server.finish().await, Ok(3));
}
#[tokio::test]
async fn mcp_shutdown_joins_an_abandoned_session_and_closes_admission() {
    let server = scripted_scenario(initialize()).await;
    let adapter = adapter(&server.base_url);
    let endpoint = server.base_url.clone().try_into().unwrap();
    let session = adapter
        .connect(&endpoint, Some(SecretString::from("mcp-key")))
        .await
        .unwrap();
    drop(session);
    adapter.shutdown().await.unwrap();
    assert!(adapter.workers.lock().await.is_empty());
    assert!(adapter.connect(&endpoint, None).await.is_err());
    assert_eq!(server.finish().await, Ok(2));
}

#[tokio::test]
async fn mcp_dropped_call_sends_protocol_cancellation_and_joins_cleanup() {
    let (arrived, observed) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let mut response = ScriptedResponse::json(
        json!({"jsonrpc":"2.0","id":1,"result":{"content":[],"isError":false}}),
    );
    response.barrier = Some((arrived, released));
    let mut script = initialize();
    script.push(ScriptedExchange::new(
        |r| {
            if r.body["method"] == "tools/call" {
                Ok(())
            } else {
                Err("call required")
            }
        },
        response,
    ));
    let mut accepted = ScriptedResponse::json_body(String::new());
    accepted.status = 202;
    script.push(ScriptedExchange::new(
        |r| {
            if r.body["method"] == "notifications/cancelled" && r.body["params"]["requestId"] == 1 {
                Ok(())
            } else {
                Err("cancellation required")
            }
        },
        accepted,
    ));
    let server = scripted_scenario(script).await;
    let adapter = adapter(&server.base_url);
    let session = adapter
        .connect(
            &server.base_url.clone().try_into().unwrap(),
            Some(SecretString::from("mcp-key")),
        )
        .await
        .unwrap();
    let task = tokio::spawn(async move {
        session
            .call(&"lookup".to_string().try_into().unwrap(), json!({}))
            .await
    });
    observed.await.unwrap();
    task.abort();
    let _ = task.await;
    release.send(()).unwrap();
    adapter.shutdown().await.unwrap();
    assert!(adapter.workers.lock().await.is_empty());
    assert_eq!(server.finish().await, Ok(4));
}
