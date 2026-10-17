use super::*;
use crate::domain::workflow::CompanyId;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

fn connection(origin: &str) -> HttpConnection {
    HttpConnection {
        company: CompanyId::new(Uuid::new_v4()),
        id: Uuid::new_v4(),
        origin: HttpOrigin::parse(origin).unwrap(),
        enabled: true,
        bearer: false,
        secret_set: false,
        revision: 1,
    }
}
async fn endpoint(response: Vec<u8>) -> (HttpConnection, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = vec![0; 8192];
        let read = socket.read(&mut bytes).await.unwrap();
        socket.write_all(&response).await.unwrap();
        String::from_utf8(bytes[..read].to_vec()).unwrap()
    });
    (connection(&origin), task)
}
async fn exchange(response: Vec<u8>, secret: Option<SecretString>) -> (AppResult<Value>, String) {
    let (connection, task) = endpoint(response).await;
    let policy = HttpDestinationPolicy::new([connection.origin.as_str().to_owned()]).unwrap();
    let client = client(&policy, &connection).await.unwrap();
    let request = HttpRequest::parse(&json!({"method":"GET","path":"/safe?key=value"})).unwrap();
    let result = send(client, &connection, secret.as_ref(), request).await;
    let request = task.await.unwrap();
    (result, request)
}

#[test]
fn workflow_http_origin_and_request_authority_cannot_be_replaced() {
    for origin in [
        "https://user:secret@example.com",
        "https://example.com/path",
        "https://example.com/?secret=x",
        "file:///etc/passwd",
        "https://example.com/#fragment",
    ] {
        assert!(HttpOrigin::parse(origin).is_err());
    }
    for path in [
        "//evil.test/path",
        "https://evil.test",
        "/\\evil.test",
        "/path#fragment",
        "/a\r\nHost:evil",
        "relative",
    ] {
        assert!(HttpRequest::parse(&json!({"method":"GET","path":path})).is_err());
    }
    for header in [
        "Authorization",
        "HOST",
        "cookie",
        "X-Api-Key",
        "Proxy-Authorization",
        "Connection",
        "Transfer-Encoding",
        "Content-Length",
        "Forwarded",
        "X-Forwarded-Host",
    ] {
        assert!(
            HttpRequest::parse(&json!({"method":"GET","path":"/","headers":{header:"secret"}}))
                .is_err()
        );
    }
    assert!(
        HttpRequest::parse(&json!({"method":"GET","path":"/","origin":"https://evil.test"}))
            .is_err()
    );
    assert!(
        HttpRequest::parse(&json!({"method":"POST","path":"/","body":"x".repeat(MAX_HTTP_BODY)}))
            .is_err()
    );
}

#[tokio::test]
async fn workflow_http_default_denies_special_addresses_and_operator_scope_is_exact() {
    for host in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.169.254",
        "192.168.1.1",
        "100.64.0.1",
        "0.0.0.0",
        "[::1]",
        "[::ffff:127.0.0.1]",
        "[fc00::1]",
        "[fe80::1]",
        "localhost",
        "metadata.google.internal",
    ] {
        let connection = connection(&format!("https://{host}"));
        assert!(
            client(&HttpDestinationPolicy::default(), &connection)
                .await
                .is_err(),
            "{host}"
        );
    }
    assert!(HttpDestinationPolicy::new(["http://localhost:8080".into()]).is_err());
    let policy = HttpDestinationPolicy::new(["http://127.0.0.1:8080".into()]).unwrap();
    assert!(
        client(&policy, &connection("http://127.0.0.1:8081"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn workflow_http_response_redacts_secret_and_sensitive_headers_before_output() {
    let body = r#"{"token":"shh-secret","escaped":"shh-\u0073ecret","authorization":"other-sensitive","shh-secret":"value"}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: hidden-secret\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let (result, request) = exchange(
        response.into_bytes(),
        Some(SecretString::from("shh-secret")),
    )
    .await;
    assert!(request.starts_with("GET /safe?key=value HTTP/1.1"));
    assert!(
        request
            .to_lowercase()
            .contains("authorization: bearer shh-secret")
    );
    let output = result.unwrap();
    let serialized = output.to_string();
    for secret in ["shh-secret", "hidden-secret", "other-sensitive"] {
        assert!(!serialized.contains(secret));
    }
    assert_eq!(output["body"]["escaped"], "[REDACTED]");
    assert_eq!(output["headers"].as_object().unwrap().len(), 1);
}

#[tokio::test]
async fn workflow_http_redirect_errors_and_oversize_never_produce_receipts() {
    for response in [
        b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/secret\r\nContent-Length: 0\r\n\r\n"
            .to_vec(),
        b"HTTP/1.1 500 Error\r\nContent-Length: 6\r\n\r\nsecret".to_vec(),
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            MAX_HTTP_RESPONSE + 1
        )
        .into_bytes(),
        format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
            MAX_HTTP_RESPONSE + 1,
            "x".repeat(MAX_HTTP_RESPONSE + 1)
        )
        .into_bytes(),
    ] {
        let (result, _) = exchange(response, None).await;
        assert!(result.is_err());
        assert!(!format!("{}", result.unwrap_err()).contains("secret"));
    }
}

#[tokio::test]
async fn workflow_http_scalar_secret_echoes_are_redacted() {
    for secret in ["123456", "true", "null"] {
        let body = format!(r#"{{"echo":{secret},"safe":"visible"}}"#);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (result, _) = exchange(response.into_bytes(), Some(SecretString::from(secret))).await;
        let output = result.unwrap();
        assert_eq!(output["body"]["echo"], "[REDACTED]");
        assert_eq!(output["body"]["safe"], "visible");
        assert!(!output.to_string().contains(secret));
    }
}

#[tokio::test]
async fn workflow_http_pending_actual_request_is_dropped_on_cancellation() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connection = connection(&format!("http://{}", listener.local_addr().unwrap()));
    let policy = HttpDestinationPolicy::new([connection.origin.as_str().to_owned()]).unwrap();
    let client = client(&policy, &connection).await.unwrap();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        assert!(socket.read(&mut bytes).await.unwrap() > 0);
        tokio::time::timeout(Duration::from_secs(2), socket.read(&mut bytes))
            .await
            .unwrap()
            .unwrap()
    });
    let request = HttpRequest::parse(&json!({"method":"POST","path":"/"})).unwrap();
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        send(client, &connection, None, request),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(
        task.await.unwrap(),
        0,
        "dropped actual request closes its connection"
    );
}

#[tokio::test]
async fn workflow_http_stalled_body_hits_read_deadline() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connection = connection(&format!("http://{}", listener.local_addr().unwrap()));
    let policy = HttpDestinationPolicy::new([connection.origin.as_str().to_owned()]).unwrap();
    let client = client(&policy, &connection).await.unwrap();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        assert!(socket.read(&mut bytes).await.unwrap() > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nx")
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(15), socket.read(&mut bytes))
            .await
            .unwrap()
            .unwrap()
    });
    let request = HttpRequest::parse(&json!({"method":"GET","path":"/"})).unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        send(client, &connection, None, request),
    )
    .await
    .unwrap();
    assert!(result.is_err(), "incomplete body cannot become output");
    assert_eq!(task.await.unwrap(), 0);
}
