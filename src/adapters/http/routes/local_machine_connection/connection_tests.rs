use super::*;
use crate::application::local_agents::ports::LocalAgentRead;
use crate::{
    adapters::persistence::local_agents::protocol_test_support::{Context, agent},
    application::local_agents::{LocalCatalog, MachineChange},
    domain::local_agents::*,
};
use futures::{SinkExt, StreamExt};
use std::collections::VecDeque;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    task::JoinHandle,
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};

#[path = "connection_submission_tests.rs"]
mod submission;

#[path = "connection_recovery_tests.rs"]
mod recovery;

#[path = "connection_failure_tests.rs"]
mod failures;
#[cfg(unix)]
#[path = "tls_tests.rs"]
pub(crate) mod tls;

pub(crate) struct CommandWriteGate {
    pub operation: OperationId,
    pub reached: tokio::sync::Notify,
}

struct Server {
    service: Arc<LocalMachineConnections>,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
    origin: String,
}
impl Server {
    fn start(store: Arc<dyn LocalProtocolStore>) -> impl std::future::Future<Output = Self> {
        Self::start_at(store, Router::new(), "127.0.0.1:0".parse().unwrap())
    }
    fn start_at(
        store: Arc<dyn LocalProtocolStore>,
        extra: Router,
        address: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = Self> {
        Self::start_with_gate(store, extra, address, None)
    }
    async fn start_with_gate(
        store: Arc<dyn LocalProtocolStore>,
        extra: Router,
        address: std::net::SocketAddr,
        gate: Option<Arc<CommandWriteGate>>,
    ) -> Self {
        let mut service = LocalMachineConnections::new(store);
        service.command_write_gate = gate;
        let service = Arc::new(service);
        let listener = tokio::net::TcpListener::bind(address).await.unwrap();
        let origin = format!(
            "ws://{}/v1/local-machines/connect",
            listener.local_addr().unwrap()
        );
        let app = router().with_state(service.clone()).merge(extra);
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
                .unwrap();
        });
        Self {
            service,
            stop,
            task: Some(task),
            origin,
        }
    }
    async fn close(mut self) {
        self.service.shutdown().await.unwrap();
        assert!(self.service.tasks.is_empty());
        assert_eq!(self.service.slots.available_permits(), MAX_CONNECTIONS);
        self.stop.cancel();
        timeout(Duration::from_secs(2), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
        self.service.shutdown.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn socket(server: &Server, c: &Context) -> WebSocketStream<TcpStream> {
    let request = connect_request(
        &server.origin,
        &c.paired.candidate.credential,
        c.paired.receipt.machine.installation_id,
    );
    let address = server
        .origin
        .strip_prefix("ws://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let stream = TcpStream::connect(address).await.unwrap();
    tokio_tungstenite::client_async(request, stream)
        .await
        .unwrap()
        .0
}

fn connect_request(
    origin: &str,
    credential: &MachineCredential,
    installation: InstallationId,
) -> tokio_tungstenite::tungstenite::http::Request<()> {
    let mut request = origin.into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", credential.expose_secret())
            .parse()
            .unwrap(),
    );
    request.headers_mut().insert(
        "x-openbot-installation-id",
        installation.to_string().parse().unwrap(),
    );
    request
        .headers_mut()
        .insert("x-openbot-protocol-version", "2".parse().unwrap());
    request
}

struct Peer<S> {
    socket: WebSocketStream<S>,
    identity: ConnectionIdentity,
    sequence: u64,
    catalog: LocalCatalog,
    commands: VecDeque<CommandDelivery>,
}
impl<S: AsyncRead + AsyncWrite + Unpin> Peer<S> {
    async fn initialize(mut socket: WebSocketStream<S>, c: &Context, sequence: u64) -> Self {
        let hello = ClientMessage::Hello {
            hello: JournalHello {
                version: 2,
                installation_id: c.paired.receipt.machine.installation_id,
                journal_id: c.journal,
                journal_sequence: sequence,
            },
        };
        write(&mut socket, &hello).await;
        let ServerMessage::Welcome { welcome } = read(&mut socket).await.unwrap() else {
            panic!("expected welcome")
        };
        assert!(!welcome.recovery_required);
        assert_eq!(welcome.acknowledged_sequence, sequence);
        let machine =
            c.f.persistence
                .machine(c.f.company, c.paired.receipt.machine.id)
                .await
                .unwrap()
                .unwrap();
        let catalog =
            c.f.persistence
                .capabilities(
                    c.f.company,
                    machine.id,
                    machine.capabilities_sequence.unwrap(),
                )
                .await
                .unwrap()
                .unwrap()
                .catalog;
        Self {
            socket,
            identity: welcome.identity,
            sequence,
            catalog,
            commands: VecDeque::new(),
        }
    }
    async fn ready(&mut self) -> bool {
        write(
            &mut self.socket,
            &ClientMessage::Ready {
                identity: self.identity,
            },
        )
        .await;
        loop {
            match read(&mut self.socket).await.unwrap() {
                ServerMessage::Ready { ready, .. } => return ready,
                ServerMessage::Command { delivery, .. } => self.commands.push_back(*delivery),
                other => panic!("unexpected message {other:?}"),
            }
        }
    }
    async fn observe(&mut self, observation: JournalObservation) {
        let sequence = observation.sequence().get();
        write(
            &mut self.socket,
            &ClientMessage::Observation {
                identity: self.identity,
                observation,
            },
        )
        .await;
        loop {
            match read(&mut self.socket).await.unwrap() {
                ServerMessage::Acknowledged { through, .. } => {
                    assert!(through >= sequence);
                    self.sequence = self.sequence.max(through);
                    return;
                }
                ServerMessage::Command { delivery, .. } => self.commands.push_back(*delivery),
                other => panic!("unexpected message {other:?}"),
            }
        }
    }
    async fn command(&mut self, wanted: OperationId) -> CommandDelivery {
        loop {
            let delivery = match self.commands.pop_front() {
                Some(delivery) => delivery,
                None => match read(&mut self.socket).await.unwrap() {
                    ServerMessage::Command { delivery, .. } => *delivery,
                    other => panic!("unexpected {other:?}"),
                },
            };
            if delivery.operation.id() == wanted {
                return delivery;
            }
            assert!(matches!(
                delivery.operation.command(),
                LocalCommand::RefreshCatalog {}
            ));
            let id = delivery.operation.id();
            self.observe(agent(self.sequence + 1, id, LocalEvidence::Receipt {}))
                .await;
            self.observe(JournalObservation::Catalog {
                sequence: EventSequence::try_from(self.sequence + 1).unwrap(),
                operation_id: id,
                catalog: self.catalog.clone(),
            })
            .await;
        }
    }
}

async fn write<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    message: &ClientMessage,
) {
    for frame in framing::encode(message).unwrap() {
        socket.send(Message::Text(frame.into())).await.unwrap();
    }
}
async fn read<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
) -> Option<ServerMessage> {
    let mut assembler = framing::Assembler::default();
    loop {
        match timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("bounded WebSocket response")?
        {
            Ok(Message::Text(text)) => {
                if let Some(bytes) = assembler.push(&text).unwrap() {
                    return Some(serde_json::from_slice(&bytes).unwrap());
                }
            }
            Ok(Message::Ping(bytes)) => socket.send(Message::Pong(bytes)).await.unwrap(),
            Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) | Err(_) => return None,
            other => panic!("unexpected frame {other:?}"),
        }
    }
}

#[tokio::test]
async fn cloudroom_connection_two_replicas_supersede_live_socket_and_join_shutdown() {
    let Some(c) = Context::new().await else {
        return;
    };
    let first = Server::start(c.f.persistence.clone()).await;
    let second = Server::start(c.f.persistence.clone()).await;
    let mut old = Peer::initialize(socket(&first, &c).await, &c, 2).await;
    let mut current = Peer::initialize(socket(&second, &c).await, &c, 2).await;
    assert!(current.identity.connection_generation > old.identity.connection_generation);
    timeout(Duration::from_secs(3), async {
        while read(&mut old.socket).await.is_some() {}
    })
    .await
    .unwrap();
    first.close().await;
    let saved:(i64,bool)=sqlx::query_as("SELECT connection_generation,connection_owner IS NOT NULL FROM local_machines WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(c.paired.receipt.machine.id.as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(
        saved,
        (current.identity.connection_generation.get() as i64, true)
    );
    assert!(current.ready().await);
    second.close().await;
    let connected: bool = sqlx::query_scalar(
        "SELECT connection_owner IS NOT NULL FROM local_machines WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(c.paired.receipt.machine.id.as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert!(!connected);
}

#[tokio::test]
async fn cloudroom_connection_lost_ack_reconnect_rewraps_receipt_and_rejects_old_envelope() {
    let Some(c) = Context::new().await else {
        return;
    };
    let server = Server::start(c.f.persistence.clone()).await;
    let mut first = Peer::initialize(socket(&server, &c).await, &c, 2).await;
    assert!(first.ready().await);
    let operation =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    let delivered = first.command(operation.id()).await;
    assert_eq!(delivered.mode, DeliveryMode::Dispatch);
    assert_eq!(
        delivered.operation.payload_digest(),
        operation.payload_digest()
    );
    let receipt = agent(
        first.sequence + 1,
        operation.id(),
        LocalEvidence::Receipt {},
    );
    let through = receipt.sequence().get();
    let old_identity = first.identity;
    write(
        &mut first.socket,
        &ClientMessage::Observation {
            identity: first.identity,
            observation: receipt.clone(),
        },
    )
    .await;
    timeout(Duration::from_secs(3), async {
        loop {
            let cursor: i64 = sqlx::query_scalar(
                "SELECT acknowledged_sequence FROM local_machines WHERE company_id=$1 AND id=$2",
            )
            .bind(c.f.company)
            .bind(c.paired.receipt.machine.id.as_uuid())
            .fetch_one(&c.f.database.pool)
            .await
            .unwrap();
            if cursor == through as i64 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(first); // Lose the already committed ACK before consuming it.
    let mut second = Peer::initialize(socket(&server, &c).await, &c, through).await;
    second.observe(receipt.clone()).await;
    assert!(second.ready().await);
    write(
        &mut second.socket,
        &ClientMessage::Observation {
            identity: old_identity,
            observation: receipt,
        },
    )
    .await;
    timeout(Duration::from_secs(3), async {
        while read(&mut second.socket).await.is_some() {}
    })
    .await
    .unwrap();
    let copies: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM local_agent_observations WHERE company_id=$1 AND sequence=$2",
    )
    .bind(c.f.company)
    .bind(through as i64)
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(copies, 1);
    server.close().await;
}

#[tokio::test]
async fn cloudroom_connection_revocation_during_bounded_hello_accepts_no_observation() {
    let Some(c) = Context::new().await else {
        return;
    };
    let server = Server::start(c.f.persistence.clone()).await;
    let mut socket = socket(&server, &c).await;
    c.f.cases
        .change(
            c.f.owner,
            c.f.company,
            c.paired.receipt.machine.id,
            MachineChange::Revoke,
        )
        .await
        .unwrap();
    write(
        &mut socket,
        &ClientMessage::Hello {
            hello: JournalHello {
                version: 2,
                installation_id: c.paired.receipt.machine.installation_id,
                journal_id: c.journal,
                journal_sequence: 2,
            },
        },
    )
    .await;
    assert!(read(&mut socket).await.is_none());
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
