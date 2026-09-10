//! Transport proof only: remote calls remain explicitly dispatched, never auto-registered.
use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
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
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let mut config = StreamableHttpClientTransportConfig::with_uri(server.base_url.clone())
            .auth_header("mcp-key");
        config.allow_stateless = true;
        config.reinit_on_expired_session = false;
        let transport = StreamableHttpClientTransport::with_client(http, config);
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            let client = ().serve(transport).await.unwrap();
            let tools = client.list_tools(None).await.unwrap();
            assert_eq!(tools.tools.len(), 1);
            assert_eq!(tools.tools[0].name, "lookup");
            // This is the guarded bridge seam. Discovery alone has executed nothing;
            // only a validated name and arguments are passed to rmcp, without Rig's rmcp helpers.
            let result = client
                .call_tool(
                    CallToolRequestParams::new("lookup")
                        .with_arguments(json!({"key":"record"}).as_object().unwrap().clone()),
                )
                .await
                .unwrap();
            assert_eq!(result.is_error, Some(false));
            assert!(
                serde_json::to_string(&result)
                    .unwrap()
                    .contains("record-value")
            );
            client.cancel().await.unwrap();
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
