use std::{
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use axum::serve::Listener;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{Instant, Sleep},
};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

use super::config::MAX_CONNECTIONS;

struct ConnectionLimits {
    maximum: usize,
    idle: Duration,
    lifetime: Duration,
}

pub struct BoundedListener {
    listener: TcpListener,
    slots: Arc<Semaphore>,
    force_close: CancellationToken,
    limits: ConnectionLimits,
}

impl BoundedListener {
    pub fn new(listener: TcpListener, force_close: CancellationToken) -> Self {
        Self::with_limits(
            listener,
            force_close,
            ConnectionLimits {
                maximum: MAX_CONNECTIONS,
                idle: Duration::from_secs(15),
                lifetime: Duration::from_secs(120),
            },
        )
    }

    fn with_limits(
        listener: TcpListener,
        force_close: CancellationToken,
        limits: ConnectionLimits,
    ) -> Self {
        Self {
            listener,
            slots: Arc::new(Semaphore::new(limits.maximum)),
            force_close,
            limits,
        }
    }
}

impl Listener for BoundedListener {
    type Io = BoundedIo;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .expect("slots remain open");
        loop {
            match self.listener.accept().await {
                Ok((stream, address)) => {
                    let _ = stream.set_nodelay(true);
                    return (
                        BoundedIo {
                            stream,
                            _permit: permit,
                            idle: Box::pin(tokio::time::sleep(self.limits.idle)),
                            lifetime: Box::pin(tokio::time::sleep(self.limits.lifetime)),
                            idle_duration: self.limits.idle,
                            stopped: Box::pin(self.force_close.clone().cancelled_owned()),
                        },
                        address,
                    );
                }
                Err(error) => {
                    tracing::warn!(kind = ?error.kind(), "listener accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

/// The native HTTP server owns each IO; dropping it releases its admission slot.
pub struct BoundedIo {
    stream: TcpStream,
    _permit: OwnedSemaphorePermit,
    idle: Pin<Box<Sleep>>,
    lifetime: Pin<Box<Sleep>>,
    idle_duration: Duration,
    stopped: Pin<Box<WaitForCancellationFutureOwned>>,
}

impl BoundedIo {
    fn check_deadline(&mut self, context: &mut Context<'_>) -> io::Result<()> {
        if self.stopped.as_mut().poll(context).is_ready() {
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        if self.idle.as_mut().poll(context).is_ready()
            || self.lifetime.as_mut().poll(context).is_ready()
        {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(())
    }

    fn made_progress(&mut self) {
        self.idle
            .as_mut()
            .reset(Instant::now() + self.idle_duration);
    }
}

impl AsyncRead for BoundedIo {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.check_deadline(context)?;
        let before = buffer.filled().len();
        let result = Pin::new(&mut this.stream).poll_read(context, buffer);
        if matches!(result, Poll::Ready(Ok(()))) && buffer.filled().len() > before {
            this.made_progress();
        }
        result
    }
}

impl AsyncWrite for BoundedIo {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        this.check_deadline(context)?;
        let result = Pin::new(&mut this.stream).poll_write(context, buffer);
        if matches!(&result, Poll::Ready(Ok(count)) if *count > 0) {
            this.made_progress();
        }
        result
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.check_deadline(context)?;
        Pin::new(&mut this.stream).poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(context)
    }
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;
