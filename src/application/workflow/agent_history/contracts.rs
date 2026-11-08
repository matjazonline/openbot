use super::{AgentConversationId, AgentExchangeId};
use crate::app_error::{AppError, AppResult};
use crate::application::agent::ModelSettings;
use crate::application::workflow::registry::ModelStep;
use crate::entities::access_level::AccessLevel;
use crate::entities::agent_capability::{AgentId, AgentRevisionId};
use crate::entities::response_contract::ResponseContract;
use serde::{Deserialize, Serialize, de::Error};
use serde_json::Value;

/// Within the existing native frozen-input/checkpoint budget; not a larger prompt budget.
pub const MAX_AGENT_HISTORY_VALUE_BYTES: usize = 1024 * 1024;

fn bounded(value: Value) -> AppResult<Value> {
    let bytes = serde_json::to_vec(&value)
        .map_err(|_| AppError::BadRequest("Invalid agent history value".into()))?;
    if bytes.len() > MAX_AGENT_HISTORY_VALUE_BYTES {
        return Err(AppError::BadRequest(
            "Agent history value exceeds its byte budget".into(),
        ));
    }
    Ok(value)
}

macro_rules! logical_value {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(Value);
        impl $name {
            pub fn new(value: Value) -> AppResult<Self> {
                bounded(value).map(Self)
            }
            pub fn value(&self) -> &Value {
                &self.0
            }
            pub fn into_value(self) -> Value {
                self.0
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
                Self::new(Value::deserialize(decoder)?).map_err(D::Error::custom)
            }
        }
    };
}

logical_value!(AgentLogicalInput);
logical_value!(AgentLogicalOutput);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExchangeState {
    Queued,
    Active,
    Completed,
    Failed,
    Cancelled,
}

impl TryFrom<String> for AgentExchangeState {
    type Error = AppError;
    fn try_from(value: String) -> AppResult<Self> {
        serde_json::from_value(Value::String(value))
            .map_err(|_| AppError::BadRequest("Invalid agent exchange state".into()))
    }
}

impl AgentExchangeState {
    pub fn permits(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (Self::Queued, Self::Active | Self::Failed | Self::Cancelled)
                    | (
                        Self::Active,
                        Self::Completed | Self::Failed | Self::Cancelled
                    )
            )
    }

    /// State eligibility only. The native safe control-retry owner proves this exception.
    pub fn can_control_retry(self) -> bool {
        self == Self::Failed
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// A durable conversation-local position, compatible with PostgreSQL's positive bigint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct AgentExchangeOrder(u64);
impl AgentExchangeOrder {
    pub fn get(self) -> u64 {
        self.0
    }
}
impl TryFrom<u64> for AgentExchangeOrder {
    type Error = AppError;
    fn try_from(value: u64) -> AppResult<Self> {
        if value == 0 || value > i64::MAX as u64 {
            return Err(AppError::BadRequest("Invalid agent exchange order".into()));
        }
        Ok(Self(value))
    }
}
impl From<AgentExchangeOrder> for u64 {
    fn from(value: AgentExchangeOrder) -> Self {
        value.get()
    }
}

/// Captured with the exchange; later schemas do not reinterpret this old output.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentExchangeProvenance {
    pub agent: AgentId,
    pub revision: AgentRevisionId,
    pub model: ModelSettings,
    pub output_contract: ResponseContract,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentExchangeOutcome {
    Queued,
    Active,
    Completed(AgentLogicalOutput),
    Failed,
    Cancelled,
}

impl AgentExchangeOutcome {
    pub fn state(&self) -> AgentExchangeState {
        match self {
            Self::Queued => AgentExchangeState::Queued,
            Self::Active => AgentExchangeState::Active,
            Self::Completed(_) => AgentExchangeState::Completed,
            Self::Failed => AgentExchangeState::Failed,
            Self::Cancelled => AgentExchangeState::Cancelled,
        }
    }
}

#[derive(Clone)]
pub struct AgentLogicalExchange {
    pub conversation: AgentConversationId,
    pub exchange: AgentExchangeId,
    pub order: AgentExchangeOrder,
    pub input: AgentLogicalInput,
    pub outcome: AgentExchangeOutcome,
    pub provenance: AgentExchangeProvenance,
    pub access_level: AccessLevel,
}

impl AgentLogicalExchange {
    /// Hidden and non-completed exchanges expose no prompt marker or count.
    pub fn completed_output_at(&self, ceiling: AccessLevel) -> Option<&AgentLogicalOutput> {
        if self.access_level > ceiling {
            return None;
        }
        match &self.outcome {
            AgentExchangeOutcome::Completed(output) => Some(output),
            _ => None,
        }
    }
}

pub fn agent_history_eligible(step: ModelStep) -> bool {
    match step {
        ModelStep::AgentRun => true,
        ModelStep::Classify | ModelStep::Decide => false,
    }
}
