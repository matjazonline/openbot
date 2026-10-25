//! Built-in provisioning tools (p08 D2): `list_company_agents` and `create_agent_channel`, the
//! agent tools through which the Agent Builder reads the company's agents and creates an agent
//! with a team channel bound to a reply-mode workflow.
//!
//! A call is a marker-first local action (§04.10): the effect, its marker and its receipt commit
//! in one transaction, and a refusal commits a typed-error receipt with no effect. There is no
//! built-in approval (P08-Q5); a deployment that wants one puts a `decision.human` step in its
//! workflow.
use crate::app_error::{AppError, AppResult};
use crate::application::workflow::actions::*;
use crate::application::workflow::agent::AgentToolObservation;
use crate::application::workflow::compiler::{Schema, SourceSpan};
use crate::application::workflow::lease::{LeasePolicy, WorkflowFence};
use crate::application::workflow::publication::{
    ActionEffect, ActionRecovery, ApprovedActionPolicy, ToolSnapshot,
};
use crate::application::workflow::registry::ToolContract;
use crate::application::workflow::tools::tool_error;
use crate::domain::workflow::{CompanyId, TypeName};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

pub const LIST_COMPANY_AGENTS: &str = "list_company_agents";
pub const CREATE_AGENT_CHANNEL: &str = "create_agent_channel";
/// The `ActionTarget::Local` resource kind of a provisioning call; its resource is the company.
pub const PROVISIONING: &str = "provisioning";

pub const PROVISIONING_FORBIDDEN: &str = "workflow.provisioning_forbidden";
pub const PROVISIONING_CONFLICT: &str = "workflow.provisioning_conflict";
pub const PROVISIONING_INVALID: &str = "workflow.invalid_input";

/// The most agents `list_company_agents` returns, by slug.
pub const MAX_LISTED_AGENTS: usize = 200;
/// The most `{provider, model}` pairs `list_company_agents` returns.
pub const MAX_LISTED_MODELS: usize = 256;
/// A listed agent's name and description are cut to these many characters.
pub const MAX_LISTED_NAME_CHARS: usize = 120;
pub const MAX_LISTED_DESCRIPTION_CHARS: usize = 300;
/// The serialized listing stays within this many bytes, under the 64 KiB a receipt may hold
/// (F050): models first, then agents by slug, until the next entry would not fit.
pub const MAX_LISTING_BYTES: usize = 48 * 1024;
const MAX_REFUSAL_MESSAGE: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisioningKind {
    ListAgents,
    CreateAgentChannel,
}
impl ProvisioningKind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            LIST_COMPANY_AGENTS => Some(Self::ListAgents),
            CREATE_AGENT_CHANNEL => Some(Self::CreateAgentChannel),
            _ => None,
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::ListAgents => LIST_COMPANY_AGENTS,
            Self::CreateAgentChannel => CREATE_AGENT_CHANNEL,
        }
    }
    const fn effect(self) -> ActionEffect {
        match self {
            Self::ListAgents => ActionEffect::Read,
            Self::CreateAgentChannel => ActionEffect::Write,
        }
    }
}

/// Names the workflow catalogue reserves for the built-in provisioning tools.
pub fn is_builtin_provisioning_tool(name: &str) -> bool {
    ProvisioningKind::parse(name).is_some()
}

/// Every built-in tool name: no company HTTP operation, workflow tool or `tool.call` literal may
/// use one (p08 D2.1).
pub fn is_reserved_tool_name(name: &str) -> bool {
    crate::application::workflow::message::is_builtin_message_tool(name)
        || is_builtin_provisioning_tool(name)
}

/// The fixed argument and result contracts. Each result is the success shape or the typed error
/// `tools::tool_error` returns, so a committed refusal is a valid receipt.
pub fn provisioning_tool_schemas(kind: ProvisioningKind) -> (Value, Value) {
    let success = match kind {
        ProvisioningKind::ListAgents => json!({
            "type": "object",
            "properties": {
                "agents": {"type": "array", "maxItems": MAX_LISTED_AGENTS, "items": {
                    "type": "object",
                    "properties": {
                        "slug": {"type": "string"},
                        "name": {"type": "string", "maxLength": MAX_LISTED_NAME_CHARS},
                        "description": {"type": ["string", "null"],
                            "maxLength": MAX_LISTED_DESCRIPTION_CHARS},
                        "provider": {"type": ["string", "null"]},
                        "model": {"type": ["string", "null"]}
                    },
                    "required": ["slug", "name", "description", "provider", "model"],
                    "additionalProperties": false
                }},
                "models": {"type": "array", "maxItems": MAX_LISTED_MODELS, "items": {
                    "type": "object",
                    "properties": {"provider": {"type": "string"}, "model": {"type": "string"}},
                    "required": ["provider", "model"],
                    "additionalProperties": false
                }},
                // More agents or models exist than the listing holds.
                "truncated": {"type": "boolean"}
            },
            "required": ["agents", "models", "truncated"],
            "additionalProperties": false
        }),
        ProvisioningKind::CreateAgentChannel => json!({
            "type": "object",
            "properties": {
                "agent": {"type": "object",
                    "properties": {"id": {"type": "string"}, "slug": {"type": "string"}},
                    "required": ["id", "slug"], "additionalProperties": false},
                "channel": {"type": "object",
                    "properties": {"id": {"type": "string"}, "slug": {"type": "string"},
                        "address": {"type": "string"}},
                    "required": ["id", "slug", "address"], "additionalProperties": false},
                "workflow_id": {"type": "string"},
                "version_id": {"type": "string"},
                "binding_id": {"type": "string"},
                "reply_mode": {"enum": ["autonomous", "reviewed"]}
            },
            "required": ["agent", "channel", "workflow_id", "version_id", "binding_id", "reply_mode"],
            "additionalProperties": false
        }),
    };
    let input = match kind {
        ProvisioningKind::ListAgents => {
            json!({"type": "object", "properties": {}, "additionalProperties": false})
        }
        ProvisioningKind::CreateAgentChannel => json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 120},
                "slug": {"type": "string", "minLength": 1, "maxLength": 64},
                "description": {"type": "string", "maxLength": 500},
                "system_prompt": {"type": "string", "minLength": 1, "maxLength": 32768},
                "provider": {"type": "string", "minLength": 1, "maxLength": 64},
                "model": {"type": "string", "minLength": 1, "maxLength": 128},
                "reply_mode": {"enum": ["autonomous", "reviewed"]},
                "channel_slug": {"type": "string", "minLength": 1, "maxLength": 64},
                "channel_name": {"type": "string", "minLength": 1, "maxLength": 120},
                "tools": {"type": "array", "maxItems": 32,
                    "items": {"type": "string", "minLength": 1, "maxLength": 128}},
                "skills": {"type": "array", "maxItems": 16,
                    "items": {"type": "string", "minLength": 1, "maxLength": 128}}
            },
            "required": ["name", "slug", "system_prompt", "provider", "model", "reply_mode"],
            "additionalProperties": false
        }),
    };
    let error = json!({
        "type": "object",
        "properties": {"error": {
            "type": "object",
            "properties": {
                "code": {"enum": [PROVISIONING_FORBIDDEN, PROVISIONING_CONFLICT, PROVISIONING_INVALID]},
                "message": {"type": "string", "maxLength": MAX_REFUSAL_MESSAGE}
            },
            "required": ["code", "message"],
            "additionalProperties": false
        }},
        "required": ["error"],
        "additionalProperties": false
    });
    (input, json!({"oneOf": [success, error]}))
}

/// The trusted built-in snapshot for one provisioning kind: fixed schemas, no connection slot,
/// reconcile recovery and never an approval. Publication and dispatch verify snapshots against
/// this, never against a registration.
pub fn builtin_provisioning_snapshot(
    company: CompanyId,
    kind: ProvisioningKind,
) -> AppResult<ToolSnapshot> {
    let (input_schema, output_schema) = provisioning_tool_schemas(kind);
    let name = TypeName::parse(kind.name()).map_err(|_| invalid())?;
    Ok(ToolSnapshot {
        workflow: None,
        company_id: company,
        contract: ToolContract {
            name: name.clone(),
            connection: None,
            input_schema,
            output_schema,
        },
        policy: ApprovedActionPolicy {
            capability: name,
            policy_revision: 1,
            effect: kind.effect(),
            recovery: ActionRecovery::Reconcile,
        },
    })
}

/// Whether a bundle snapshot is exactly the trusted built-in for its name.
pub fn is_trusted_provisioning_snapshot(snapshot: &ToolSnapshot) -> AppResult<bool> {
    if snapshot.kind() != crate::application::workflow::publication::ToolKind::Provisioning {
        return Ok(false);
    }
    let Some(kind) = ProvisioningKind::parse(snapshot.contract.name.as_str()) else {
        return Ok(false);
    };
    let trusted = builtin_provisioning_snapshot(snapshot.company_id, kind)?;
    Ok(serde_json::to_value(snapshot).map_err(|_| invalid())?
        == serde_json::to_value(&trusted).map_err(|_| invalid())?)
}

/// How the created channel's runs answer: the shipped template behind each mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyMode {
    /// `agent.run` → `message.reply`.
    Autonomous,
    /// `agent.run` → `decision.human` → `message.reply`; the requester reviews.
    Reviewed,
}
impl ReplyMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Autonomous => "autonomous",
            Self::Reviewed => "reviewed",
        }
    }
}

/// `create_agent_channel` arguments, already valid against the input schema.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateAgentChannel {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub description: Option<String>,
    pub system_prompt: String,
    pub provider: String,
    pub model: String,
    pub reply_mode: ReplyMode,
    #[serde(default)]
    pub channel_slug: Option<String>,
    #[serde(default)]
    pub channel_name: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}
impl CreateAgentChannel {
    pub fn parse(arguments: &Value) -> AppResult<Self> {
        serde_json::from_value(arguments.clone()).map_err(|_| invalid())
    }
}

/// A deterministic refusal (D2.2a, F046). Built only at the named checks: the requester's
/// authority, the slug pre-checks and unique violations matched by constraint name, the pure
/// validators and the compile/publication diagnostics. Never inferred from an error variant,
/// which also carries transient failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisioningRefusal {
    Forbidden,
    Conflict(String),
    Invalid(String),
}
impl ProvisioningRefusal {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Forbidden => PROVISIONING_FORBIDDEN,
            Self::Conflict(_) => PROVISIONING_CONFLICT,
            Self::Invalid(_) => PROVISIONING_INVALID,
        }
    }
    pub fn message(&self) -> String {
        let message = match self {
            Self::Forbidden => {
                "Only a company owner or admin, writing in the app, may use this tool".into()
            }
            Self::Conflict(message) | Self::Invalid(message) => message.clone(),
        };
        message.chars().take(MAX_REFUSAL_MESSAGE).collect()
    }
    /// The committed result a refused call returns to the model.
    pub fn tool_error(&self) -> Value {
        tool_error(self.code(), &self.message())
    }
}

/// Either a provisioning outcome or the refusal to commit in its place.
pub type Provisioned<T> = Result<T, ProvisioningRefusal>;

/// "Create assistant channel" (p08 D2.4): an existing company agent answers a new team channel
/// on the chosen reply mode. The caller is the requester.
#[derive(Debug, Clone)]
pub struct AssistantChannelRequest {
    pub agent_slug: String,
    pub channel_slug: String,
    pub channel_name: String,
    pub reply_mode: ReplyMode,
    /// The client's idempotency key: a retry replays the stored result; the same key with a
    /// different request (or caller) conflicts.
    pub command_key: String,
}

/// The most bytes of an assistant-channel command key.
pub const MAX_COMMAND_KEY_BYTES: usize = 128;

#[async_trait]
pub trait AssistantChannels: Send + Sync {
    /// Owner/admin only (`NotFound` otherwise). `Ok(Err(refusal))` wrote nothing; a key reused
    /// for a different request is `Conflict`.
    async fn create_assistant_channel(
        &self,
        company: CompanyId,
        caller: crate::application::workflow::WorkflowActor,
        request: AssistantChannelRequest,
    ) -> AppResult<Provisioned<Value>>;

    /// The stored result of a command key, which a replay would return. `None` when nothing was
    /// committed under it or the caller is not the company's owner or an admin; it takes no lock.
    async fn assistant_channel_result(
        &self,
        company: CompanyId,
        caller: crate::application::workflow::WorkflowActor,
        command_key: &str,
    ) -> AppResult<Option<Value>>;
}

/// One saved model call of a provisioning tool.
#[derive(Debug, Clone)]
pub struct ProvisioningToolCall {
    pub kind: ProvisioningKind,
    pub arguments: Value,
    pub call: ModelToolCallId,
}

/// The frozen action request for one call; the trusted built-in must be in the run's bundle.
#[async_trait]
pub trait ProvisioningPreparation: Send + Sync {
    async fn provisioning_tool_request(
        &self,
        scope: ActionScope,
        kind: ProvisioningKind,
        arguments: Value,
    ) -> AppResult<ActionRequest>;
}

/// The direct-tool entry: freeze the call, then dispatch it as a local action.
pub struct ProvisioningActions<P, D> {
    persistence: P,
    dispatch: ActionService<D>,
    lease: LeasePolicy,
}
impl<P, D> ProvisioningActions<P, D> {
    pub fn new(persistence: P, dispatch: D, lease: LeasePolicy) -> Self {
        Self {
            persistence,
            dispatch: ActionService::new(dispatch),
            lease,
        }
    }
}
impl<P: ProvisioningPreparation + ActionIntents + Clone, D: ActionDispatch>
    ProvisioningActions<P, D>
{
    pub async fn call_tool(
        &self,
        fence: WorkflowFence,
        call: ProvisioningToolCall,
    ) -> AppResult<AgentToolObservation> {
        // Arguments the contract refuses go back to the model before any marker exists.
        let (input_schema, _) = provisioning_tool_schemas(call.kind);
        let span = SourceSpan {
            start: 0,
            end: 0,
            line: 1,
            column: 1,
        };
        if let Err(diagnostic) = Schema::compile(input_schema, "/input", span)
            .and_then(|schema| schema.validate(&call.arguments, "/input", span))
        {
            return Ok(AgentToolObservation::Completed(
                ProvisioningRefusal::Invalid(diagnostic.message.to_string()).tool_error(),
            ));
        }
        let request = self
            .persistence
            .provisioning_tool_request(
                crate::application::workflow::message::tool_scope(fence),
                call.kind,
                call.arguments,
            )
            .await?;
        let intent = ActionService::new(self.persistence.clone())
            .prepare_tool(request, call.call)
            .await?;
        let dispatched = self
            .dispatch
            .dispatch_local(&ActionDispatchRequest {
                subject: intent.approval_subject(),
                fence,
                lease: self.lease,
            })
            .await?;
        match dispatched {
            LocalDispatchResult::Committed(receipt) => {
                Ok(AgentToolObservation::Completed(receipt.result))
            }
            LocalDispatchResult::ApprovalParked(wait) => Ok(AgentToolObservation::Parked(wait)),
            // A built-in never requires approval; a protected policy here is a defect.
            LocalDispatchResult::ApprovalRequired => Err(AppError::Internal(
                "Built-in provisioning never requires approval".into(),
            )),
            // A local marker commits with its receipt, so this cannot follow a provisioning call.
            LocalDispatchResult::PossibleDispatchExists => Ok(AgentToolObservation::Uncertain),
        }
    }
}

fn invalid() -> AppError {
    AppError::BadRequest("Invalid provisioning tool".into())
}

#[cfg(test)]
#[path = "provisioning_tests.rs"]
mod tests;
