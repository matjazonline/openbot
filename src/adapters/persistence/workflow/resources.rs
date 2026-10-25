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

pub(super) async fn check_readiness(
    tx: &mut Transaction<'_, Postgres>,
    configuration: &binding::ConfiguredBinding,
) -> AppResult<()> {
    let company = configuration.company_id();
    let definition = configuration.bundle().compiled().graph().definition();
    for tool in &configuration.bundle().snapshots().tools {
        let slot = match tool.kind() {
            ToolKind::Message => {
                authority::builtin_message_tool(tool)?;
                continue;
            }
            ToolKind::Provisioning => {
                authority::builtin_provisioning_tool(tool)?;
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
            .resources()
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
        .bundle()
        .compiled()
        .graph()
        .definition()
        .resources
    {
        let id = configuration.resources()[&requirement.slot];
        let status = inspect_on(tx, configuration.company_id(), id)
            .await?
            .ok_or_else(missing)?;
        validate_resource(configuration.company_id(), id, requirement, &status)?;
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
