//! The production tool capture for publication (p08 D1.8): the trusted message built-ins plus the
//! HTTP operations and MCP tools the publisher selected, merged into one dependency set. Agent
//! and child captures nest around it (`ChildPublicationDirectory` outermost, WF-C001 r2).
use super::http::{HttpPublicationDirectory, HttpPublicationSelection};
use super::mcp::{McpPublicationDirectory, McpPublicationSelection};
use super::*;
use crate::application::workflow::lifecycle::{PublicationDependencies, PublicationDirectory};
use crate::application::workflow::templates::CompanyWorkflowDraft;

pub struct ToolPublicationDirectory {
    pub persistence: PostgresPersistence,
    pub http: Vec<HttpPublicationSelection>,
    pub mcp: Vec<McpPublicationSelection>,
}

#[async_trait]
impl PublicationDirectory for ToolPublicationDirectory {
    async fn capture(
        &self,
        actor: WorkflowActor,
        draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        let message = message::MessagePublicationDirectory {
            persistence: self.persistence.clone(),
        }
        .capture(actor, draft)
        .await?;
        let http = HttpPublicationDirectory {
            persistence: self.persistence.clone(),
            selections: self.http.clone(),
        }
        .capture(actor, draft)
        .await?;
        let mcp = McpPublicationDirectory {
            persistence: self.persistence.clone(),
            selections: self.mcp.clone(),
        }
        .capture(actor, draft)
        .await?;
        let mut snapshots = message.snapshots;
        for captured in [http, mcp] {
            snapshots.tools.extend(captured.snapshots.tools);
        }
        Ok(PublicationDependencies {
            snapshots,
            children: Vec::new(),
        })
    }
}
