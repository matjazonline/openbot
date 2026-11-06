use super::sample::{PreparedSample, SampleRequest};
use crate::app_error::AppResult;
use crate::application::workflow::lifecycle::DefinitionTarget;
use crate::domain::workflow::DraftRevision;
use async_trait::async_trait;

#[async_trait]
pub trait SampleDefinitionCapture: Send + Sync {
    async fn capture(
        &self,
        target: DefinitionTarget,
        expected: DraftRevision,
        request: SampleRequest,
    ) -> AppResult<PreparedSample>;
    async fn authorize_release(&self, target: DefinitionTarget) -> AppResult<()>;
}
