//! `/ui/schedules` — the Schedules workspace:
//! 1st column: Schedule name items
//! 2nd column: Paginated schedule runs (threads)
//! 3rd column: Thread messages and interactive chat / review

use std::{convert::Infallible, sync::Arc};

use axum::{
    Form, Router,
    extract::{FromRequestParts, Path, Query, State},
    http::request::Parts,
    response::{
        Html, IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use serde::Deserialize;
use tokio_stream::{Stream, StreamExt};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{
        app_state::AppState,
        auth::{AuthError, AuthenticatedUser},
        pages,
        routes::{
            channel::reply_headers,
            schedule::UiScheduleForm,
            ui::{load_account, load_scoped_company, wake_ups, workspace_user},
        },
    },
    app_error::{AppError, AppResult},
    entities::{company::Company, schedule::ScheduleRun, value_objects::EmailAddress},
    infra::config::AppConfig,
    infra::events::MailboxEvents,
    services::email_parser::RawInboundPayload,
    use_cases::{
        agent::AgentUseCases, channel::ChannelUseCases, company::CompanyUseCases,
        schedule::ScheduleUseCases, thread::ReplyDelivery, thread::ThreadUseCases,
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
    "/ui/schedules/{id}/events",
    "/ui/schedules/thread/{thread_id}",
    "/ui/schedules/thread/{thread_id}/reply",
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
        .route(SCHEDULE_UI_PATHS[8], get(schedule_runs_stream))
        .route(SCHEDULE_UI_PATHS[9], get(thread_pane))
        .route(SCHEDULE_UI_PATHS[10], axum::routing::post(reply_in_thread))
}

#[derive(Debug, Clone, Deserialize)]
pub struct SchedulesQuery {
    pub company_id: Option<Uuid>,
    pub schedule_id: Option<Uuid>,
    pub thread_id: Option<Uuid>,
    pub page: Option<usize>,
    pub new: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompanyScopedQuery {
    pub company_id: Uuid,
    pub schedule_id: Option<Uuid>,
    pub page: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReplyForm {
    pub reply_text: String,
}

struct SchedulesWorkspace {
    company_use_cases: Arc<CompanyUseCases>,
    channel_use_cases: Arc<ChannelUseCases>,
    schedule_use_cases: Arc<ScheduleUseCases>,
    agent_use_cases: Arc<AgentUseCases>,
    thread_use_cases: Arc<ThreadUseCases>,
    user_use_cases: Arc<UserUseCases>,
    config: Arc<AppConfig>,
    user_id: Uuid,
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
            agent_use_cases: state.agent_use_cases.clone(),
            thread_use_cases: state.thread_use_cases.clone(),
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
        load_scoped_company(&self.company_use_cases, self.user_id, company_id).await
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

        Ok(Html(pages::schedule_runs_column(
            &pages::ScheduleRunsColumnProps {
                company_id,
                schedule: &schedule,
                channel: channel.as_ref(),
                runs: &runs,
                selected_thread_id: runs.first().map(|run| run.thread_id),
                page,
                has_next,
            },
            pages::FragmentSwap::Inline,
        )))
    }
}

const PAGE_SIZE: i64 = 15;

/// `Re:` a subject without stacking a second prefix on a reply to a reply.
fn reply_subject(subject: &str) -> String {
    if subject.trim_start().to_lowercase().starts_with("re:") {
        subject.to_string()
    } else {
        format!("Re: {subject}")
    }
}

#[instrument(skip(workspace, headers))]
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

    let schedules = workspace
        .schedule_use_cases
        .list_company_schedules(workspace.user_id, company.id)
        .await?;

    let channels = workspace
        .channel_use_cases
        .list_company_channels(workspace.user_id, company.id)
        .await?;

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

        let selected_thread_id = query
            .thread_id
            .or_else(|| display_runs.first().map(|r| r.thread_id));

        let runs_col = pages::schedule_runs_column(
            &pages::ScheduleRunsColumnProps {
                company_id: company.id,
                schedule,
                channel,
                runs: &display_runs,
                selected_thread_id,
                page: page_num,
                has_next,
            },
            pages::FragmentSwap::Inline,
        );

        let thread_pane = if let Some(thread_id) = selected_thread_id {
            let messages = workspace
                .thread_use_cases
                .thread_persistence()
                .list_messages_by_thread_id(thread_id)
                .await?;

            let subject = display_runs
                .iter()
                .find(|r| r.thread_id == thread_id)
                .map(|r| r.subject.as_str())
                .unwrap_or(&schedule.subject_template);

            let first_agent = if let Some(ch) = channel {
                if let Some(&first_id) = ch.agent_ids.as_ref().and_then(|ids| ids.first()) {
                    workspace
                        .agent_use_cases
                        .get_company_agent(workspace.user_id, company.id, first_id)
                        .await?
                } else {
                    None
                }
            } else {
                None
            };

            pages::schedule_thread_pane(&pages::ScheduleThreadPaneProps {
                company_id: company.id,
                schedule,
                channel,
                agent: first_agent.as_ref(),
                thread_id,
                subject,
                messages: &messages,
            })
        } else {
            pages::schedules_empty_pane(
                "No execution runs found for this schedule. Click 'Run Now' to trigger a run.",
                pages::FragmentSwap::Inline,
            )
        };

        (runs_col, thread_pane)
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
        runs_html: &runs_html,
        pane_html: &pane_html,
    })))
}

#[instrument(skip(workspace))]
async fn schedule_runs_fragment(
    workspace: SchedulesWorkspace,
    Query(query): Query<CompanyScopedQuery>,
) -> AppResult<Html<String>> {
    let Some(schedule_id) = query.schedule_id else {
        return Ok(Html(String::new()));
    };

    workspace
        .runs_column(query.company_id, schedule_id, query.page)
        .await
}

/// Keep the schedule-specific run list current using the same committed-message and activity
/// notifications as the inbox. The database remains the source of truth: an event is only a
/// prompt to re-render the current page, so lagged/coalesced notifications are harmless.
#[instrument(skip(workspace, events))]
async fn schedule_runs_stream(
    workspace: SchedulesWorkspace,
    State(events): State<MailboxEvents>,
    Path(schedule_id): Path<Uuid>,
    Query(query): Query<CompanyScopedQuery>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let schedule = workspace
        .schedule_use_cases
        .get_schedule(workspace.user_id, query.company_id, schedule_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Schedule not found".into()))?;
    let channel_id = schedule.channel_id;
    let page = query.page;
    let mut changes = Box::pin(wake_ups(&events, "schedule-runs", move |event| {
        event.is_message_in_channel(channel_id) || event.is_activity_in_channel(channel_id)
    }));

    let stream = async_stream::stream! {
        while let Some(_change) = changes.next().await {
            match workspace.runs_column(query.company_id, schedule_id, page).await {
                Ok(Html(column)) => yield Ok(Event::default().event("schedule-runs").data(column)),
                Err(error) => {
                    tracing::warn!(%error, %schedule_id, "Schedule runs stream query failed");
                    return;
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[instrument(skip(workspace))]
async fn thread_pane(
    workspace: SchedulesWorkspace,
    Path(thread_id): Path<Uuid>,
    Query(query): Query<CompanyScopedQuery>,
) -> AppResult<Html<String>> {
    let (_, company) = workspace.scoped_company(Some(query.company_id)).await?;
    let Some(company) = company else {
        return Ok(Html(String::new()));
    };

    let Some(schedule_id) = query.schedule_id else {
        return Ok(Html(String::new()));
    };

    let schedule = workspace
        .schedule_use_cases
        .get_schedule(workspace.user_id, company.id, schedule_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Schedule not found".into()))?;

    workspace
        .schedule_use_cases
        .authorize_schedule_run_thread(workspace.user_id, company.id, schedule_id, thread_id)
        .await?;

    let channel = workspace
        .channel_use_cases
        .get_company_channel(workspace.user_id, company.id, schedule.channel_id)
        .await?;

    let messages = workspace
        .thread_use_cases
        .thread_persistence()
        .list_messages_by_thread_id(thread_id)
        .await?;

    let subject = messages
        .first()
        .map(|m| m.subject.as_str())
        .unwrap_or(&schedule.subject_template);

    let first_agent = if let Some(ref ch) = channel {
        if let Some(&first_id) = ch.agent_ids.as_ref().and_then(|ids| ids.first()) {
            workspace
                .agent_use_cases
                .get_company_agent(workspace.user_id, company.id, first_id)
                .await?
        } else {
            None
        }
    } else {
        None
    };

    Ok(Html(pages::schedule_thread_pane(
        &pages::ScheduleThreadPaneProps {
            company_id: company.id,
            schedule: &schedule,
            channel: channel.as_ref(),
            agent: first_agent.as_ref(),
            thread_id,
            subject,
            messages: &messages,
        },
    )))
}

#[instrument(skip(workspace, form))]
async fn reply_in_thread(
    workspace: SchedulesWorkspace,
    Path(thread_id): Path<Uuid>,
    Query(query): Query<CompanyScopedQuery>,
    Form(form): Form<ReplyForm>,
) -> AppResult<Html<String>> {
    let (_, company) = workspace.scoped_company(Some(query.company_id)).await?;
    let Some(company) = company else {
        return Ok(Html(String::new()));
    };

    let Some(schedule_id) = query.schedule_id else {
        return Ok(Html(String::new()));
    };

    let schedule = workspace
        .schedule_use_cases
        .get_schedule(workspace.user_id, company.id, schedule_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Schedule not found".into()))?;

    workspace
        .schedule_use_cases
        .authorize_schedule_run_thread(workspace.user_id, company.id, schedule_id, thread_id)
        .await?;

    let channel = workspace
        .channel_use_cases
        .get_company_channel(workspace.user_id, company.id, schedule.channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;

    if form.reply_text.trim().is_empty() {
        return Err(AppError::BadRequest("A message is required.".into()));
    }

    let account = load_account(&workspace.user_use_cases, workspace.user_id).await?;
    let sender = EmailAddress::from(account.email.as_str());
    let address = channel.inbound_address(&company.slug, &workspace.config.app_domain_name);

    // A reply here takes the same route as one sent from the mailbox: writing the row directly
    // would store a message no agent ever answers, and with no threading headers on it.
    let history = workspace
        .thread_use_cases
        .thread_persistence()
        .list_messages_by_thread_id(thread_id)
        .await?;
    let in_reply_to = history.last().map(|message| message.message_id.as_str());
    let subject = history
        .first()
        .map(|message| reply_subject(&message.subject))
        .unwrap_or_else(|| reply_subject(&schedule.subject_template));

    let payload = RawInboundPayload {
        to: address.to_string(),
        from: sender.to_string(),
        subject: Some(subject.clone()),
        text: Some(form.reply_text.clone()),
        headers: reply_headers(in_reply_to),
        ..Default::default()
    };

    let ingest = workspace
        .thread_use_cases
        .queue_inbound_for_agent(payload, ReplyDelivery::InAppOnly)
        .await?;

    if ingest.thread.is_none() {
        let reason = ingest
            .reason
            .unwrap_or_else(|| "The channel rejected this message.".to_string());
        return Err(AppError::BadRequest(reason));
    }

    // Load full messages
    let messages = workspace
        .thread_use_cases
        .thread_persistence()
        .list_messages_by_thread_id(thread_id)
        .await?;

    let first_agent =
        if let Some(&first_id) = channel.agent_ids.as_ref().and_then(|ids| ids.first()) {
            workspace
                .agent_use_cases
                .get_company_agent(workspace.user_id, company.id, first_id)
                .await?
        } else {
            None
        };

    Ok(Html(pages::schedule_thread_pane(
        &pages::ScheduleThreadPaneProps {
            company_id: company.id,
            schedule: &schedule,
            channel: Some(&channel),
            agent: first_agent.as_ref(),
            thread_id,
            subject: &subject,
            messages: &messages,
        },
    )))
}

#[instrument(skip(workspace))]
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

    Ok(Html(pages::schedule_form_pane(
        &pages::ScheduleFormPaneProps {
            company_id: company.id,
            channels: &channels,
            schedule: None,
            error: None,
        },
    )))
}

#[instrument(skip(workspace))]
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

    Ok(Html(pages::schedule_form_pane(
        &pages::ScheduleFormPaneProps {
            company_id: company.id,
            channels: &channels,
            schedule: Some(&schedule),
            error: None,
        },
    )))
}

/// What the Run Now and Pause buttons submit: the company scope, plus the state Pause is asking
/// for. Both re-render the runs column they live in.
#[derive(Debug, Deserialize)]
pub struct ScheduleActionForm {
    pub company_id: Uuid,
    pub page: Option<usize>,
    pub enabled: Option<String>,
}

impl ScheduleActionForm {
    fn wants_enabled(&self) -> bool {
        // A checkbox posts "on"; the Pause/Resume button posts the state it is switching to.
        matches!(self.enabled.as_deref(), Some("true") | Some("on"))
    }
}

#[instrument(skip(workspace))]
async fn run_schedule_now(
    workspace: SchedulesWorkspace,
    Path(id): Path<Uuid>,
    Form(form): Form<ScheduleActionForm>,
) -> AppResult<Html<String>> {
    workspace
        .schedule_use_cases
        .trigger_schedule_now(workspace.user_id, form.company_id, id)
        .await?;

    workspace.runs_column(form.company_id, id, form.page).await
}

#[instrument(skip(workspace))]
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
    let sidebar = pages::schedules_sidebar_list(
        form.company_id,
        &schedules,
        Some(id),
        pages::FragmentSwap::OutOfBand,
    );

    // The button targets the runs column; htmx applies the sidebar copy separately via OOB swap.
    Ok(Html(format!("{runs_column}{sidebar}")))
}

#[instrument(skip(_workspace))]
async fn close_pane(
    _workspace: SchedulesWorkspace,
    Query(_query): Query<CompanyScopedQuery>,
) -> AppResult<Html<String>> {
    Ok(Html(pages::schedules_empty_pane(
        "Select a schedule or run on the left.",
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

#[instrument(skip(workspace, form))]
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

#[instrument(skip(workspace, form))]
async fn update_schedule(
    workspace: SchedulesWorkspace,
    Path(id): Path<Uuid>,
    Form(form): Form<CreateScheduleForm>,
) -> AppResult<Response> {
    let company_id = form.schedule.company_id;
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

#[instrument(skip(workspace))]
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

#[instrument(skip(_workspace))]
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
        FragmentSwap, MailboxUser, ScheduleRunsColumnProps, ScheduleThreadPaneProps, SchedulesPage,
        schedule_runs_column, schedule_thread_pane, schedules_page,
    };
    use crate::entities::{
        company::Company,
        message::{Message, MessageDirection, MessageRole},
        schedule::{
            ChannelSchedule, ScheduleDeliveryMode, ScheduleRun, ScheduleTimezone, ScheduleType,
        },
        task::TaskStatus,
        value_objects::MessageId,
    };
    use chrono::Utc;

    fn test_company() -> Company {
        Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Schedule Corp".into(),
            slug: "sched".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        }
    }

    fn test_schedule(company_id: Uuid, channel_id: Uuid) -> ChannelSchedule {
        ChannelSchedule {
            id: Uuid::new_v4(),
            company_id,
            channel_id,
            name: "Morning Report".into(),
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(3600),
            subject_template: "[Daily] Morning Report - {{date}}".into(),
            prompt_template: "Analyze tickets".into(),
            delivery_mode: ScheduleDeliveryMode::MailboxOnly,
            recipient_emails: vec![],
            timezone: ScheduleTimezone::utc(),
            enabled: true,
            last_run_at: None,
            next_run_at: Some(Utc::now()),
            last_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
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
        let user_email = EmailAddress::from("admin@example.com");
        let user = MailboxUser {
            id: Uuid::new_v4(),
            username: "admin",
            email: &user_email,
            avatar_url: None,
            is_operator: false,
        };

        let run = ScheduleRun {
            thread_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            channel_id,
            subject: "[Daily] Report".into(),
            task_status: TaskStatus::Completed,
            lock_expires_at: None,
            latest_response: Some("Done.".into()),
            message_count: 2,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        // Every surface of the workspace, so a button on any of them is covered.
        let rendered = [
            schedules_page(&SchedulesPage {
                user: &user,
                companies: std::slice::from_ref(&test_company()),
                company: &test_company(),
                schedules: std::slice::from_ref(&schedule),
                selected_schedule_id: Some(schedule.id),
                runs_html: "",
                pane_html: "",
            }),
            schedule_runs_column(
                &ScheduleRunsColumnProps {
                    company_id,
                    schedule: &schedule,
                    channel: None,
                    runs: std::slice::from_ref(&run),
                    selected_thread_id: Some(run.thread_id),
                    page: 1,
                    has_next: true,
                },
                FragmentSwap::Inline,
            ),
            schedule_thread_pane(&ScheduleThreadPaneProps {
                company_id,
                schedule: &schedule,
                channel: None,
                agent: None,
                thread_id: run.thread_id,
                subject: "Daily Report",
                messages: &[],
            }),
            pages::schedule_form_pane(&pages::ScheduleFormPaneProps {
                company_id,
                channels: &[],
                schedule: Some(&schedule),
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
        let channel_id = Uuid::new_v4();
        let schedule = test_schedule(company.id, channel_id);
        let user_email = EmailAddress::from("admin@example.com");

        let user = MailboxUser {
            id: Uuid::new_v4(),
            username: "admin",
            email: &user_email,
            avatar_url: None,
            is_operator: false,
        };

        let html = schedules_page(&SchedulesPage {
            user: &user,
            companies: std::slice::from_ref(&company),
            company: &company,
            schedules: std::slice::from_ref(&schedule),
            selected_schedule_id: Some(schedule.id),
            runs_html: r#"<div id="schedule-runs-column">Runs Column</div>"#,
            pane_html: r#"<div id="schedule-pane">Detail Pane</div>"#,
        });

        // Icon rail should light the Schedules section
        assert!(html.contains("href=\"/ui/schedules?company_id="));
        assert!(html.contains("btn-primary\" title=\"Schedules\""));

        // Column 1: Sidebar list with schedule name and cadence
        assert!(html.contains("Morning Report"));
        assert!(html.contains("Every hour"));
        assert!(!html.contains(r#"class="text-primary font-mono"#));

        // Column 2 & 3 wrapped in workspace
        assert!(html.contains("id=\"schedules-workspace\""));
        assert!(html.contains("Runs Column"));
        assert!(html.contains("Detail Pane"));
    }

    #[test]
    fn schedule_runs_column_renders_run_items_with_activity_badges() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let schedule = test_schedule(company_id, channel_id);

        let run = ScheduleRun {
            thread_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            channel_id,
            subject: "[Daily] Morning Report - 2026-08-25".into(),
            task_status: TaskStatus::Processing,
            lock_expires_at: Some(Utc::now() + chrono::Duration::minutes(5)),
            latest_response: Some("All systems operational.".into()),
            message_count: 2,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let html = schedule_runs_column(
            &ScheduleRunsColumnProps {
                company_id,
                schedule: &schedule,
                channel: None,
                runs: std::slice::from_ref(&run),
                selected_thread_id: Some(run.thread_id),
                page: 1,
                has_next: false,
            },
            FragmentSwap::Inline,
        );

        assert!(html.contains("[Daily] Morning Report - 2026-08-25"));
        assert!(html.contains("All systems operational."));
        assert!(html.contains("2 msgs"));
        assert!(html.contains("Running")); // Processing with active lease
        assert!(html.contains("Run Now"));
        assert!(html.contains(&format!(
            r#"sse-connect="/ui/schedules/{}/events?company_id={company_id}&page=1""#,
            schedule.id
        )));
        assert!(html.contains(r#"sse-swap="schedule-runs""#));
    }

    #[test]
    fn schedule_thread_pane_renders_messages_and_reply_form() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let schedule = test_schedule(company_id, channel_id);
        let thread_id = Uuid::new_v4();

        let prompt_msg = Message {
            id: Uuid::new_v4(),
            thread_id,
            message_id: MessageId::new("<prompt1@domain.com>"),
            in_reply_to: None,
            references_list: vec![],
            sender: EmailAddress::from("system@domain.com"),
            recipients_to: vec![],
            recipients_cc: vec![],
            subject: "Daily Audit".into(),
            clean_text_body: "Please run morning audit.".into(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Inbound,
            role: MessageRole::System,
            thread_index: None,
            created_at: Utc::now(),
        };

        let agent_reply_msg = Message {
            id: Uuid::new_v4(),
            thread_id,
            message_id: MessageId::new("<reply1@domain.com>"),
            in_reply_to: Some(MessageId::new("<prompt1@domain.com>")),
            references_list: vec![MessageId::new("<prompt1@domain.com>")],
            sender: EmailAddress::from("agent@domain.com"),
            recipients_to: vec![],
            recipients_cc: vec![],
            subject: "Re: Daily Audit".into(),
            clean_text_body: "Audit completed: **0 errors found**.".into(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            thread_index: None,
            created_at: Utc::now(),
        };

        let html = schedule_thread_pane(&ScheduleThreadPaneProps {
            company_id,
            schedule: &schedule,
            channel: None,
            agent: None,
            thread_id,
            subject: "Daily Audit",
            messages: &[prompt_msg, agent_reply_msg],
        });

        assert!(html.contains("Daily Audit"));
        assert!(html.contains("Please run morning audit."));
        assert!(html.contains("0 errors found")); // Markdown rendered
        assert!(html.contains(r#"hx-ext="sse""#));
        assert!(html.contains(&format!(
            r#"sse-connect="/ui/events?company_id={company_id}&channel_id={}&thread_id={thread_id}&after="#,
            Uuid::nil()
        )));
        assert!(html.contains(r#"sse-swap="message""#));
        assert!(html.contains(r#"sse-swap="activity""#));
        assert!(html.contains("hx-post=\"/ui/schedules/thread/"));
        assert!(html.contains("Reply</button>"));
    }

    /// The browser's Create Schedule submission, field for field, must reach the handler.
    /// The pane posts every control in the form — including the ones its show/hide boxes have
    /// hidden — so the empty `scheduled_at` and `recipient_emails` are part of a normal interval
    /// create, and the numeric cadence arrives as a string like every urlencoded value.
    #[tokio::test]
    async fn create_schedule_form_deserializes_browser_submission() {
        use axum::body::Body;
        use axum::extract::FromRequest;
        use axum::http::{Request, header};

        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let body = format!(
            "company_id={company_id}&name=Daily+Operations+Report&channel_id={channel_id}\
             &schedule_type=interval&interval_seconds=3600&scheduled_at=\
             &subject_template=%5BDaily%5D+Report&prompt_template=Summarise+the+day\
             &timezone=UTC&delivery_mode=mailbox_only&recipient_emails="
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
        assert_eq!(write.delivery_mode, ScheduleDeliveryMode::MailboxOnly);
        assert_eq!(write.scheduled_at, None);
        assert!(write.recipient_emails.is_empty());
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
        let body = format!(
            "company_id={company_id}&name=Quarter+Close&channel_id={channel_id}\
             &schedule_type=one_off&interval_seconds=&scheduled_at=2026-08-25T14%3A30\
             &subject_template=Close&prompt_template=Close+the+quarter\
             &timezone=UTC&delivery_mode=email_custom&recipient_emails=a%40x.com%2C+b%40x.com"
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
        assert_eq!(write.recipient_emails.len(), 2);
    }
}
