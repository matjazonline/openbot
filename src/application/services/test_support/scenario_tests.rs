use super::*;
use crate::services::test_support::{
    register_scripted_agent_base_url, require_scripted_endpoint, scripted_agent_base_url,
};

#[tokio::test]
async fn registration_is_scoped_and_missing_external_endpoints_fail_locally() {
    for endpoint in [
        None,
        Some("https://192.0.2.1/"),
        Some("http://127.0.0.1:1/"),
    ] {
        assert_eq!(
            require_scripted_endpoint(endpoint),
            Err("model fixture endpoint is not registered")
        );
    }
    let agent = uuid::Uuid::new_v4();
    let server = scripted_scenario(vec![]).await;
    let endpoint = server.base_url.clone();
    register_scripted_agent_base_url(agent, &endpoint);
    assert_eq!(require_scripted_endpoint(Some(&endpoint)), Ok(()));
    assert_eq!(scripted_agent_base_url(agent), Some(endpoint.clone()));
    assert_eq!(server.finish().await, Ok(0));
    assert_eq!(scripted_agent_base_url(agent), None);
    assert!(require_scripted_endpoint(Some(&endpoint)).is_err());
}

#[tokio::test]
async fn shutdown_cancels_a_blocked_response_and_awaits_its_connection() {
    let (arrived, arrival) = oneshot::channel();
    let (_release, released) = oneshot::channel();
    let mut response = ScriptedResponse::json(Value::Null);
    response.barrier = Some((arrived, released));
    let server = scripted_scenario(vec![ScriptedExchange::new(|_| Ok(()), response)]).await;
    let client = reqwest::Client::new();
    let request = client.post(&server.base_url).json(&Value::Null).send();
    let shutdown = async {
        tokio::time::timeout(WAIT, arrival).await.unwrap().unwrap();
        let completion = tokio::time::timeout(Duration::from_secs(1), server.finish()).await;
        assert_eq!(
            completion.unwrap().unwrap_err(),
            "scenario stopped during an exchange"
        );
    };
    let (result, ()) = tokio::join!(request, shutdown);
    assert!(result.is_err());
}

#[tokio::test]
async fn declared_disconnect_and_retry_are_counted_without_a_fallback_response() {
    let mut disconnected = ScriptedResponse::json(Value::Null);
    disconnected.disconnect = true;
    let mut limited = ScriptedResponse::json(serde_json::json!({"error":"synthetic rate limit"}));
    limited.status = 429;
    limited.headers.push(("Retry-After".into(), "0".into()));
    let server = scripted_scenario(vec![
        ScriptedExchange::new(
            |request| {
                if request.body["attempt"] != 1 {
                    return Err("first attempt");
                }
                Ok(())
            },
            disconnected,
        ),
        ScriptedExchange::new(
            |request| {
                if request.body["attempt"] != 2 {
                    return Err("declared retry");
                }
                Ok(())
            },
            limited,
        ),
    ])
    .await;
    let client = reqwest::Client::new();
    assert!(
        client
            .post(&server.base_url)
            .json(&serde_json::json!({"attempt":1}))
            .send()
            .await
            .is_err()
    );
    let response = client
        .post(&server.base_url)
        .json(&serde_json::json!({"attempt":2}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 429);
    assert_eq!(response.headers()["retry-after"], "0");
    assert_eq!(server.finish().await, Ok(2));
}

#[tokio::test]
async fn dropping_a_fixture_cancels_its_listener_and_removes_registration() {
    let server = scripted_scenario(vec![]).await;
    let endpoint = server.base_url.clone();
    let worker = server.worker.as_ref().unwrap().abort_handle();
    drop(server);
    tokio::time::timeout(WAIT, async {
        while !worker.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(require_scripted_endpoint(Some(&endpoint)).is_err());
    assert!(reqwest::Client::new().post(endpoint).send().await.is_err());
}

#[tokio::test]
async fn malformed_framing_fails_completion_without_capturing_authentication() {
    let server = scripted_scenario(vec![ScriptedExchange::new(
        |_| Ok(()),
        ScriptedResponse::json(Value::Null),
    )])
    .await;
    let address = server
        .base_url
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket.write_all(b"POST / HTTP/1.1\r\nAuthorization: fixture-private-key\r\nContent-Length: 4194305\r\n\r\n").await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(WAIT, socket.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        server.finish().await.unwrap_err(),
        "invalid scripted request framing"
    );
}
