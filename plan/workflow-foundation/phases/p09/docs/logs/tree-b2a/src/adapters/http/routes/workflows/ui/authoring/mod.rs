//! `/ui/workflows`: the library, the editor and its validation preview, publication, the
//! versions tab with compare and usage, archive, and the step catalogue (p09 B1a); the
//! bindings tab, the channel card, workflow tools and the agent pane's cards (p09 B1b). Each write is
//! the JSON routes' own command (`definitions::*_draft`), so the browser and the API cannot
//! drift on what a save, a validation or a publication means.
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
use crate::application::workflow::reads::{
    DefinitionDetail, MAX_OPTION_NAMES, PageRequest, PublicationOptions,
};
use crate::domain::workflow::{VersionId, WorkflowId};
use axum::{
    body::Bytes,
    extract::{Path, Query},
};
use serde::Deserialize;
use std::collections::BTreeMap;

mod bindings;
mod editor;
mod library;
mod tools;
mod versions;

use bindings::*;
use editor::*;
use library::*;
use tools::*;
use versions::*;

const NEW_ROUTE: &str = "/ui/workflows/new";
const COPY_ROUTE: &str = "/ui/workflows/templates/{template_id}/copy";
const SAVE_ROUTE: &str = "/ui/workflows/{workflow_id}/save";
const PUBLISH_ROUTE: &str = "/ui/workflows/{workflow_id}/publish";
const ARCHIVE_ROUTE: &str = "/ui/workflows/{workflow_id}/archive";
const BINDINGS_ROUTE: &str = "/ui/workflows/{workflow_id}/bindings";
const ACTIVATE_ROUTE: &str = "/ui/workflows/{workflow_id}/bindings/{binding_id}/activate";
const DEACTIVATE_ROUTE: &str = "/ui/workflows/{workflow_id}/bindings/{binding_id}/deactivate";
const REGISTER_TOOL_ROUTE: &str = "/ui/workflows/{workflow_id}/tools";
const REVOKE_TOOL_ROUTE: &str = "/ui/workflows/tools/revoke";
const GRANT_ROUTE: &str = "/ui/workflows/agents/{agent_id}/tools";
/// Every write this workspace registers; the CSRF test routes these same patterns.
#[cfg(test)]
const WRITE_ROUTES: [&str; 11] = [
    NEW_ROUTE,
    COPY_ROUTE,
    SAVE_ROUTE,
    PUBLISH_ROUTE,
    ARCHIVE_ROUTE,
    BINDINGS_ROUTE,
    ACTIVATE_ROUTE,
    DEACTIVATE_ROUTE,
    REGISTER_TOOL_ROUTE,
    REVOKE_TOOL_ROUTE,
    GRANT_ROUTE,
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
        .route(BINDINGS_ROUTE, post(save_binding))
        .route(
            "/ui/workflows/{workflow_id}/bindings/form",
            get(binding_form),
        )
        .route(ACTIVATE_ROUTE, post(activate_binding))
        .route(DEACTIVATE_ROUTE, post(deactivate_binding))
        .route(
            "/ui/workflows/channels/{channel_id}/bindings",
            get(channel_bindings),
        )
        .route("/ui/workflows/tools", get(tools_pane))
        .route(REVOKE_TOOL_ROUTE, post(revoke_tool))
        .route(REGISTER_TOOL_ROUTE, post(register_tool))
        .route(GRANT_ROUTE, get(agent_tools).post(grant_agent_tools))
        .route("/ui/workflows/agents/{agent_id}/usage", get(agent_usage))
}

/// The sidebar's page size.
const LIST_PAGE: i64 = 50;

#[derive(Debug, Deserialize)]
pub struct WorkspaceQuery {
    pub company_id: Option<Uuid>,
    pub workflow_id: Option<Uuid>,
    pub tab: Option<String>,
    pub new: Option<String>,
    pub tools: Option<String>,
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
    Conflict {
        current: u64,
        theirs: String,
    },
    MovedAfterSave {
        saved: u64,
        current: u64,
        theirs: String,
    },
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
            operation: tool.as_str().to_owned(),
        })
        .collect();
    let mcp = selections
        .mcp
        .iter()
        .map(|(key, connection)| McpSelectionBody {
            connection: *connection,
            slot: key.slot.as_str().to_owned(),
            tool: key.tool.as_str().to_owned(),
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
                pages::McpSelectionKey {
                    slot: need.slot.clone(),
                    tool: need.tool.clone(),
                },
                only.connection,
            );
        }
    }
    for need in &needs.tools {
        let mut offered = options.http.iter().filter(|o| o.name == need.tool.as_str());
        if let (Some(only), None) = (offered.next(), offered.next()) {
            selections.http.insert(need.tool.clone(), only.connection);
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
            pages::WorkflowTab::Bindings => {
                self.bindings_body(company, &detail, None, None).await?
            }
        };
        Ok(self.render_pane(company, &detail, tab, body).await)
    }
}

#[cfg(test)]
mod tests;
