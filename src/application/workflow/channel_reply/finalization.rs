use crate::app_error::AppResult;
use crate::application::workflow::{
    activation::ActivationRequest, completion::CommittedWorkflowStep,
};
use async_trait::async_trait;

pub enum FinalizationProgress {
    Unstaged,
    Pending,
    Completed(Box<CommittedWorkflowStep>),
    Refused,
}

/// A saved final completion takes precedence over every workflow handler.
#[async_trait]
pub trait WorkflowFinalizations: Send + Sync {
    async fn resume_finalization(
        &self,
        scope: ActivationRequest,
    ) -> AppResult<FinalizationProgress>;
}
