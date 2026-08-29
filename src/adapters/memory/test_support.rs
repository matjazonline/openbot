//! Localhost HTTP doubles shared by the memory adapter tests.
//!
//! Real sockets rather than a mocking crate: the bounds these tests pin — a `Content-Length`
//! rejected before the body is read, a chunked body that crosses the cap mid-transfer, a response
//! that never ends — only exist at the transport, so a stubbed client would not exercise them.

use std::{sync::Arc, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{Barrier, mpsc, oneshot},
};

/// Answer one request with `status` and `body`, handing back the raw request that arrived.
pub async fn mock_server(status: u16, body: &'static str) -> (String, oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 16 * 1024];
        let read = stream.read(&mut request).await.unwrap();
        request.truncate(read);
        let _ = sender.send(String::from_utf8_lossy(&request).into_owned());
        stream
            .write_all(http_response(status, body).as_bytes())
            .await
            .unwrap();
    });
    (format!("http://{address}"), receiver)
}

/// Answer a fixed number of requests in arrival order, collecting each raw request.
///
/// One connection per response: every adapter here sets `Connection: close`, so a client that
/// makes N calls opens N sockets whether or not it made them concurrently.
pub async fn scripted_server(
    responses: Vec<(u16, String)>,
) -> (String, mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        for (status, body) in responses {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0; 64 * 1024];
            let read = stream.read(&mut request).await.unwrap();
            request.truncate(read);
            let _ = sender.send(String::from_utf8_lossy(&request).into_owned());
            let _ = stream
                .write_all(http_response(status, &body).as_bytes())
                .await;
        }
    });
    (format!("http://{address}"), receiver)
}

/// Answer every request with the same status and body, for a fan-out whose arrival order is not
/// deterministic. Requests are reported on the channel as they land.
pub async fn uniform_server(
    connections: usize,
    status: u16,
    body: &'static str,
) -> (String, mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        for _ in 0..connections {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let sender = sender.clone();
            tokio::spawn(async move {
                let mut request = vec![0; 64 * 1024];
                let read = stream.read(&mut request).await.unwrap();
                request.truncate(read);
                let _ = sender.send(String::from_utf8_lossy(&request).into_owned());
                let _ = stream
                    .write_all(http_response(status, body).as_bytes())
                    .await;
            });
        }
    });
    (format!("http://{address}"), receiver)
}

/// Write bytes back verbatim — a malformed frame, an oversized declared length, or a body that
/// never completes.
pub async fn raw_response_server(response: Vec<u8>, hold_open: Option<Duration>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 16 * 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream.write_all(&response).await.unwrap();
        if let Some(duration) = hold_open {
            tokio::time::sleep(duration).await;
        }
    });
    format!("http://{address}")
}

/// Hold every connection open until all `connections` have arrived, so a test can prove a fan-out
/// is concurrent rather than sequential.
pub async fn concurrent_server(
    connections: usize,
    body: &'static str,
) -> (String, mpsc::Receiver<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let barrier = Arc::new(Barrier::new(connections + 1));
    let (sender, receiver) = mpsc::channel(connections);
    tokio::spawn(async move {
        for _ in 0..connections {
            let (mut stream, _) = listener.accept().await.unwrap();
            let barrier = barrier.clone();
            let sender = sender.clone();
            tokio::spawn(async move {
                let mut request = vec![0; 64 * 1024];
                let read = stream.read(&mut request).await.unwrap();
                sender.send(read).await.unwrap();
                barrier.wait().await;
                let _ = stream.write_all(http_response(200, body).as_bytes()).await;
            });
        }
        barrier.wait().await;
    });
    (format!("http://{address}"), receiver)
}

fn http_response(status: u16, body: &str) -> String {
    let reason = if (200..300).contains(&status) {
        "OK"
    } else {
        "Error"
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// The request line of a captured raw request, e.g. `POST /v1/default/banks/x HTTP/1.1`.
pub fn request_line(request: &str) -> &str {
    request.lines().next().unwrap_or_default()
}

/// The JSON body of a captured raw request.
pub fn request_body(request: &str) -> serde_json::Value {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or_default();
    serde_json::from_str(body).unwrap_or_else(|error| panic!("body was not JSON ({error}): {body}"))
}
