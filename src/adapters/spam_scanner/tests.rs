use super::*;
#[test]
fn test_heuristic_scanner_spam_keywords() {
    let result = HeuristicScanner::scan(
        Some("URGENT: CLAIM YOUR REWARD NOW!!!"),
        Some("spammer@evil.com"),
        Some("Send bitcoin to wallet address: 1ABC123 and get 100% free cash! Check bit.ly/123"),
        None,
    );

    assert!(result.score >= 5.0);
    assert!(result.reasons.iter().any(|r| r.contains("URGENT")));
    assert!(
        result
            .reasons
            .iter()
            .any(|r| r.contains("CLAIM YOUR REWARD"))
    );
    assert!(result.reasons.iter().any(|r| r.contains("bit.ly")));
}

fn input() -> SpamScanInput<'static> {
    SpamScanInput {
        raw_mime: b"body",
        subject: Some("URGENT"),
        from: None,
        body_text: None,
        body_html: None,
    }
}
#[tokio::test]
async fn spam_scanner_composes_local_policy_without_a_network_backend() {
    let scanner = SpamScannerAdapter::new(SpamScannerSettings {
        heuristic: true,
        external: None,
        threshold: 1.0,
    })
    .unwrap();
    let result = scanner.scan(input()).await;
    assert_eq!(result.score, 1.5);
    assert!(result.is_spam);
}

async fn response_server(bytes: Vec<u8>, http: bool) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 1024];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        if http {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            );
            socket.write_all(header.as_bytes()).await.unwrap();
        }
        // A client may close as soon as it detects the response bound.
        let _ = socket.write_all(&bytes).await;
    });
    (endpoint, handle)
}
#[tokio::test]
async fn spam_scanner_rspamd_response_accepts_the_bound_and_refuses_one_over() {
    for size in [MAX_RESPONSE_BYTES, MAX_RESPONSE_BYTES + 1] {
        let mut bytes = b"{\"score\":7.5}".to_vec();
        bytes.resize(size, b' ');
        let (endpoint, server) = response_server(bytes, true).await;
        let scanner = SpamScannerAdapter::new(SpamScannerSettings {
            heuristic: false,
            external: Some(ExternalScannerSettings::Rspamd {
                endpoint: format!("http://{endpoint}/checkv2"),
            }),
            threshold: 5.0,
        })
        .unwrap();
        let result = scanner.external(b"body").await;
        server.await.unwrap();
        if size == MAX_RESPONSE_BYTES {
            assert_eq!(result.unwrap().score, 7.5);
        } else {
            assert!(result.unwrap_err().to_string().contains("exceeds"));
        }
    }
}
#[tokio::test]
async fn spam_scanner_spamd_headers_accept_the_bound_and_refuse_one_over() {
    for size in [MAX_RESPONSE_BYTES, MAX_RESPONSE_BYTES + 1] {
        let mut bytes = b"SPAMD/1.1 0 EX_OK\r\nSpam: True ; 8.5 / 5.0\r\nX: ".to_vec();
        bytes.resize(size - 4, b'x');
        bytes.extend_from_slice(b"\r\n\r\n");
        let (endpoint, server) = response_server(bytes, false).await;
        let scanner = SpamScannerAdapter::new(SpamScannerSettings {
            heuristic: false,
            external: Some(ExternalScannerSettings::Spamd { endpoint }),
            threshold: 5.0,
        })
        .unwrap();
        let result = scanner.external(b"body").await;
        server.await.unwrap();
        if size == MAX_RESPONSE_BYTES {
            assert_eq!(result.unwrap().score, 8.5);
        } else {
            assert!(result.unwrap_err().to_string().contains("exceed"));
        }
    }
}
#[tokio::test]
async fn spam_scanner_tcp_deadline_closes_the_actual_connection_and_keeps_local_score() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let mut server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        socket.read_to_end(&mut bytes).await.unwrap();
        assert!(!bytes.is_empty());
    });
    let scanner = SpamScannerAdapter::new(SpamScannerSettings {
        heuristic: true,
        external: Some(ExternalScannerSettings::Spamd { endpoint }),
        threshold: 1.0,
    })
    .unwrap();
    let result = scanner.scan(input()).await;
    let joined = tokio::time::timeout(Duration::from_secs(2), &mut server).await;
    if joined.is_err() {
        server.abort();
        let _ = server.await;
    }
    assert!(
        joined.is_ok(),
        "timed-out scan left its TCP connection active"
    );
    joined.unwrap().unwrap();
    assert_eq!(result.score, 1.5);
    assert!(result.is_spam);
}
#[test]
fn spam_scanner_rejects_invalid_enabled_endpoints_at_construction() {
    assert!(
        SpamScannerAdapter::new(SpamScannerSettings {
            heuristic: false,
            external: Some(ExternalScannerSettings::Rspamd {
                endpoint: "file:///tmp/spam".into()
            }),
            threshold: 5.0
        })
        .is_err()
    );
    assert!(
        SpamScannerAdapter::new(SpamScannerSettings {
            heuristic: false,
            external: Some(ExternalScannerSettings::Spamd {
                endpoint: String::new()
            }),
            threshold: 5.0
        })
        .is_err()
    );
}
