//! Production composition of `message.send` / `message.reply` steps and direct message tools
//! (04.11-D2 §7): the shared ActionService over the SQL publication writer with the trusted
//! built-in messaging authority, and the delivery composer that freezes email parts. No
//! provider I/O happens here; the delivery engine sends after commit. Worker startup and agent
//! tool exposure are wired in 06/08.
use crate::{
    adapters::{
        persistence::{
            PostgresPersistence,
            workflow::{action_dispatch::PostgresActionDispatch, message::MessageActionAuthority},
        },
        workflow_http::NoLocal,
    },
    app_error::AppResult,
    application::{
        transport::DeliveryComposer,
        workflow::{
            lease::{ClaimedWorkflow, LeasePolicy, WorkflowFence, WorkflowHandlerOutcome},
            message::{MessageActions, MessageObservation, MessageToolCall},
            polling::WorkflowStepKind,
            worker::WorkflowHandler,
        },
    },
};
use async_trait::async_trait;

pub struct MessageRuntime {
    inner: MessageActions<
        PostgresPersistence,
        PostgresActionDispatch<MessageActionAuthority, NoLocal>,
    >,
}
impl MessageRuntime {
    /// `app_domain_name` is the deployment domain the sending channel's address is built on,
    /// the same value the email renderer composes Message-IDs with.
    pub fn new(
        persistence: PostgresPersistence,
        composer: DeliveryComposer,
        app_domain_name: String,
        lease: LeasePolicy,
    ) -> Self {
        Self::with_approvals(persistence, composer, app_domain_name, lease, None)
    }
    /// `notices` lets a protected publication park for review; without them it fails closed.
    pub fn with_approvals(
        persistence: PostgresPersistence,
        composer: DeliveryComposer,
        app_domain_name: String,
        lease: LeasePolicy,
        notices: Option<crate::application::workflow::decisions::DecisionNotices>,
    ) -> Self {
        let mut dispatch =
            PostgresActionDispatch::new(persistence.clone(), MessageActionAuthority, NoLocal);
        if let Some(notices) = notices {
            dispatch = dispatch.with_decision_notices(notices);
        }
        Self {
            inner: MessageActions::new(persistence, dispatch, composer, app_domain_name, lease),
        }
    }
    /// Direct tool entry; Pending never claims provider acceptance (phase 06 resumes saved calls).
    pub async fn call_tool(
        &self,
        fence: WorkflowFence,
        call: MessageToolCall,
    ) -> AppResult<MessageObservation> {
        Box::pin(self.inner.call_tool(fence, call)).await
    }
}
#[async_trait]
impl WorkflowHandler for MessageRuntime {
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        self.inner.supports(kind)
    }
    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        // Bound the publication composition at the adapter seam.
        Box::pin(self.inner.execute(kind, claim)).await
    }
}
