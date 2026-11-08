//! Real TLS termination in front of the production Axum route, matching the deployed topology.
use super::*;
use crate::adapters::persistence::local_agents::protocol_test_support::turn;
use std::{os::unix::fs::PermissionsExt, path::PathBuf, process::Command};
use tokio_rustls::{
    TlsAcceptor, TlsConnector,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    },
};

struct CertificateFiles(PathBuf);
impl Drop for CertificateFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn certificate() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let files = CertificateFiles(
        std::env::temp_dir().join(format!("openbot-cloudroom-tls-{}", Uuid::new_v4())),
    );
    std::fs::create_dir(&files.0).unwrap();
    std::fs::set_permissions(&files.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(files.0.join("config"),"[req]\ndistinguished_name=dn\nprompt=no\nx509_extensions=extensions\n[dn]\nCN=localhost\n[extensions]\nsubjectAltName=DNS:localhost\nbasicConstraints=critical,CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n").unwrap();
    for arguments in [
        vec![
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1", "-config", "config",
            "-keyout", "key.pem", "-out", "cert.pem",
        ],
        vec![
            "x509", "-in", "cert.pem", "-outform", "der", "-out", "cert.der",
        ],
        vec![
            "pkcs8", "-topk8", "-nocrypt", "-in", "key.pem", "-outform", "der", "-out", "key.der",
        ],
    ] {
        let result = Command::new("openssl")
            .args(arguments)
            .current_dir(&files.0)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "OpenSSL test certificate command failed"
        );
    }
    (
        CertificateDer::from(std::fs::read(files.0.join("cert.der")).unwrap()),
        PrivatePkcs8KeyDer::from(std::fs::read(files.0.join("key.der")).unwrap()).into(),
    )
}

struct Proxy {
    address: std::net::SocketAddr,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
}
impl Proxy {
    async fn start(
        server: &Server,
        cert: CertificateDer<'static>,
        key: PrivateKeyDer<'static>,
    ) -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let backend = server
            .origin
            .strip_prefix("ws://")
            .unwrap()
            .split('/')
            .next()
            .unwrap()
            .to_owned();
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let task = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {biased;
                    _=shutdown.cancelled()=>break,
                    _=clients.join_next(),if !clients.is_empty()=>{},
                    accepted=listener.accept()=> {
                        let (socket,_)=accepted.unwrap();if clients.len()>=2 {drop(socket);continue;}
                        let acceptor=acceptor.clone();let backend=backend.clone();let stop=shutdown.clone();
                        clients.spawn(async move {
                            let relay=async {
                                let Ok(mut tls)=acceptor.accept(socket).await else {return;};
                                let Ok(mut tcp)=TcpStream::connect(backend).await else {return;};
                                let _=tokio::io::copy_bidirectional(&mut tls,&mut tcp).await;
                            };
                            tokio::select! {biased; _=stop.cancelled()=>{},_=timeout(Duration::from_secs(30),relay)=>{}}
                        });
                    }
                }
            }
            clients.abort_all();
            while clients.join_next().await.is_some() {}
        });
        Self {
            address,
            stop,
            task: Some(task),
        }
    }
    async fn close(mut self) {
        self.stop.cancel();
        timeout(Duration::from_secs(2), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn connector(roots: rustls::RootCertStore) -> TlsConnector {
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

/// Reuse the production route and TLS terminator for plugin lifecycle tests.
pub(crate) struct TestTlsServer {
    server: Option<Server>,
    proxy: Proxy,
    pub origin: String,
    pub certificate: CertificateDer<'static>,
}

impl TestTlsServer {
    pub fn start(store: Arc<dyn LocalProtocolStore>) -> impl std::future::Future<Output = Self> {
        Self::with_routes(store, Router::new())
    }
    pub fn with_routes(
        store: Arc<dyn LocalProtocolStore>,
        extra: Router,
    ) -> impl std::future::Future<Output = Self> {
        Self::with_gate(store, extra, None)
    }
    pub async fn with_gate(
        store: Arc<dyn LocalProtocolStore>,
        extra: Router,
        gate: Option<Arc<CommandWriteGate>>,
    ) -> Self {
        let server =
            Server::start_with_gate(store, extra, "127.0.0.1:0".parse().unwrap(), gate).await;
        let (certificate, key) = certificate();
        let proxy = Proxy::start(&server, certificate.clone(), key).await;
        let origin = format!("https://localhost:{}", proxy.address.port());
        Self {
            server: Some(server),
            proxy,
            origin,
            certificate,
        }
    }

    pub async fn restart(&mut self, store: Arc<dyn LocalProtocolStore>, extra: Router) {
        let old = self.server.take().unwrap();
        let address = old
            .origin
            .strip_prefix("ws://")
            .unwrap()
            .split('/')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        old.close().await;
        self.server = Some(Server::start_at(store, extra, address).await);
    }
    pub async fn close(mut self) {
        self.proxy.close().await;
        self.server.take().unwrap().close().await;
    }
}

#[tokio::test]
async fn cloudroom_connection_real_wss_validates_certificate_and_commits_result() {
    let Some(c) = Context::new().await else {
        return;
    };
    let server = Server::start(c.f.persistence.clone()).await;
    let (cert, key) = certificate();
    let proxy = Proxy::start(&server, cert.clone(), key).await;
    let bad = connector(rustls::RootCertStore::empty())
        .connect(
            ServerName::try_from("localhost").unwrap(),
            TcpStream::connect(proxy.address).await.unwrap(),
        )
        .await;
    assert!(
        bad.is_err(),
        "an untrusted TLS certificate must fail before any credentials are sent"
    );
    let generation: i64 = sqlx::query_scalar(
        "SELECT connection_generation FROM local_machines WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(c.paired.receipt.machine.id.as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(generation, c.fence.generation.get() as i64);
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert).unwrap();
    let tls = connector(roots)
        .connect(
            ServerName::try_from("localhost").unwrap(),
            TcpStream::connect(proxy.address).await.unwrap(),
        )
        .await
        .unwrap();
    let request = connect_request(
        &format!(
            "wss://localhost:{}/v1/local-machines/connect",
            proxy.address.port()
        ),
        &c.paired.candidate.credential,
        c.paired.receipt.machine.installation_id,
    );
    let (socket, _) = tokio_tungstenite::client_async(request, tls).await.unwrap();
    let mut peer = Peer::initialize(socket, &c, 2).await;
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
    peer.observe(turn(
        peer.sequence + 1,
        operation.id(),
        "turn_tls",
        TurnState::Completed,
        Some("TLS result"),
    ))
    .await;
    let result: (String, String) = sqlx::query_as(
        "SELECT outcome,output FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(operation.id().as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(result, ("succeeded".to_owned(), "TLS result".to_owned()));
    drop(peer);
    proxy.close().await;
    server.close().await;
}
