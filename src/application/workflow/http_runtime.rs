use super::*;
use crate::app_error::AppError;
use crate::application::workflow::{
    actions::*, lease::*, polling::WorkflowStepKind, worker::WorkflowHandler,
};
use tokio_util::sync::CancellationToken;

pub struct HttpToolCall {
    pub connection: Uuid,
    pub operation: TypeName,
    pub arguments: Value,
    pub call: ModelToolCallId,
}
#[async_trait]
pub trait HttpActionPreparation: Send + Sync {
    async fn http_step_request(
        &self,
        claim: &ClaimedWorkflow,
        kind: &WorkflowStepKind,
    ) -> AppResult<ActionRequest>;
    async fn http_tool_request(
        &self,
        scope: ActionScope,
        call: &HttpToolCall,
    ) -> AppResult<ActionRequest>;
}

/// Both entrances prepare the same immutable invocation and consume only committed receipts.
pub struct HttpActions<P, D, R> {
    persistence: P,
    dispatch: ActionService<D>,
    provider: R,
    lease: LeasePolicy,
}
impl<P, D, R> HttpActions<P, D, R> {
    pub fn new(persistence: P, dispatch: D, provider: R, lease: LeasePolicy) -> Self {
        Self {
            persistence,
            dispatch: ActionService::new(dispatch),
            provider,
            lease,
        }
    }
}
impl<P: HttpActionPreparation + ActionIntents + Clone, D: ActionDispatch, R: RemoteAction>
    HttpActions<P, D, R>
{
    pub async fn call_tool(
        &self,
        fence: WorkflowFence,
        call: HttpToolCall,
        cancel: &CancellationToken,
    ) -> AppResult<RemoteDispatchObservation> {
        let scope = ActionScope {
            company: fence.scope.company,
            run: fence.scope.run,
            execution: fence.scope.execution,
        };
        let request = self.persistence.http_tool_request(scope, &call).await?;
        HttpRequest::parse(&request.arguments)?;
        let intent = ActionService::new(self.persistence.clone())
            .prepare_tool(request, call.call)
            .await?;
        self.dispatch
            .dispatch_remote(
                &ActionDispatchRequest {
                    subject: intent.approval_subject(),
                    fence,
                    lease: self.lease,
                },
                &self.provider,
                cancel,
            )
            .await
    }
    /// Preparation errors happen before `dispatch_remote` and can never follow a marker;
    /// the 04.10-D3 mapping relies on keeping the two phases apart.
    async fn step(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> Result<RemoteDispatchObservation, StepError> {
        let request = self
            .persistence
            .http_step_request(claim, kind)
            .await
            .map_err(StepError::Preparation)?;
        HttpRequest::parse(&request.arguments).map_err(StepError::Preparation)?;
        let intent = ActionService::new(self.persistence.clone())
            .prepare_step(request)
            .await
            .map_err(StepError::Preparation)?;
        self.dispatch
            .dispatch_remote(
                &ActionDispatchRequest {
                    subject: intent.approval_subject(),
                    fence: claim.fence,
                    lease: self.lease,
                },
                &self.provider,
                &CancellationToken::new(),
            )
            .await
            .map_err(StepError::Dispatch)
    }
}
enum StepError {
    Preparation(AppError),
    Dispatch(AppError),
}
#[async_trait]
impl<P: HttpActionPreparation + ActionIntents + Clone, D: ActionDispatch, R: RemoteAction>
    WorkflowHandler for HttpActions<P, D, R>
{
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        matches!(kind.0.as_str(), "http.request" | "tool.call")
    }
    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        self.run(kind, claim).await
    }
}
impl<P: HttpActionPreparation + ActionIntents + Clone, D: ActionDispatch, R: RemoteAction>
    HttpActions<P, D, R>
{
    /// Protected calls suspend until their frozen invocation is approved.
    pub async fn run(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        use ActionStepOutcome as Outcome;
        let outcome = match self.step(kind, claim).await {
            Ok(RemoteDispatchObservation::Committed(receipt)) => return Ok(receipt.result).into(),
            Ok(RemoteDispatchObservation::ApprovalParked(wait)) => {
                return WorkflowHandlerOutcome::Suspended(SuspendedWorkflow { wait });
            }
            Ok(RemoteDispatchObservation::ApprovalRequired) => Outcome::ApprovalRequired,
            Ok(RemoteDispatchObservation::PossibleDispatchExists) => {
                Outcome::PossibleDispatchExists
            }
            Ok(RemoteDispatchObservation::Interrupted) => Outcome::Interrupted,
            Err(StepError::Preparation(error)) => {
                return Err(action_step_failure(
                    ActionFamily::Http,
                    Outcome::Preparation(&error),
                ))
                .into();
            }
            Err(StepError::Dispatch(error)) => {
                return Err(action_step_failure(
                    ActionFamily::Http,
                    Outcome::Dispatch(&error),
                ))
                .into();
            }
        };
        Err(action_step_failure(ActionFamily::Http, outcome)).into()
    }
}
