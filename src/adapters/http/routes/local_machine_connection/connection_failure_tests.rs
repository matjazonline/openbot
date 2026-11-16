use super::*;
use crate::adapters::persistence::local_agents::protocol_test_support::turn;

#[cfg(unix)]
#[path = "connection_resource_tests.rs"]
mod resources;

struct HeaderCase {
    protocol: &'static str,
    authorization: Option<String>,
    status: StatusCode,
}

#[tokio::test]
async fn cloudroom_connection_database_outage_is_not_invalid_credentials() {
    let Some(c) = Context::new().await else {
        return;
    };
    let unavailable = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    // A live owned TCP port without a PostgreSQL server makes acquisition really fail/timeout.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_millis(100))
        .connect_lazy(&format!(
            "postgres://invalid@{}/invalid",
            unavailable.local_addr().unwrap()
        ))
        .unwrap();
    let store = Arc::new(crate::adapters::persistence::PostgresPersistence::new(pool));
    let server = Server::start(store).await;
    let request = connect_request(
        &server.origin,
        &c.paired.candidate.credential,
        c.paired.receipt.machine.installation_id,
    );
    let error = tokio_tungstenite::connect_async(request).await.unwrap_err();
    let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
        panic!("expected HTTP storage failure")
    };
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    server.close().await;
}

#[tokio::test]
async fn cloudroom_connection_header_authentication_and_version_refuse_before_lease() {
    let Some(c) = Context::new().await else {
        return;
    };
    let server = Server::start(c.f.persistence.clone()).await;
    let cases = [
        HeaderCase {
            protocol: "2",
            authorization: None,
            status: StatusCode::UNAUTHORIZED,
        },
        HeaderCase {
            protocol: "2",
            authorization: Some(format!("Bearer {}", "0".repeat(64))),
            status: StatusCode::UNAUTHORIZED,
        },
        HeaderCase {
            protocol: "1",
            authorization: Some(format!(
                "Bearer {}",
                c.paired.candidate.credential.expose_secret()
            )),
            status: StatusCode::BAD_REQUEST,
        },
    ];
    for case in cases {
        let mut request = server.origin.clone().into_client_request().unwrap();
        request.headers_mut().insert(
            "x-openbot-installation-id",
            c.paired
                .receipt
                .machine
                .installation_id
                .to_string()
                .parse()
                .unwrap(),
        );
        request
            .headers_mut()
            .insert("x-openbot-protocol-version", case.protocol.parse().unwrap());
        if let Some(value) = case.authorization {
            request
                .headers_mut()
                .insert("authorization", value.parse().unwrap());
        }
        let error = tokio_tungstenite::connect_async(request).await.unwrap_err();
        let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
            panic!("expected HTTP refusal")
        };
        assert_eq!(response.status(), case.status);
    }
    let generation: i64 = sqlx::query_scalar(
        "SELECT connection_generation FROM local_machines WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(c.paired.receipt.machine.id.as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(generation, c.fence.generation.get() as i64);
    server.close().await;
}

#[tokio::test]
async fn cloudroom_connection_handshake_timeout_and_shutdown_join_pending_hello() {
    let Some(c) = Context::new().await else {
        return;
    };
    let server = Server::start(c.f.persistence.clone()).await;
    let mut pending = socket(&server, &c).await;
    let closed = timeout(Duration::from_secs(HANDSHAKE_SECONDS + 2), pending.next())
        .await
        .unwrap();
    assert!(matches!(
        closed,
        None | Some(Ok(Message::Close(_))) | Some(Err(_))
    ));
    let _another = socket(&server, &c).await;
    timeout(Duration::from_secs(2), server.close())
        .await
        .unwrap();
}

#[tokio::test]
async fn cloudroom_connection_oversized_binary_and_unknown_frame_close_without_ack() {
    let Some(c) = Context::new().await else {
        return;
    };
    let server = Server::start(c.f.persistence.clone()).await;
    for message in [
        Message::Text("x".repeat(MAX_FRAME_BYTES + 1).into()),
        Message::Binary(vec![0; 16].into()),
    ] {
        let mut peer = Peer::initialize(socket(&server, &c).await, &c, 2).await;
        peer.socket.send(message).await.unwrap();
        assert!(read(&mut peer.socket).await.is_none());
    }
    let mut peer = Peer::initialize(socket(&server, &c).await, &c, 2).await;
    let mut frame: serde_json::Value = serde_json::from_str(
        &framing::encode(&ClientMessage::Heartbeat {
            identity: peer.identity,
        })
        .unwrap()[0],
    )
    .unwrap();
    frame["version"] = serde_json::json!(2);
    peer.socket
        .send(Message::Text(frame.to_string().into()))
        .await
        .unwrap();
    assert!(read(&mut peer.socket).await.is_none());
    let cursor: i64 = sqlx::query_scalar(
        "SELECT acknowledged_sequence FROM local_machines WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(c.paired.receipt.machine.id.as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(cursor, 2);
    server.close().await;
}

#[tokio::test]
async fn cloudroom_connection_partial_terminal_output_replays_after_shutdown_without_early_ack() {
    let Some(c) = Context::new().await else {
        return;
    };
    let server = Server::start(c.f.persistence.clone()).await;
    let mut peer = Peer::initialize(socket(&server, &c).await, &c, 2).await;
    assert!(peer.ready().await);
    let operation =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    peer.command(operation.id()).await;
    peer.observe(agent(
        peer.sequence + 1,
        operation.id(),
        LocalEvidence::Receipt {},
    ))
    .await;
    peer.observe(agent(
        peer.sequence + 1,
        operation.id(),
        LocalEvidence::SubmissionIntent {},
    ))
    .await;
    let before = peer.sequence;
    let output = "x".repeat(64 * 1024);
    let observation = turn(
        before + 1,
        operation.id(),
        "turn_partial",
        TurnState::Completed,
        Some(&output),
    );
    let frames = framing::encode(&ClientMessage::Observation {
        identity: peer.identity,
        observation: observation.clone(),
    })
    .unwrap();
    assert!(frames.len() > 1);
    peer.socket
        .send(Message::Text(frames[0].clone().into()))
        .await
        .unwrap();
    partial_read_barrier(&mut peer.socket).await;
    let saved:(i64,Option<String>)=sqlx::query_as("SELECT machine.acknowledged_sequence,operation.outcome FROM local_machines AS machine JOIN local_agent_operations AS operation ON operation.company_id=machine.company_id AND operation.machine_id=machine.id WHERE machine.company_id=$1 AND operation.id=$2")
        .bind(c.f.company).bind(operation.id().as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(saved, (before as i64, None));
    timeout(Duration::from_secs(2), server.close())
        .await
        .unwrap();
    drop(peer);
    let restarted = Server::start(c.f.persistence.clone()).await;
    let mut recovered = Peer::initialize(socket(&restarted, &c).await, &c, before).await;
    recovered.observe(observation).await;
    assert!(recovered.ready().await);
    let saved:(String,usize)=sqlx::query_as::<_,(String,i32)>("SELECT outcome,octet_length(output) FROM local_agent_operations WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(operation.id().as_uuid()).fetch_one(&c.f.database.pool).await.map(|(outcome,len)|(outcome,len as usize)).unwrap();
    assert_eq!(saved, ("succeeded".to_owned(), output.len()));
    restarted.close().await;
}

async fn partial_read_barrier<S: AsyncRead + AsyncWrite + Unpin>(socket: &mut WebSocketStream<S>) {
    let marker = b"partial-assembly-read".to_vec();
    socket
        .send(Message::Ping(marker.clone().into()))
        .await
        .unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Pong(bytes) if bytes.as_ref() == marker.as_slice() => break,
                Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
}
