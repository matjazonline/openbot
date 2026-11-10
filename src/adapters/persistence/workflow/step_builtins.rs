//! Capture trusted step contracts without resolving a native provider or local machine.
use super::*;
use crate::application::local_agents::workflow::builtin_snapshot;
use crate::application::workflow::message::{MessageKind, builtin_message_snapshot};

pub struct StepBuiltinPublicationDirectory {
    pub persistence: PostgresPersistence,
}
#[async_trait]
impl PublicationDirectory for StepBuiltinPublicationDirectory {
    async fn capture(
        &self,
        actor: WorkflowActor,
        draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut tx = self.persistence.pool().begin().await?;
            super::authority::authorize_company(&mut tx, draft.company(), actor).await?;
            tx.commit().await?;
            dependencies(draft.company())
        })
        .await
        .map_err(|_| AppError::Timeout("Workflow step contract capture timed out".into()))?
    }
}
pub(super) fn dependencies(company: CompanyId) -> AppResult<PublicationDependencies> {
    Ok(PublicationDependencies {
        snapshots: publication::DependencySnapshots {
            tools: vec![
                builtin_message_snapshot(company, MessageKind::Send)?,
                builtin_message_snapshot(company, MessageKind::Reply)?,
                builtin_message_snapshot(company, MessageKind::Ask)?,
                builtin_snapshot(company)?,
            ],
            ..Default::default()
        },
        children: vec![],
    })
}
