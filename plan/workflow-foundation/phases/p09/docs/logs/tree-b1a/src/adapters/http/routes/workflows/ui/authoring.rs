//! `/ui/workflows`: the library, the editor and its validation preview, publication, the
//! versions tab with compare and usage, archive, and the step catalogue (p09 B1a). Each write is
//! the JSON routes' own command (`definitions::*_draft`), so the browser and the API cannot
//! drift on what a save, a validation or a publication means.
use super::super::bindings::Visibility;
use super::super::definitions::{
    CopiedTemplate, HttpSelectionBody, McpSelectionBody, PublishRequest, archive_draft,
    copy_template_draft, publish_draft, save_draft, validate_draft,
};
use super::*;
use crate::adapters::workflow_source::WorkflowSourceDecoder;
use crate::application::workflow::authoring::{PublicationNeeds, line_diff, publication_needs};
use crate::application::workflow::compiler::{Diagnostic, SourceDecoder};
use crate::application::workflow::lifecycle::DraftContent;
use crate::application::workflow::publication::PublishedBundle;
use crate::application::workflow::reads::{DefinitionDetail, PageRequest, PublicationOptions};
use crate::domain::workflow::{VersionId, WorkflowId};
use axum::{
    body::Bytes,
    extract::{Path, Query},
};
use serde::Deserialize;
use std::collections::BTreeMap;

const NEW_ROUTE: &str = "/ui/workflows/new";
const COPY_ROUTE: &str = "/ui/workflows/templates/{template_id}/copy";
const SAVE_ROUTE: &str = "/ui/workflows/{workflow_id}/save";
const PUBLISH_ROUTE: &str = "/ui/workflows/{workflow_id}/publish";
const ARCHIVE_ROUTE: &str = "/ui/workflows/{workflow_id}/archive";
/// Every write this workspace registers; the CSRF test routes these same patterns.
#[cfg(test)]
const WRITE_ROUTES: [&str; 5] = [
    NEW_ROUTE,
    COPY_ROUTE,
    SAVE_ROUTE,
    PUBLISH_ROUTE,
    ARCHIVE_ROUTE,
];

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/workflows", get(workspace))
        .route("/ui/workflows/list", get(list_page))
        .route(NEW_ROUTE, get(new_pane).post(create_draft))
        .route("/ui/workflows/catalogue", get(catalogue))
        .route(COPY_ROUTE, post(copy_template))
        .route("/ui/workflows/{workflow_id}", get(pane))
        .route(SAVE_ROUTE, post(save))
        .route(PUBLISH_ROUTE, post(publish))
        .route(ARCHIVE_ROUTE, post(archive))
        .route("/ui/workflows/{workflow_id}/compare", get(compare))
}

/// The sidebar's page size.
const LIST_PAGE: i64 = 50;

#[derive(Debug, Deserialize)]
pub struct WorkspaceQuery {
    pub company_id: Option<Uuid>,
    pub workflow_id: Option<Uuid>,
    pub tab: Option<String>,
    pub new: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompanyQuery {
    pub company_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub company_id: Uuid,
    pub after: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct PaneQuery {
    pub company_id: Uuid,
    pub tab: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompareQuery {
    pub company_id: Uuid,
    pub from: String,
    pub to: String,
}

/// What the editor shows besides the stored draft: what was typed, what the last command said.
#[derive(Default)]
struct EditorState {
    typed: Option<Typed>,
    banner: Option<Banner>,
    diagnostic: Option<Diagnostic>,
    /// The revision that validated, and what it compiled to.
    preview: Option<(u64, Arc<PublishedBundle>)>,
    selections: Option<pages::WorkflowSelections>,
}

struct Typed {
    title: String,
    description: String,
    source: String,
    expected_revision: u64,
}

enum Banner {
    Saved(String),
    Conflict { current: u64, theirs: String },
    Refused(String),
}

fn selection_bodies(
    selections: &pages::WorkflowSelections,
) -> (Vec<HttpSelectionBody>, Vec<McpSelectionBody>) {
    let http = selections
        .http
        .iter()
        .map(|(tool, connection)| HttpSelectionBody {
            connection: *connection,
            slot: None,
            operation: tool.clone(),
        })
        .collect();
    let mcp = selections
        .mcp
        .iter()
        .map(|((slot, tool), connection)| McpSelectionBody {
            connection: *connection,
            slot: slot.clone(),
            tool: tool.clone(),
        })
        .collect();
    (http, mcp)
}

/// A selection the author has not made yet, pre-filled where exactly one connection offers the
/// tool: still a visible, changeable choice in the form.
fn default_selections(
    needs: &PublicationNeeds,
    options: &PublicationOptions,
) -> pages::WorkflowSelections {
    let mut selections = pages::WorkflowSelections::default();
    for need in &needs.mcp {
        let mut offered = options.mcp.iter().filter(|o| o.name == need.tool.as_str());
        if let (Some(only), None) = (offered.next(), offered.next()) {
            selections.mcp.insert(
                (need.slot.as_str().to_owned(), need.tool.as_str().to_owned()),
                only.connection,
            );
        }
    }
    for need in &needs.tools {
        let mut offered = options.http.iter().filter(|o| o.name == need.tool.as_str());
        if let (Some(only), None) = (offered.next(), offered.next()) {
            selections
                .http
                .insert(need.tool.as_str().to_owned(), only.connection);
        }
    }
    selections
}

/// A refused validation or publication the author can act on, as the editor's banner; any
/// other failure is the request's.
fn refusal(error: WorkflowError) -> Result<EditorState, WorkflowError> {
    match error {
        WorkflowError::Diagnostic(diagnostic) => Ok(EditorState {
            diagnostic: Some(*diagnostic),
            ..Default::default()
        }),
        WorkflowError::App(AppError::BadRequest(message)) => Ok(EditorState {
            banner: Some(Banner::Refused(message)),
            ..Default::default()
        }),
        WorkflowError::App(AppError::NotFound(_)) => Ok(EditorState {
            banner: Some(Banner::Refused(
                "A selected connection, tool, agent or child workflow is not available to this \
                 company now: it may be disabled, not ready, or no longer approved."
                    .into(),
            )),
            ..Default::default()
        }),
        other => Err(other),
    }
}

impl Ui {
    async fn detail(&self, company: Uuid, workflow: Uuid) -> AppResult<DefinitionDetail> {
        self.engine
            .reads()
            .definition(super::company(company), WorkflowId::new(workflow))
            .await?
            .ok_or_else(|| AppError::NotFound("Workflow definition not found".into()))
    }

    async fn list(
        &self,
        company: Uuid,
        after: Option<Uuid>,
    ) -> AppResult<(
        Vec<crate::application::workflow::reads::DefinitionSummary>,
        Option<Uuid>,
    )> {
        let page = self
            .engine
            .reads()
            .list_definitions(
                super::company(company),
                PageRequest::new(after, Some(LIST_PAGE))?,
            )
            .await?;
        let next = page
            .more
            .then(|| page.items.last().map(|last| last.workflow.as_uuid()))
            .flatten();
        Ok((page.items, next))
    }

    /// The sidebar's first page again, out of band, after a write that adds or renames one.
    async fn menu_oob(&self, company: Uuid, selected: Uuid) -> AppResult<String> {
        let (items, next) = self.list(company, None).await?;
        Ok(format!(
            r#"<ul id="workflow-menu" hx-swap-oob="true" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2">{}</ul>"#,
            pages::workflow_list_entries(&pages::WorkflowList {
                company_id: company,
                items: &items,
                selected: Some(selected),
                next,
            })
        ))
    }

    async fn editor_body(
        &self,
        company: Uuid,
        detail: &DefinitionDetail,
        state: EditorState,
    ) -> AppResult<String> {
        let workflow = detail.summary.workflow.as_uuid();
        let draft = match &state.typed {
            Some(typed) => pages::EditorDraft {
                title: &typed.title,
                description: &typed.description,
                source: &typed.source,
                expected_revision: typed.expected_revision,
            },
            None => pages::EditorDraft {
                title: &detail.summary.title,
                description: &detail.description,
                source: &detail.source,
                expected_revision: detail.summary.revision,
            },
        };
        let needs = WorkflowSourceDecoder
            .decode(draft.source)
            .and_then(|decoded| publication_needs(&decoded));
        let options = match &needs {
            Ok(needs) if !needs.is_empty() => {
                self.engine
                    .authoring_reads()
                    .publication_options(super::company(company))
                    .await?
            }
            _ => PublicationOptions::default(),
        };
        let selections = match (&state.selections, &needs) {
            (Some(selections), _) => selections.clone(),
            (None, Ok(needs)) => default_selections(needs, &options),
            (None, Err(_)) => pages::WorkflowSelections::default(),
        };
        let preview_html = match &state.preview {
            None => None,
            Some((revision, bundle)) => {
                let agent_names: BTreeMap<Uuid, String> = self
                    .agents
                    .list_company_agents(self.user, company)
                    .await?
                    .into_iter()
                    .map(|agent| (agent.id, agent.name))
                    .collect();
                Some(pages::workflow_preview(&pages::PreviewView {
                    company_id: company,
                    workflow,
                    revision: *revision,
                    bundle,
                    selections: &selections,
                    agent_names: &agent_names,
                }))
            }
        };
        let banner = state.banner.as_ref().map(|banner| match banner {
            Banner::Saved(message) => pages::EditorBanner::Saved(message),
            Banner::Conflict { current, theirs } => pages::EditorBanner::Conflict {
                current: *current,
                theirs,
            },
            Banner::Refused(message) => pages::EditorBanner::Refused(message),
        });
        Ok(pages::workflow_editor(&pages::EditorView {
            company_id: company,
            workflow,
            archived: detail.summary.archived,
            draft,
            banner,
            diagnostic: state.diagnostic.as_ref(),
            preview_html: preview_html.as_deref(),
            needs: needs.as_ref(),
            options: &options,
            selections: &selections,
        }))
    }

    async fn versions_body(
        &self,
        company: Uuid,
        detail: &DefinitionDetail,
        banner: Option<&str>,
        compare_html: Option<&str>,
    ) -> AppResult<String> {
        let company_id = super::company(company);
        let usage = self
            .engine
            .authoring_reads()
            .workflow_usage(company_id, detail.summary.workflow)
            .await?;
        let mut visibility = Visibility::new(&self.engine, company_id, self.actor()?);
        let mut visible = Vec::new();
        let mut hidden = 0;
        for binding in usage.bindings {
            if visibility.allows(binding.binding.association).await? {
                visible.push(binding);
            } else {
                hidden += 1;
            }
        }
        visibility.report("workflow-usage");
        Ok(pages::workflow_versions(&pages::VersionsView {
            company_id: company,
            workflow: detail.summary.workflow.as_uuid(),
            draft_revision: detail.summary.revision,
            versions: &detail.versions,
            versions_truncated: detail.versions_truncated,
            usage: &visible,
            usage_truncated: usage.truncated,
            hidden_bindings: hidden,
            banner,
            compare_html,
        }))
    }

    /// The whole pane: header, tabs and the tab's body.
    async fn render_pane(
        &self,
        company: Uuid,
        detail: &DefinitionDetail,
        tab: pages::WorkflowTab,
        body: String,
    ) -> String {
        let template = detail
            .summary
            .template_origin
            .as_ref()
            .and_then(|origin| origin.get("template"))
            .and_then(serde_json::Value::as_str);
        pages::workflow_pane(
            &pages::WorkflowHeader {
                company_id: company,
                workflow: detail.summary.workflow.as_uuid(),
                title: &detail.summary.title,
                revision: detail.summary.revision,
                archived: detail.summary.archived,
                versions: detail.versions.len(),
                template,
            },
            tab,
            &body,
        )
    }

    async fn pane_html(
        &self,
        company: Uuid,
        workflow: Uuid,
        tab: pages::WorkflowTab,
        state: EditorState,
    ) -> AppResult<String> {
        let detail = self.detail(company, workflow).await?;
        let body = match tab {
            pages::WorkflowTab::Editor => self.editor_body(company, &detail, state).await?,
            pages::WorkflowTab::Versions => {
                let banner = match &state.banner {
                    Some(Banner::Saved(message)) => Some(message.as_str()),
                    _ => None,
                };
                self.versions_body(company, &detail, banner, None).await?
            }
        };
        Ok(self.render_pane(company, &detail, tab, body).await)
    }
}

/// GET /ui/workflows — the workspace for the selected company and workflow.
async fn workspace(ui: Ui, Query(query): Query<WorkspaceQuery>) -> AppResult<Html<String>> {
    let frame = ui.frame(query.company_id).await?;
    let Some(company) = frame.company.as_ref() else {
        return Ok(Html(frame.render(&ui.config, |_, _| String::new())));
    };
    let company_id = company.id;
    let (items, next) = ui.list(company_id, None).await?;
    let creating = matches!(query.new.as_deref(), Some("1") | Some("true"));
    let selected = query.workflow_id.or_else(|| {
        (!creating)
            .then(|| items.first().map(|item| item.workflow.as_uuid()))
            .flatten()
    });
    let pane = if creating {
        new_pane_html(&ui, company_id, None)
    } else {
        match selected {
            Some(workflow) => {
                ui.pane_html(
                    company_id,
                    workflow,
                    pages::WorkflowTab::parse(query.tab.as_deref()),
                    EditorState::default(),
                )
                .await?
            }
            None => pages::workflow_empty_pane(
                "No workflows yet. Choose New to copy a template or start a blank draft.",
            ),
        }
    };
    let list = pages::WorkflowList {
        company_id,
        items: &items,
        selected,
        next,
    };
    Ok(Html(frame.render(&ui.config, |user, company| {
        pages::workflow_workspace_page(&pages::WorkflowWorkspace {
            user,
            company,
            list: &list,
            pane_html: &pane,
        })
    })))
}

/// GET /ui/workflows/list — the next sidebar page, replacing its "Load more" row.
async fn list_page(ui: Ui, Query(query): Query<ListQuery>) -> AppResult<Html<String>> {
    ui.company(query.company_id).await?;
    let (items, next) = ui.list(query.company_id, query.after).await?;
    Ok(Html(pages::workflow_list_entries(&pages::WorkflowList {
        company_id: query.company_id,
        items: &items,
        selected: None,
        next,
    })))
}

fn new_pane_html(ui: &Ui, company: Uuid, error: Option<&str>) -> String {
    pages::workflow_new_pane(&pages::WorkflowNewPane {
        company_id: company,
        templates: ui.engine.template_catalogue().entries(),
        error,
    })
}

/// GET /ui/workflows/new — the templates and the blank-draft form.
async fn new_pane(ui: Ui, Query(query): Query<CompanyQuery>) -> AppResult<Html<String>> {
    ui.company(query.company_id).await?;
    Ok(Html(new_pane_html(&ui, query.company_id, None)))
}

/// A blank draft that compiles: one `data.map` step passing the input through.
fn blank_source(workflow: Uuid) -> String {
    format!(
        "format_version: 1\nworkflow_id: {workflow}\ninput_schema: {{type: object}}\nparameter_schema: {{type: object, additionalProperties: false}}\noutput_schema: true\nresources: []\nentry: start\nsteps:\n  start:\n    type: data.map\n    with:\n      value: {{ref: /input}}\n      output_schema: {{literal: true}}\n    routes: {{success: $end}}\nlimits: {{max_steps: 10, max_context_bytes: 65536}}\n"
    )
}

fn workflow_url(company: Uuid, workflow: Uuid, tab: &str) -> String {
    format!("/ui/workflows?company_id={company}&workflow_id={workflow}&tab={tab}")
}

/// The pane of a draft a create or copy just made (or a retry of it found), with the sidebar.
async fn created(ui: &Ui, company: Uuid, workflow: Uuid, message: String) -> AppResult<Response> {
    let pane = ui
        .pane_html(
            company,
            workflow,
            pages::WorkflowTab::Editor,
            EditorState {
                banner: Some(Banner::Saved(message)),
                ..Default::default()
            },
        )
        .await?;
    let menu = ui.menu_oob(company, workflow).await?;
    Ok(pushed(
        format!("{pane}{menu}"),
        &workflow_url(company, workflow, "editor"),
    ))
}

/// POST /ui/workflows/new — a blank draft whose identity the form's key derives, so a double
/// submit makes one draft.
async fn create_draft(
    ui: Ui,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Response> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let key = crate::application::workflow::IdempotencyKey::parse(form.text("command_key"))?;
    let workflow = keyed_id("workflow.ui_draft", super::company(company), key.as_str());
    let title = form.text("title");
    let saved = save_draft(
        &ui.engine,
        company,
        ui.actor()?,
        workflow,
        None,
        DraftContent {
            title: title.trim().to_owned(),
            description: String::new(),
            source: blank_source(workflow),
        },
    )
    .await;
    match saved {
        Ok(_) => Ok(created(&ui, company, workflow, "Draft created.".into()).await?),
        // The key's draft exists: this is a retry of the create that made it.
        Err(WorkflowError::App(AppError::Conflict(_)))
            if ui
                .engine
                .reads()
                .definition(super::company(company), WorkflowId::new(workflow))
                .await?
                .is_some() =>
        {
            Ok(created(&ui, company, workflow, "Draft created.".into()).await?)
        }
        Err(WorkflowError::App(AppError::BadRequest(message))) => {
            Ok(Html(new_pane_html(&ui, company, Some(&message))).into_response())
        }
        Err(error) => Err(error),
    }
}

/// POST /ui/workflows/templates/{template}/copy — the template at the catalogue revision the
/// page offered, into a draft the key derives.
async fn copy_template(
    ui: Ui,
    Path(template): Path<String>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Response> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let copied = copy_template_draft(
        &ui.engine,
        company,
        ui.actor()?,
        &template,
        form.number("revision")?,
        &form.text("command_key"),
    )
    .await;
    match copied {
        Ok(CopiedTemplate { workflow, .. }) => Ok(created(
            &ui,
            company,
            workflow,
            "Template copied into a draft of this company. Edit it, validate it, then publish."
                .into(),
        )
        .await?),
        Err(WorkflowError::App(AppError::Conflict(message) | AppError::NotFound(message))) => {
            Ok(Html(new_pane_html(
                &ui,
                company,
                Some(&format!("The template was not copied: {message}")),
            ))
            .into_response())
        }
        Err(error) => Err(error),
    }
}

/// GET /ui/workflows/{workflow} — one workflow's pane.
async fn pane(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<PaneQuery>,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    Ok(Html(
        ui.pane_html(
            company,
            workflow,
            pages::WorkflowTab::parse(query.tab.as_deref()),
            EditorState::default(),
        )
        .await?,
    ))
}

/// POST /ui/workflows/{workflow}/save — save against the revision the form was opened with,
/// then (intent `validate`) compile that revision. A refused save keeps the typed text; a
/// conflict shows what the newer revision has.
async fn save(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let selections = pages::WorkflowSelections::from_fields(form.pairs());
    let typed = Typed {
        title: form.text("title"),
        description: form.text("description"),
        source: form.text("source"),
        expected_revision: form.number("expected_revision")?,
    };
    let saved = save_draft(
        &ui.engine,
        company,
        ui.actor()?,
        workflow,
        Some(typed.expected_revision),
        DraftContent {
            title: typed.title.clone(),
            description: typed.description.clone(),
            source: typed.source.clone(),
        },
    )
    .await;
    let state = match saved {
        Ok(draft) => {
            let revision = draft.revision().get();
            if form.get("intent") == Some("validate") {
                validated(&ui, company, workflow, revision, selections).await?
            } else {
                EditorState {
                    banner: Some(Banner::Saved(format!("Saved revision {revision}."))),
                    selections: Some(selections),
                    ..Default::default()
                }
            }
        }
        Err(WorkflowError::App(AppError::Conflict(_))) => {
            let current = ui.detail(company, workflow).await?;
            let banner = if current.summary.archived {
                Banner::Refused(
                    "This workflow was archived; the draft can no longer be saved.".into(),
                )
            } else {
                Banner::Conflict {
                    current: current.summary.revision,
                    theirs: current.source.clone(),
                }
            };
            EditorState {
                typed: Some(Typed {
                    // The next save deliberately replaces the newer revision.
                    expected_revision: current.summary.revision,
                    ..typed
                }),
                banner: Some(banner),
                selections: Some(selections),
                ..Default::default()
            }
        }
        Err(WorkflowError::App(AppError::BadRequest(message))) => EditorState {
            typed: Some(typed),
            banner: Some(Banner::Refused(message)),
            selections: Some(selections),
            ..Default::default()
        },
        Err(error) => return Err(error),
    };
    Ok(Html(
        ui.pane_html(company, workflow, pages::WorkflowTab::Editor, state)
            .await?,
    ))
}

/// Compile the saved revision with the selections, as the editor reports it.
async fn validated(
    ui: &Ui,
    company: Uuid,
    workflow: Uuid,
    revision: u64,
    selections: pages::WorkflowSelections,
) -> WorkflowResult<EditorState> {
    let (http, mcp) = selection_bodies(&selections);
    let result = validate_draft(
        &ui.engine,
        company,
        ui.actor()?,
        workflow,
        revision,
        &http,
        &mcp,
    )
    .await;
    let mut state = match result {
        Ok(bundle) => EditorState {
            banner: Some(Banner::Saved(format!(
                "Saved revision {revision}; it is valid."
            ))),
            preview: Some((revision, bundle)),
            ..Default::default()
        },
        Err(error) => {
            let mut state = refusal(error)?;
            state.banner = Some(match state.banner.take() {
                // The save went through; say so beside why validation did not.
                Some(Banner::Refused(why)) => Banner::Refused(format!(
                    "Saved revision {revision}, but it could not be validated: {why}"
                )),
                _ => Banner::Saved(format!(
                    "Saved revision {revision}; it does not validate yet."
                )),
            });
            state
        }
    };
    state.selections = Some(selections);
    Ok(state)
}

/// POST /ui/workflows/{workflow}/publish — publish the validated revision with the validated
/// selections, then show where every version runs.
async fn publish(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Response> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let selections = pages::WorkflowSelections::from_fields(form.pairs());
    let (http, mcp) = selection_bodies(&selections);
    let expected = form.number("expected_revision")?;
    let published = publish_draft(
        &ui.engine,
        company,
        ui.actor()?,
        workflow,
        PublishRequest {
            expected_revision: expected,
            command_key: &form.text("command_key"),
            http: &http,
            mcp: &mcp,
        },
    )
    .await;
    match published {
        Ok((version, _)) => {
            let detail = ui.detail(company, workflow).await?;
            let banner = format!(
                "Published version {} from draft revision {expected}. Publishing changes no binding: each keeps its version until you reconfigure it.",
                pages::short_id(version.as_uuid())
            );
            let body = ui
                .versions_body(company, &detail, Some(&banner), None)
                .await?;
            let pane = ui
                .render_pane(company, &detail, pages::WorkflowTab::Versions, body)
                .await;
            Ok(pushed(pane, &workflow_url(company, workflow, "versions")))
        }
        Err(WorkflowError::App(AppError::Conflict(_))) => {
            let current = ui.detail(company, workflow).await?;
            let state = EditorState {
                banner: Some(Banner::Refused(format!(
                    "Revision {expected} is no longer the draft (now revision {}{}), or this key \
                     already published something else. Validate the current draft again before \
                     publishing.",
                    current.summary.revision,
                    if current.summary.archived {
                        ", archived"
                    } else {
                        ""
                    }
                ))),
                selections: Some(selections),
                ..Default::default()
            };
            Ok(Html(
                ui.pane_html(company, workflow, pages::WorkflowTab::Editor, state)
                    .await?,
            )
            .into_response())
        }
        Err(error) => {
            let mut state = refusal(error)?;
            state.selections = Some(selections);
            Ok(Html(
                ui.pane_html(company, workflow, pages::WorkflowTab::Editor, state)
                    .await?,
            )
            .into_response())
        }
    }
}

/// POST /ui/workflows/{workflow}/archive — archive the draft at the revision the page showed.
async fn archive(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Response> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let expected = form.number("expected_revision")?;
    let state = match archive_draft(&ui.engine, company, ui.actor()?, workflow, expected).await {
        Ok(()) => EditorState {
            banner: Some(Banner::Saved(
                "Archived. Its versions stay readable and keep serving the bindings that use them."
                    .into(),
            )),
            ..Default::default()
        },
        Err(WorkflowError::App(AppError::Conflict(_))) => EditorState {
            banner: Some(Banner::Refused(format!(
                "The draft changed since revision {expected}, or is already archived; nothing was archived."
            ))),
            ..Default::default()
        },
        Err(error) => return Err(error),
    };
    let pane = ui
        .pane_html(company, workflow, pages::WorkflowTab::Editor, state)
        .await?;
    let menu = ui.menu_oob(company, workflow).await?;
    Ok(Html(format!("{pane}{menu}")).into_response())
}

/// GET /ui/workflows/{workflow}/compare — a line diff between two of: the draft, a version.
async fn compare(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompareQuery>,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let detail = ui.detail(company, workflow).await?;
    let side = |name: &str| -> AppResult<Option<Uuid>> {
        match name {
            "draft" => Ok(None),
            other => other
                .parse()
                .map(Some)
                .map_err(|_| AppError::BadRequest("Compare names the draft or a version".into())),
        }
    };
    let mut sources = Vec::with_capacity(2);
    for name in [&query.from, &query.to] {
        sources.push(match side(name)? {
            None => (
                format!("draft revision {}", detail.summary.revision),
                detail.source.clone(),
            ),
            Some(version) => {
                let version = ui
                    .engine
                    .authoring_reads()
                    .version_source(
                        super::company(company),
                        WorkflowId::new(workflow),
                        VersionId::new(version),
                    )
                    .await?
                    .ok_or_else(|| AppError::NotFound("Workflow version not found".into()))?;
                (
                    format!(
                        "version {} (draft revision {})",
                        pages::short_id(version.version.as_uuid()),
                        version.draft_revision
                    ),
                    version.source,
                )
            }
        });
    }
    let (to_label, to_source) = sources.pop().expect("two sides");
    let (from_label, from_source) = sources.pop().expect("two sides");
    Ok(Html(pages::diff_block(
        &line_diff(&from_source, &to_source),
        &from_label,
        &to_label,
    )))
}

/// GET /ui/workflows/catalogue — the step catalogue and the company's agents.
async fn catalogue(ui: Ui, Query(query): Query<CompanyQuery>) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let agents = ui.agents.list_company_agents(ui.user, company).await?;
    let references: Vec<pages::AgentReference<'_>> = agents
        .iter()
        .map(|agent| pages::AgentReference {
            slug: &agent.slug,
            name: &agent.name,
            model: match (&agent.provider, &agent.model) {
                (Some(provider), Some(model)) => Some(format!("{provider} / {model}")),
                _ => None,
            },
        })
        .collect();
    Ok(Html(pages::workflow_catalogue(
        ui.engine.step_catalogue(),
        &references,
    )))
}

#[cfg(test)]
#[path = "authoring_tests.rs"]
mod tests;
