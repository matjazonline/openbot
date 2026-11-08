//! One deployment instance of each provider client.
use super::*;
pub(super) struct Clients {
    pub(super) integrations: Arc<dyn crate::application::integrations::IntegrationGateway>,
    pub(super) integration_bindings:
        Arc<dyn crate::application::use_cases::integration_binding::IntegrationBindings>,
    pub(super) mcp: Arc<dyn crate::services::mcp_client::McpClient>,
    pub(super) mcp_use_cases: Arc<crate::use_cases::mcp::McpUseCases>,
    pub(super) resend: Arc<CompanyResendApiClients>,
}
pub(super) fn build(f: &foundation::Foundation) -> anyhow::Result<Clients> {
    let policy = crate::adapters::mcp::policy::EndpointPolicy::new(
        std::env::var("MCP_INTERNAL_ENDPOINTS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
    )?;
    let mcp = Arc::new(crate::adapters::mcp::HttpMcpClient::new(policy));
    let p = &f.persistence;
    let services = crate::infra::integrations::gateway(p, &f.config, mcp.clone())?;
    let mcp_use_cases = Arc::new(crate::use_cases::mcp::McpUseCases::new(
        p.clone(),
        p.clone(),
        p.clone(),
        mcp.clone(),
        services.discovery,
    ));
    let resend = Arc::new(CompanyResendApiClients::new(
        p.clone(),
        f.config.resend_api.clone(),
    )?);
    Ok(Clients {
        integrations: services.gateway,
        integration_bindings: services.bindings,
        mcp,
        mcp_use_cases,
        resend,
    })
}
