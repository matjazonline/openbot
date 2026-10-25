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
        if self.selections.len() > MAX_MCP_OPERATIONS {
            return Err(crate::application::workflow::mcp::invalid());
        }
        let company = draft.company();
        let mut tx = begin_http(&self.persistence).await?;
        authority::authorize_company(&mut tx, company, actor).await?;
        let mut tools = Vec::new();
        for selection in &self.selections {
            let connection = connection_on(&mut tx, company, selection.connection)
                .await?
                .ok_or_else(missing)?;
            if status_on(&mut tx, &connection).await?.readiness != ResourceReadiness::Ready {
                return Err(missing());
            }
            let snapshot: Value = sqlx::query_scalar("SELECT snapshot FROM workflow_mcp_operations WHERE company_id=$1 AND connection_id=$2 AND name=$3")
                .bind(company.as_uuid()).bind(connection.id).bind(selection.tool.as_str()).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
            let mut tool: publication::ToolSnapshot =
                serde_json::from_value(snapshot).map_err(|_| invalid())?;
            let current = policy_on(&mut tx, &connection, &tool).await?;
            tool = current.tool;
            tool.contract.connection = Some(selection.slot.clone());
            tools.push(tool);
        }
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
