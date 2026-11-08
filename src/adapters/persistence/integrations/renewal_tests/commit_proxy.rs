//! Fixture-only PostgreSQL wire proxy: discard one real committed transaction's ACK.
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgSslMode},
};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::{Notify, Semaphore},
};
#[derive(Clone, Copy)]
pub(crate) enum Fault {
    CredentialWrite,
    PendingReservation,
}
struct Shared {
    fault: Fault,
    armed: AtomicBool,
    pub dropped: AtomicUsize,
    notify: Notify,
    pub after_commit: Arc<Semaphore>,
}
pub(crate) struct CommitProxy {
    pub options: PgConnectOptions,
    shared: Arc<Shared>,
    worker: tokio::task::JoinHandle<()>,
}
impl Drop for CommitProxy {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
impl CommitProxy {
    pub async fn new(pool: &PgPool, fault: Fault) -> Self {
        let options = pool.connect_options().as_ref().clone();
        assert!(matches!(options.get_host(), "localhost" | "127.0.0.1"));
        let backend = ("127.0.0.1", options.get_port());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let shared = Arc::new(Shared {
            fault,
            armed: AtomicBool::new(true),
            dropped: AtomicUsize::new(0),
            notify: Notify::new(),
            after_commit: Arc::new(Semaphore::new(1)),
        });
        let state = shared.clone();
        let worker = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    result=listener.accept()=>{let Ok((client,_))=result else{break};let state=state.clone();tasks.spawn(async move{let _=connection(client,backend,state).await;});}
                    _=tasks.join_next(),if !tasks.is_empty()=>{}
                }
            }
        });
        Self {
            options: options
                .host("127.0.0.1")
                .port(port)
                .ssl_mode(PgSslMode::Disable),
            shared,
            worker,
        }
    }
    pub fn gate(&self) -> Arc<Semaphore> {
        self.shared.after_commit.clone()
    }
    pub fn dropped(&self) -> usize {
        self.shared.dropped.load(Ordering::SeqCst)
    }
    pub async fn wait_drop(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let wake = self.shared.notify.notified();
                if self.dropped() > 0 {
                    break;
                }
                wake.await;
            }
        })
        .await
        .unwrap();
    }
}
struct Frame {
    kind: u8,
    body: Vec<u8>,
}
async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<Frame>> {
    let kind = match reader.read_u8().await {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    };
    let length = reader.read_u32().await?;
    if !(4..=1048576).contains(&length) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Fixture PostgreSQL frame bound",
        ));
    }
    let mut body = vec![0; (length - 4) as usize];
    reader.read_exact(&mut body).await?;
    Ok(Some(Frame { kind, body }))
}
async fn send_frame<W: AsyncWrite + Unpin>(writer: &mut W, frame: &Frame) -> io::Result<()> {
    writer.write_u8(frame.kind).await?;
    writer.write_u32(frame.body.len() as u32 + 4).await?;
    writer.write_all(&frame.body).await?;
    writer.flush().await
}
fn target(frame: &Frame, fault: Fault) -> bool {
    if !matches!(frame.kind, b'P' | b'Q') {
        return false;
    }
    let needle: &[u8] = match fault {
        Fault::CredentialWrite => b"UPDATE integration_accounts SET renewal_attempt_id=NULL",
        Fault::PendingReservation => b"UPDATE integration_accounts SET renewal_attempt_id=$2",
    };
    frame.body.windows(needle.len()).any(|s| s == needle)
}
async fn connection(
    mut client: TcpStream,
    backend: (&str, u16),
    shared: Arc<Shared>,
) -> io::Result<()> {
    let mut server = TcpStream::connect(backend).await?;
    client.set_nodelay(true)?;
    server.set_nodelay(true)?;
    let startup_len = client.read_u32().await?;
    if !(8..=65536).contains(&startup_len) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Fixture startup bound",
        ));
    }
    let mut startup = vec![0; (startup_len - 4) as usize];
    client.read_exact(&mut startup).await?;
    server.write_u32(startup_len).await?;
    server.write_all(&startup).await?;
    server.flush().await?;
    let (mut from_client, mut to_client) = client.into_split();
    let (mut from_server, mut to_server) = server.into_split();
    let dirty = AtomicBool::new(false);
    let upstream = async {
        while let Some(frame) = read_frame(&mut from_client).await? {
            if target(&frame, shared.fault) {
                dirty.store(true, Ordering::SeqCst);
            }
            send_frame(&mut to_server, &frame).await?;
        }
        Ok::<_, io::Error>(())
    };
    let downstream = async {
        while let Some(frame) = read_frame(&mut from_server).await? {
            if frame.kind == b'C'
                && frame.body == b"COMMIT\0"
                && dirty.load(Ordering::SeqCst)
                && shared.armed.swap(false, Ordering::SeqCst)
            {
                // CommandComplete confirms commit; consume ReadyForQuery before dropping ACK.
                while let Some(done) = read_frame(&mut from_server).await? {
                    if done.kind == b'Z' {
                        assert_eq!(done.body, b"I");
                        break;
                    }
                }
                shared.dropped.fetch_add(1, Ordering::SeqCst);
                shared.notify.notify_one();
                let permit = shared.after_commit.acquire().await.unwrap();
                drop(permit);
                to_client.shutdown().await?;
                return Ok::<_, io::Error>(());
            }
            if frame.kind == b'Z' && frame.body == b"I" {
                dirty.store(false, Ordering::SeqCst);
            }
            send_frame(&mut to_client, &frame).await?;
        }
        Ok::<_, io::Error>(())
    };
    tokio::select! {result=upstream=>result,result=downstream=>result}
}
