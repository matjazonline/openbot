//! Caller abandonment and adapter shutdown compete for one real protocol cleanup.
use super::*;
use crate::services::test_support::ScriptedRequest;
use std::sync::{Arc, Mutex};

const VERSION: &str = "2025-06-18";

fn accepted() -> ScriptedResponse {
    let mut response = ScriptedResponse::json_body(String::new());
    response.status = 202;
    response
}

fn protocol(request: &ScriptedRequest, session: Option<&str>) -> Result<(), &'static str> {
    if !request.header_matches("authorization", "Bearer mcp-key")
        || !request.header_matches("mcp-protocol-version", VERSION)
        || session.is_some_and(|id| !request.header_matches("mcp-session-id", id))
    {
        return Err("cleanup must preserve the actual connection protocol and credential metadata");
    }
    Ok(())
}

fn handshake(session: Option<&'static str>) -> Vec<ScriptedExchange> {
    let mut response = ScriptedResponse::json(json!({"jsonrpc":"2.0","id":0,"result":{
        "protocolVersion":VERSION,"capabilities":{"tools":{}},
        "serverInfo":{"name":"cleanup fixture","version":"1"}}}));
    if let Some(session) = session {
        response
            .headers
            .push(("Mcp-Session-Id".into(), session.into()));
    }
    vec![
        ScriptedExchange::new(
            |request| {
                if request.body["method"] == "initialize" {
                    Ok(())
                } else {
                    Err("initialize required")
                }
            },
            response,
        ),
        ScriptedExchange::new(
            move |request| {
                protocol(request, session)?;
                if request.body["method"] == "notifications/initialized" {
                    Ok(())
                } else {
                    Err("initialized required")
                }
            },
            accepted(),
        ),
    ]
}

async fn competing_cleanup(session: Option<&'static str>) {
    let (arrived, observed) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let id = Arc::new(Mutex::new(None::<Value>));
    let captured = id.clone();
    let mut response = ScriptedResponse::json(json!({"jsonrpc":"2.0","id":1,
        "result":{"content":[],"isError":false}}));
    response.barrier = Some((arrived, released));
    let mut script = handshake(session);
    script.push(ScriptedExchange::new(
        move |request| {
            protocol(request, session)?;
            if request.body["method"] != "tools/call" {
                return Err("one effect call required");
            }
            *captured.lock().unwrap() = Some(request.body["id"].clone());
            Ok(())
        },
        response,
    ));
    script.push(ScriptedExchange::new(
        move |request| {
            protocol(request, session)?;
            if request.body["method"] == "notifications/cancelled"
                && id.lock().unwrap().as_ref() == Some(&request.body["params"]["requestId"])
            {
                Ok(())
            } else {
                Err("exactly the dispatched request must be cancelled")
            }
        },
        accepted(),
    ));
    if session.is_some() {
        script.push(ScriptedExchange::new(
            move |request| {
                protocol(request, session)?;
                if request.method == "DELETE" {
                    Ok(())
                } else {
                    Err("DELETE must follow cancellation")
                }
            },
            accepted(),
        ));
    }
    let server = scripted_scenario(script).await;
    let adapter = Arc::new(adapter(&server.base_url));
    let connected = adapter
        .connect(
            &server.base_url.clone().try_into().unwrap(),
            Some(SecretString::from("mcp-key")),
        )
        .await
        .unwrap();
    let call = tokio::spawn(async move {
        connected
            .call(&"lookup".to_owned().try_into().unwrap(), json!({}))
            .await
    });
    observed.await.unwrap();
    let start = Arc::new(tokio::sync::Barrier::new(3));
    let drop_start = start.clone();
    let dropped = tokio::spawn(async move {
        drop_start.wait().await;
        call.abort();
        assert!(
            !matches!(call.await, Ok(Ok(_))),
            "held call cannot finish successfully before abandonment"
        );
        release.send(()).unwrap();
    });
    let stop_start = start.clone();
    let stopping = adapter.clone();
    let shutdown = tokio::spawn(async move {
        stop_start.wait().await;
        stopping.shutdown().await.unwrap();
    });
    start.wait().await;
    dropped.await.unwrap();
    shutdown.await.unwrap();
    assert!(adapter.workers.lock().await.is_empty());
    assert_eq!(
        server.finish().await,
        Ok(if session.is_some() { 5 } else { 4 })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_drop_and_shutdown_compete_for_one_stateless_cancellation() {
    Box::pin(competing_cleanup(None)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_drop_and_shutdown_cancel_before_one_stateful_delete() {
    Box::pin(competing_cleanup(Some("cleanup-session"))).await;
}
