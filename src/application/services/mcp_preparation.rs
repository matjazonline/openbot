//! Runtime preparation authority. These capabilities never enter a protocol payload.
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum McpPreparationFailure {
    #[error("MCP preparation denied")]
    Denied,
    #[error("MCP credential changed")]
    Changed,
    #[error("MCP preparation cancelled")]
    Cancelled,
    #[error("MCP preparation deadline elapsed")]
    Deadline,
    #[error("MCP preparation unavailable")]
    Unavailable,
}
pub type McpPreparationResult<T> = Result<T, McpPreparationFailure>;

#[async_trait]
pub trait McpRequestAuthority: Send + Sync {
    async fn check(&self) -> McpPreparationResult<()>;
}

#[derive(Clone)]
pub struct McpPreparation {
    pub deadline: DateTime<Utc>,
    pub cancellation: CancellationToken,
    authority: Arc<dyn McpRequestAuthority>,
    failure: Arc<AtomicU8>,
}
impl McpPreparation {
    pub fn new(
        deadline: DateTime<Utc>,
        cancellation: CancellationToken,
        authority: Arc<dyn McpRequestAuthority>,
    ) -> Self {
        Self {
            deadline,
            cancellation,
            authority,
            failure: Arc::new(AtomicU8::new(0)),
        }
    }
    pub async fn check(&self) -> McpPreparationResult<()> {
        let remaining = (self.deadline - Utc::now())
            .to_std()
            .map_err(|_| self.remember(McpPreparationFailure::Deadline))?;
        let result = tokio::select! { biased;
            _ = self.cancellation.cancelled() => Err(McpPreparationFailure::Cancelled),
            _ = tokio::time::sleep(remaining) => Err(McpPreparationFailure::Deadline),
            result = self.authority.check() => result,
        };
        result.map_err(|failure| self.remember(failure))?;
        if self.cancellation.is_cancelled() {
            return Err(self.remember(McpPreparationFailure::Cancelled));
        }
        if Utc::now() >= self.deadline {
            return Err(self.remember(McpPreparationFailure::Deadline));
        }
        Ok(())
    }
    pub fn failure(&self) -> Option<McpPreparationFailure> {
        match self.failure.load(Ordering::Acquire) {
            1 => Some(McpPreparationFailure::Denied),
            2 => Some(McpPreparationFailure::Changed),
            3 => Some(McpPreparationFailure::Cancelled),
            4 => Some(McpPreparationFailure::Deadline),
            5 => Some(McpPreparationFailure::Unavailable),
            _ => None,
        }
    }
    pub fn remember(&self, failure: McpPreparationFailure) -> McpPreparationFailure {
        let code = match failure {
            McpPreparationFailure::Denied => 1,
            McpPreparationFailure::Changed => 2,
            McpPreparationFailure::Cancelled => 3,
            McpPreparationFailure::Deadline => 4,
            McpPreparationFailure::Unavailable => 5,
        };
        let _ = self
            .failure
            .compare_exchange(0, code, Ordering::AcqRel, Ordering::Acquire);
        failure
    }
}
