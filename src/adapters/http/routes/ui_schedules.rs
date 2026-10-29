//! `/ui/schedules` — the Schedules workspace:
//! 1st column: Schedule name items
//! 2nd column: Paginated schedule occurrences and the workflow runs they started
//! 3rd column: The schedule form

use std::sync::Arc;

use axum::{
    Form, Router,
    extract::{FromRequestParts, Path, Query},
    http::request::Parts,
    response::{Html, IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{
        app_state::AppState,
        auth::{AuthError, AuthenticatedUser},
        pages,
        routes::{
            schedule::{UiScheduleForm, run_now_key},
            ui::{load_account, load_managed_company, managed_company_membership, workspace_user},
        },
    },
    app_error::{AppError, AppResult},
    entities::{company::Company, schedule::ScheduleRun, value_objects::EmailAddress},
    infra::config::AppConfig,
    use_cases::{
        channel::ChannelUseCases,
        company::CompanyUseCases,
        schedule::{ScheduleBindingChoice, ScheduleUseCases},
        user::UserUseCases,
    },
};

/// Every path this workspace serves, named once so the pages and the router cannot disagree.
///
/// A button pointing at a path nobody routes is a silent 404 — the page renders, the click does
/// nothing. `schedule_pages_only_target_routed_paths` walks the rendered HTML against this list,
/// so adding a button without its route fails the build instead of the user's next click.
pub(crate) const SCHEDULE_UI_PATHS: &[&str] = &[
    "/ui/schedules",
    "/ui/schedules/new",
    "/ui/schedules/close",
    "/ui/schedules/runs",
    "/ui/schedules/{id}",
    "/ui/schedules/{id}/edit",
    "/ui/schedules/{id}/run-now",
    "/ui/schedules/{id}/toggle",
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            SCHEDULE_UI_PATHS[0],
            get(schedules_page).post(create_schedule),
        )
        .route(SCHEDULE_UI_PATHS[1], get(create_pane))
        .route(SCHEDULE_UI_PATHS[2], get(close_pane))
        .route(SCHEDULE_UI_PATHS[3], get(schedule_runs_fragment))
        .route(
            SCHEDULE_UI_PATHS[4],
            get(schedule_page_redirect)
                .put(update_schedule)
                .delete(delete_schedule),
        )
        .route(SCHEDULE_UI_PATHS[5], get(edit_schedule_pane))
        .route(SCHEDULE_UI_PATHS[6], axum::routing::post(run_schedule_now))
        .route(SCHEDULE_UI_PATHS[7], axum::routing::post(toggle_schedule))
}

#[derive(Debug, Clone, Deserialize)]
pub struct SchedulesQuery {
    pub company_id: Option<Uuid>,
    pub schedule_id: Option<Uuid>,
    pub page: Option<usize>,
    pub new: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompanyScopedQuery {
    pub company_id: Uuid,
    pub schedule_id: Option<Uuid>,
    pub page: Option<usize>,
}

struct SchedulesWorkspace {
    company_use_cases: Arc<CompanyUseCases>,
    channel_use_cases: Arc<ChannelUseCases>,
    schedule_use_cases: Arc<ScheduleUseCases>,
    user_use_cases: Arc<UserUseCases>,
    config: Arc<AppConfig>,
    user_id: Uuid,
}

#[derive(Debug, Clone, Copy)]
enum RunsColumnRender {
    LiveRegion,
    RefreshFragment,
}

impl FromRequestParts<AppState> for SchedulesWorkspace {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;

        Ok(Self {
            company_use_cases: state.company_use_cases.clone(),
            channel_use_cases: state.channel_use_cases.clone(),
            schedule_use_cases: state.schedule_use_cases.clone(),
            user_use_cases: state.user_use_cases.clone(),
            config: state.config.clone(),
            user_id: user.id,
        })
    }
}

impl SchedulesWorkspace {
    async fn scoped_company(
        &self,
        company_id: Option<Uuid>,
    ) -> AppResult<(Vec<Company>, Option<Company>)> {
        load_managed_company(&self.company_use_cases, self.user_id, company_id).await
    }

    /// The workflow bindings a schedule may name, and the listed ones already do.
    async fn bindings(&self, company_id: Uuid) -> AppResult<Vec<ScheduleBindingChoice>> {
        self.schedule_use_cases
            .binding_choices(self.user_id, company_id, None)
            .await
    }

    /// One page of a schedule's runs. `PAGE_SIZE + 1` is fetched so the pager knows whether a
    /// next page exists without counting the whole table.
    async fn runs_page(
        &self,
        company_id: Uuid,
        schedule_id: Uuid,
        page: usize,
    ) -> AppResult<(Vec<ScheduleRun>, bool)> {
        let offset = (page as i64 - 1) * PAGE_SIZE;
        let runs = self
            .schedule_use_cases
            .list_schedule_runs(self.user_id, company_id, schedule_id, offset, PAGE_SIZE + 1)
            .await?;

        let has_next = runs.len() as i64 > PAGE_SIZE;
        Ok((
            runs.into_iter().take(PAGE_SIZE as usize).collect(),
            has_next,
        ))
    }

    /// The runs column as a standalone fragment, which is what every write in this workspace
    /// swaps back into the page.
    async fn runs_column(
        &self,
        company_id: Uuid,
        schedule_id: Uuid,
        page: Option<usize>,
    ) -> AppResult<Html<String>> {
        self.render_runs_column(company_id, schedule_id, page, RunsColumnRender::LiveRegion)
            .await
    }

    async fn runs_column_fragment(
        &self,
        company_id: Uuid,
        schedule_id: Uuid,
        page: Option<usize>,
    ) -> AppResult<Html<String>> {
        self.render_runs_column(
            company_id,
            schedule_id,
            page,
            RunsColumnRender::RefreshFragment,
        )
        .await
    }

    async fn render_runs_column(
        &self,
        company_id: Uuid,
        schedule_id: Uuid,
        page: Option<usize>,
        render: RunsColumnRender,
    ) -> AppResult<Html<String>> {
        let page = page.unwrap_or(1).max(1);
        let schedule = self
            .schedule_use_cases
            .get_schedule(self.user_id, company_id, schedule_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Schedule not found".into()))?;

        let channel = self
            .channel_use_cases
            .get_company_channel(self.user_id, company_id, schedule.channel_id)
            .await?;

        let (runs, has_next) = self.runs_page(company_id, schedule.id, page).await?;

        let props = pages::ScheduleRunsColumnProps {
            company_id,
            schedule: &schedule,
            channel: channel.as_ref(),
            runs: &runs,
            page,
            has_next,
        };
        let html = match render {
            RunsColumnRender::LiveRegion => {
                pages::schedule_runs_column(&props, pages::FragmentSwap::Inline)
            }
            RunsColumnRender::RefreshFragment => pages::schedule_runs_column_fragment(&props),
        };

        Ok(Html(html))
    }
}

const PAGE_SIZE: i64 = 15;

#[instrument(skip_all)]
async fn schedules_page(
    workspace: SchedulesWorkspace,
    headers: axum::http::HeaderMap,
    Query(query): Query<SchedulesQuery>,
) -> AppResult<Html<String>> {
    let account = load_account(&workspace.user_use_cases, workspace.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let workspace_user = workspace_user(&account, &account_email, &workspace.config);

    let (companies, company) = workspace.scoped_company(query.company_id).await?;
    let Some(company) = company else {
        return Ok(Html(pages::mailbox_no_company_page(&workspace_user)));
    };
    let workspace_user = workspace_user
        .with_company_membership(managed_company_membership(&company, workspace.user_id));

    let schedules = workspace
        .schedule_use_cases
        .list_company_schedules(workspace.user_id, company.id)
        .await?;

    let channels = workspace
        .channel_use_cases
        .list_company_channels(workspace.user_id, company.id)
        .await?;

    let bindings = workspace.bindings(company.id).await?;

    let selected_schedule = query
        .schedule_id
        .and_then(|id| schedules.iter().find(|s| s.id == id))
        .or_else(|| schedules.first());

    let creating = matches!(query.new.as_deref(), Some("1") | Some("true"));

    let page_num = query.page.unwrap_or(1).max(1);
    let offset = (page_num as i64 - 1) * PAGE_SIZE;

    let (runs_html, pane_html) = if creating {
        let form_pane = pages::schedule_form_pane(&pages::ScheduleFormPaneProps {
            company_id: company.id,
            channels: &channels,
            schedule: None,
            bindings: &bindings,
            error: None,
        });
        (String::new(), form_pane)
    } else if let Some(schedule) = selected_schedule {
        let channel = channels.iter().find(|c| c.id == schedule.channel_id);
        let runs = workspace
            .schedule_use_cases
            .list_schedule_runs(
                workspace.user_id,
                company.id,
                schedule.id,
                offset,
                PAGE_SIZE + 1,
            )
            .await?;

        let has_next = runs.len() as i64 > PAGE_SIZE;
        let display_runs: Vec<ScheduleRun> = runs.into_iter().take(PAGE_SIZE as usize).collect();

        let runs_col = pages::schedule_runs_column(
            &pages::ScheduleRunsColumnProps {
                company_id: company.id,
                schedule,
                channel,
                runs: &display_runs,
                page: page_num,
                has_next,
            },
            pages::FragmentSwap::Inline,
        );
        let pane = pages::schedules_empty_pane(
            if display_runs.is_empty() {
                "No runs yet. Click 'Run Now' to start one."
            } else {
                "Each occurrence lists the workflow run it started. Click 'Edit' to change the schedule."
            },
            pages::FragmentSwap::Inline,
        );

        (runs_col, pane)
    } else {
        (
            String::new(),
            pages::schedules_empty_pane(
                "No schedules yet. Create your first automated schedule using the button on the left.",
                pages::FragmentSwap::Inline,
            ),
        )
    };

    if headers.get("HX-Target").and_then(|v| v.to_str().ok()) == Some("schedules-workspace") {
        return Ok(Html(format!("{runs_html}{pane_html}")));
    }

    Ok(Html(pages::schedules_page(&pages::SchedulesPage {
        user: &workspace_user,
        companies: &companies,
        company: &company,
        schedules: &schedules,
        selected_schedule_id: selected_schedule.map(|s| s.id),
        bindings: &bindings,
        runs_html: &runs_html,
        pane_html: &pane_html,
    })))
}

#[instrument(skip_all)]
async fn schedule_runs_fragment(
    workspace: SchedulesWorkspace,
    Query(query): Query<CompanyScopedQuery>,
) -> AppResult<Html<String>> {
    let Some(schedule_id) = query.schedule_id else {
        return Ok(Html(String::new()));
    };

    // The live region polls this for its column, so it answers with the column alone.
    workspace
        .runs_column_fragment(query.company_id, schedule_id, query.page)
        .await
}

#[instrument(skip_all)]
async fn create_pane(
    workspace: SchedulesWorkspace,
    Query(query): Query<CompanyScopedQuery>,
) -> AppResult<Html<String>> {
    let (_, company) = workspace.scoped_company(Some(query.company_id)).await?;
    let Some(company) = company else {
        return Ok(Html(String::new()));
    };

    let channels = workspace
        .channel_use_cases
        .list_company_channels(workspace.user_id, company.id)
        .await?;

    let bindings = workspace.bindings(company.id).await?;

    Ok(Html(pages::schedule_form_pane(
        &pages::ScheduleFormPaneProps {
            company_id: company.id,
            channels: &channels,
            schedule: None,
            bindings: &bindings,
            error: None,
        },
    )))
}

#[instrument(skip_all)]
async fn edit_schedule_pane(
    workspace: SchedulesWorkspace,
    Path(id): Path<Uuid>,
    Query(query): Query<CompanyScopedQuery>,
) -> AppResult<Html<String>> {
    let (_, company) = workspace.scoped_company(Some(query.company_id)).await?;
    let Some(company) = company else {
        return Ok(Html(String::new()));
    };

    let schedule = workspace
        .schedule_use_cases
        .get_schedule(workspace.user_id, company.id, id)
        .await?
        .ok_or_else(|| AppError::NotFound("Schedule not found".into()))?;

    let channels = workspace
        .channel_use_cases
        .list_company_channels(workspace.user_id, company.id)
        .await?;

    let bindings = workspace.bindings(company.id).await?;

    Ok(Html(pages::schedule_form_pane(
        &pages::ScheduleFormPaneProps {
            company_id: company.id,
            channels: &channels,
            schedule: Some(&schedule),
            bindings: &bindings,
            error: None,
        },
    )))
}

/// What the Run Now and Pause buttons submit: the company scope, plus the state Pause is asking
/// for or Run Now's command key. Both re-render the runs column they live in.
#[derive(Debug, Deserialize)]
pub struct ScheduleActionForm {
    pub company_id: Uuid,
    pub page: Option<usize>,
    pub enabled: Option<String>,
    pub command_key: Option<String>,
}

impl ScheduleActionForm {
    fn wants_enabled(&self) -> bool {
        // A checkbox posts "on"; the Pause/Resume button posts the state it is switching to.
        matches!(self.enabled.as_deref(), Some("true") | Some("on"))
    }
}

#[instrument(skip_all)]
async fn run_schedule_now(
    workspace: SchedulesWorkspace,
    Path(id): Path<Uuid>,
    Form(form): Form<ScheduleActionForm>,
) -> AppResult<Html<String>> {
    workspace
        .schedule_use_cases
        .trigger_schedule_now(
            workspace.user_id,
            form.company_id,
            id,
            &run_now_key(form.command_key),
        )
        .await?;

    workspace.runs_column(form.company_id, id, form.page).await
}

#[instrument(skip_all)]
async fn toggle_schedule(
    workspace: SchedulesWorkspace,
    Path(id): Path<Uuid>,
    Form(form): Form<ScheduleActionForm>,
) -> AppResult<Html<String>> {
    workspace
        .schedule_use_cases
        .toggle_schedule(workspace.user_id, form.company_id, id, form.wants_enabled())
        .await?;

    let Html(runs_column) = workspace
        .runs_column(form.company_id, id, form.page)
        .await?;
    let schedules = workspace
        .schedule_use_cases
        .list_company_schedules(workspace.user_id, form.company_id)
        .await?;
    let bindings = workspace.bindings(form.company_id).await?;
    let sidebar = pages::schedules_sidebar_list(
        form.company_id,
        &schedules,
        Some(id),
        &bindings,
        pages::FragmentSwap::OutOfBand,
    );

    // The button targets the runs column; htmx applies the sidebar copy separately via OOB swap.
    Ok(Html(format!("{runs_column}{sidebar}")))
}

#[instrument(skip_all)]
async fn close_pane(
    _workspace: SchedulesWorkspace,
    Query(_query): Query<CompanyScopedQuery>,
) -> AppResult<Html<String>> {
    Ok(Html(pages::schedules_empty_pane(
        "Select a schedule on the left.",
        pages::FragmentSwap::Inline,
    )))
}

/// The schedule form plus the channel it is being filed under. The schedule half is
/// [`UiScheduleForm`] itself rather than a copy of its fields, so the two forms cannot drift.
#[derive(Debug, Deserialize)]
pub struct CreateScheduleForm {
    pub channel_id: Uuid,
    #[serde(flatten)]
    pub schedule: UiScheduleForm,
}

#[instrument(skip_all)]
async fn create_schedule(
    workspace: SchedulesWorkspace,
    Form(form): Form<CreateScheduleForm>,
) -> AppResult<Response> {
    let company_id = form.schedule.company_id;
    let channel_id = form.channel_id;
    let write = form.schedule.into_write()?;

    let created = workspace
        .schedule_use_cases
        .create_schedule(workspace.user_id, company_id, channel_id, write)
        .await?;

    Ok((
        [(
            "HX-Redirect",
            format!(
                "/ui/schedules?company_id={company_id}&schedule_id={}",
                created.id
            ),
        )],
        (),
    )
        .into_response())
}

#[instrument(skip_all)]
async fn update_schedule(
    workspace: SchedulesWorkspace,
    Path(id): Path<Uuid>,
    Form(form): Form<CreateScheduleForm>,
) -> AppResult<Response> {
    let company_id = form.schedule.company_id;
    // The edit form carries the fixed channel as scope; tampering is rejected by the use case.
    let channel_id = form.channel_id;
    let write = form.schedule.into_write()?;

    let updated = workspace
        .schedule_use_cases
        .update_schedule(workspace.user_id, company_id, id, channel_id, write)
        .await?;

    Ok((
        [(
            "HX-Redirect",
            format!(
                "/ui/schedules?company_id={company_id}&schedule_id={}",
                updated.id
            ),
        )],
        (),
    )
        .into_response())
}

#[instrument(skip_all)]
async fn delete_schedule(
    workspace: SchedulesWorkspace,
    Path(id): Path<Uuid>,
    Form(query): Form<CompanyScopedQuery>,
) -> AppResult<Response> {
    let company_id = query.company_id;

    workspace
        .schedule_use_cases
        .delete_schedule(workspace.user_id, company_id, id)
        .await?;

    Ok((
        [(
            "HX-Redirect",
            format!("/ui/schedules?company_id={company_id}"),
        )],
        (),
    )
        .into_response())
}

#[instrument(skip_all)]
async fn schedule_page_redirect(
    _workspace: SchedulesWorkspace,
    Path(id): Path<Uuid>,
    Query(query): Query<CompanyScopedQuery>,
) -> AppResult<Response> {
    let company_id = query.company_id;
    Ok((
        [(
            "HX-Redirect",
            format!("/ui/schedules?company_id={company_id}&schedule_id={id}"),
        )],
        (),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::http::pages::{
        FragmentSwap, MailboxUser, ScheduleRunsColumnProps, SchedulesPage, schedule_runs_column,
        schedule_runs_column_fragment, schedules_page,
    };
    use crate::entities::{
        company::Company,
        company_member::CompanyMembership,
        schedule::{
            ChannelSchedule, OccurrenceStatus, ScheduleRun, ScheduleTimezone, ScheduleType,
        },
    };
    use chrono::Utc;

    fn test_company() -> Company {
        Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Schedule Corp".into(),
            slug: "sched".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }
    }

    fn test_schedule(company_id: Uuid, channel_id: Uuid) -> ChannelSchedule {
        ChannelSchedule {
            id: Uuid::new_v4(),
            company_id,
            channel_id,
            binding_id: Uuid::new_v4(),
            name: "Morning Report".into(),
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(3600),
            timezone: ScheduleTimezone::utc(),
            enabled: true,
            last_run_at: None,
            next_run_at: Some(Utc::now()),
            last_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn binding_of(schedule: &ChannelSchedule) -> ScheduleBindingChoice {
        ScheduleBindingChoice {
            binding_id: schedule.binding_id,
            channel_id: None,
            title: "Daily digest".into(),
            active: true,
        }
    }

    fn occurrence(status: OccurrenceStatus) -> ScheduleRun {
        ScheduleRun {
            id: Uuid::new_v4(),
            scheduled_for: Utc::now(),
            manual: false,
            status,
            run_id: None,
            run_state: None,
            refusal: None,
            last_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn admin(email: &EmailAddress) -> MailboxUser<'_> {
        MailboxUser {
            id: Uuid::new_v4(),
            username: "admin",
            email,
            avatar_url: None,
            is_operator: false,
            company_membership: CompanyMembership::Admin,
        }
    }

    /// Every `/ui/schedules` link and form in the rendered workspace must point at a path the
    /// router actually serves. Run Now and Pause shipped pointing at paths nobody routed, which
    /// renders perfectly and 404s on click — nothing else in the suite catches that.
    #[test]
    fn schedule_pages_only_target_routed_paths() {
        /// A rendered URL against a route pattern, `{...}` matching one segment.
        fn matches(pattern: &str, path: &str) -> bool {
            let pattern: Vec<&str> = pattern.split('/').collect();
            let path: Vec<&str> = path.split('/').collect();
            pattern.len() == path.len()
                && pattern
                    .iter()
                    .zip(&path)
                    .all(|(p, actual)| p.starts_with('{') || p == actual)
        }

        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let schedule = test_schedule(company_id, channel_id);
        let bindings = [binding_of(&schedule)];
        let user_email = EmailAddress::from("admin@example.com");
        let user = admin(&user_email);
        let run = occurrence(OccurrenceStatus::Materialized);

        // Every surface of the workspace, so a button on any of them is covered.
        let rendered = [
            schedules_page(&SchedulesPage {
                user: &user,
                companies: std::slice::from_ref(&test_company()),
                company: &test_company(),
                schedules: std::slice::from_ref(&schedule),
                selected_schedule_id: Some(schedule.id),
                bindings: &bindings,
                runs_html: "",
                pane_html: "",
            }),
            schedule_runs_column(
                &ScheduleRunsColumnProps {
                    company_id,
                    schedule: &schedule,
                    channel: None,
                    runs: std::slice::from_ref(&run),
                    page: 1,
                    has_next: true,
                },
                FragmentSwap::Inline,
            ),
            pages::schedule_form_pane(&pages::ScheduleFormPaneProps {
                company_id,
                channels: &[],
                schedule: Some(&schedule),
                bindings: &bindings,
                error: None,
            }),
        ]
        .join("\n");

        let targets: Vec<&str> = rendered
            .split('"')
            .filter(|token| token.starts_with("/ui/schedules"))
            .collect();

        assert!(
            targets.len() >= 8,
            "expected the workspace to emit its own links, found {targets:?}"
        );

        for target in targets {
            let path = target.split('?').next().unwrap_or(target);
            assert!(
                SCHEDULE_UI_PATHS.iter().any(|route| matches(route, path)),
                "the workspace links to {path}, which no route in SCHEDULE_UI_PATHS serves"
            );
        }
    }

    #[test]
    fn schedules_page_renders_3_columns_and_sidebar_items() {
        let company = test_company();
        let schedule = test_schedule(company.id, Uuid::new_v4());
        let user_email = EmailAddress::from("admin@example.com");
        let user = admin(&user_email);

        let html = schedules_page(&SchedulesPage {
            user: &user,
            companies: std::slice::from_ref(&company),
            company: &company,
            schedules: std::slice::from_ref(&schedule),
            selected_schedule_id: Some(schedule.id),
            bindings: &[binding_of(&schedule)],
            runs_html: r#"<div id="schedule-runs-column">Runs Column</div>"#,
            pane_html: r#"<div id="schedule-pane">Detail Pane</div>"#,
        });

        // Icon rail should light the Schedules section
        assert!(html.contains("href=\"/ui/schedules?company_id="));
        assert!(html.contains("btn-primary\" title=\"Schedules\""));

        // Column 1: Sidebar list with schedule name, cadence and the workflow it runs
        assert!(html.contains("Morning Report"));
        assert!(html.contains("Every hour"));
        assert!(html.contains("Daily digest"));

        // Column 2 & 3 wrapped in workspace
        assert!(html.contains("id=\"schedules-workspace\""));
        assert!(html.contains("Runs Column"));
        assert!(html.contains("Detail Pane"));
    }

    #[test]
    fn schedule_runs_column_renders_occurrences_with_their_runs() {
        let company_id = Uuid::new_v4();
        let schedule = test_schedule(company_id, Uuid::new_v4());
        let mut started = occurrence(OccurrenceStatus::Materialized);
        let run_id = Uuid::new_v4();
        started.run_id = Some(run_id);
        started.run_state = Some("running".into());
        let mut refused = occurrence(OccurrenceStatus::Failed);
        refused.manual = true;
        refused.refusal = Some("unavailable".into());
        refused.last_error =
            Some("The workflow binding refused this occurrence (unavailable)".into());
        let runs = [started, refused];

        let html = schedule_runs_column(
            &ScheduleRunsColumnProps {
                company_id,
                schedule: &schedule,
                channel: None,
                runs: &runs,
                page: 1,
                has_next: false,
            },
            FragmentSwap::Inline,
        );

        assert!(html.contains("Started"));
        assert!(html.contains(&run_id.simple().to_string()[..8]));
        assert!(html.contains("running"));
        assert!(html.contains("Failed"));
        assert!(html.contains("run now"));
        assert!(html.contains("refused this occurrence (unavailable)"));
        assert!(html.contains("Run Now"));
        assert!(html.contains(r#"name="command_key""#));
        assert!(html.contains(r#"id="schedule-runs-live" class="contents""#));
        assert!(html.contains(&format!(
            r#"hx-get="/ui/schedules/runs?company_id={company_id}&schedule_id={}&page=1""#,
            schedule.id
        )));
        assert!(html.contains(r#"hx-trigger="every 15s""#));
        assert!(html.contains(r##"hx-target="#schedule-runs-column""##));

        let refresh = schedule_runs_column_fragment(&ScheduleRunsColumnProps {
            company_id,
            schedule: &schedule,
            channel: None,
            runs: &runs,
            page: 1,
            has_next: false,
        });
        assert!(refresh.contains(r#"id="schedule-runs-column""#));
        assert!(!refresh.contains("hx-trigger"));
        assert!(!refresh.contains(r#"id="schedule-runs-live""#));
    }

    /// The browser's Create Schedule submission, field for field, must reach the handler.
    /// The pane posts every control in the form — including the ones its show/hide boxes have
    /// hidden — so the empty `scheduled_at` is part of a normal interval create, and the numeric
    /// cadence arrives as a string like every urlencoded value.
    #[tokio::test]
    async fn create_schedule_form_deserializes_browser_submission() {
        use axum::body::Body;
        use axum::extract::FromRequest;
        use axum::http::{Request, header};

        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let binding_id = Uuid::new_v4();
        let body = format!(
            "company_id={company_id}&name=Daily+Operations+Report&channel_id={channel_id}\
             &schedule_type=interval&interval_seconds=3600&scheduled_at=\
             &timezone=UTC&binding_id={binding_id}"
        )
        .replace(['\n', ' '], "");

        let req = Request::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();

        let form = Form::<CreateScheduleForm>::from_request(req, &())
            .await
            .expect("the Create Schedule form must deserialize, not 422")
            .0;

        assert_eq!(form.channel_id, channel_id);
        assert_eq!(form.schedule.company_id, company_id);
        assert_eq!(form.schedule.name, "Daily Operations Report");
        assert_eq!(form.schedule.interval_seconds, Some(3600));

        let write = form
            .schedule
            .into_write()
            .expect("form must map to a write");
        assert_eq!(write.schedule_type, ScheduleType::Interval);
        assert_eq!(write.scheduled_at, None);
        assert_eq!(write.binding_id, binding_id);
    }

    /// The one-off half of the same pane: the cadence select is hidden but still submits, and the
    /// datetime lands as the browser's `datetime-local` value rather than RFC3339.
    #[tokio::test]
    async fn create_schedule_form_deserializes_one_off_submission() {
        use axum::body::Body;
        use axum::extract::FromRequest;
        use axum::http::{Request, header};

        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let binding_id = Uuid::new_v4();
        let body = format!(
            "company_id={company_id}&name=Quarter+Close&channel_id={channel_id}\
             &schedule_type=one_off&interval_seconds=&scheduled_at=2026-08-25T14%3A30\
             &timezone=UTC&binding_id={binding_id}"
        )
        .replace(['\n', ' '], "");

        let req = Request::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();

        let form = Form::<CreateScheduleForm>::from_request(req, &())
            .await
            .expect("the one-off Create Schedule form must deserialize, not 422")
            .0;

        assert_eq!(form.schedule.interval_seconds, None);

        let write = form
            .schedule
            .into_write()
            .expect("form must map to a write");
        assert_eq!(write.schedule_type, ScheduleType::OneOff);
        assert_eq!(
            write.scheduled_at.map(|at| at.to_rfc3339()),
            Some("2026-08-25T14:30:00+00:00".to_string())
        );
        assert_eq!(write.binding_id, binding_id);
    }

    #[test]
    fn schedule_editor_keeps_channel_fixed_and_creation_keeps_selector() {
        let company = test_company();
        let schedule = test_schedule(company.id, Uuid::new_v4());
        let bindings = [binding_of(&schedule)];
        let mut props = pages::ScheduleFormPaneProps {
            company_id: company.id,
            channels: &[],
            schedule: Some(&schedule),
            bindings: &bindings,
            error: None,
        };
        let edit = pages::schedule_form_pane(&props);
        assert!(!edit.contains(r#"<select name="channel_id""#));
        assert!(edit.contains(&format!(
            r#"<input type="hidden" name="channel_id" value="{}">"#,
            schedule.channel_id
        )));
        props.schedule = None;
        let create = pages::schedule_form_pane(&props);
        assert!(create.contains(r#"<select name="channel_id" required"#));
        assert!(!create.contains(r#"<input type="hidden" name="channel_id""#));
    }

    /// The form names the workflow binding, and no field the binding now owns.
    #[test]
    fn the_schedule_form_picks_a_binding_and_nothing_it_replaced() {
        let company = test_company();
        let schedule = test_schedule(company.id, Uuid::new_v4());
        let other = ScheduleBindingChoice {
            binding_id: Uuid::new_v4(),
            channel_id: Some(schedule.channel_id),
            title: "Weekly audit".into(),
            active: false,
        };
        let bindings = [binding_of(&schedule), other.clone()];
        let form = pages::schedule_form_pane(&pages::ScheduleFormPaneProps {
            company_id: company.id,
            channels: &[],
            schedule: Some(&schedule),
            bindings: &bindings,
            error: None,
        });
        assert!(form.contains(r#"<select name="binding_id" required"#));
        assert!(form.contains(&format!(r#"value="{}" selected"#, schedule.binding_id)));
        assert!(form.contains("Weekly audit (inactive)"));
        for removed in [
            "subject_template",
            "prompt_template",
            "delivery_mode",
            "recipient_emails",
            "run_as_user_id",
        ] {
            assert!(!form.contains(removed), "the form still renders {removed}");
        }
    }
}
