//! Test-only PostgreSQL wire relay. Observe a real COMMIT CommandComplete from
//! PostgreSQL before withholding it; never simulate success or log protocol data.
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use std::time::Duration;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Notify,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

pub struct Proxy {
    pub pool: sqlx::PgPool,
    pub mode: Arc<AtomicU8>,
    pub committed: Arc<Notify>,
    pub release: Arc<Notify>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}
async fn frame<R: AsyncRead + Unpin>(stream: &mut R) -> std::io::Result<(u8, Vec<u8>)> {
    let tag = stream.read_u8().await?;
    let size = stream.read_u32().await?;
    if !(4..=1024 * 1024).contains(&size) {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0; (size - 4) as usize];
    stream.read_exact(&mut body).await?;
    Ok((tag, body))
}
impl Proxy {
    pub async fn new(options: &PgConnectOptions) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let upstream = (options.get_host().to_owned(), options.get_port());
        let stop = CancellationToken::new();
        let stop_task = stop.clone();
        let mode = Arc::new(AtomicU8::new(0));
        let observed = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (m, o, r) = (mode.clone(), observed.clone(), release.clone());
        let task = tokio::spawn(async move {
            let mut relays = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _=stop_task.cancelled()=>break,
                    accepted=listener.accept()=>{
                        let (mut client,_)=accepted.unwrap();
                        let upstream=upstream.clone(); let m=m.clone(); let o=o.clone(); let r=r.clone();
                        relays.spawn(async move {
                            let mut server=TcpStream::connect(upstream).await?;
                            let size=client.read_u32().await?;
                            if !(8..=16384).contains(&size) {return Err(std::io::ErrorKind::InvalidData.into());}
                            let mut startup=vec![0;(size-4) as usize]; client.read_exact(&mut startup).await?;
                            server.write_u32(size).await?; server.write_all(&startup).await?;
                            let (mut cr,mut cw)=client.into_split(); let (mut sr,mut sw)=server.into_split();
                            let to_server=async {tokio::io::copy(&mut cr,&mut sw).await.map(|_|())};
                            let to_client=async {
                                loop {
                                    let (tag,body)=frame(&mut sr).await?;
                                    if tag==b'C' && body==b"COMMIT\0" {
                                        match m.swap(0,Ordering::AcqRel) {
                                            1=>{o.notify_one(); return Ok(());},
                                            2=>{o.notify_one(); r.notified().await;},
                                            _=>{},
                                        }
                                    }
                                    cw.write_u8(tag).await?; cw.write_u32((body.len()+4) as u32).await?; cw.write_all(&body).await?;
                                }
                            };
                            tokio::select! {result=to_server=>result,result=to_client=>result}
                        });
                    },
                    Some(result)=relays.join_next(),if !relays.is_empty()=>{let _=result.unwrap();},
                }
            }
            relays.abort_all();
            while relays.join_next().await.is_some() {}
        });
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(2))
            .connect_with(
                options
                    .clone()
                    .host("127.0.0.1")
                    .port(address.port())
                    .ssl_mode(PgSslMode::Disable),
            )
            .await
            .unwrap();
        Self {
            pool,
            mode,
            committed: observed,
            release,
            stop,
            task,
        }
    }
    pub async fn observed(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.committed.notified())
            .await
            .expect("actual PostgreSQL COMMIT completion observed");
    }
    pub async fn close(self) {
        self.pool.close().await;
        self.stop.cancel();
        self.task.await.unwrap();
    }
}
