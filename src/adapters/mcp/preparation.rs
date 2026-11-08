//! Per-attempt control; connection-owned and vault-authorized preparation are explicit modes.
use super::*;
use crate::application::services::mcp_client::{
    McpPreparation, McpPreparationFailure as Failure, McpPreparationResult,
};
use chrono::{DateTime, Utc};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
enum Mode {
    ConnectionOwned,
    Prepared(McpPreparation),
}
#[derive(Clone)]
pub(super) struct Control {
    pub stop: CancellationToken,
    deadline: DateTime<Utc>,
    mode: Mode,
}
impl Control {
    pub fn owned(shutdown: &CancellationToken) -> Self {
        Self {
            stop: shutdown.child_token(),
            deadline: Utc::now() + chrono::Duration::seconds(30),
            mode: Mode::ConnectionOwned,
        }
    }
    pub fn prepared(shutdown: &CancellationToken, work: McpPreparation) -> Self {
        Self {
            stop: shutdown.child_token(),
            deadline: work
                .deadline
                .min(Utc::now() + chrono::Duration::seconds(30)),
            mode: Mode::Prepared(work),
        }
    }
    pub async fn run<T>(
        &self,
        work: impl std::future::Future<Output = McpPreparationResult<T>>,
    ) -> McpPreparationResult<T> {
        let remaining = (self.deadline - Utc::now())
            .to_std()
            .map_err(|_| self.record(Failure::Deadline))?;
        let cancellation = match &self.mode {
            Mode::ConnectionOwned => self.stop.clone(),
            Mode::Prepared(work) => work.cancellation.clone(),
        };
        tokio::select! {biased;
            _=self.stop.cancelled()=>Err(self.record(Failure::Cancelled)),
            _=cancellation.cancelled()=>Err(self.record(Failure::Cancelled)),
            _=tokio::time::sleep(remaining)=>Err(self.record(Failure::Deadline)),
            result=work=>result.map_err(|failure|self.record(failure)),
        }
    }
    pub async fn check(&self) -> McpPreparationResult<()> {
        self.run(async {
            match &self.mode {
                Mode::ConnectionOwned => Ok(()),
                Mode::Prepared(work) => work.check().await,
            }
        })
        .await
    }
    pub fn failure(&self) -> Failure {
        match &self.mode {
            Mode::Prepared(work) => work.failure().unwrap_or(Failure::Unavailable),
            Mode::ConnectionOwned => Failure::Unavailable,
        }
    }
    pub fn record(&self, failure: Failure) -> Failure {
        match &self.mode {
            Mode::Prepared(work) => work.remember(failure),
            Mode::ConnectionOwned => failure,
        }
    }
}

/// Held by every SDK HTTP client and every returned byte/SSE stream, never the supervisor.
pub(super) struct HttpLifetime(Option<tokio::sync::oneshot::Sender<()>>);
impl Drop for HttpLifetime {
    fn drop(&mut self) {
        if let Some(done) = self.0.take() {
            let _ = done.send(());
        }
    }
}
pub(super) fn lifetime() -> (Arc<HttpLifetime>, tokio::sync::oneshot::Receiver<()>) {
    let (done, finished) = tokio::sync::oneshot::channel();
    (Arc::new(HttpLifetime(Some(done))), finished)
}
