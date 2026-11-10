use super::*;
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn integration_gateway_native_mcp_sse_body_owns_completion_after_client_release() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        assert!(socket.read(&mut bytes).await.unwrap() > 0);
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
        let _ = released.await;
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();
    let response = client.get(url).send().await.unwrap();
    let control = Control::owned(&tokio_util::sync::CancellationToken::new());
    let (lifetime, mut finished) = crate::adapters::mcp::preparation::lifetime();
    let mut stream = bounded(
        response,
        Arc::new(std::sync::atomic::AtomicUsize::new(1024)),
        control.clone(),
        lifetime.clone(),
    );
    // rmcp's POST-SSE task owns only this returned stream, not an HTTP client clone.
    drop(lifetime);
    drop(client);
    assert!(matches!(
        finished.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    control.stop.cancel();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    drop(stream);
    finished.await.unwrap();
    let _ = release.send(());
    server.await.unwrap();
}
