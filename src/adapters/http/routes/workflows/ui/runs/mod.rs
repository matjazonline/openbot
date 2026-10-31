//! `/ui/workflow-runs` (p09 B2a): the runs workspace, its list, one run's pane and one step of
//! it, the controls (cancel, retry, rerun, the reconciliation annotation) through the JSON
//! routes' own commands, and the run event stream the page reloads on. A run is shown to whoever
//! the JSON routes show it to: an owner or admin of the company who can see the run's channel.
use super::super::bindings::{Visibility, company_agent, company_workflow, visible_binding};
use super::super::board::visible_board;
use super::*;
use crate::adapters::http::routes::deserialize_empty_string_as_none;
use crate::application::workflow::authoring::workflow_graph;
use crate::application::workflow::board::{BoardWindow, RunBoardScope};
use crate::application::workflow::reads::{
    MAX_BINDING_OPTIONS, MAX_LISTED, PageRequest, RunFilter, RunLineage, RunStateName, RunSummary,
};
use crate::application::workflow::run_views::{RunView, StepView};
use crate::domain::workflow::{RunId, WorkflowBindingId, WorkflowId};
use axum::extract::{Path, Query};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

mod controls;
mod events;

use controls::*;

const LIST_PAGE: i64 = 50;
const CANCEL_ROUTE: &str = "/ui/workflow-runs/{run_id}/cancel";
const RETRY_ROUTE: &str = "/ui/workflow-runs/{run_id}/retry";
const RERUN_ROUTE: &str = "/ui/workflow-runs/{run_id}/rerun";
const RECONCILE_ROUTE: &str = "/ui/workflow-runs/{run_id}/reconcile";
/// Every write this workspace registers; the CSRF test routes these same patterns.
#[cfg(test)]
const WRITE_ROUTES: [&str; 4] = [CANCEL_ROUTE, RETRY_ROUTE, RERUN_ROUTE, RECONCILE_ROUTE];

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/workflow-runs", get(workspace))
        .route("/ui/workflow-runs/list", get(list))
        .route("/ui/workflow-runs/events", get(events::stream))
        .route("/ui/workflow-runs/{run_id}", get(pane))
        .route("/ui/workflow-runs/{run_id}/steps/{execution_id}", get(step))
        .route(CANCEL_ROUTE, post(cancel))
        .route(RETRY_ROUTE, post(retry))
        .route(RERUN_ROUTE, post(rerun))
        .route(RECONCILE_ROUTE, post(reconcile))
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceQuery {
    pub company_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub binding_id: Option<Uuid>,
    /// A workflow of the company; "" (the select's "any") is none.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub workflow_id: Option<Uuid>,
    /// An agent of the company; "" is none.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub agent_id: Option<Uuid>,
    pub state: Option<String>,
    /// `board` for the board; anything else, or nothing, is the list.
    pub view: Option<String>,
    /// The board's finished-column window (see [`pages::board_window`]).
    pub window: Option<String>,
    /// With `view=board`, `columns` answers only the `#run-board` region.
    pub part: Option<String>,
    /// `columns` from the board's reload leaf, which keeps only the columns: the toolbar's
    /// options are not read.
    pub reload: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub company_id: Uuid,
    pub binding_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub workflow_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub agent_id: Option<Uuid>,
    pub state: Option<String>,
    pub after: Option<Uuid>,
    /// The run open in the pane, highlighted in the entries.
    pub run_id: Option<Uuid>,
    /// How many rows the list has read so far (visible and hidden).
    pub shown: Option<usize>,
    pub part: Option<ListPart>,
}

/// Which part of the list a GET answers: absent, the whole region (a filter change); `entries`,
/// the entries again as far as the list had read (a wake-up or another run opened); `more`, the
/// next page after `after` ("Load more").
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListPart {
    Entries,
    More,
}

/// One read of the list: the runs the caller may see, how many were left out, the cursor past
/// them, and the workflow titles they name.
struct RunPage {
    items: Vec<RunSummary>,
    hidden: usize,
    next: Option<Uuid>,
    titles: HashMap<Uuid, String>,
}

impl RunPage {
    /// Rows this read took from the list, visible or not.
    fn read(&self) -> usize {
        self.items.len() + self.hidden
    }
}

/// The scope a request names: binding, workflow and agent ids as given.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScopeIds {
    pub binding: Option<Uuid>,
    pub workflow: Option<Uuid>,
    pub agent: Option<Uuid>,
}

impl ScopeIds {
    fn of_workspace(query: &WorkspaceQuery) -> Self {
        Self {
            binding: query.binding_id,
            workflow: query.workflow_id,
            agent: query.agent_id,
        }
    }

    fn of_list(query: &ListQuery) -> Self {
        Self {
            binding: query.binding_id,
            workflow: query.workflow_id,
            agent: query.agent_id,
        }
    }

    fn view<'a>(&self, state: Option<&'a str>) -> pages::RunFilterView<'a> {
        pages::RunFilterView {
            binding: self.binding,
            workflow: self.workflow,
            agent: self.agent,
            state,
        }
    }
}

/// What a board read renders: the whole region with the toolbar's options, or (the reload leaf's
/// `reload=columns`) only what the leaf keeps, the columns, leaving the options unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoardRead {
    Region,
    Columns,
}

impl BoardRead {
    fn of(query: &WorkspaceQuery) -> Self {
        match query.reload.as_deref() {
            Some("columns") => Self::Columns,
            _ => Self::Region,
        }
    }
}

/// A scope resolved company-scoped: a binding the caller may see, a workflow and an agent of the
/// company. Any other id is not found, never ignored.
#[derive(Debug, Clone, Copy, Default)]
struct Scope {
    binding: Option<WorkflowBindingId>,
    workflow: Option<WorkflowId>,
    agent: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct CompanyQuery {
    pub company_id: Uuid,
}

/// A state filter value the list accepts; anything else lists every state.
fn state_filter(state: Option<&str>) -> Option<String> {
    state
        .and_then(RunStateName::parse)
        .map(|state| state.as_str().to_owned())
}

/// The graph's progress marks: a step whose newest activation finished is done; one that has not
/// is current while the run goes on and stopped once it has ended; a failed run's newest step
/// failed.
fn marks(view: &RunView) -> BTreeMap<String, pages::NodeMark> {
    let mut marks = BTreeMap::new();
    let ended = pages::run_ended(&view.detail.summary.state);
    // Oldest first, so a later activation of a step decides its mark.
    for execution in view.detail.executions.iter().rev() {
        let mark = if execution.completed_at.is_some() {
            pages::NodeMark::Done
        } else if ended {
            pages::NodeMark::Stopped
        } else {
            pages::NodeMark::Current
        };
        marks.insert(execution.step.clone(), mark);
    }
    if view.detail.summary.state == "failed"
        && let Some(newest) = view.detail.executions.first()
    {
        marks.insert(newest.step.clone(), pages::NodeMark::Failed);
    }
    marks
}

impl Ui {
    /// The run, when the caller may see it: the company is one they manage (`self.company`
    /// first), and the run's channel is one they can see. Anything else is not found.
    async fn visible_run(&self, company: Uuid, run: Uuid) -> AppResult<RunView> {
        let not_found = || AppError::NotFound("Workflow run not found".into());
        let company_id = super::company(company);
        let view = self
            .engine
            .run_views()
            .run_view(company_id, RunId::new(run))
            .await?
            .ok_or_else(not_found)?;
        let mut visibility = Visibility::new(&self.engine, company_id, self.actor()?);
        if !visibility.allows(view.detail.summary.association).await? {
            return Err(not_found());
        }
        Ok(view)
    }

    /// User names by id, for the actors a run's history names.
    async fn people(&self, company: Uuid) -> AppResult<HashMap<Uuid, String>> {
        Ok(self
            .engine
            .authoring_reads()
            .binding_options(super::company(company))
            .await?
            .people
            .into_iter()
            .map(|person| (person.id, person.label))
            .collect())
    }

    async fn run_pane_html(
        &self,
        company: Uuid,
        run: Uuid,
        banner: Option<pages::RunBanner<'_>>,
    ) -> AppResult<String> {
        let parts = self.pane_parts(company, run).await?;
        Ok(pages::run_pane(&parts.view(
            company,
            self.user,
            Uuid::new_v4(),
            banner,
        )))
    }

    /// What a pane renders from, for a run the caller may see.
    async fn pane_parts(&self, company: Uuid, run: Uuid) -> AppResult<PaneParts> {
        let view = self.visible_run(company, run).await?;
        let graph_html = match self
            .engine
            .run_views()
            .run_bundle(super::company(company), RunId::new(run))
            .await?
        {
            Some(bundle) => pages::workflow_graph_view(
                &workflow_graph(bundle.compiled().graph().definition()),
                &marks(&view),
            ),
            None => String::new(),
        };
        let people = self.people(company).await?;
        Ok(PaneParts {
            view,
            graph_html,
            people,
        })
    }

    /// `ids` resolved company-scoped (see [`Scope`]).
    async fn scope(&self, company: Uuid, ids: ScopeIds) -> AppResult<Scope> {
        let company_id = super::company(company);
        Ok(Scope {
            binding: match ids.binding {
                Some(binding) => Some(
                    visible_binding(&self.engine, company_id, self.actor()?, binding)
                        .await?
                        .binding,
                ),
                None => None,
            },
            workflow: match ids.workflow {
                Some(workflow) => Some(company_workflow(&self.engine, company_id, workflow).await?),
                None => None,
            },
            agent: match ids.agent {
                Some(agent) => Some(company_agent(&self.engine, company_id, agent).await?),
                None => None,
            },
        })
    }

    /// The workflows and agents the scope controls offer, each at most [`MAX_BINDING_OPTIONS`]
    /// by label, with whether it was cut.
    async fn scope_options(&self, company: Uuid) -> AppResult<pages::ScopeOptions> {
        let definitions = self
            .engine
            .reads()
            .list_definitions(
                super::company(company),
                PageRequest::new(None, Some(MAX_BINDING_OPTIONS))?,
            )
            .await?;
        let workflows = definitions
            .items
            .into_iter()
            .map(|definition| pages::ScopeOption {
                id: definition.workflow.as_uuid(),
                label: match (definition.title.is_empty(), definition.archived) {
                    (true, _) => "(untitled workflow)".to_owned(),
                    (false, true) => format!("{} (archived)", definition.title),
                    (false, false) => definition.title,
                },
            })
            .collect();
        let (agents, agents_cut) = self
            .engine
            .authoring_reads()
            .agent_options(super::company(company))
            .await?;
        let agents = agents
            .into_iter()
            .map(|agent| pages::ScopeOption {
                id: agent.id,
                label: agent.label,
            })
            .collect();
        Ok(pages::ScopeOptions {
            workflows,
            workflows_cut: definitions.more,
            agents,
            agents_cut,
        })
    }

    /// The `#run-board` region of `company`'s top-level runs in the scope `ids` names (resolved
    /// company-scoped; an id outside it is not found), read now, with `selected` highlighted.
    async fn board_html(
        &self,
        company: Uuid,
        ids: ScopeIds,
        window: BoardWindow,
        selected: Option<Uuid>,
        read: BoardRead,
    ) -> AppResult<String> {
        self.board_html_at(company, ids, window, selected, read, chrono::Utc::now())
            .await
    }

    /// [`Self::board_html`] read as of `as_of`: the window's cutoff and the cards' ages count from
    /// it, so every read (each wake-up) recomputes them.
    async fn board_html_at(
        &self,
        company: Uuid,
        ids: ScopeIds,
        window: BoardWindow,
        selected: Option<Uuid>,
        read: BoardRead,
        as_of: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<String> {
        let company_id = super::company(company);
        let actor = self.actor()?;
        let resolved = self.scope(company, ids).await?;
        let scope = RunBoardScope {
            binding: resolved.binding,
            workflow: resolved.workflow,
            agent: resolved.agent,
        };
        let board = visible_board(&self.engine, company_id, actor, &scope, window, as_of).await?;
        let workflows: Vec<Uuid> = board
            .columns
            .iter()
            .flat_map(|column| column.cards.iter().map(|card| card.workflow.as_uuid()))
            .collect();
        let titles = self
            .engine
            .run_views()
            .workflow_titles(company_id, &workflows)
            .await?;
        let options = match read {
            BoardRead::Region => self.scope_options(company).await?,
            BoardRead::Columns => pages::ScopeOptions::default(),
        };
        Ok(pages::run_board(&pages::RunBoardView {
            company_id: company,
            columns: &board.columns,
            children: &board.children,
            titles: &titles,
            window,
            scope: ids.view(None),
            options: &options,
            selected,
            as_of,
        }))
    }

    /// Up to `size` rows of the list after `after`, as the caller may see them.
    async fn run_page(
        &self,
        company: Uuid,
        filter: RunFilter,
        after: Option<Uuid>,
        size: i64,
    ) -> AppResult<RunPage> {
        let company_id = super::company(company);
        let page = self
            .engine
            .reads()
            .list_runs(company_id, &filter, PageRequest::new(after, Some(size))?)
            .await?;
        let next = page
            .more
            .then(|| page.items.last().map(|last| last.run.as_uuid()))
            .flatten();
        let mut visibility = Visibility::new(&self.engine, company_id, self.actor()?);
        let mut visible = Vec::new();
        let mut hidden = 0;
        for run in page.items {
            if visibility.allows(run.association).await? {
                visible.push(run);
            } else {
                hidden += 1;
            }
        }
        visibility.report("workflow-runs-ui");
        let workflows: Vec<Uuid> = visible.iter().map(|run| run.workflow.as_uuid()).collect();
        let titles = self
            .engine
            .run_views()
            .workflow_titles(company_id, &workflows)
            .await?;
        Ok(RunPage {
            items: visible,
            hidden,
            next,
            titles,
        })
    }
}

/// GET /ui/workflow-runs — the workspace, with `run_id` open when given: the list, or with
/// `view=board` the board.
async fn workspace(ui: Ui, Query(query): Query<WorkspaceQuery>) -> AppResult<Html<String>> {
    if pages::RunsView::parse(query.view.as_deref()) == pages::RunsView::Board {
        return board_workspace(ui, query).await;
    }
    let frame = ui.frame(query.company_id).await?;
    let Some(company) = frame.company.as_ref() else {
        return Ok(Html(frame.render(&ui.config, |_, _| String::new())));
    };
    let company_id = company.id;
    let state = state_filter(query.state.as_deref());
    let ids = ScopeIds::of_workspace(&query);
    let scope = ui.scope(company_id, ids).await?;
    let page = ui
        .run_page(
            company_id,
            run_filter(scope, state.clone()),
            None,
            LIST_PAGE,
        )
        .await?;
    let options = ui.scope_options(company_id).await?;
    let list_html = pages::run_list(&pages::RunListView {
        company_id,
        items: &page.items,
        hidden: page.hidden,
        next: page.next,
        selected: query.run_id,
        read: page.read(),
        filter: ids.view(state.as_deref()),
        titles: &page.titles,
        options: &options,
    });
    let pane_html = match query.run_id {
        Some(run) => ui.run_pane_html(company_id, run, None).await?,
        None => pages::run_empty_pane("Choose a run to see what it did."),
    };
    Ok(Html(frame.render(&ui.config, |user, company| {
        pages::runs_workspace_page(&pages::RunsWorkspace {
            user,
            company,
            list_html: &list_html,
            pane_html: &pane_html,
        })
    })))
}

/// The board workspace, or with `part=columns` its `#run-board` region alone (a filter change
/// or a wake-up), for a company the caller manages.
async fn board_workspace(ui: Ui, query: WorkspaceQuery) -> AppResult<Html<String>> {
    let window = pages::board_window(query.window.as_deref());
    if query.part.as_deref() == Some("columns") {
        let requested = query
            .company_id
            .ok_or_else(|| AppError::NotFound("Company not found".into()))?;
        let company = ui.company(requested).await?.id;
        return Ok(Html(
            ui.board_html(
                company,
                ScopeIds::of_workspace(&query),
                window,
                query.run_id,
                BoardRead::of(&query),
            )
            .await?,
        ));
    }
    let frame = ui.frame(query.company_id).await?;
    let Some(company) = frame.company.as_ref() else {
        return Ok(Html(frame.render(&ui.config, |_, _| String::new())));
    };
    let company_id = company.id;
    let board_html = ui
        .board_html(
            company_id,
            ScopeIds::of_workspace(&query),
            window,
            query.run_id,
            BoardRead::Region,
        )
        .await?;
    let pane_html = match query.run_id {
        Some(run) => Some(ui.run_pane_html(company_id, run, None).await?),
        None => None,
    };
    Ok(Html(frame.render(&ui.config, |user, company| {
        pages::runs_board_page(&pages::RunsBoardPage {
            user,
            company,
            board_html: &board_html,
            pane_html: pane_html.as_deref(),
        })
    })))
}

/// GET /ui/workflow-runs/list — the list region, its entries again, or the next page (see
/// [`ListPart`]).
async fn list(ui: Ui, Query(query): Query<ListQuery>) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let state = state_filter(query.state.as_deref());
    let shown = query.shown.unwrap_or(0).min(MAX_LISTED as usize);
    let (after, size) = match query.part {
        Some(ListPart::More) => (query.after, (MAX_LISTED - shown as i64).clamp(1, LIST_PAGE)),
        // As many rows as the list held, from the top, so a wake-up keeps what "Load more" added
        // (up to the bound of one read).
        Some(ListPart::Entries) => (
            None,
            i64::try_from(shown)
                .unwrap_or(MAX_LISTED)
                .clamp(LIST_PAGE, MAX_LISTED),
        ),
        None => (None, LIST_PAGE),
    };
    let ids = ScopeIds::of_list(&query);
    let scope = ui.scope(company, ids).await?;
    let page = ui
        .run_page(company, run_filter(scope, state.clone()), after, size)
        .await?;
    // Only the whole region shows the controls; a wake-up or "Load more" reads no options.
    let options = match query.part {
        None => ui.scope_options(company).await?,
        Some(_) => pages::ScopeOptions::default(),
    };
    let view = pages::RunListView {
        company_id: company,
        items: &page.items,
        hidden: page.hidden,
        next: page.next,
        selected: query.run_id,
        read: match query.part {
            Some(ListPart::More) => shown + page.read(),
            _ => page.read(),
        },
        filter: ids.view(state.as_deref()),
        titles: &page.titles,
        options: &options,
    };
    Ok(Html(match query.part {
        Some(ListPart::More | ListPart::Entries) => pages::run_list_update(&view),
        None => pages::run_list(&view),
    }))
}

fn run_filter(scope: Scope, state: Option<String>) -> RunFilter {
    RunFilter {
        binding: scope.binding,
        workflow: scope.workflow,
        state,
        agent: scope.agent,
        // The list is the forensic feed: children stay in it, labelled with their parent.
        lineage: RunLineage::Any,
    }
}

/// A run's pane, its graph with progress, and the people it names.
struct PaneParts {
    view: RunView,
    graph_html: String,
    people: HashMap<Uuid, String>,
}

impl PaneParts {
    fn view<'a>(
        &'a self,
        company: Uuid,
        viewer: Uuid,
        epoch: Uuid,
        banner: Option<pages::RunBanner<'a>>,
    ) -> pages::RunPaneView<'a> {
        pages::RunPaneView {
            company_id: company,
            viewer,
            epoch,
            run: &self.view,
            graph_html: &self.graph_html,
            people: &self.people,
            banner,
        }
    }
}

/// Which part of the pane a GET answers: the whole pane, or the body its wake-ups reload.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PanePart {
    Body,
}

#[derive(Debug, Deserialize)]
struct PaneQuery {
    company_id: Uuid,
    part: Option<PanePart>,
    /// The pane's epoch, sent back by a body reload (see `pages::control_key`).
    epoch: Option<Uuid>,
}

/// GET /ui/workflow-runs/{run} — the run's pane, or with `part=body` its body alone (a
/// wake-up). Opening a run also tells the list to move its highlight.
async fn pane(
    ui: Ui,
    Path(run): Path<Uuid>,
    Query(query): Query<PaneQuery>,
) -> AppResult<Response> {
    let company = ui.company(query.company_id).await?.id;
    let parts = ui.pane_parts(company, run).await?;
    Ok(match query.part {
        Some(PanePart::Body) => {
            let epoch = query.epoch.unwrap_or_else(Uuid::new_v4);
            Html(pages::run_body(&parts.view(company, ui.user, epoch, None))).into_response()
        }
        None => selected(
            Html(pages::run_pane(&parts.view(
                company,
                ui.user,
                Uuid::new_v4(),
                None,
            )))
            .into_response(),
        ),
    })
}

/// `response`, telling the list once it has settled that another run is open
/// (`#run-selected` now names it).
pub(super) fn selected(mut response: Response) -> Response {
    response.headers_mut().insert(
        "HX-Trigger-After-Settle",
        axum::http::HeaderValue::from_static("run-selected"),
    );
    response
}

/// The agent a model step ran, by the handle its frozen inputs name, from the run's bundle.
fn frozen_agent<'a>(
    bundle: &'a crate::application::workflow::publication::PublishedBundle,
    step: &StepView,
) -> Option<pages::FrozenAgentView<'a>> {
    let handle = step.frozen_inputs.as_ref()?.get("agent")?.as_str()?;
    let agent = bundle
        .snapshots()
        .agents
        .iter()
        .find(|agent| agent.key.as_str() == handle)?;
    Some(pages::FrozenAgentView {
        handle: agent.key.as_str(),
        model: format!("{} / {}", agent.model.provider.as_str(), agent.model.model),
        tools: agent.tools.iter().map(|tool| tool.as_str()).collect(),
        skills: agent.skills.iter().map(|skill| skill.as_str()).collect(),
    })
}

/// GET /ui/workflow-runs/{run}/steps/{execution} — one step's body, when its row opens.
async fn step(
    ui: Ui,
    Path((run, execution)): Path<(Uuid, Uuid)>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    ui.visible_run(company, run).await?;
    let company_id = super::company(company);
    let step = ui
        .engine
        .run_views()
        .step_view(company_id, RunId::new(run), execution)
        .await?
        .ok_or_else(|| AppError::NotFound("Workflow step not found".into()))?;
    let bundle = ui
        .engine
        .run_views()
        .run_bundle(company_id, RunId::new(run))
        .await?;
    let agent = bundle
        .as_deref()
        .and_then(|bundle| frozen_agent(bundle, &step));
    Ok(Html(pages::run_step_body(&pages::StepBodyView {
        company_id: company,
        step: &step,
        agent,
    })))
}

#[cfg(test)]
#[path = "runs_tests.rs"]
mod tests;
