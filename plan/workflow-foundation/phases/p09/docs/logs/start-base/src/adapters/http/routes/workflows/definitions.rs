//! Workflow definitions and templates: drafts saved with the revision the editor saw, validated
//! and published by key, archived; shipped templates listed and copied by key.
use super::*;
use crate::adapters::persistence::workflow::http::HttpPublicationSelection;
use crate::adapters::persistence::workflow::mcp::McpPublicationSelection;
use crate::application::workflow::IdempotencyKey;
use crate::application::workflow::lifecycle::{
    DefinitionTarget, DraftContent, PublishDraft, SaveDraft,
};
use crate::application::workflow::reads::DefinitionSummary;
use crate::application::workflow::templates::CopyTemplateRequest;
use crate::domain::workflow::{
    DraftRevision, ResourceName, TemplateId, TemplateRevision, TypeName, VersionId, WorkflowId,
};
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

fn target(company_id: Uuid, actor: WorkflowActor, workflow: Uuid) -> DefinitionTarget {
    DefinitionTarget {
        company: company(company_id),
        actor,
        workflow: WorkflowId::new(workflow),
    }
}

fn summary_json(summary: &DefinitionSummary) -> Value {
    json!({
        "workflow_id": summary.workflow.as_uuid(),
        "revision": summary.revision,
        "title": summary.title,
        "archived": summary.archived,
        "template_origin": summary.template_origin,
    })
}

pub async fn list(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> WorkflowResult<Json<Value>> {
    let company = company(company_id);
    authorize(&engine, company, actor(&user)?, RelatedAssociation::Company).await?;
    let page = engine
        .reads()
        .list_definitions(company, query.page()?)
        .await?;
    let next = page
        .more
        .then(|| page.items.last().map(|last| last.workflow.as_uuid()))
        .flatten();
    Ok(page_json(
        page.items.iter().map(summary_json).collect(),
        next,
    ))
}

pub async fn read(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, workflow)): Path<(Uuid, Uuid)>,
) -> WorkflowResult<Json<Value>> {
    let company = company(company_id);
    authorize(&engine, company, actor(&user)?, RelatedAssociation::Company).await?;
    let detail = engine
        .reads()
        .definition(company, WorkflowId::new(workflow))
        .await?
        .ok_or_else(|| AppError::NotFound("Workflow definition not found".into()))?;
    let mut body = summary_json(&detail.summary);
    body["description"] = json!(detail.description);
    body["source"] = json!(detail.source);
    body["versions"] = json!(
        detail
            .versions
            .iter()
            .map(|version| json!({
                "version_id": version.version.as_uuid(),
                "draft_revision": version.draft_revision,
                "content_hash": version.content_hash,
                "created_at": version.created_at,
            }))
            .collect::<Vec<_>>()
    );
    body["versions_truncated"] = json!(detail.versions_truncated);
    Ok(ok(body))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveBody {
    /// Absent creates the draft; present must be its current revision.
    pub expected_revision: Option<u64>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub source: String,
}

pub async fn save(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, workflow)): Path<(Uuid, Uuid)>,
    Json(body): Json<SaveBody>,
) -> WorkflowResult<Json<Value>> {
    let expected = body
        .expected_revision
        .map(|value| revision(value, DraftRevision::new))
        .transpose()?;
    let draft = engine
        .definitions(Vec::new(), Vec::new())
        .save(SaveDraft {
            target: target(company_id, actor(&user)?, workflow),
            expected,
            content: DraftContent {
                title: body.title,
                description: body.description,
                source: body.source,
            },
        })
        .await?;
    Ok(ok(json!({"workflow_id": draft.workflow().as_uuid(),
        "revision": draft.revision().get()})))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionBody {
    pub expected_revision: u64,
}

/// Validation compiles through the same publication directories as publish, so a draft naming a
/// company HTTP or MCP operation validates with the selections it will publish with.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidateBody {
    pub expected_revision: u64,
    #[serde(default)]
    pub http: Vec<HttpSelectionBody>,
    #[serde(default)]
    pub mcp: Vec<McpSelectionBody>,
}

pub async fn validate(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, workflow)): Path<(Uuid, Uuid)>,
    Json(body): Json<ValidateBody>,
) -> WorkflowResult<Json<Value>> {
    let (http, mcp) = selections(&body.http, &body.mcp)?;
    let bundle = engine
        .definitions(http, mcp)
        .validate(
            target(company_id, actor(&user)?, workflow),
            revision(body.expected_revision, DraftRevision::new)?,
        )
        .await?;
    Ok(ok(
        json!({"valid": true, "content_hash": bundle.content_hash()}),
    ))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpSelectionBody {
    pub connection: Uuid,
    pub slot: Option<String>,
    pub operation: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSelectionBody {
    pub connection: Uuid,
    pub slot: String,
    pub tool: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishBody {
    pub expected_revision: u64,
    /// A retry with the key returns the version it published.
    pub command_key: String,
    #[serde(default)]
    pub http: Vec<HttpSelectionBody>,
    #[serde(default)]
    pub mcp: Vec<McpSelectionBody>,
}

fn name<'a, T>(
    value: &'a str,
    parse: impl FnOnce(&'a str) -> Result<T, crate::domain::workflow::NameError>,
) -> AppResult<T> {
    parse(value).map_err(|_| AppError::BadRequest(format!("'{value}' is not a valid name")))
}

type Selections = (Vec<HttpPublicationSelection>, Vec<McpPublicationSelection>);

fn selections(http: &[HttpSelectionBody], mcp: &[McpSelectionBody]) -> AppResult<Selections> {
    let http = http
        .iter()
        .map(|selection| {
            Ok(HttpPublicationSelection {
                connection: selection.connection,
                slot: selection
                    .slot
                    .as_deref()
                    .map(|slot| name(slot, ResourceName::parse))
                    .transpose()?,
                operation: name(&selection.operation, TypeName::parse)?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let mcp = mcp
        .iter()
        .map(|selection| {
            Ok(McpPublicationSelection {
                connection: selection.connection,
                slot: name(&selection.slot, ResourceName::parse)?,
                tool: name(&selection.tool, TypeName::parse)?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok((http, mcp))
}

pub async fn publish(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, workflow)): Path<(Uuid, Uuid)>,
    Json(body): Json<PublishBody>,
) -> WorkflowResult<Json<Value>> {
    let (http, mcp) = selections(&body.http, &body.mcp)?;
    let company = company(company_id);
    let key = IdempotencyKey::parse(&body.command_key)?;
    // The version is the key's and the selections': a retry publishes (or replays) the same one,
    // and the key with other selections names another version, which the command refuses.
    let selected = serde_json::to_string(&json!({"http": body.http, "mcp": body.mcp}))
        .map_err(|_| AppError::BadRequest("Invalid publication selections".into()))?;
    let version = VersionId::new(keyed_id(
        &format!("workflow.publish:{workflow}"),
        company,
        &format!("{}\0{selected}", key.as_str()),
    ));
    let bundle = engine
        .definitions(http, mcp)
        .publish(PublishDraft {
            target: target(company_id, actor(&user)?, workflow),
            expected: revision(body.expected_revision, DraftRevision::new)?,
            version,
            key,
        })
        .await?;
    Ok(ok(
        json!({"version_id": version.as_uuid(), "content_hash": bundle.content_hash()}),
    ))
}

pub async fn archive(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, workflow)): Path<(Uuid, Uuid)>,
    Json(body): Json<RevisionBody>,
) -> WorkflowResult<StatusCode> {
    engine
        .definitions(Vec::new(), Vec::new())
        .archive(
            target(company_id, actor(&user)?, workflow),
            revision(body.expected_revision, DraftRevision::new)?,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn templates(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> WorkflowResult<Json<Value>> {
    authorize(
        &engine,
        company(company_id),
        actor(&user)?,
        RelatedAssociation::Company,
    )
    .await?;
    Ok(ok(json!(
        engine
            .template_catalogue()
            .entries()
            .iter()
            .map(|template| json!({
                "template_id": template.id().as_str(),
                "revision": template.revision().get(),
                "title": template.title(),
                "description": template.description(),
            }))
            .collect::<Vec<_>>()
    )))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopyBody {
    /// The catalogue revision the caller chose; a newer one is a conflict.
    pub revision: u64,
    /// A retry with the key returns the draft it copied.
    pub command_key: String,
}

pub async fn copy_template(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, template)): Path<(Uuid, String)>,
    Json(body): Json<CopyBody>,
) -> WorkflowResult<(StatusCode, Json<Value>)> {
    let company = company(company_id);
    let actor = actor(&user)?;
    let key = IdempotencyKey::parse(&body.command_key)?;
    let template = name(&template, TemplateId::parse)?;
    let workflow = WorkflowId::new(keyed_id("workflow.template_copy", company, key.as_str()));
    let request = CopyTemplateRequest::new(
        company,
        actor,
        template.clone(),
        revision(body.revision, TemplateRevision::new)?,
        workflow,
    )?;
    match engine
        .template_copies()
        .copy(engine.template_catalogue(), request)
        .await
    {
        Ok(draft) => Ok((
            StatusCode::CREATED,
            ok(
                json!({"workflow_id": draft.workflow().as_uuid(), "revision": draft.revision().get()}),
            ),
        )),
        // The key's draft already exists: a retry of this copy returns it; anything else under
        // the key is a conflict.
        Err(crate::application::workflow::templates::TemplateError::Application(
            AppError::Conflict(message),
        )) => {
            let existing = engine.reads().definition(company, workflow).await?;
            match existing {
                Some(detail)
                    if detail
                        .summary
                        .template_origin
                        .as_ref()
                        .and_then(|origin| origin.get("template").and_then(Value::as_str))
                        == Some(template.as_str()) =>
                {
                    Ok((
                        StatusCode::OK,
                        ok(json!({"workflow_id": workflow.as_uuid(),
                            "revision": detail.summary.revision})),
                    ))
                }
                _ => Err(AppError::Conflict(message).into()),
            }
        }
        Err(error) => Err(error.into()),
    }
}
