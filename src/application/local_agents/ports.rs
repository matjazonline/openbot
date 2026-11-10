use super::CapabilitySnapshot;
use crate::domain::local_agents::EventSequence;
use crate::{
    application::app_error::AppResult,
    domain::local_agents::{
        LocalAgentProfile, LocalAgentSession, LocalMachine, MachineId, ProfileId, SessionId,
    },
};
use async_trait::async_trait;
use uuid::Uuid;

/// Every lookup carries its company scope; application use cases also authorize the
/// session's original conversation/workflow association before returning its data.
#[async_trait]
pub trait LocalAgentRead: Send + Sync {
    async fn machine(&self, company_id: Uuid, id: MachineId) -> AppResult<Option<LocalMachine>>;
    async fn profile(
        &self,
        company_id: Uuid,
        id: ProfileId,
    ) -> AppResult<Option<LocalAgentProfile>>;
    async fn session(
        &self,
        company_id: Uuid,
        id: SessionId,
    ) -> AppResult<Option<LocalAgentSession>>;
    async fn capabilities(
        &self,
        company_id: Uuid,
        machine: MachineId,
        sequence: EventSequence,
    ) -> AppResult<Option<CapabilitySnapshot>>;
}
