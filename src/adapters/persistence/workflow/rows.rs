use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredOrigin {
    format: u8,
    template: TemplateId,
    revision: u64,
    source_identity: String,
}
pub(super) fn origin_json(draft: &CompanyWorkflowDraft) -> AppResult<Option<Value>> {
    draft
        .origin()
        .map(|origin| {
            serde_json::to_value(StoredOrigin {
                format: 1,
                template: origin.template().clone(),
                revision: origin.revision().get(),
                source_identity: origin.source_identity().as_str().into(),
            })
            .map_err(|_| invalid())
        })
        .transpose()
}

#[derive(sqlx::FromRow)]
pub(super) struct DraftRow {
    pub company_id: Uuid,
    pub workflow_id: Uuid,
    pub revision: i64,
    pub title: String,
    pub description: String,
    pub source: String,
    pub template_origin: Option<Value>,
    pub archived: bool,
}
impl DraftRow {
    pub fn restore(self) -> AppResult<DraftState> {
        let origin = self
            .template_origin
            .map(|value| {
                let origin: StoredOrigin = serde_json::from_value(value).map_err(|_| invalid())?;
                if origin.format != 1 {
                    return Err(invalid());
                }
                TemplateOrigin::restore(
                    origin.template,
                    TemplateRevision::new(origin.revision).ok_or_else(invalid)?,
                    origin.source_identity,
                )
            })
            .transpose()?;
        if self.title.trim().is_empty()
            || self.title.len() > MAX_TITLE_BYTES
            || self.description.len() > MAX_DESCRIPTION_BYTES
            || self.source.len() > compiler::MAX_SOURCE_BYTES
        {
            return Err(invalid());
        }
        Ok(DraftState {
            draft: CompanyWorkflowDraft::saved(
                CompanyId::new(self.company_id),
                WorkflowId::new(self.workflow_id),
                DraftRevision::new(u64::try_from(self.revision).map_err(|_| invalid())?)
                    .ok_or_else(invalid)?,
                DraftContent {
                    title: self.title,
                    description: self.description,
                    source: self.source,
                },
                origin,
            ),
            archived: self.archived,
        })
    }
}
pub(super) const DRAFT_COLUMNS: &str = "company_id, id AS workflow_id, revision, title, description, source, template_origin, archived";

#[derive(sqlx::FromRow)]
pub(super) struct VersionRow {
    company_id: Uuid,
    workflow_id: Uuid,
    id: Uuid,
    draft_revision: i64,
    command_key: String,
    actor_id: Uuid,
    title: String,
    description: String,
    source: String,
    template_origin: Option<Value>,
    bundle: Vec<u8>,
    content_hash: String,
}
pub(super) const VERSION_COLUMNS: &str = "company_id, workflow_id, id, draft_revision, \
    command_key, actor_id, title, description, source, template_origin, bundle, content_hash";
impl VersionRow {
    pub fn restore(self) -> AppResult<PublishedCommand> {
        let bundle = restore_bundle(
            &self.bundle,
            &crate::adapters::workflow_source::WorkflowSourceDecoder,
        )?;
        let definition = bundle.compiled().graph().definition();
        if bundle.company_id().as_uuid() != self.company_id
            || definition.workflow_id.as_uuid() != self.workflow_id
            || definition.version_id.as_uuid() != self.id
            || bundle.content_hash().as_str() != self.content_hash
            || bundle.compiled().source() != self.source
        {
            return Err(invalid());
        }
        let draft = DraftRow {
            company_id: self.company_id,
            workflow_id: self.workflow_id,
            revision: self.draft_revision,
            title: self.title,
            description: self.description,
            source: self.source,
            template_origin: self.template_origin,
            archived: false,
        }
        .restore()?
        .draft;
        Ok(PublishedCommand {
            request: PublishDraft {
                target: DefinitionTarget {
                    company: draft.company(),
                    workflow: draft.workflow(),
                    actor: WorkflowActor::authenticated(self.actor_id).map_err(|_| invalid())?,
                },
                expected: draft.revision(),
                version: VersionId::new(self.id),
                key: IdempotencyKey::parse(self.command_key).map_err(|_| invalid())?,
            },
            draft,
            bundle,
        })
    }
}
