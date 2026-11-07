use super::*;
use crate::application::transport::DeliveryComposer;
use crate::application::workflow::lease::{
    LeasePolicy, SuspendedWorkflow, WorkflowFailure, WorkflowHandlerOutcome,
};
use crate::application::workflow::worker::WorkflowHandler;
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};

/// `message.send`/`message.reply` steps and direct message tools share one invocation path.
pub struct MessageActions<P, D> {
    persistence: P,
    dispatch: D,
    planner: MessagePlanner,
    lease: LeasePolicy,
}
impl<P, D> MessageActions<P, D> {
    pub fn new(
        persistence: P,
        dispatch: D,
        composer: DeliveryComposer,
        app_domain_name: String,
        lease: LeasePolicy,
    ) -> Self {
        Self {
            persistence,
            dispatch,
            planner: MessagePlanner::new(composer, app_domain_name),
            lease,
        }
    }
}

enum StepError {
    Preparation(AppError),
    Dispatch(AppError),
}

impl<P, D> MessageActions<P, D>
where
    P: MessageActionPreparation + ActionIntents + Clone,
    D: MessageDispatch,
{
    /// Direct tool entry. Re-entry with the same model call observes the same invocation.
    pub async fn call_tool(
        &self,
        fence: WorkflowFence,
        call: MessageToolCall,
    ) -> AppResult<MessageObservation> {
        let request = self
            .persistence
            .message_tool_request(tool_scope(fence), &call)
            .await?;
        let arguments = MessageArguments::parse(call.kind, &request.arguments)?;
        let intent = ActionService::new(self.persistence.clone())
            .prepare_tool(request.clone(), call.call)
            .await?;
        let dispatch = self.dispatch_request(&intent, fence);
        let publication =
            Box::pin(self.publish(&request, &arguments, &dispatch, MessageOrigin::ModelTool))
                .await
                .map_err(|error| match error {
                    StepError::Preparation(error) | StepError::Dispatch(error) => error,
                })?;
        Ok(match publication {
            MessagePublication::Rejected(error) => return Err(error),
            MessagePublication::Committed(receipt) => MessageObservation::Committed(receipt),
            MessagePublication::Suspended(wait) => MessageObservation::DeliveryParked(wait),
            MessagePublication::Pending | MessagePublication::PossibleDispatchExists => {
                MessageObservation::Pending
            }
            MessagePublication::Uncertain => MessageObservation::Uncertain,
            MessagePublication::ApprovalRequired => MessageObservation::ApprovalRequired,
            MessagePublication::ApprovalParked(wait) => MessageObservation::ApprovalParked(wait),
            MessagePublication::ActionLimit => MessageObservation::ActionLimit,
            MessagePublication::ContinuationLimit => MessageObservation::ContinuationLimit,
        })
    }

    fn dispatch_request(
        &self,
        intent: &ActionIntent,
        fence: WorkflowFence,
    ) -> ActionDispatchRequest {
        ActionDispatchRequest {
            subject: intent.approval_subject(),
            fence,
            lease: self.lease,
        }
    }

    async fn step(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> Result<MessagePublication, StepError> {
        let message_kind =
            MessageKind::parse(&kind.0).ok_or_else(|| StepError::Preparation(invalid_message()))?;
        let request = self
            .persistence
            .message_step_request(claim, kind)
            .await
            .map_err(StepError::Preparation)?;
        let arguments = MessageArguments::parse(message_kind, &request.arguments)
            .map_err(StepError::Preparation)?;
        let intent = ActionService::new(self.persistence.clone())
            .prepare_step(request.clone())
            .await
            .map_err(StepError::Preparation)?;
        let dispatch = self.dispatch_request(&intent, claim.fence);
        self.publish(&request, &arguments, &dispatch, MessageOrigin::Step)
            .await
    }

    /// Replay first, so a committed invocation never depends on routes that changed later.
    async fn publish(
        &self,
        request: &ActionRequest,
        arguments: &MessageArguments,
        dispatch: &ActionDispatchRequest,
        origin: MessageOrigin,
    ) -> Result<MessagePublication, StepError> {
        if let Some(existing) = self
            .dispatch
            .observe(dispatch, origin)
            .await
            .map_err(StepError::Dispatch)?
        {
            return Ok(existing);
        }
        let routes = self
            .persistence
            .message_routes(request, arguments)
            .await
            .map_err(StepError::Preparation)?;
        let plan = self
            .planner
            .plan(
                MessagePublicationIdentity {
                    company: request.scope.company,
                    invocation: dispatch.subject.invocation,
                },
                arguments,
                &routes,
            )
            .await
            .map_err(StepError::Preparation)?;
        self.dispatch
            .publish(dispatch, origin, &plan)
            .await
            .map_err(StepError::Dispatch)
    }
}

fn failure(class: FailureClass, code: &'static str) -> WorkflowFailure {
    WorkflowFailure {
        failure: StepFailure::new(
            class,
            FailureCode::parse(code).expect("static message failure code"),
            None,
        )
        .expect("bounded message failure"),
        safety: RetrySafety::SafeToRetry,
    }
}

/// A failed receipt is a classified terminal step failure; nothing is resent.
pub fn message_delivery_failed() -> WorkflowFailure {
    failure(FailureClass::Terminal, "message.delivery_failed")
}

#[async_trait]
impl<P, D> WorkflowHandler for MessageActions<P, D>
where
    P: MessageActionPreparation + ActionIntents + Clone,
    D: MessageDispatch,
{
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        MessageKind::parse(&kind.0).is_some()
    }
    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        use ActionStepOutcome as Outcome;
        let family = ActionFamily::Message;
        let outcome = match self.step(kind, claim).await {
            Ok(MessagePublication::Committed(receipt)) => {
                return if receipt.result.get("status").and_then(Value::as_str) == Some("accepted") {
                    WorkflowHandlerOutcome::Completed(Ok(receipt.result))
                } else {
                    WorkflowHandlerOutcome::Completed(Err(message_delivery_failed()))
                };
            }
            Ok(MessagePublication::Suspended(wait) | MessagePublication::ApprovalParked(wait)) => {
                return WorkflowHandlerOutcome::Suspended(SuspendedWorkflow { wait });
            }
            // Refused before any marker: classified exactly as a preparation refusal.
            Ok(MessagePublication::Rejected(error)) => {
                return WorkflowHandlerOutcome::Completed(Err(action_step_failure(
                    family,
                    Outcome::Preparation(&error),
                )));
            }
            Ok(MessagePublication::ApprovalRequired) => Outcome::ApprovalRequired,
            Ok(MessagePublication::ActionLimit) => Outcome::ActionLimit,
            // Defensive: a step's delivery wait completes the step without reopening its job,
            // and a step job takes at most one continuation (its approval), so this is never
            // reached in practice. Terminal and observable rather than a park it cannot leave.
            Ok(MessagePublication::ContinuationLimit) => Outcome::ActionLimit,
            Ok(
                MessagePublication::Pending
                | MessagePublication::Uncertain
                | MessagePublication::PossibleDispatchExists,
            ) => Outcome::PossibleDispatchExists,
            Err(StepError::Preparation(error)) => {
                return WorkflowHandlerOutcome::Completed(Err(action_step_failure(
                    family,
                    Outcome::Preparation(&error),
                )));
            }
            Err(StepError::Dispatch(error)) => {
                return WorkflowHandlerOutcome::Completed(Err(action_step_failure(
                    family,
                    Outcome::Dispatch(&error),
                )));
            }
        };
        WorkflowHandlerOutcome::Completed(Err(action_step_failure(family, outcome)))
    }
}
