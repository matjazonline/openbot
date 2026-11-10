//! One deployment instance of each provider client.
use super::*;
pub(super) struct Clients {
    pub(super) mcp: Arc<dyn crate::services::mcp_client::McpClient>,
    pub(super) mcp_use_cases: Arc<crate::use_cases::mcp::McpUseCases>,
    pub(super) resend: Arc<CompanyResendApiClients>,
}
pub(super) fn build(
    f: &foundation::Foundation,
    memory: Option<&crate::infra::memory_config::MemoryServiceConfig>,
) -> anyhow::Result<Clients> {
    let mut policy = crate::adapters::mcp::policy::EndpointPolicy::new(
        std::env::var("MCP_INTERNAL_ENDPOINTS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
    )?;
    // No company MCP connection may reach the memory service (phase 05 E5).
    if let Some(memory) = memory {
        policy = policy.reserving(&memory.base_url)?;
    }
    let mcp = Arc::new(crate::adapters::mcp::HttpMcpClient::new(policy));
    let p = &f.persistence;
    let mcp_use_cases = Arc::new(crate::use_cases::mcp::McpUseCases::new(
        p.clone(),
        p.clone(),
        p.clone(),
        mcp.clone(),
    ));
    let resend = Arc::new(CompanyResendApiClients::new(
        p.clone(),
        f.config.resend_api.clone(),
    )?);
    Ok(Clients {
        mcp,
        mcp_use_cases,
        resend,
    })
}
