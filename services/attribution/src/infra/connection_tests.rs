use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn listener(maximum: usize, idle: Duration, lifetime: Duration) -> BoundedListener {
    BoundedListener::with_limits(
        TcpListener::bind("127.0.0.1:0").await.unwrap(),
        CancellationToken::new(),
        ConnectionLimits {
            maximum,
            idle,
            lifetime,
        },
    )
}

#[tokio::test]
async fn attribution_startup_competing_connections_wait_for_owned_slot() {
    let mut listener = listener(1, Duration::from_secs(5), Duration::from_secs(10)).await;
    let first = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (first_io, _) = listener.accept().await;
    let second = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
    assert_eq!(listener.slots.available_permits(), 0);
    drop(first_io);
    let (second_io, address) = tokio::time::timeout(Duration::from_secs(1), listener.accept())
        .await
        .unwrap();
    assert_eq!(address, second.local_addr().unwrap());
    drop(second_io);
    assert_eq!(listener.slots.available_permits(), 1);
    drop((first, second));
}

#[tokio::test]
async fn attribution_startup_idle_connection_times_out() {
    let mut listener = listener(1, Duration::from_millis(50), Duration::from_secs(2)).await;
    let _peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (mut connection, _) = listener.accept().await;
    let error = tokio::time::timeout(Duration::from_secs(1), connection.read_u8())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}

#[tokio::test]
async fn attribution_startup_connection_lifetime_bounds_slow_progress() {
    let mut listener = listener(1, Duration::from_secs(2), Duration::from_millis(100)).await;
    let mut peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    peer.write_all(&[1; 100]).await.unwrap();
    let (mut connection, _) = listener.accept().await;
    assert_eq!(connection.read_u8().await.unwrap(), 1);
    let error = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if let Err(error) = connection.read_u8().await {
                break error;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}

#[tokio::test]
async fn attribution_startup_forced_shutdown_wakes_blocked_io() {
    let mut listener = listener(1, Duration::from_secs(30), Duration::from_secs(60)).await;
    let _peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (mut connection, _) = listener.accept().await;
    let stop = listener.force_close.clone();
    let (read, ()) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(connection.read_u8(), async {
            tokio::task::yield_now().await;
            stop.cancel();
        })
    })
    .await
    .unwrap();
    assert_eq!(read.unwrap_err().kind(), io::ErrorKind::ConnectionAborted);
}
