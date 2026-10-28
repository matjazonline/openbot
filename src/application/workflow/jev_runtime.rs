//! Tool-free TypeSafe execution ports and exact bounded wire contract.
use super::{
    AssetName, JevAssets, JevAuthorization, JevCredential, JevImage, JevModel, parse_assets,
};
use crate::app_error::{AppError, AppResult};
use crate::application::workflow::{
    lease::*, polling::WorkflowStepKind, registry::jev as contract, worker::WorkflowHandler,
};
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
use async_trait::async_trait;
use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;
use uuid::Uuid;

/// Contains frozen data, never credentials. Debug deliberately omits payloads.
#[derive(Clone)]
pub struct JevRequest {
    body: Vec<u8>,
    identity: Vec<u8>,
    asset_pixels: u64,
    images: bool,
    ready: bool,
    model: JevModel,
    questions: contract::Questions,
}
impl JevRequest {
    pub fn prepare(inputs: &Value) -> AppResult<Self> {
        contract::validate_inputs(inputs).map_err(|_| invalid())?;
        let model = JevModel::parse(inputs["model"].as_str().ok_or_else(invalid)?)?;
        let questions = contract::Questions::parse(&inputs["questions"]).map_err(|_| invalid())?;
        contract::validate_state(&inputs["state"]).map_err(|_| invalid())?;
        let assets = parse_assets(inputs.get("assets")).map_err(|_| invalid())?;
        let mut identity = json!({"model":model,"state":inputs["state"],"questions":questions});
        if !assets.is_empty() {
            identity["assets"] = serde_json::to_value(&assets).map_err(|_| invalid())?;
        }
        let body = serde_json::to_vec(&identity).map_err(|_| invalid())?;
        if body.len() > contract::MAX_REQUEST_BYTES {
            return Err(invalid());
        }
        Ok(Self {
            identity: body.clone(),
            images: !assets.is_empty(),
            ready: assets.is_empty(),
            asset_pixels: 0,
            body,
            model,
            questions,
        })
    }
    pub fn hydrate(mut self, images: BTreeMap<AssetName, JevImage>) -> AppResult<Self> {
        let mut wire: Value = serde_json::from_slice(&self.identity).map_err(|_| invalid())?;
        let refs = parse_assets(wire.get("assets")).map_err(|_| invalid())?;
        if refs.len() != images.len() || refs.keys().ne(images.keys()) {
            return Err(invalid());
        }
        if refs.is_empty() {
            return Ok(self);
        }
        self.asset_pixels = 0;
        let mut state_assets = serde_json::Map::new();
        let mut wire_assets = serde_json::Map::new();
        for (name, image) in images {
            if super::ImageDigest::of(image.bytes()) != refs[&name].sha256 {
                return Err(invalid());
            }
            let mut hash = Sha256::new();
            hash.update(&self.identity);
            hash.update(name.as_str().as_bytes());
            let placeholder = format!("jev_asset_{:x}", hash.finalize());
            if self
                .identity
                .windows(placeholder.len())
                .any(|window| window == placeholder.as_bytes())
            {
                return Err(invalid());
            }
            state_assets.insert(name.as_str().into(), json!(placeholder));
            wire_assets.insert(placeholder, json!({"type":"image","format":image.format(),"data_base64":base64::engine::general_purpose::STANDARD.encode(image.bytes())}));
            self.asset_pixels += image.pixels();
        }
        wire["state"] = json!({"content":wire["state"],"assets":state_assets});
        wire["assets"] = json!(wire_assets);
        self.body = serde_json::to_vec(&wire).map_err(|_| invalid())?;
        if self.body.len() > contract::MAX_REQUEST_BYTES {
            return Err(invalid());
        }
        self.ready = true;
        Ok(self)
    }
    pub fn identity(&self) -> &[u8] {
        &self.identity
    }
    pub fn has_images(&self) -> bool {
        self.images
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    pub fn model(&self) -> &JevModel {
        &self.model
    }
    pub fn digest(&self) -> Vec<u8> {
        Sha256::digest(&self.body).to_vec()
    }
    // One token per UTF-8 byte is conservative; include the bounded answer envelope.
    pub fn reserved_input(&self) -> i64 {
        self.body.len() as i64 + self.asset_pixels as i64
    }
    pub fn reserved_output(&self) -> i64 {
        contract::MAX_RESPONSE_BYTES as i64
    }
    pub fn validate_response(&self, value: Value) -> Result<JevResponse, JevError> {
        let schema = self.questions.output_schema(self.model.as_str());
        let validator = jsonschema::validator_for(&schema).map_err(|_| JevError::Protocol)?;
        if !validator.is_valid(&value) || contract::validate_output(&value).is_err() {
            return Err(JevError::Protocol);
        }
        let count = |name| {
            value["usage"][name]
                .as_i64()
                .filter(|n| (0..=i32::MAX as i64).contains(n))
                .ok_or(JevError::Protocol)
        };
        Ok(JevResponse {
            input_tokens: count("input_tokens")?,
            output_tokens: count("output_tokens")?,
            value,
        })
    }
}
pub struct JevResponse {
    pub value: Value,
    pub input_tokens: i64,
    pub output_tokens: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JevError {
    Invalid,
    Protocol,
    Transport,
    Timeout,
    Http(u16, Option<Duration>),
}
impl JevError {
    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::Transport | Self::Timeout | Self::Http(429 | 500..=599, _)
        )
    }
    pub fn code(self) -> &'static str {
        match self {
            Self::Invalid => "jev.invalid_request",
            Self::Protocol => "jev.provider_protocol",
            Self::Transport => "jev.transport",
            Self::Timeout => "jev.timeout",
            Self::Http(429 | 529, _) => "jev.rate_limited",
            Self::Http(401, _) => "jev.credentials",
            Self::Http(_, _) => "jev.http",
        }
    }
}
#[async_trait]
pub trait JevProvider: Send + Sync {
    /// Acquire only in-memory transport capacity, before database admission/authorization.
    async fn acquire(&self, deadline: Instant) -> Result<Box<dyn JevProviderSlot>, JevError>;
}
#[async_trait]
pub trait JevProviderSlot: Send + Sync {
    /// Consumes held capacity; no capacity queue may follow final credential authorization.
    /// Cancellation drops the real operation and capacity. No detached work.
    async fn evaluate(
        self: Box<Self>,
        request: &JevRequest,
        credential: &JevCredential,
        deadline: Instant,
    ) -> Result<JevResponse, JevError>;
}
/// This grant is produced only after an acknowledged fresh durable admission.
pub struct JevAdmission {
    pub(crate) request_id: Uuid,
}
impl JevAdmission {
    pub fn request_id(&self) -> Uuid {
        self.request_id
    }
}
pub enum JevAdmissionResult {
    New(JevAdmission),
    AlreadyAdmitted,
    BudgetExhausted,
    TokenLimit,
    Deferred(Duration),
}
#[async_trait]
pub trait JevExecutions: Send + Sync {
    async fn replay_jev(
        &self,
        claim: &ClaimedWorkflow,
        request: &JevRequest,
        policy: LeasePolicy,
    ) -> AppResult<Option<WorkflowHandlerResult>>;
    async fn admit_jev(
        &self,
        claim: &ClaimedWorkflow,
        request: &JevRequest,
        policy: LeasePolicy,
    ) -> AppResult<JevAdmissionResult>;
    async fn settle_jev(
        &self,
        claim: &ClaimedWorkflow,
        admission: &JevAdmission,
        request: &JevRequest,
        response: &JevResponse,
        latency: Duration,
        policy: LeasePolicy,
    ) -> AppResult<()>;
    async fn fail_jev(
        &self,
        claim: &ClaimedWorkflow,
        admission: &JevAdmission,
        failure: JevError,
        latency: Duration,
        policy: LeasePolicy,
    ) -> AppResult<()>;
}
pub struct JevRuntime<P> {
    persistence: P,
    provider: Arc<dyn JevProvider>,
    policy: LeasePolicy,
    assets: Option<Arc<dyn JevAssets>>,
}
impl<P> JevRuntime<P> {
    pub fn with_assets(mut self, assets: Arc<dyn JevAssets>) -> Self {
        self.assets = Some(assets);
        self
    }
    pub fn set_assets(&mut self, assets: Arc<dyn JevAssets>) {
        self.assets = Some(assets);
    }
    pub fn new(persistence: P, provider: Arc<dyn JevProvider>, policy: LeasePolicy) -> Self {
        Self {
            persistence,
            provider,
            policy,
            assets: None,
        }
    }
}
struct JevInvocation {
    request: JevRequest,
    admission: JevAdmission,
    slot: Box<dyn JevProviderSlot>,
    started: Instant,
    deadline: Instant,
}
impl<P: JevExecutions + JevAuthorization> JevRuntime<P> {
    async fn run(&self, claim: &ClaimedWorkflow) -> WorkflowHandlerResult {
        let request = JevRequest::prepare(&claim.activation.inputs)
            .map_err(|_| failure("jev.invalid_request", false))?;
        let started = Instant::now();
        let deadline = claim
            .window
            .run_deadline
            .min(started + Duration::from_secs(30));
        // Capacity precedes asset reads and final current-use authorization.
        let slot = self
            .provider
            .acquire(deadline)
            .await
            .map_err(|error| failure(error.code(), error.retryable()))?;
        let request = if request.has_images() {
            let reader = self
                .assets
                .as_ref()
                .ok_or_else(|| failure("jev.asset_storage_unconfigured", false))?;
            let images = tokio::time::timeout_at(deadline, reader.read(claim))
                .await
                .map_err(|_| failure("jev.asset_storage", true))?
                .map_err(|e| failure(e.code(), e.retryable()))?;
            request
                .hydrate(images)
                .map_err(|_| failure("jev.invalid_request", false))?
        } else {
            request
        };
        if let Some(saved) = self
            .persistence
            .replay_jev(claim, &request, self.policy)
            .await
            .map_err(storage_failure)?
        {
            return saved;
        }
        let admission = match self
            .persistence
            .admit_jev(claim, &request, self.policy)
            .await
            .map_err(storage_failure)?
        {
            JevAdmissionResult::New(grant) => grant,
            JevAdmissionResult::AlreadyAdmitted => return Err(failure("jev.unresolved", true)),
            JevAdmissionResult::BudgetExhausted => return Err(failure("jev.model_budget", false)),
            JevAdmissionResult::TokenLimit => return Err(failure("jev.token_limit", false)),
            JevAdmissionResult::Deferred(delay) => {
                drop(slot);
                tokio::time::sleep_until((Instant::now() + delay).min(claim.window.run_deadline))
                    .await;
                return Err(failure("jev.rate_limited", true));
            }
        };
        // Keep the provider/settlement future off the worker's debug poll frame.
        Box::pin(self.invoke(
            claim,
            JevInvocation {
                request,
                admission,
                slot,
                started,
                deadline,
            },
        ))
        .await
    }
    async fn invoke(
        &self,
        claim: &ClaimedWorkflow,
        invocation: JevInvocation,
    ) -> WorkflowHandlerResult {
        let JevInvocation {
            request,
            admission,
            slot,
            started,
            deadline,
        } = invocation;
        // This final fenced authorization reloads the selected key; it is never checkpointed.
        let credential = self
            .persistence
            .authorize_jev(claim)
            .await
            .map_err(storage_failure)?;
        // Box the provider boundary to keep the worker's debug poll frame shallow.
        let result = Box::pin(slot.evaluate(&request, &credential, deadline)).await;
        let latency = started.elapsed();
        match result {
            Ok(response) => {
                self.persistence
                    .settle_jev(claim, &admission, &request, &response, latency, self.policy)
                    .await
                    .map_err(storage_failure)?;
                tracing::info!(provider="typesafe", model=%request.model().as_str(), run_id=%claim.fence.scope.run, execution_id=%claim.fence.scope.execution, step=%claim.activation.step, input_tokens=response.input_tokens, output_tokens=response.output_tokens, latency_ms=latency.as_millis(), state="settled", "Jev invocation");
                Ok(response.value)
            }
            Err(error) => {
                self.persistence
                    .fail_jev(claim, &admission, error, latency, self.policy)
                    .await
                    .map_err(storage_failure)?;
                tracing::info!(provider="typesafe", model=%request.model().as_str(), run_id=%claim.fence.scope.run, execution_id=%claim.fence.scope.execution, step=%claim.activation.step, latency_ms=latency.as_millis(), retryable=error.retryable(), code=error.code(), "Jev invocation failed");
                if let JevError::Http(_, Some(delay)) = error {
                    tokio::time::sleep_until(
                        (Instant::now() + delay.min(Duration::from_secs(60)))
                            .min(claim.window.run_deadline),
                    )
                    .await;
                }
                Err(failure(error.code(), error.retryable()))
            }
        }
    }
}
#[async_trait]
impl<P: JevExecutions + JevAuthorization> WorkflowHandler for JevRuntime<P> {
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        kind.0 == "jev.evaluate"
    }
    async fn execute(
        &self,
        _: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        self.run(claim).await.into()
    }
}
fn invalid() -> AppError {
    AppError::BadRequest("Invalid Jev request".into())
}
fn storage_failure(error: AppError) -> WorkflowFailure {
    let retryable = !matches!(
        error,
        AppError::BadRequest(_) | AppError::NotFound(_) | AppError::InvalidCredentials
    );
    failure(
        if retryable {
            "jev.storage"
        } else {
            "jev.authorization"
        },
        retryable,
    )
}
fn failure(code: &str, retryable: bool) -> WorkflowFailure {
    WorkflowFailure {
        failure: StepFailure::new(
            if retryable {
                FailureClass::Retryable
            } else {
                FailureClass::Terminal
            },
            FailureCode::parse(code).expect("static failure code"),
            None,
        )
        .expect("bounded static failure"),
        safety: RetrySafety::SafeToRetry,
    }
}
