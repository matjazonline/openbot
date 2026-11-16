use super::*;
use crate::adapters::persistence::local_agents::test_support::Candidate;
use std::{
    net::{IpAddr, Ipv4Addr},
    os::fd::AsRawFd,
};

#[derive(sqlx::FromRow)]
struct SlowState {
    disconnected: bool,
    possible: i64,
    cancelled: i64,
}

#[tokio::test]
async fn cloudroom_connection_concurrent_replicas_with_independent_pools_accept_one_hello() {
    let Some(c) = Context::new().await else {
        return;
    };
    let options = c.f.database.pool.connect_options().as_ref().clone();
    let first_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options.clone())
        .await
        .unwrap();
    let second_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .unwrap();
    let first = Server::start(Arc::new(
        crate::adapters::persistence::PostgresPersistence::new(first_pool.clone()),
    ))
    .await;
    let second = Server::start(Arc::new(
        crate::adapters::persistence::PostgresPersistence::new(second_pool.clone()),
    ))
    .await;
    let barrier = tokio::sync::Barrier::new(2);
    let open_first = async {
        barrier.wait().await;
        socket(&first, &c).await
    };
    let open_second = async {
        barrier.wait().await;
        socket(&second, &c).await
    };
    let (mut a, mut b) = tokio::join!(open_first, open_second);
    let hello = ClientMessage::Hello {
        hello: JournalHello {
            version: 2,
            installation_id: c.paired.receipt.machine.installation_id,
            journal_id: c.journal,
            journal_sequence: 2,
        },
    };
    let first_hello = async {
        write(&mut a, &hello).await;
        read(&mut a).await
    };
    let second_hello = async {
        write(&mut b, &hello).await;
        read(&mut b).await
    };
    let (a, b) = tokio::join!(first_hello, second_hello);
    assert_ne!(a.is_some(), b.is_some());
    let Some(ServerMessage::Welcome { welcome }) = a.or(b) else {
        panic!("one current lease must accept the hello")
    };
    let generation:i64=sqlx::query_scalar("SELECT connection_generation FROM local_machines WHERE company_id=$1 AND id=$2 AND connection_owner IS NOT NULL")
        .bind(c.f.company).bind(c.paired.receipt.machine.id.as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(
        generation,
        welcome.identity.connection_generation.get() as i64
    );
    first.close().await;
    second.close().await;
    first_pool.close().await;
    second_pool.close().await;
}

#[tokio::test]
async fn cloudroom_connection_global_capacity_bounds_pending_upgrades_and_releases_all() {
    let Some(c) = Context::new().await else {
        return;
    };
    let server = Server::start(c.f.persistence.clone()).await;
    let address = server
        .origin
        .strip_prefix("ws://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let mut sockets = Vec::new();
    for index in 0..=MAX_CONNECTIONS {
        let code =
            c.f.cases
                .create_pairing(c.f.owner, c.f.company, None)
                .await
                .unwrap();
        let candidate = Candidate::new(code.code);
        c.f.cases
            .pair(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, index as u8 + 2)),
                candidate.claim.clone(),
            )
            .await
            .unwrap();
        let request = connect_request(
            &server.origin,
            &candidate.credential,
            candidate.claim.installation_id,
        );
        let connected =
            tokio_tungstenite::client_async(request, TcpStream::connect(address).await.unwrap())
                .await;
        if index < MAX_CONNECTIONS {
            sockets.push(connected.unwrap().0);
        } else {
            let tokio_tungstenite::tungstenite::Error::Http(response) = connected.unwrap_err()
            else {
                panic!("expected bounded admission")
            };
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        }
    }
    assert_eq!(server.service.slots.available_permits(), 0);
    timeout(Duration::from_secs(5), server.close())
        .await
        .unwrap();
    drop(sockets);
}

#[tokio::test]
async fn cloudroom_connection_shutdown_cancels_slow_consumer_and_keeps_possible_delivery_uncertain()
{
    let Some(c) = Context::new().await else {
        return;
    };
    let server = Server::start(c.f.persistence.clone()).await;
    let address = server
        .origin
        .strip_prefix("ws://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let stream = TcpStream::connect(address).await.unwrap();
    let requested: libc::c_int = 1024;
    // A real bounded kernel receive buffer makes the unread escaped prompts apply backpressure.
    let result = unsafe {
        libc::setsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &requested as *const _ as *const libc::c_void,
            std::mem::size_of_val(&requested) as libc::socklen_t,
        )
    };
    assert_eq!(result, 0);
    let request = connect_request(
        &server.origin,
        &c.paired.candidate.credential,
        c.paired.receipt.machine.installation_id,
    );
    let socket = tokio_tungstenite::client_async(request, stream)
        .await
        .unwrap()
        .0;
    let mut peer = Peer::initialize(socket, &c, 2).await;
    assert!(peer.ready().await);
    let large = LocalCommand::Send {
        session_id: c.session.id,
        thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
        prompt: LocalPrompt::try_from("\u{1}".repeat(MAX_PROMPT_BYTES)).unwrap(),
    };
    let first =
        c.f.persistence
            .enqueue_operation(&c.admission(large.clone()))
            .await
            .unwrap();
    peer.command(first.id()).await;
    for _ in 0..7 {
        c.f.persistence
            .enqueue_operation(&c.admission(large.clone()))
            .await
            .unwrap();
    }
    // Stop polling the receiver while the server redelivers the unacknowledged large commands.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(server.service.tasks.len() > 1);
    timeout(Duration::from_secs(3), server.close())
        .await
        .unwrap();
    let state:SlowState=sqlx::query_as("SELECT machine.connection_owner IS NULL AS disconnected,(SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND delivery_attempted_at IS NOT NULL AND outcome IS NULL) AS possible,(SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND outcome='cancelled') AS cancelled FROM local_machines AS machine WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(c.paired.receipt.machine.id.as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert!(state.disconnected);
    assert!(state.possible > 0);
    assert_eq!(state.cancelled, 0);
}
