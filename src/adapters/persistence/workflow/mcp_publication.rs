use super::*;
use crate::application::workflow::lifecycle::{PublicationDependencies, PublicationDirectory};
use crate::application::workflow::templates::CompanyWorkflowDraft;
use crate::domain::workflow::ResourceName;

/// Explicit authoring selections of approved MCP tools. Only a live approval on a ready
/// connection supplies the snapshot; names and slots here are requests, never policy.
pub struct McpPublicationDirectory {
    pub persistence: PostgresPersistence,
    pub selections: Vec<McpPublicationSelection>,
}
#[derive(Clone)]
pub struct McpPublicationSelection {
    pub connection: Uuid,
    pub slot: ResourceName,
    pub tool: TypeName,
}
#[async_trait]
impl PublicationDirectory for McpPublicationDirectory {
    async fn capture(
        &self,
        actor: WorkflowActor,
        draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        let mut tx = management::begin(&self.persistence, draft.company(), actor).await?;
        management::authorize_publication_on(&mut tx, draft.company(), actor, draft.workflow())
            .await?;
        let reach = crate::application::workflow::reads::PublicationReach {
            workflow: draft.workflow(),
            author: actor.user_id(),
        };
        let tools = capture_on(
            &mut tx,
            draft.company(),
            reach,
            draft.source(),
            &self.selections,
        )
        .await?;
        tx.commit().await?;
        Ok(PublicationDependencies {
            snapshots: publication::DependencySnapshots {
                tools,
                ..Default::default()
            },
            children: vec![],
        })
    }
}

pub(in crate::adapters::persistence::workflow) async fn capture_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    reach: crate::application::workflow::reads::PublicationReach,
    source: &str,
    selections: &[McpPublicationSelection],
) -> AppResult<Vec<publication::ToolSnapshot>> {
    if selections.len() > MAX_MCP_OPERATIONS {
        return Err(crate::application::workflow::mcp::invalid());
    }
    let mut tools = Vec::new();
    for selection in selections {
        let connection = connection_on(tx, company, selection.connection)
            .await?
            .ok_or_else(missing)?;
        if status_on(tx, &connection).await?.readiness != ResourceReadiness::Ready {
            return Err(missing());
        }
        crate::adapters::persistence::unit_resources::check_definition_reach_on(
            tx,
            company,
            reach,
            crate::application::ports::unit_resources::UnitResourceKind::McpConnection,
            connection.id,
            connection.unit_id,
            format!(
                "{} for tool {} (slot {}){}",
                connection.slug,
                selection.tool,
                selection.slot,
                super::super::resources::steps_using(
                    source,
                    Some(selection.slot.as_str()),
                    selection.tool.as_str(),
                )
            ),
        )
        .await?;
        let snapshot: Value = sqlx::query_scalar("SELECT snapshot FROM workflow_mcp_operations WHERE revoked_at IS NULL AND company_id=$1 AND connection_id=$2 AND name=$3")
                .bind(company.as_uuid()).bind(connection.id).bind(selection.tool.as_str()).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
        let mut tool: publication::ToolSnapshot =
            serde_json::from_value(snapshot).map_err(|_| invalid())?;
        let current = policy_on(tx, &connection, &tool).await?;
        tool = current.tool;
        tool.contract.connection = Some(selection.slot.clone());
        tools.push(tool);
    }
    Ok(tools)
}
