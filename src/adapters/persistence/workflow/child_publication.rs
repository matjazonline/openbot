use super::http::begin_http;
use super::*;
use crate::application::workflow::lifecycle::{PublicationDependencies, PublicationDirectory};
use std::collections::BTreeSet;

/// Wraps the other publication directories and captures the pinned child bundles a draft
/// declares (07-D1 §D1.1) and the workflow tools its agents were granted pin (07-D2 §D2.2):
/// selectable versions of the same company only, at most 256 in all, and no more bytes in total
/// than one bundle may hold, checked before any bundle is read. A missing or archived child is
/// left out, so validation reports `publication.missing`. Every production `DefinitionService`
/// composition must include it, outside `AgentPublicationDirectory` (notice WF-C001 r2).
pub struct ChildPublicationDirectory<D> {
    pub persistence: PostgresPersistence,
    pub inner: D,
}

#[async_trait]
impl<D: PublicationDirectory> PublicationDirectory for ChildPublicationDirectory<D> {
    async fn capture(
        &self,
        actor: WorkflowActor,
        draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        let mut captured = self.inner.capture(actor, draft).await?;
        // A draft that does not parse captures nothing here; compilation reports it with spans.
        let Ok(decoded) = crate::adapters::workflow_source::decode(draft.source()) else {
            return Ok(captured);
        };
        let Ok(parsed) = crate::application::workflow::compiler::parse_workflow(
            &decoded,
            VersionId::new(Uuid::nil()),
        ) else {
            return Ok(captured);
        };
        let tools = captured
            .snapshots
            .tools
            .iter()
            .filter_map(|tool| match tool.kind() {
                ToolKind::Workflow(pin) => {
                    Some((pin.workflow_id.as_uuid(), pin.version_id.as_uuid()))
                }
                _ => None,
            });
        let pinned: BTreeSet<(Uuid, Uuid)> = parsed
            .controls
            .values()
            .map(|control| {
                (
                    control.child_workflow_id.as_uuid(),
                    control.child_version_id.as_uuid(),
                )
            })
            .chain(tools)
            .collect();
        if pinned.is_empty() {
            return Ok(captured);
        }
        if pinned.len() > MAX_DEPENDENCIES {
            return Err(AppError::BadRequest(
                "At most 256 child bundles are allowed".into(),
            ));
        }
        let (workflows, versions): (Vec<Uuid>, Vec<Uuid>) = pinned.into_iter().unzip();
        let mut tx = begin_http(&self.persistence).await?;
        authority::authorize_company(&mut tx, draft.company(), actor).await?;
        let selectable: Vec<(Uuid, i32)> = sqlx::query_as(
            "SELECT version.id, octet_length(version.bundle) \
             FROM unnest($2::uuid[], $3::uuid[]) AS pinned(workflow_id, id) \
             JOIN workflow_versions AS version ON version.company_id = $1 \
               AND version.workflow_id = pinned.workflow_id AND version.id = pinned.id \
             JOIN workflow_definitions AS definition ON definition.company_id = version.company_id \
               AND definition.id = version.workflow_id \
             WHERE NOT definition.archived",
        )
        .bind(draft.company().as_uuid())
        .bind(&workflows)
        .bind(&versions)
        .fetch_all(&mut *tx)
        .await?;
        within_bundle_bound(selectable.iter().map(|(_, bytes)| *bytes))?;
        let ids: Vec<Uuid> = selectable.into_iter().map(|(id, _)| id).collect();
        let bundles: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT bundle FROM workflow_versions WHERE company_id = $1 AND id = ANY($2) ORDER BY id",
        )
        .bind(draft.company().as_uuid())
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        for bytes in bundles {
            captured.children.push(restore_bundle(
                &bytes,
                &crate::adapters::workflow_source::WorkflowSourceDecoder,
            )?);
        }
        Ok(captured)
    }
}

/// Refuse pinned children whose stored bundles together exceed what one bundle may hold, before
/// any of them is read.
pub(super) fn within_bundle_bound(sizes: impl IntoIterator<Item = i32>) -> AppResult<()> {
    let total = sizes
        .into_iter()
        .map(|bytes| usize::try_from(bytes).unwrap_or(usize::MAX))
        .fold(0, usize::saturating_add);
    if total > MAX_BUNDLE_BYTES {
        return Err(AppError::BadRequest(
            "Pinned child bundles exceed the bundle size limit".into(),
        ));
    }
    Ok(())
}
