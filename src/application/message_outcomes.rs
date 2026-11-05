use uuid::Uuid;

use crate::{app_error::AppResult, entities::message_outcome::MessageBindingOutcome};

/// The simulation's read (p08 D4.6): every binding outcome of the given messages in a thread, in
/// message then binding id order. The caller has proved the thread is the reader's to inspect;
/// the company scopes every index.
#[async_trait::async_trait]
pub trait MessageOutcomeReader: Send + Sync {
    async fn message_outcomes(
        &self,
        scope: MessageOutcomeScope,
        message_ids: &[Uuid],
    ) -> AppResult<Vec<MessageBindingOutcome>>;
}

#[derive(Clone, Copy)]
pub struct MessageOutcomeScope {
    pub company: crate::domain::workflow::CompanyId,
    pub thread_id: Uuid,
    pub viewer: crate::application::workflow::WorkflowActor,
}

pub(crate) fn project_outcome(
    run: &mut crate::entities::message_outcome::AdmittedRun,
    visibility: crate::application::knowledge::PayloadVisibility,
) {
    if visibility.is_visible() {
        return;
    }
    run.payload_restricted = true;
    for execution in &mut run.executions {
        execution.route = None;
    }
    run.model_usage.clear();
}
