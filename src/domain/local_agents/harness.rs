use super::PayloadDigest;
use crate::entities::agent_capability::{AgentId, AgentRevisionId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NativeBundleDigest(PayloadDigest);

impl NativeBundleDigest {
    pub fn of(bytes: &[u8]) -> Self {
        Self(PayloadDigest::try_from(format!("{:x}", Sha256::digest(bytes))).expect("SHA256 hex"))
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
impl TryFrom<String> for NativeBundleDigest {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(Self(value.try_into()?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeHarnessSelection {
    Unconfigured,
    Agent {
        agent_id: AgentId,
        revision_id: AgentRevisionId,
        digest: NativeBundleDigest,
    },
}
