//! Caller abandonment and adapter shutdown compete for one real protocol cleanup.
use super::super::registration_test_support::RegistrationProbe;
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

fn cleanup_script(
    session: Option<&'static str>,
    response: ScriptedResponse,
) -> Vec<ScriptedExchange> {
    let id = Arc::new(Mutex::new(None::<Value>));
    let captured = id.clone();
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
    script
}

async fn competing_cleanup(
    session: Option<&'static str>,
    registration: Option<(Arc<RegistrationProbe>, std::sync::mpsc::Sender<()>)>,
) {
    let (arrived, observed) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let mut response = ScriptedResponse::json(json!({"jsonrpc":"2.0","id":1,
        "result":{"content":[],"isError":false}}));
    response.barrier = Some((arrived, released));
    let server = scripted_scenario(cleanup_script(session, response)).await;
    let mut client = adapter(&server.base_url);
    client.registration_probe = registration.as_ref().map(|(probe, _)| probe.clone());
    let adapter = Arc::new(client);
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
    if let Some((probe, _)) = &registration {
        probe.enqueued.notified().await;
    }
    let start = Arc::new(tokio::sync::Barrier::new(3));
    let drop_start = start.clone();
    let dropped = tokio::spawn(async move {
        drop_start.wait().await;
        call.abort();
        assert!(
            !matches!(call.await, Ok(Ok(_))),
            "held call cannot finish successfully before abandonment"
        );
    });
    let stop_start = start.clone();
    let stopping = adapter.clone();
    let shutdown = tokio::spawn(async move {
        stop_start.wait().await;
        stopping.shutdown().await.unwrap();
    });
    start.wait().await;
    dropped.await.unwrap();
    if let Some((probe, resume)) = registration {
        probe.delete_entered.notified().await;
        // In the old ordering, let cleanup consume the absent ID before resuming publication.
        // This forces a real wire-order failure instead of relying on thread scheduling luck.
        if !probe.gate_blocked.load(std::sync::atomic::Ordering::SeqCst) {
            probe.cleanup_claimed.notified().await;
        }
        resume.send(()).unwrap();
    }
    release.send(()).unwrap();
    shutdown.await.unwrap();
    assert!(adapter.workers.lock().await.is_empty());
    assert_eq!(
        server.finish().await,
        Ok(if session.is_some() { 5 } else { 4 })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_drop_and_shutdown_compete_for_one_stateless_cancellation() {
    Box::pin(competing_cleanup(None, None)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_drop_and_shutdown_cancel_before_one_stateful_delete() {
    Box::pin(competing_cleanup(Some("cleanup-session"), None)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mcp_enqueue_preemption_cancels_original_call_before_shutdown_deletes_session() {
    Box::pin(competing_cleanup(
        Some("handoff-session"),
        Some(RegistrationProbe::new()),
    ))
    .await;
}

#[tokio::test]
async fn mcp_completed_protocol_error_does_not_cancel_or_retry_the_call() {
    let mut script = handshake(None);
    script.push(ScriptedExchange::new(
        |request| {
            protocol(request, None)?;
            if request.body["method"] == "tools/call" && request.body["id"] == 1 {
                Ok(())
            } else {
                Err("one call required")
            }
        },
        ScriptedResponse::json(json!({"jsonrpc":"2.0","id":1,
            "error":{"code":-32603,"message":"remote failure"}})),
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
    assert!(
        session
            .call(&"lookup".to_owned().try_into().unwrap(), json!({}))
            .await
            .is_err()
    );
    session.close().await.unwrap();
    adapter.shutdown().await.unwrap();
    assert_eq!(server.finish().await, Ok(3));
}
