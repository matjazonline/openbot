use super::bounds::{MAX_MODELS_PER_PROVIDER, MAX_PROJECTS, MAX_PROVIDERS};
use crate::{
    application::transport::bounded::BoundedVec,
    domain::local_agents::{
        CloudroomProjectId, EventSequence, ExecutionSelection, InstallationId, LocalProvider,
        LocalReasoningLevel, MachineId, ModelId,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalCatalog {
    pub projects: BoundedVec<CloudroomProjectId, MAX_PROJECTS>,
    pub providers: BoundedVec<ProviderCatalog, MAX_PROVIDERS>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalog {
    pub provider: LocalProvider,
    pub models: BoundedVec<ModelCapabilities, MAX_MODELS_PER_PROVIDER>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilities {
    pub model: ModelId,
    pub reasoning_levels: BoundedVec<LocalReasoningLevel, 8>,
}

impl LocalCatalog {
    pub fn validate_selection(&self, selection: &ExecutionSelection) -> Result<(), &'static str> {
        if !self.projects.contains(&selection.project_id) {
            return Err("project is not advertised");
        }
        let model = self
            .providers
            .iter()
            .filter(|provider| provider.provider == selection.provider)
            .flat_map(|provider| provider.models.iter())
            .find(|model| model.model == selection.model)
            .ok_or("exact provider/model is not advertised")?;
        if !model.reasoning_levels.contains(&selection.policy.reasoning) {
            return Err("reasoning level is not supported by this model");
        }
        Ok(())
    }
}

/// A machine's capabilities_sequence references this accepted installation-scoped snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilitySnapshot {
    pub machine_id: MachineId,
    pub installation_id: InstallationId,
    pub sequence: EventSequence,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub catalog: LocalCatalog,
}
