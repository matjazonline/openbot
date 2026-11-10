use super::*;
use crate::application::workflow::binding::{ResourceDirectory, ResourceStatus, validate_resource};

/// HTTP connections, company MCP connections and the company's built-in messaging resource
/// (identified by the company itself) are the supported resource owners. Supported contracts
/// are the currently approved operation names; a ready resource still grants nothing by itself.
pub(super) async fn inspect_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    id: RuntimeResourceId,
) -> AppResult<Option<ResourceStatus>> {
    if id.as_uuid() == company.as_uuid() {
        // The caller already holds the company authority this resource is owned by.
        return message::messaging_status(company).map(Some);
    }
    if let Some(connection) = http::connection_on(tx, company, id.as_uuid()).await? {
        return http::status_on(tx, &connection).await.map(Some);
    }
    if let Some(status) = jev::status_on(tx, company, id.as_uuid()).await? {
        return Ok(Some(status));
    }
    match mcp::connection_on(tx, company, id.as_uuid()).await? {
        Some(connection) => mcp::status_on(tx, &connection).await.map(Some),
        None => Ok(None),
    }
}

#[async_trait]
impl ResourceDirectory for PostgresPersistence {
    async fn inspect(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: RuntimeResourceId,
    ) -> AppResult<Option<ResourceStatus>> {
        let mut tx = self.pool.begin().await?;
        authority::authorize_company(&mut tx, company, actor).await?;
        let status = inspect_on(&mut tx, company, id).await?;
        tx.commit().await?;
        Ok(status)
    }
}

pub(super) struct ReadinessSelection<'a> {
    pub company: CompanyId,
    pub bundle: &'a PublishedBundle,
    pub resources:
        &'a std::collections::BTreeMap<crate::domain::workflow::ResourceName, RuntimeResourceId>,
}

impl<'a> From<&'a binding::ConfiguredBinding> for ReadinessSelection<'a> {
    fn from(binding: &'a binding::ConfiguredBinding) -> Self {
        Self {
            company: binding.company_id(),
            bundle: binding.bundle(),
            resources: binding.resources(),
        }
    }
}

pub(super) async fn check_readiness(
    tx: &mut Transaction<'_, Postgres>,
    configuration: ReadinessSelection<'_>,
) -> AppResult<()> {
    let company = configuration.company;
    let definition = configuration.bundle.compiled().graph().definition();
    for tool in &configuration.bundle.snapshots().tools {
        let slot = match tool.kind() {
            ToolKind::LocalAgent => {
                if !crate::application::local_agents::workflow::is_trusted_snapshot(tool)? {
                    return Err(missing());
                }
                continue;
            }
            ToolKind::Message => {
                authority::builtin_message_tool(tool)?;
                continue;
            }
            ToolKind::Provisioning => {
                authority::builtin_provisioning_tool(tool)?;
                continue;
            }
            ToolKind::Knowledge => {
                authority::builtin_knowledge_tool(tool)?;
                continue;
            }
            ToolKind::Memory => {
                authority::builtin_memory_tool(tx, tool, super::workflow_tools::ToolCheck::Run)
                    .await?;
                continue;
            }
            ToolKind::Http => {
                http::registered_on(tx, company, tool).await?;
                continue;
            }
            // A frozen pin runs while its name is registered for the same workflow (P07-Q2).
            ToolKind::Workflow(_) => {
                super::workflow_tools::registered_on(
                    tx,
                    company,
                    tool,
                    super::workflow_tools::ToolCheck::Run,
                )
                .await?;
                continue;
            }
            ToolKind::Slot(slot) => slot,
        };
        let id = configuration
            .resources
            .get(slot)
            .ok_or_else(missing)?
            .as_uuid();
        match slot_kind(definition, slot) {
            Some("http") => {
                let connection = http::connection_on(tx, company, id)
                    .await?
                    .ok_or_else(missing)?;
                http::policy_on(tx, connection.company, connection.id, tool).await?;
            }
            Some("mcp") => {
                let connection = mcp::connection_on(tx, company, id)
                    .await?
                    .ok_or_else(missing)?;
                mcp::policy_on(tx, &connection, tool).await?;
            }
            _ => return Err(missing()),
        }
    }
    for requirement in &configuration
        .bundle
        .compiled()
        .graph()
        .definition()
        .resources
    {
        let id = *configuration
            .resources
            .get(&requirement.slot)
            .ok_or_else(missing)?;
        let status = inspect_on(tx, configuration.company, id)
            .await?
            .ok_or_else(missing)?;
        validate_resource(configuration.company, id, requirement, &status)?;
    }
    jev_binding_models(tx, &configuration).await
}

async fn jev_binding_models(
    tx: &mut Transaction<'_, Postgres>,
    configuration: &ReadinessSelection<'_>,
) -> AppResult<()> {
    for (slot, models) in crate::application::workflow::jev::required_models(configuration.bundle)?
    {
        let id = configuration.resources.get(&slot).ok_or_else(missing)?;
        let connection = jev::connection_on(tx, configuration.company, id.as_uuid())
            .await?
            .ok_or_else(jev::revoked)?;
        if !connection.enabled || !models.is_subset(&connection.models) {
            return Err(jev::revoked());
        }
    }
    Ok(())
}

/// The declared resource kind of a tool's slot in this exact workflow definition.
pub(super) fn slot_kind<'a>(
    definition: &'a crate::domain::workflow::WorkflowDefinition,
    slot: &crate::domain::workflow::ResourceName,
) -> Option<&'a str> {
    definition
        .resources
        .iter()
        .find(|resource| &resource.slot == slot)
        .map(|resource| resource.kind.as_str())
}
