use super::*;
use crate::application::workflow::binding::{
    ResourceDirectory, ResourceReadiness, ResourceStatus, validate_resource,
};
use crate::entities::mcp::McpAuth;
use std::collections::BTreeSet;

/// Only the existing MCP UUID owner is supported. A connection being ready
/// promises no tool policy or execution grant. Contract labels have no current
/// authority mapping, so supported_contracts intentionally remains empty.
pub(super) async fn inspect_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    id: RuntimeResourceId,
) -> AppResult<Option<ResourceStatus>> {
    let Some(connection) =
        super::super::mcp::connection_on(&mut *tx, company.as_uuid(), id.as_uuid()).await?
    else {
        return Ok(None);
    };
    let readiness = if !connection.enabled || connection.tool_grants.is_empty() {
        ResourceReadiness::Revoked
    } else if connection.auth == McpAuth::Bearer && !connection.secret_set {
        ResourceReadiness::Unavailable
    } else {
        ResourceReadiness::Ready
    };
    Ok(Some(ResourceStatus {
        id,
        company_id: company,
        kind: TypeName::parse("mcp").map_err(|_| invalid())?,
        supported_contracts: BTreeSet::new(),
        authorized: true,
        readiness,
    }))
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
