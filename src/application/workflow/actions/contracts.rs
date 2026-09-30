use crate::application::app_error::{AppError, AppResult};
use crate::domain::workflow::{ActionInvocationId, CompanyId, ExecutionId, RunId, TypeName};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionScope {
    pub company: CompanyId,
    pub run: RunId,
    pub execution: ExecutionId,
}

/// A company-owned resource identity, never a URL or credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionTarget {
    Connection {
        resource: Uuid,
        resource_kind: TypeName,
    },
    Local {
        resource: Uuid,
        resource_kind: TypeName,
    },
}

/// Supplied by the trusted execution boundary; frozen content remains subject to current access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionPolicyContext {
    pub actor: Uuid,
    pub capability_ceiling: Vec<TypeName>,
    pub approval_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ModelToolCallId(String);
impl ModelToolCallId {
    pub fn parse(value: impl Into<String>) -> AppResult<Self> {
        let value = value.into();
        if value.is_empty() || value.len() > 128 || !value.bytes().all(|c| (33..=126).contains(&c))
        {
            return Err(invalid());
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for ModelToolCallId {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(decoder)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalSubject {
    pub invocation: ActionInvocationId,
    pub argument_digest: ArgumentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ArgumentDigest(pub(super) String);
impl ArgumentDigest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for ArgumentDigest {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        let value = String::deserialize(decoder)?;
        if value.len() != 64
            || !value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(serde::de::Error::custom("Invalid action digest"));
        }
        Ok(Self(value))
    }
}

/// An immutable preparation result. This never carries successful tool output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionIntent {
    pub invocation: ActionInvocationId,
    pub argument_digest: ArgumentDigest,
    pub idempotency_key: ActionIdempotencyKey,
    pub decision: ActionPolicyDecision,
    pub replayed: bool,
}
impl ActionIntent {
    pub fn approval_subject(&self) -> ApprovalSubject {
        ApprovalSubject {
            invocation: self.invocation,
            argument_digest: self.argument_digest.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionPolicyDecision {
    Unevaluated,
    ApprovalRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionIdempotencyKey(pub(super) String);
impl ActionIdempotencyKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Future receipt records are authoritative effect evidence; checkpoint copies are replay data.
/// A missing receipt does not prove that a remote operation was not applied.
#[derive(Clone)]
pub struct ActionReceipt {
    pub subject: ApprovalSubject,
    pub result: Value,
}

pub(super) fn invalid() -> AppError {
    AppError::BadRequest("Invalid or oversized action operation".into())
}
