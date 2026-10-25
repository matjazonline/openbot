//! `/ui/workflow-runs` (p09 B2a): the runs workspace, its list, one run's pane and one step of
//! it, the controls (cancel, retry, rerun, the reconciliation annotation) through the JSON
//! routes' own commands, and the run event stream the page reloads on. A run is shown to whoever
//! the JSON routes show it to: an owner or admin of the company who can see the run's channel.
use super::super::bindings::Visibility;
use super::*;
use crate::application::workflow::authoring::workflow_graph;
use crate::application::workflow::reads::{MAX_LISTED, PageRequest, RunFilter, RunSummary};
use crate::application::workflow::run_views::{RunView, StepView};
use crate::domain::workflow::{RunId, WorkflowBindingId};
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
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub company_id: Uuid,
    pub binding_id: Option<Uuid>,
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

#[derive(Debug, Deserialize)]
pub struct CompanyQuery {
    pub company_id: Uuid,
}

/// A state filter value the list accepts; anything else lists every state.
fn state_filter(state: Option<&str>) -> Option<String> {
    state
        .filter(|state| {
            matches!(
                *state,
                "queued" | "running" | "waiting" | "succeeded" | "failed" | "cancelled"
            )
        })
        .map(str::to_owned)
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

/// GET /ui/workflow-runs — the workspace, with `run_id` open when given.
async fn workspace(ui: Ui, Query(query): Query<WorkspaceQuery>) -> AppResult<Html<String>> {
    let frame = ui.frame(query.company_id).await?;
    let Some(company) = frame.company.as_ref() else {
        return Ok(Html(frame.render(&ui.config, |_, _| String::new())));
    };
    let company_id = company.id;
    let state = state_filter(query.state.as_deref());
    let page = ui
        .run_page(
            company_id,
            run_filter(query.binding_id, state.clone()),
            None,
            LIST_PAGE,
        )
        .await?;
    let list_html = pages::run_list(&pages::RunListView {
        company_id,
        items: &page.items,
        hidden: page.hidden,
        next: page.next,
        selected: query.run_id,
        read: page.read(),
        filter: pages::RunFilterView {
            binding: query.binding_id,
            state: state.as_deref(),
        },
        titles: &page.titles,
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
    let page = ui
        .run_page(
            company,
            run_filter(query.binding_id, state.clone()),
            after,
            size,
        )
        .await?;
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
        filter: pages::RunFilterView {
            binding: query.binding_id,
            state: state.as_deref(),
        },
        titles: &page.titles,
    };
    Ok(Html(match query.part {
        Some(ListPart::More | ListPart::Entries) => pages::run_list_update(&view),
        None => pages::run_list(&view),
    }))
}

fn run_filter(binding: Option<Uuid>, state: Option<String>) -> RunFilter {
    RunFilter {
        binding: binding.map(WorkflowBindingId::new),
        state,
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
