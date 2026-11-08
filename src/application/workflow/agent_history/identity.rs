use super::super::{
    CompanyId, RelatedChannelId, RelatedThreadId, WorkflowActor,
    lease::{LeasePolicy, WorkflowFence},
};
use crate::app_error::AppResult;
use crate::domain::workflow::{ExecutionId, RunId};
use crate::entities::agent_capability::AgentId;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! history_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);
        impl $name {
            pub fn new(value: Uuid) -> Self {
                Self(value)
            }
            pub fn as_uuid(self) -> Uuid {
                self.0
            }
        }
    };
}

history_id!(AgentConversationId);
history_id!(AgentExchangeId);
history_id!(AgentPrivateSessionId);
history_id!(SampleConversationId);

impl AgentExchangeId {
    /// Attempts, repairs and continuations keep the same native activation.
    pub fn for_execution(execution: ExecutionId) -> Self {
        Self::new(execution.as_uuid())
    }
}

/// Normal authenticated admission metadata, never an `agent.run` input or override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentHistorySessionRef {
    pub from_run: RunId,
}

/// Only a trusted persistence resolver produces these scopes; no JSON decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentConversationBoundary {
    Thread {
        channel: RelatedChannelId,
        thread: RelatedThreadId,
    },
    PrivateSession {
        session: AgentPrivateSessionId,
    },
    ChildBranch {
        run: RunId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConversationIdentity {
    pub company: CompanyId,
    pub agent: AgentId,
    pub conversation: AgentConversationId,
    pub boundary: AgentConversationBoundary,
    /// Actual persisted execution actor, not a mutable binding's run-as setting.
    pub actor: WorkflowActor,
}

#[derive(Debug, Clone, Copy)]
pub struct ResolveAgentHistoryIdentity {
    pub fence: WorkflowFence,
    pub lease_policy: LeasePolicy,
}

#[async_trait]
pub trait AgentHistoryIdentity: Send + Sync {
    /// None means an authorized model-only step, never an authorization fallback.
    async fn resolve_agent_history_identity(
        &self,
        request: ResolveAgentHistoryIdentity,
    ) -> AppResult<Option<AgentConversationIdentity>>;
}
