use crate::app_error::{AppError, AppResult};
use crate::entities::agent_capability::AgentToolAlias;
use serde::{Deserialize, Serialize};

pub const MAX_AGENT_MODEL_TOOL_NAME_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentModelToolName(String);

impl AgentModelToolName {
    pub fn for_alias(alias: &AgentToolAlias) -> AppResult<Self> {
        Self::parse(alias.as_str().replace('.', "__")).map_err(AppError::BadRequest)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn parse(value: String) -> Result<Self, String> {
        if value.is_empty()
            || value.len() > MAX_AGENT_MODEL_TOOL_NAME_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err("Agent tool aliases must produce a model function name of 1–64 ASCII letters, digits, underscores or hyphens".into());
        }
        Ok(Self(value))
    }
}

impl<'de> Deserialize<'de> for AgentModelToolName {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(decoder)?).map_err(serde::de::Error::custom)
    }
}
