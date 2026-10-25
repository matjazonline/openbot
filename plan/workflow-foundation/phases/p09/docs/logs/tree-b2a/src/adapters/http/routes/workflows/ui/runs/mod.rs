//! `/ui/workflow-runs` (p09 B2a): the runs workspace, its list, one run's pane and one step of
//! it, the controls (cancel, retry, rerun, the reconciliation annotation) through the JSON
//! routes' own commands, and the run event stream the page reloads on. A run is shown to whoever
//! the JSON routes show it to: an owner or admin of the company who can see the run's channel.
use super::super::bindings::Visibility;
use super::*;
use crate::application::workflow::authoring::workflow_graph;
use crate::application::workflow::reads::{PageRequest, RunFilter, RunSummary};
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
    /// A "Load more" request: answer the entries only, not the whole region.
    pub more: Option<String>,
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
        Ok(pages::run_pane(&parts.view(company, banner)))
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

    /// One page of the list: the runs the caller may see, how many were left out, the cursor.
    async fn run_page(
        &self,
        company: Uuid,
        binding: Option<Uuid>,
        state: Option<String>,
        after: Option<Uuid>,
    ) -> AppResult<(Vec<RunSummary>, usize, Option<Uuid>, HashMap<Uuid, String>)> {
        let company_id = super::company(company);
        let page = self
            .engine
            .reads()
            .list_runs(
                company_id,
                &RunFilter {
                    binding: binding.map(WorkflowBindingId::new),
                    state,
                },
                PageRequest::new(after, Some(LIST_PAGE))?,
            )
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
        Ok((visible, hidden, next, titles))
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
    let (items, hidden, next, titles) = ui
        .run_page(company_id, query.binding_id, state.clone(), None)
        .await?;
    let list_html = pages::run_list(&pages::RunListView {
        company_id,
        items: &items,
        hidden,
        next,
        selected: query.run_id,
        filter: pages::RunFilterView {
            binding: query.binding_id,
            state: state.as_deref(),
        },
        titles: &titles,
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

/// GET /ui/workflow-runs/list — the list region again (a wake-up or a filter change), or with
/// `more` only the next page's entries.
async fn list(ui: Ui, Query(query): Query<ListQuery>) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let state = state_filter(query.state.as_deref());
    let (items, hidden, next, titles) = ui
        .run_page(company, query.binding_id, state.clone(), query.after)
        .await?;
    let view = pages::RunListView {
        company_id: company,
        items: &items,
        hidden,
        next,
        selected: None,
        filter: pages::RunFilterView {
            binding: query.binding_id,
            state: state.as_deref(),
        },
        titles: &titles,
    };
    Ok(Html(if query.more.is_some() {
        pages::run_list_entries(&view)
    } else {
        pages::run_list(&view)
    }))
}

/// GET /ui/workflow-runs/{run} — `#run-pane`, also what a wake-up reloads.
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
        banner: Option<pages::RunBanner<'a>>,
    ) -> pages::RunPaneView<'a> {
        pages::RunPaneView {
            company_id: company,
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
}

/// GET /ui/workflow-runs/{run} — the run's pane, or with `part=body` its body alone.
async fn pane(
    ui: Ui,
    Path(run): Path<Uuid>,
    Query(query): Query<PaneQuery>,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let parts = ui.pane_parts(company, run).await?;
    let view = parts.view(company, None);
    Ok(Html(match query.part {
        Some(PanePart::Body) => pages::run_body(&view),
        None => pages::run_pane(&view),
    }))
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
