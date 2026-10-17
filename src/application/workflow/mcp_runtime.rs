use super::*;
use crate::application::{
    services::mcp_client::{McpClient, McpSession},
    workflow::{actions::*, lease::*, polling::WorkflowStepKind, worker::WorkflowHandler},
};
use crate::domain::workflow::ResourceName;
use crate::entities::mcp::McpEndpoint;
use secrecy::SecretString;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// A direct agent MCP tool call. The slot resolves only through the run's saved binding.
pub struct McpToolCall {
    pub slot: ResourceName,
    pub tool: TypeName,
    pub arguments: Value,
    pub call: ModelToolCallId,
}

#[async_trait]
pub trait McpActionPreparation: Send + Sync {
    /// The exact frozen request of an `mcp.call` step, after current authority and
    /// resource readiness checks (the same ones reservation enforces).
    async fn mcp_step_request(
        &self,
        claim: &ClaimedWorkflow,
        kind: &WorkflowStepKind,
    ) -> AppResult<ActionRequest>;
    async fn mcp_tool_request(
        &self,
        scope: ActionScope,
        call: &McpToolCall,
    ) -> AppResult<ActionRequest>;
}

/// Use-time facts read in their own short transaction under current authority. Never a
/// grant: reservation re-decides under its locks.
pub struct McpUse {
    pub endpoint: McpEndpoint,
    pub token: Option<SecretString>,
    pub credential_revision: i64,
    pub approval_required: bool,
}
#[async_trait]
pub trait McpActionUse: Send + Sync {
    async fn mcp_use(&self, request: &ActionRequest) -> AppResult<McpUse>;
}

/// `mcp.call` steps and direct agent MCP tools share one prepared invocation path.
pub struct McpActions<P, D> {
    persistence: P,
    dispatch: ActionService<D>,
    client: Arc<dyn McpClient>,
    lease: LeasePolicy,
}
impl<P, D> McpActions<P, D> {
    pub fn new(
        persistence: P,
        dispatch: D,
        client: Arc<dyn McpClient>,
        lease: LeasePolicy,
    ) -> Self {
        Self {
            persistence,
            dispatch: ActionService::new(dispatch),
            client,
            lease,
        }
    }
}

enum Failure {
    Preparation(AppError),
    Unavailable,
    ContractChanged,
    Dispatch(AppError),
}
impl Failure {
    fn into_error(self) -> AppError {
        match self {
            Self::Preparation(error) | Self::Dispatch(error) => error,
            Self::Unavailable => AppError::BadRequest("MCP server unavailable".into()),
            Self::ContractChanged => {
                AppError::Conflict("MCP tool no longer matches its approved contract".into())
            }
        }
    }
}

impl<P, D> McpActions<P, D>
where
    P: McpActionPreparation + McpActionUse + ActionIntents + ActionEffectStates + Clone + 'static,
    D: ActionDispatch,
{
    pub async fn call_tool(
        &self,
        fence: WorkflowFence,
        call: McpToolCall,
        cancel: &CancellationToken,
    ) -> AppResult<RemoteDispatchObservation> {
        let scope = ActionScope {
            company: fence.scope.company,
            run: fence.scope.run,
            execution: fence.scope.execution,
        };
        let request = self.persistence.mcp_tool_request(scope, &call).await?;
        let intent = ActionService::new(self.persistence.clone())
            .prepare_tool(request.clone(), call.call)
            .await?;
        Box::pin(self.dispatch_prepared(request, intent, fence, cancel))
            .await
            .map_err(Failure::into_error)
    }

    async fn step(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> Result<RemoteDispatchObservation, Failure> {
        let request = self
            .persistence
            .mcp_step_request(claim, kind)
            .await
            .map_err(Failure::Preparation)?;
        let intent = ActionService::new(self.persistence.clone())
            .prepare_step(request.clone())
            .await
            .map_err(Failure::Preparation)?;
        Box::pin(self.dispatch_prepared(request, intent, claim.fence, &CancellationToken::new()))
            .await
    }

    /// While the invocation is only `Prepared`, connect, initialize and compare live
    /// discovery before any possible-dispatch marker exists, so transport failure or drift
    /// fails cleanly. Otherwise reservation decides without MCP I/O (receipt, existing
    /// marker, protection); only an evidence-permitted re-reservation opens lazily.
    async fn dispatch_prepared(
        &self,
        request: ActionRequest,
        intent: ActionIntent,
        fence: WorkflowFence,
        cancel: &CancellationToken,
    ) -> Result<RemoteDispatchObservation, Failure> {
        let subject = intent.approval_subject();
        let state = self
            .persistence
            .effect_state(request.scope, &subject)
            .await
            .map_err(Failure::Dispatch)?;
        let provider = McpCall::<P> {
            persistence: self.persistence.clone(),
            client: self.client.clone(),
            opened: Mutex::new(None),
        };
        if state == ActionEffectState::Prepared && !request.context.approval_required {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(RemoteDispatchObservation::Interrupted),
                opened = provider.open(&request) => opened?,
            }
        }
        let observation = self
            .dispatch
            .dispatch_remote(
                &ActionDispatchRequest {
                    subject,
                    fence,
                    lease: self.lease,
                },
                &provider,
                cancel,
            )
            .await;
        provider.close_unused().await;
        observation.map_err(Failure::Dispatch)
    }
}

#[async_trait]
impl<P, D> WorkflowHandler for McpActions<P, D>
where
    P: McpActionPreparation + McpActionUse + ActionIntents + ActionEffectStates + Clone + 'static,
    D: ActionDispatch,
{
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        kind.0 == "mcp.call"
    }
    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        self.run(kind, claim).await.into()
    }
}
impl<P, D> McpActions<P, D>
where
    P: McpActionPreparation + McpActionUse + ActionIntents + ActionEffectStates + Clone + 'static,
    D: ActionDispatch,
{
    /// The step's completed result: MCP steps never suspend.
    pub async fn run(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerResult {
        use ActionStepOutcome as Outcome;
        match self.step(kind, claim).await {
            Ok(RemoteDispatchObservation::Committed(receipt)) => {
                if mcp_result_is_error(&receipt.result) {
                    // The receipt keeps the bounded error result; a tool error never proves
                    // that the effect was not applied, so no new call is ever made for it.
                    fail(Outcome::ToolError)
                } else {
                    Ok(receipt.result)
                }
            }
            Ok(RemoteDispatchObservation::ApprovalRequired) => fail(Outcome::ApprovalRequired),
            Ok(RemoteDispatchObservation::PossibleDispatchExists) => {
                fail(Outcome::PossibleDispatchExists)
            }
            Ok(RemoteDispatchObservation::Interrupted) => fail(Outcome::Interrupted),
            Err(Failure::Preparation(error)) => fail(Outcome::Preparation(&error)),
            Err(Failure::Unavailable) => fail(Outcome::Unavailable),
            Err(Failure::ContractChanged) => fail(Outcome::ContractChanged),
            Err(Failure::Dispatch(error)) => fail(Outcome::Dispatch(&error)),
        }
    }
}

struct Opened {
    session: Box<dyn McpSession>,
    credential_revision: i64,
}

/// The provider for exactly one dispatch. A session opened before reservation is moved into
/// the single `tools/call`; dropping this value (cancellation, lease loss) drops the session,
/// whose supervised worker sends protocol cancellation and closes it.
struct McpCall<P> {
    persistence: P,
    client: Arc<dyn McpClient>,
    opened: Mutex<Option<Opened>>,
}
impl<P: McpActionUse> McpCall<P> {
    async fn open(&self, request: &ActionRequest) -> Result<(), Failure> {
        let usage = self
            .persistence
            .mcp_use(request)
            .await
            .map_err(Failure::Preparation)?;
        if usage.approval_required {
            // Reservation reports protection without contacting the server.
            return Ok(());
        }
        let opened = connect_checked(&*self.client, usage, request).await?;
        *self.opened.lock().map_err(|_| Failure::Unavailable)? = Some(opened);
        Ok(())
    }
    fn take(&self) -> Option<Opened> {
        self.opened.lock().ok().and_then(|mut opened| opened.take())
    }
    async fn close_unused(&self) {
        if let Some(opened) = self.take() {
            close(opened.session).await;
        }
    }
}

async fn connect_checked(
    client: &dyn McpClient,
    usage: McpUse,
    request: &ActionRequest,
) -> Result<Opened, Failure> {
    let session = client
        .connect(&usage.endpoint, usage.token)
        .await
        .map_err(|_| Failure::Unavailable)?;
    match session.discover().await {
        Ok(tools) if discovery_matches(&request.contract, &tools) => Ok(Opened {
            session,
            credential_revision: usage.credential_revision,
        }),
        Ok(_) => {
            close(session).await;
            Err(Failure::ContractChanged)
        }
        Err(_) => {
            close(session).await;
            Err(Failure::Unavailable)
        }
    }
}

async fn close(session: Box<dyn McpSession>) {
    if session.close().await.is_err() {
        tracing::warn!("MCP session close failed");
    }
}

#[async_trait]
impl<P: McpActionUse> RemoteAction for McpCall<P> {
    /// MCP registers no replay contract: annotations and session ids never prove safety.
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, action: &FrozenAction, _: &ProviderInvocation) -> AppResult<Value> {
        let request = action.request();
        let opened = match self.take() {
            Some(opened) => opened,
            // Only an evidence-permitted re-reservation reaches this: after the marker, any
            // failure here stays an unknown effect and nothing is sent.
            None => {
                let usage = self.persistence.mcp_use(request).await?;
                connect_checked(&*self.client, usage, request)
                    .await
                    .map_err(Failure::into_error)?
            }
        };
        // Immediately before tools/call: current authority, unchanged credential, no new
        // protection. Revocation or rotation since the session opened aborts unsent.
        match self.persistence.mcp_use(request).await {
            Ok(current)
                if current.credential_revision == opened.credential_revision
                    && !current.approval_required => {}
            Ok(_) => {
                close(opened.session).await;
                return Err(AppError::Conflict(
                    "MCP credential or policy changed before the call".into(),
                ));
            }
            Err(error) => {
                close(opened.session).await;
                return Err(error);
            }
        }
        let name = crate::entities::mcp::McpToolName::try_from(
            request.contract.contract.name.as_str().to_owned(),
        )
        .map_err(|_| invalid())?;
        let result = opened.session.call(&name, request.arguments.clone()).await;
        close(opened.session).await;
        Ok(result?.envelope())
    }
}

fn fail(outcome: ActionStepOutcome<'_>) -> WorkflowHandlerResult {
    Err(action_step_failure(ActionFamily::Mcp, outcome))
}
