//! `channel.call` (plan agent-workflow-call, gate AWC-P01-D1 r3): the one built-in through
//! which an agent asks another desk and receives its answer in the same `agent.run` step.
//!
//! This module owns the model-visible contract (D2), the ordered admission policy over typed
//! facts (D1.1), the child ceiling (D1.4) and the intake check (D3). Persistence reads the facts
//! under its own locks; nothing here performs I/O.
use crate::app_error::{AppError, AppResult};
use crate::application::agent::{ActionEffect, ActionRecovery, ApprovedActionPolicy};
use crate::application::transport::ingress::{CanonicalContent, MAX_SUBJECT_BYTES};
use crate::application::workflow::{
    compiler::{Schema, SourceSpan},
    publication::{ToolKind, ToolSnapshot},
    registry::ToolContract,
    tools::MAX_TOOL_RESULT_BYTES,
    triggers::MESSAGE_INPUT_BODY_BYTES,
};
use crate::domain::workflow::{CompanyId, RunId, TypeName};
use crate::entities::access_level::AccessLevel;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use uuid::Uuid;

pub const CHANNEL_CALL: &str = "channel.call";

/// What the model is told the built-in does (05 A.2 extends it with directory discovery).
pub const CHANNEL_CALL_DESCRIPTION: &str = "Ask another desk to do a piece of work and wait for its answer. channel_id is the desk: its selected responder handles the request with its own configuration, so only the request content varies per call. The answer arrives as this tool's result in the same step, after the desk's workflow and any review it requires have finished. The result's closure says how the work ended: done, handed_off, denied (with a code and reason) or no_reply. Pass fields only when the desk declares an intake.";

// Errors: the caller must change the call (D2).
pub const INVALID_INPUT: &str = "channel_call.invalid_input";
pub const DEADLINE_INVALID: &str = "channel_call.deadline_invalid";
pub const TARGET_UNAVAILABLE: &str = "channel_call.target_unavailable";
pub const CYCLE: &str = "channel_call.cycle";
pub const INTAKE_NOT_DECLARED: &str = "channel_call.intake_not_declared";
pub const INTAKE_INVALID: &str = "channel_call.intake_invalid";
pub const AUDIENCE_REFUSED: &str = "channel_call.audience_refused";
pub const REVOKED: &str = "channel_call.revoked";
pub const REVIEW_REJECTED: &str = "channel_call.review_rejected";
pub const REVIEW_EXPIRED: &str = "channel_call.review_expired";
/// A succeeded call child whose stored reply does not decode: a defect guard only, since an
/// invalid projection fails inside the child (notice AWC-C004 r2).
pub const RESPONSE_INVALID: &str = "channel_call.response_invalid";
/// A `channel.hand_off` naming a run that is not a call this run consumed as `done` (D7).
pub const HAND_OFF_INVALID: &str = "channel_call.hand_off_invalid";
/// `message.ask` inside a call child: the requester is an agent and cannot answer (D8).
pub const INTERACTIVE_UNSUPPORTED: &str = "channel_call.interactive_unsupported";
/// `channel.deny` or `channel.hand_off` in a run that answers no message or call (D2a).
pub const RESPONSE_STEP_UNOWNED: &str = "channel.response_step_unowned";

// Denied closures: the destination declined the work (D1.1 steps 5 and 8, `channel.deny`).
pub const DENIED: &str = "channel.denied";
pub const DESK_DISABLED: &str = "channel_call.desk_disabled";
pub const NO_RESPONDER: &str = "channel_call.no_responder";
pub const DESTINATION_UNAVAILABLE: &str = "channel_call.destination_unavailable";

/// The largest serialized `fields` object a call or a reply carries.
pub const MAX_FIELDS_BYTES: usize = 16 * 1024;
/// A denied closure's reason is bounded like a subject.
pub const MAX_REASON_BYTES: usize = MAX_SUBJECT_BYTES;
/// The most failing instance paths an `intake_invalid` error names.
pub const MAX_INTAKE_PATHS: usize = 16;
const MAX_PATH_CHARS: usize = 256;
const MAX_MESSAGE_CHARS: usize = 2000;

/// Plan 11 03C supplies a per-rule allowance. Until request rules exist none allows anything
/// above public data, so a call's child reads level 0 (user, 2026-11-22, gate U4).
pub const ALLOWANCE_WITHOUT_REQUEST_RULES: AccessLevel = AccessLevel::PUBLIC;

pub fn is_builtin_channel_call_tool(name: &str) -> bool {
    name == CHANNEL_CALL
}

// ---------------------------------------------------------------------------------------------
// D2 — the request.

/// One parsed call. Only the destination, the content, the optional intake fields and the
/// optional deadline come from the model; every other fact is the runtime's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelCallRequest {
    pub channel: Uuid,
    pub content: CanonicalContent,
    pub fields: Option<Map<String, Value>>,
    pub deadline: Option<DateTime<Utc>>,
}

impl ChannelCallRequest {
    /// The destination desk alone, read before any lock so the dispatch can lock its accounts
    /// (gate AWC-P03-D1 r2 D1 step 0). It judges nothing else: a replayed call is answered from
    /// its record before `parse` judges its deadline.
    pub fn desk(arguments: &Value) -> Option<Uuid> {
        arguments
            .get("channel_id")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
    }

    /// D1.1 step 1: shape only, no target facts. `run_deadline` is the calling run's absolute
    /// deadline; a call's own deadline must fall strictly between now and it.
    pub fn parse(
        arguments: &Value,
        now: DateTime<Utc>,
        run_deadline: DateTime<Utc>,
    ) -> Result<Self, ChannelCallError> {
        let invalid = |message: &str| ChannelCallError::new(INVALID_INPUT, message);
        let object = arguments
            .as_object()
            .ok_or_else(|| invalid("The call must be an object"))?;
        if let Some(unknown) = object.keys().find(|key| {
            !matches!(
                key.as_str(),
                "channel_id" | "content" | "fields" | "deadline"
            )
        }) {
            return Err(invalid(&format!("Unknown field {}", bounded(unknown, 64))));
        }
        let channel = object
            .get("channel_id")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| invalid("channel_id must be a desk id"))?;
        let content = parse_content(object.get("content"))?;
        let fields = match object.get("fields") {
            None => None,
            Some(Value::Object(fields)) => {
                if json_bytes(&Value::Object(fields.clone()))? > MAX_FIELDS_BYTES {
                    return Err(invalid("fields is larger than a call may carry"));
                }
                Some(fields.clone())
            }
            Some(_) => return Err(invalid("fields must be an object")),
        };
        let deadline = match object.get("deadline") {
            None => None,
            Some(Value::String(text)) => {
                let deadline = DateTime::parse_from_rfc3339(text)
                    .map_err(|_| {
                        ChannelCallError::new(DEADLINE_INVALID, "deadline must be RFC 3339")
                    })?
                    .with_timezone(&Utc);
                if deadline <= now || deadline >= run_deadline {
                    return Err(ChannelCallError::new(
                        DEADLINE_INVALID,
                        "deadline must be in the future and earlier than this run's deadline",
                    ));
                }
                Some(deadline)
            }
            Some(_) => {
                return Err(ChannelCallError::new(
                    DEADLINE_INVALID,
                    "deadline must be RFC 3339",
                ));
            }
        };
        Ok(Self {
            channel,
            content,
            fields,
            deadline,
        })
    }

    /// The child's input (01 A.5): the canonical source message the runtime stored for this call,
    /// the calling agent's address as sender, the content and the validated intake fields. No
    /// source id, sender or other identity comes from the model.
    pub fn trigger_input(
        &self,
        source_message: crate::domain::entities::message::CanonicalMessageId,
        caller_address: &str,
    ) -> super::triggers::MessageTriggerInput {
        super::triggers::MessageTriggerInput::new(
            source_message,
            self.content.body_text(),
            self.content.subject(),
            caller_address,
        )
        .with_fields(self.fields.clone())
    }

    /// The child's run deadline (03 A.5): the earlier of the caller's and the call's own.
    pub fn child_deadline(&self, run_deadline: DateTime<Utc>) -> DateTime<Utc> {
        self.deadline
            .map_or(run_deadline, |own| own.min(run_deadline))
    }
}

fn parse_content(value: Option<&Value>) -> Result<CanonicalContent, ChannelCallError> {
    let invalid = |message: &str| ChannelCallError::new(INVALID_INPUT, message);
    let content = value
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("content must be an object with a body"))?;
    if content
        .keys()
        .any(|key| !matches!(key.as_str(), "subject" | "body"))
    {
        return Err(invalid("content takes only subject and body"));
    }
    let text = |key: &str| match content.get(key) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.as_str())),
        Some(_) => Err(invalid("subject and body must be strings")),
    };
    let body = text("body")?.ok_or_else(|| invalid("content needs a body"))?;
    if body.len() > MESSAGE_INPUT_BODY_BYTES {
        return Err(invalid("body is longer than a call may carry"));
    }
    let content = CanonicalContent::parse(text("subject")?.unwrap_or_default(), body)
        .map_err(|_| invalid("subject or body exceeds its bound"))?;
    // Emptiness is judged on the canonical text: invisible characters are stripped first.
    if content.body_text().trim().is_empty() {
        return Err(invalid("content needs a body"));
    }
    Ok(content)
}

/// The digest binding a saved call's arguments to its invocation record, so a reused call ID
/// with other arguments is refused rather than replayed (02 A.3).
pub fn arguments_digest(arguments: &Value) -> AppResult<String> {
    super::canonical::digest(arguments)
}

// ---------------------------------------------------------------------------------------------
// The dispatch port (gate AWC-P03-D1 r2 D1).

/// One saved model call of `channel.call`: the origin the agent runtime claims, which the
/// dispatch proves against the run's manifest and the harness checkpoint, and the call.
#[derive(Debug, Clone)]
pub struct ChannelCallToolCall {
    pub origin: crate::application::agent::AgentToolOrigin,
    pub call: crate::domain::workflow::ToolCallId,
    pub arguments: Value,
}

/// Starts the call's child and parks the agent job, or answers a call that is refused or
/// whose child already finished. Never `Uncertain`: record, child, wait and park commit
/// together.
#[async_trait::async_trait]
pub trait ChannelCallDispatch: Send + Sync {
    async fn call_channel(
        &self,
        fence: super::lease::WorkflowFence,
        call: ChannelCallToolCall,
    ) -> AppResult<super::agent::AgentToolObservation>;
}

// ---------------------------------------------------------------------------------------------
// D2 — the result.

/// How the destination answered. The closure is the result kind (gate D2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelCallOutcome {
    Done {
        child: RunId,
        reply: CallReply,
    },
    HandedOff {
        child: RunId,
        to: RunId,
        reply: CallReply,
    },
    Denied {
        code: DeniedCode,
        reason: String,
        /// `None` when destination admission declined before any child existed.
        launched: Option<(RunId, CallReply)>,
    },
    NoReply {
        child: RunId,
    },
}

/// The approved response content and its typed result fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallReply {
    pub content: CanonicalContent,
    pub fields: Option<Map<String, Value>>,
}

/// Why a destination declined: the procedure's own `channel.deny`, or one of the destination
/// admission refusals of D1.1 steps 5 and 8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeniedCode {
    Procedure,
    UnitArchived,
    DeskDisabled,
    NoResponder,
    DestinationUnavailable,
    UnitBudgetExhausted,
}

impl DeniedCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Procedure => DENIED,
            Self::UnitArchived => AppError::UNIT_ARCHIVED,
            Self::DeskDisabled => DESK_DISABLED,
            Self::NoResponder => NO_RESPONDER,
            Self::DestinationUnavailable => DESTINATION_UNAVAILABLE,
            Self::UnitBudgetExhausted => crate::app_error::BudgetExhausted::CODE,
        }
    }

    /// The fixed reason a destination refusal gives; it names no configuration.
    pub const fn admission_reason(self) -> &'static str {
        match self {
            Self::Procedure => "The desk declined this request",
            Self::UnitArchived => "The desk's unit is archived",
            Self::DeskDisabled => "The desk is disabled",
            Self::NoResponder => "The desk has no responder for requests",
            Self::DestinationUnavailable => "The desk cannot take requests right now",
            Self::UnitBudgetExhausted => "The desk's unit has used its spend cap",
        }
    }
}

impl ChannelCallOutcome {
    /// The closure name the stored result carries (AWC-P04-D1 r2 D7).
    pub const fn closure(&self) -> &'static str {
        match self {
            Self::Done { .. } => "done",
            Self::HandedOff { .. } => "handed_off",
            Self::Denied { .. } => "denied",
            Self::NoReply { .. } => "no_reply",
        }
    }

    /// A destination admission refusal: a denied closure with no child (D1.1 steps 5 and 8).
    pub fn refused(code: DeniedCode) -> Self {
        Self::Denied {
            code,
            reason: code.admission_reason().to_owned(),
            launched: None,
        }
    }

    /// The tool result value. Refused when it would exceed a tool result's bound, so the
    /// caller reports `workflow.tool_result_too_large` instead.
    pub fn to_value(&self) -> AppResult<Value> {
        let value = match self {
            Self::Done { child, reply } => {
                let mut value = json!({"closure":"done","child_run_id":child.as_uuid()});
                reply.write(&mut value);
                value
            }
            Self::HandedOff { child, to, reply } => {
                let mut value = json!({"closure":"handed_off","child_run_id":child.as_uuid(),
                    "run_id":to.as_uuid()});
                reply.write(&mut value);
                value
            }
            Self::Denied {
                code,
                reason,
                launched,
            } => {
                let mut value = json!({"closure":"denied","code":code.as_str(),
                    "reason":bounded_bytes(reason, MAX_REASON_BYTES)});
                if let Some((child, reply)) = launched {
                    value["child_run_id"] = json!(child.as_uuid());
                    reply.write(&mut value);
                }
                value
            }
            Self::NoReply { child } => json!({"closure":"no_reply","child_run_id":child.as_uuid()}),
        };
        if serde_json::to_vec(&value).map_err(|_| invalid())?.len() > MAX_TOOL_RESULT_BYTES {
            return Err(AppError::BadRequest(
                "The channel call result is larger than a tool result may be".into(),
            ));
        }
        Ok(value)
    }
}

impl CallReply {
    fn write(&self, value: &mut Value) {
        value["content"] =
            json!({"subject":self.content.subject(),"body":self.content.body_text()});
        if let Some(fields) = &self.fields {
            value["fields"] = Value::Object(fields.clone());
        }
    }
}

/// A typed error the model receives instead of a result (`tools::tool_error` shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelCallError {
    pub code: &'static str,
    pub message: String,
    /// `intake_invalid` only: the intake schema and the failing instance paths.
    pub intake: Option<(Value, Vec<String>)>,
}

impl ChannelCallError {
    pub fn new(code: &'static str, message: &str) -> Self {
        Self {
            code,
            message: bounded(message, MAX_MESSAGE_CHARS),
            intake: None,
        }
    }

    pub fn to_value(&self) -> Value {
        let mut value = super::tools::tool_error(self.code, &self.message);
        if let Some((schema, paths)) = &self.intake {
            value["error"]["schema"] = schema.clone();
            value["error"]["paths"] = json!(paths);
        }
        value
    }
}

// ---------------------------------------------------------------------------------------------
// D1.1 — the ordered admission policy over facts persistence reads.

/// What persistence knows about the destination desk, read in the dispatch transaction.
/// `None` for a desk that does not exist in the calling company.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeskFacts {
    /// The calling agent's principal passes the desk's internal-relay participant rule (U2).
    pub writable: bool,
    /// The desk is already in the calling run's active channel-call ancestry.
    pub in_ancestry: bool,
    pub unit_archived: bool,
    pub enabled: bool,
    /// A selected responder that is active, message-triggered and inbox-associated.
    pub responder: bool,
}

/// What a target check decides. A later check runs only when every earlier one passed, so no
/// answer more specific than `target_unavailable` reaches a caller that may not write (I2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetDecision {
    Proceed,
    /// The caller must change the call.
    Error(&'static str),
    /// The destination declined; a denied closure with no child.
    Denied(DeniedCode),
}

/// D1.1 steps 3–5.
pub fn decide_target(desk: Option<DeskFacts>) -> TargetDecision {
    let Some(desk) = desk.filter(|desk| desk.writable) else {
        return TargetDecision::Error(TARGET_UNAVAILABLE);
    };
    if desk.in_ancestry {
        return TargetDecision::Error(CYCLE);
    }
    if desk.unit_archived {
        return TargetDecision::Denied(DeniedCode::UnitArchived);
    }
    if !desk.enabled {
        return TargetDecision::Denied(DeniedCode::DeskDisabled);
    }
    if !desk.responder {
        return TargetDecision::Denied(DeniedCode::NoResponder);
    }
    TargetDecision::Proceed
}

/// D1.1 step 6: the call's `fields` against the destination's captured intake
/// (`input_schema.properties.fields`), before any row exists. The outer error is a defect: the
/// intake was bounded and compiled when the responder's binding was activated.
pub fn check_intake(
    intake: Option<&Value>,
    fields: Option<&Map<String, Value>>,
) -> AppResult<Result<(), ChannelCallError>> {
    let (intake, fields) = match (intake, fields) {
        (_, None) => return Ok(Ok(())),
        (None, Some(_)) => {
            return Ok(Err(ChannelCallError::new(
                INTAKE_NOT_DECLARED,
                "This desk declares no intake; send the request without fields",
            )));
        }
        (Some(intake), Some(fields)) => (intake, fields),
    };
    let schema = Schema::compile(intake.clone(), "/input_schema/properties/fields", SPAN)
        .map_err(|_| AppError::Internal("A captured intake schema does not compile".into()))?;
    let paths = schema.failing_paths(&Value::Object(fields.clone()), MAX_INTAKE_PATHS);
    if paths.is_empty() {
        return Ok(Ok(()));
    }
    Ok(Err(ChannelCallError {
        intake: Some((
            intake.clone(),
            paths
                .into_iter()
                .map(|path| bounded(&path, MAX_PATH_CHARS))
                .collect(),
        )),
        ..ChannelCallError::new(
            INTAKE_INVALID,
            "fields does not match this desk's intake; correct the listed paths and call again",
        )
    }))
}

// ---------------------------------------------------------------------------------------------
// D3 — the declared intake and result fields.

/// The most properties an intake or result-fields schema declares.
pub const MAX_FIELDS_PROPERTIES: usize = 32;
/// The largest serialized intake or result-fields schema.
pub const MAX_FIELDS_SCHEMA_BYTES: usize = 16 * 1024;
const MAX_DESCRIPTION_CHARS: usize = 1000;
const STRING_KEYWORDS: [&str; 6] = [
    "type",
    "description",
    "minLength",
    "maxLength",
    "pattern",
    "format",
];
const NUMBER_KEYWORDS: [&str; 4] = ["type", "description", "minimum", "maximum"];
const ARRAY_KEYWORDS: [&str; 6] = [
    "type",
    "description",
    "items",
    "minItems",
    "maxItems",
    "uniqueItems",
];

/// A version's declared `fields` sub-schema: `properties.fields` of an object schema.
pub fn declared_fields(schema: Option<&Value>) -> Option<&Value> {
    schema?.get("properties")?.get("fields")
}

/// Whether an input schema makes `fields` required; mail cannot supply it (01 C.1).
pub fn fields_required(schema: Option<&Value>) -> bool {
    schema
        .and_then(|schema| schema.get("required"))
        .and_then(Value::as_array)
        .is_some_and(|required| required.iter().any(|name| name == "fields"))
}

/// One bound for intake and result fields (01 C.2): an object of at most 32 described
/// properties, each a string, number, boolean, string enum or array of strings, within
/// 16 KiB. `Err` names the failing path below the sub-schema and says why.
pub fn check_fields_schema(schema: &Value) -> Result<(), (String, &'static str)> {
    let at = |path: &str, why| Err((path.to_owned(), why));
    if serde_json::to_vec(schema).map_or(true, |bytes| bytes.len() > MAX_FIELDS_SCHEMA_BYTES) {
        return at("", "is larger than 16 KiB");
    }
    let Some(object) = schema.as_object() else {
        return at("", "must be an object schema");
    };
    if object.get("type").and_then(Value::as_str) != Some("object") {
        return at("/type", "must be \"object\"");
    }
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "type" | "description" | "properties" | "required" | "additionalProperties"
        ) {
            return at(&format!("/{key}"), "is not allowed in a fields schema");
        }
    }
    if object
        .get("additionalProperties")
        .is_some_and(|value| !value.is_boolean())
    {
        return at("/additionalProperties", "must be true or false");
    }
    let Some(properties) = object.get("properties").and_then(Value::as_object) else {
        return at("/properties", "must name the fields");
    };
    if properties.is_empty() || properties.len() > MAX_FIELDS_PROPERTIES {
        return at("/properties", "must declare between 1 and 32 fields");
    }
    if let Some(required) = object.get("required") {
        let names = required.as_array().filter(|names| {
            names.iter().all(|name| {
                name.as_str()
                    .is_some_and(|name| properties.contains_key(name))
            })
        });
        if names.is_none() {
            return at("/required", "must list declared fields");
        }
    }
    for (name, property) in properties {
        check_field(property).map_err(|(path, why)| (format!("/properties/{name}{path}"), why))?;
    }
    Ok(())
}

fn check_field(property: &Value) -> Result<(), (String, &'static str)> {
    let at = |path: &str, why| Err((path.to_owned(), why));
    let Some(object) = property.as_object() else {
        return at("", "must be a schema object");
    };
    if !object
        .get("description")
        .and_then(Value::as_str)
        .is_some_and(|text| {
            !text.trim().is_empty() && text.chars().count() <= MAX_DESCRIPTION_CHARS
        })
    {
        return at("/description", "every field needs a description");
    }
    if let Some(values) = object.get("enum") {
        let strings = values
            .as_array()
            .is_some_and(|values| !values.is_empty() && values.iter().all(Value::is_string));
        if !strings {
            return at("/enum", "must list strings");
        }
        return only(object, &["description", "enum", "type"]).and_then(|()| {
            match object.get("type") {
                None => Ok(()),
                Some(kind) if kind == "string" => Ok(()),
                Some(_) => at("/type", "an enum field is a string"),
            }
        });
    }
    match object.get("type").and_then(Value::as_str) {
        Some("string") => only(object, &STRING_KEYWORDS),
        Some("number") => only(object, &NUMBER_KEYWORDS),
        Some("boolean") => only(object, &["type", "description"]),
        Some("array") => {
            only(object, &ARRAY_KEYWORDS)?;
            let items = object.get("items").and_then(Value::as_object);
            let strings = items.is_some_and(|items| {
                items.get("type").and_then(Value::as_str) == Some("string")
                    && items.keys().all(|key| {
                        matches!(key.as_str(), "type" | "minLength" | "maxLength" | "pattern")
                    })
            });
            if strings {
                Ok(())
            } else {
                at("/items", "an array field holds strings")
            }
        }
        _ => at(
            "/type",
            "must be string, number, boolean, a string enum or an array of strings",
        ),
    }
}

fn only(object: &Map<String, Value>, allowed: &[&str]) -> Result<(), (String, &'static str)> {
    match object.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err((format!("/{key}"), "is not allowed for this field type")),
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------------------------
// D1.4 — levels.

/// The child's ceiling: the lowest of the calling run's ceiling, the request allowance and the
/// level of the destination binding's `run_as` person.
pub fn child_ceiling(
    caller: AccessLevel,
    allowance: AccessLevel,
    run_as: AccessLevel,
) -> AccessLevel {
    caller.min(allowance).min(run_as)
}

/// D1.1 step 7: the request carries the calling run's current content level; it is stored only
/// at or below the child's ceiling.
pub fn release_request(content: AccessLevel, ceiling: AccessLevel) -> Result<(), ChannelCallError> {
    if content > ceiling {
        return Err(ChannelCallError::new(
            AUDIENCE_REFUSED,
            "This run has read content the destination desk may not receive",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The trusted built-in snapshot.

/// The fixed (input, output) contract. The output is any closure or a typed error.
pub fn channel_call_schemas() -> (Value, Value) {
    let uuid = json!({"type":"string","format":"uuid","maxLength":36});
    let content = crate::application::workflow::registry::message_content_schema();
    let fields = json!({"type":"object"});
    let input = json!({
        "type":"object","additionalProperties":false,"required":["channel_id","content"],
        "properties":{
            "channel_id":uuid,
            "content":content,
            "fields":fields,
            "deadline":{"type":"string","format":"date-time","maxLength":64}
        }
    });
    let reply = json!({
        "content":content,"fields":fields
    });
    let closure = |kind: &str, required: &[&str], extra: Value| {
        let mut properties = json!({"closure":{"const":kind},"child_run_id":uuid});
        for (key, value) in reply.as_object().into_iter().flatten() {
            properties[key] = value.clone();
        }
        for (key, value) in extra.as_object().into_iter().flatten() {
            properties[key] = value.clone();
        }
        json!({"type":"object","additionalProperties":false,"required":required,"properties":properties})
    };
    let error = json!({
        "type":"object","additionalProperties":false,"required":["error"],
        "properties":{"error":{
            "type":"object","additionalProperties":false,"required":["code","message"],
            "properties":{"code":{"type":"string","minLength":1,"maxLength":128},
                "message":{"type":"string","minLength":1,"maxLength":MAX_MESSAGE_CHARS},
                "schema":{"type":"object"},
                "paths":{"type":"array","maxItems":MAX_INTAKE_PATHS,
                    "items":{"type":"string","maxLength":MAX_PATH_CHARS}}}
        }}
    });
    let output = json!({"oneOf":[
        closure("done", &["closure","child_run_id","content"], json!({})),
        closure("handed_off", &["closure","child_run_id","run_id","content"], json!({"run_id":uuid})),
        closure("denied", &["closure","code","reason"], json!({
            "code":{"type":"string","minLength":1,"maxLength":128},
            "reason":{"type":"string","minLength":1,"maxLength":MAX_REASON_BYTES}
        })),
        closure("no_reply", &["closure","child_run_id"], json!({})),
        error
    ]});
    (input, output)
}

pub fn builtin_channel_call_snapshot(company: CompanyId) -> AppResult<ToolSnapshot> {
    let name = TypeName::parse(CHANNEL_CALL).map_err(|_| invalid())?;
    let (input_schema, output_schema) = channel_call_schemas();
    Ok(ToolSnapshot {
        company_id: company,
        workflow: None,
        contract: ToolContract {
            name: name.clone(),
            connection: None,
            input_schema,
            output_schema,
        },
        policy: ApprovedActionPolicy {
            capability: name,
            policy_revision: 1,
            effect: ActionEffect::Write,
            recovery: ActionRecovery::Reconcile,
        },
    })
}

/// Whether a snapshot is exactly the trusted built-in.
pub fn is_trusted_channel_call_snapshot(snapshot: &ToolSnapshot) -> AppResult<bool> {
    if snapshot.kind() != ToolKind::ChannelCall {
        return Ok(false);
    }
    let trusted = builtin_channel_call_snapshot(snapshot.company_id)?;
    Ok(serde_json::to_value(snapshot).map_err(|_| invalid())?
        == serde_json::to_value(trusted).map_err(|_| invalid())?)
}

const SPAN: SourceSpan = SourceSpan {
    start: 0,
    end: 0,
    line: 1,
    column: 1,
};

fn invalid() -> AppError {
    AppError::Internal("Invalid built-in channel call contract".into())
}

fn json_bytes(value: &Value) -> Result<usize, ChannelCallError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| ChannelCallError::new(INVALID_INPUT, "The call is not valid JSON"))
}

fn bounded(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

fn bounded_bytes(text: &str, bytes: usize) -> &str {
    if text.len() <= bytes {
        return text;
    }
    let mut end = bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
#[path = "channel_call_tests.rs"]
mod tests;
