//! The runtimes an agent can actually be executed on.
//!
//! Each submodule implements [`AgentHarness`] for one of them: it takes the harness-neutral
//! [`AgentCapabilitySpec`] the application handed it, compiles it into whatever its runtime
//! accepts, runs the turn, and translates the runtime's callbacks back into the application's
//! ports. Nothing above this directory names a runtime.
//!
//! The registry these register *into* lives in `src/application/services/harness/`, not here:
//! `src/AGENTS.md` -- an abstraction must not live inside the outer adapter it is intended to
//! abstract.
//!
//! [`AgentHarness`]: crate::services::harness::AgentHarness
//! [`AgentCapabilitySpec`]: crate::entities::harness::AgentCapabilitySpec

pub mod ai_agents;
pub mod rig;

/// Construct exactly the deployment's supported runtimes. No tenant credentials or network probes
/// are involved: provider authentication and model availability are checked by real requests.
pub fn deployment_registry(
    default: crate::entities::harness::HarnessKind,
    capabilities: std::sync::Arc<dyn crate::use_cases::skill::AgentCapabilityReader>,
    mcp: std::sync::Arc<crate::services::mcp_runtime::McpRuntime>,
) -> anyhow::Result<crate::services::harness::HarnessRegistry> {
    use crate::{entities::harness::HarnessKind, services::harness::HarnessRegistry};
    use std::sync::Arc;
    let providers = rig::providers::ProviderRegistry::standard()?;
    let registry = HarnessRegistry::new()
        .register(
            HarnessKind::AiAgents,
            Arc::new(ai_agents::AiAgentsHarness::new()),
        )?
        .register(
            HarnessKind::Rig,
            Arc::new(rig::RigHarness::with_providers(
                providers,
                capabilities,
                mcp,
            )?),
        )?;
    registry.require(default)?;
    Ok(registry)
}
