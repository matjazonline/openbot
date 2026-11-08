use crate::domain::integrations::{
    authority::{AuthorizationSnapshot, HostAuthorityFacts},
    *,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "Value", into = "Value")]
pub struct OperationArguments(Value);
impl OperationArguments {
    pub fn new(value: Value) -> IntegrationResult<Self> {
        if serde_json::to_vec(&value)
            .map_err(|_| IntegrationError::Invalid)?
            .len()
            > 16 * 1024
            || !value.is_object()
            || forbidden(&value)
        {
            return Err(IntegrationError::Invalid);
        }
        Ok(Self(value))
    }
    pub fn value(&self) -> &Value {
        &self.0
    }
}
// The operation may set only ordinary content/conditional headers. Every other
// header is adapter-owned, so adding a registered key cannot open an argument bypass.
pub(crate) fn is_operation_header(name: &str) -> bool {
    ["Accept", "Content-Type", "If-Match", "If-None-Match"]
        .iter()
        .any(|allowed| name.eq_ignore_ascii_case(allowed))
}
fn invalid_headers(value: &Value) -> bool {
    let Some(headers) = value.as_object() else {
        return true;
    };
    let mut seen = std::collections::BTreeSet::new();
    headers.iter().any(|(key, value)| {
        !seen.insert(key.to_ascii_lowercase())
            || !is_operation_header(key)
            || !value
                .as_str()
                .is_some_and(|text| text.len() <= 2048 && !text.chars().any(char::is_control))
    })
}
fn forbidden(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            if key.eq_ignore_ascii_case("headers") && invalid_headers(value) {
                return true;
            }
            matches!(
                key.to_ascii_lowercase().replace('-', "_").as_str(),
                "authorization"
                    | "proxy_authorization"
                    | "cookie"
                    | "set_cookie"
                    | "credential"
                    | "credentials"
                    | "injection"
                    | "auth_header"
                    | "api_key"
                    | "x_api_key"
                    | "access_token"
                    | "refresh_token"
            ) || forbidden(value)
        }),
        Value::Array(values) => values.iter().any(forbidden),
        _ => false,
    }
}
impl TryFrom<Value> for OperationArguments {
    type Error = IntegrationError;
    fn try_from(v: Value) -> IntegrationResult<Self> {
        Self::new(v)
    }
}
impl From<OperationArguments> for Value {
    fn from(v: OperationArguments) -> Value {
        v.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedInvocation {
    pub authorization: AuthorizationSnapshot,
    pub operation: OperationId,
    pub target: super::OperationTarget,
    pub arguments: OperationArguments,
    pub request_digest: [u8; 32],
    pub correlation: CorrelationId,
    pub dispatch: DispatchId,
    pub deadline: DateTime<Utc>,
    pub cancellation: CancellationId,
    pub host_authority: HostAuthorityReference,
}
/// Runtime-only context; wire protocols carry deadline and opaque cancellation ID.
#[derive(Clone)]
pub struct ExecutionContext {
    pub(crate) authority: std::sync::Arc<dyn DispatchAuthority>,
    pub(crate) dispatch: std::sync::Arc<dyn super::BusinessDispatch>,
    pub cancellation: CancellationToken,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "effect", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionOutcome {
    PreDispatch { failure: super::PreDispatchFailure },
    Committed { data: ProviderData },
    Uncertain,
}
impl<'de> Deserialize<'de> for ExecutionOutcome {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "effect", rename_all = "snake_case", deny_unknown_fields)]
        enum Fields {
            PreDispatch { failure: super::PreDispatchFailure },
            Committed { data: ProviderData },
            Uncertain {},
        }
        Ok(match Fields::deserialize(deserializer)? {
            Fields::PreDispatch { failure } => Self::PreDispatch { failure },
            Fields::Committed { data } => Self::Committed { data },
            Fields::Uncertain {} => Self::Uncertain,
        })
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ProviderData(Value);
impl ProviderData {
    pub fn value(&self) -> &Value {
        &self.0
    }
    pub fn from_redacted(value: Value) -> IntegrationResult<Self> {
        if serde_json::to_vec(&value)
            .map_err(|_| IntegrationError::Invalid)?
            .len()
            > 64 * 1024
        {
            return Err(IntegrationError::Limit);
        }
        Ok(Self(value))
    }
}
#[async_trait]
pub trait DispatchAuthority: Send + Sync {
    /// Re-read persisted host authority for this exact request after every wait,
    /// immediately before business dispatch. No successful default exists.
    async fn check(
        &self,
        invocation: &AuthorizedInvocation,
        credential_gate: &dyn CredentialDispatchGate,
    ) -> IntegrationResult<()>;
}

/// Called by the host while its current authority locks remain held. No secrets
/// or database transaction types cross this required application port.
#[async_trait]
pub trait CredentialDispatchGate: Send + Sync {
    async fn check(
        &self,
        invocation: &AuthorizedInvocation,
        host: HostAuthorityFacts,
    ) -> IntegrationResult<()>;
}

impl<'de> Deserialize<'de> for ProviderData {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_redacted(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
