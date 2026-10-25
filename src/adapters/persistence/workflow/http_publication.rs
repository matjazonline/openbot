use super::*;
use crate::application::workflow::lifecycle::{PublicationDependencies, PublicationDirectory};
use crate::application::workflow::templates::CompanyWorkflowDraft;
use crate::domain::workflow::ResourceName;

/// Explicit authoring selections; only live persisted registrations can supply facts.
/// Names and slot aliases here are requests, never policies or credentials.
pub struct HttpPublicationDirectory {
    pub persistence: PostgresPersistence,
    pub selections: Vec<HttpPublicationSelection>,
}
#[derive(Clone)]
pub struct HttpPublicationSelection {
    pub connection: Uuid,
    pub slot: Option<ResourceName>,
    pub operation: TypeName,
}
#[async_trait]
impl PublicationDirectory for HttpPublicationDirectory {
    async fn capture(
        &self,
        actor: WorkflowActor,
        draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        if self.selections.len() > 256 {
            return Err(invalid());
        }
        let mut tx = begin_http(&self.persistence).await?;
        authority::authorize_company(&mut tx, draft.company(), actor).await?;
        let mut tools = Vec::new();
        for selection in &self.selections {
            let connection = connection_on(&mut tx, draft.company(), selection.connection)
                .await?
                .ok_or_else(missing)?;
            if !connection.enabled || (connection.bearer && !connection.secret_set) {
                return Err(missing());
            }
            let snapshot: Value = sqlx::query_scalar("SELECT snapshot FROM workflow_http_operations WHERE company_id=$1 AND connection_id=$2 AND name=$3")
                .bind(draft.company().as_uuid()).bind(connection.id).bind(selection.operation.as_str()).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
            let mut tool: publication::ToolSnapshot =
                serde_json::from_value(snapshot).map_err(|_| invalid())?;
            HttpOperation {
                tool: tool.clone(),
                approval_required: false,
            }
            .validate(draft.company())?;
            tool.contract.connection = selection.slot.clone();
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
