use super::*;
use crate::application::workflow::{
    actions::*, lease::*, polling::WorkflowStepKind, worker::WorkflowHandler,
};
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
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
    async fn step(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> AppResult<RemoteDispatchObservation> {
        let request = self.persistence.http_step_request(claim, kind).await?;
        HttpRequest::parse(&request.arguments)?;
        let intent = ActionService::new(self.persistence.clone())
            .prepare_step(request)
            .await?;
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
    }
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
    ) -> WorkflowHandlerResult {
        match self.step(kind, claim).await {
            Ok(RemoteDispatchObservation::Committed(receipt)) => Ok(receipt.result),
            _ => Err(WorkflowFailure {
                failure: StepFailure::new(
                    FailureClass::Terminal,
                    FailureCode::parse("http.unresolved").expect("static failure code"),
                    None,
                )
                .expect("bounded failure"),
                safety: RetrySafety::EffectOutcomeUnknown,
            }),
        }
    }
}
