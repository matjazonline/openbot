//! Own rmcp's running service outside request futures. A dropped HTTP request or tool future closes
//! its command channel; the tracked worker cancels the actual operation and awaits session cleanup.
use super::*;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

enum Command {
    Discover(oneshot::Sender<AppResult<Vec<McpDiscoveredTool>>>),
    Call {
        name: McpToolName,
        args: Value,
        reply: oneshot::Sender<AppResult<Value>>,
    },
    Close(oneshot::Sender<AppResult<()>>),
}
pub(super) struct ProxySession {
    sender: mpsc::Sender<Command>,
}
pub(super) struct Receiver(mpsc::Receiver<Command>);
impl ProxySession {
    pub fn channel() -> (Self, Receiver) {
        let (sender, receiver) = mpsc::channel(1);
        (Self { sender }, Receiver(receiver))
    }
}
#[async_trait]
impl McpSession for ProxySession {
    async fn discover(&self) -> AppResult<Vec<McpDiscoveredTool>> {
        let (reply, result) = oneshot::channel();
        self.sender
            .try_send(Command::Discover(reply))
            .map_err(|_| failed("MCP session busy or closed"))?;
        result
            .await
            .map_err(|_| failed("MCP discovery cancelled"))?
    }
    async fn call(&self, name: &McpToolName, args: Value) -> AppResult<Value> {
        let (reply, result) = oneshot::channel();
        self.sender
            .try_send(Command::Call {
                name: name.clone(),
                args,
                reply,
            })
            .map_err(|_| failed("MCP session busy or closed"))?;
        result
            .await
            .map_err(|_| failed("MCP effect indeterminate; do not retry"))?
    }
    async fn close(self: Box<Self>) -> AppResult<()> {
        let (reply, result) = oneshot::channel();
        if self.sender.send(Command::Close(reply)).await.is_err() {
            return Ok(());
        }
        result.await.map_err(|_| failed("MCP shutdown failed"))?
    }
}
pub(super) async fn run(session: Session, mut receiver: Receiver, shutdown: CancellationToken) {
    let mut close_reply = None;
    loop {
        let command = tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            command = receiver.0.recv() => command,
        };
        match command {
            Some(Command::Discover(mut reply)) => {
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => break,
                    _ = reply.closed() => break,
                    result = session.discover() => { let _ = reply.send(result); }
                }
            }
            Some(Command::Call {
                name,
                args,
                mut reply,
            }) => {
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => break,
                    _ = reply.closed() => break,
                    result = session.call(&name,args) => { let _ = reply.send(result); }
                }
            }
            Some(Command::Close(reply)) => {
                close_reply = Some(reply);
                break;
            }
            None => break,
        }
    }
    let closed = Box::new(session).close().await;
    if let Some(reply) = close_reply {
        let _ = reply.send(closed);
    } else if closed.is_err() {
        tracing::warn!("MCP abandoned session shutdown failed");
    }
}
