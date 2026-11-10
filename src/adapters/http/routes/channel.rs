use std::{convert::Infallible, sync::Arc};

use axum::{
    Form, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        Html, IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use tokio_stream::{Stream, StreamExt};
use tracing::instrument;
use uuid::Uuid;

use crate::adapters::http::app_state::AppState;
use crate::adapters::http::auth::AuthenticatedUser;
use crate::adapters::http::pages;
use crate::adapters::protocols::email::EmailChannelSelectorParser;
use crate::adapters::protocols::email::EmailIdentity;
use crate::adapters::protocols::email::EmailIngressAdapter;
use crate::adapters::protocols::email::EmailIngressTrust;
use crate::adapters::protocols::email::parser::RawInboundPayload;
use crate::app_error::AppError;
use crate::app_error::AppResult;
use crate::application::message_outcomes::MessageOutcomeReader;
use crate::application::ports::channel::{ChannelOwnershipWrite, ChannelWrite};
use crate::entities::channel::Channel;
use crate::entities::cursor::ThreadCursor;
use crate::entities::department::DepartmentId;
use crate::entities::thread::Thread;
use crate::entities::user::Viewer;
use crate::entities::value_objects::EmailAddress;
use crate::infra::config::AppConfig;
use crate::infra::events::MailboxEvents;
use crate::use_cases::channel::ChannelUseCases;
use crate::use_cases::company::CompanyUseCases;
use crate::use_cases::thread::InboundIngestResult;
use crate::use_cases::thread::IngressOrigin;
use crate::use_cases::thread::ReplyDelivery;
use crate::use_cases::thread::SimulationMode;
use crate::use_cases::thread::ThreadUseCases;

use super::live_updates::simulation_wake_ups;

mod simulation;
use simulation::{
    open_simulated_thread_get, open_simulated_thread_post, simulate_channel_handler,
    simulate_channel_page, simulation_stream,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/companies/{company_id}/channels",
            get(list_channels_page).post(create_channel_handler),
        )
        .route(
            "/companies/{company_id}/channels/{id}",
            put(update_channel_handler).delete(delete_channel_handler),
        )
        .route(
            "/companies/{company_id}/channels/{id}/edit",
            get(edit_channel_form),
        )
        .route(
            "/companies/{company_id}/channels/{id}/cancel",
            get(cancel_channel_edit),
        )
        .route(
            "/companies/{company_id}/channels/{id}/simulate",
            get(simulate_channel_page).post(simulate_channel_handler),
        )
        .route(
            "/companies/{company_id}/channels/{id}/simulate/events",
            get(simulation_stream),
        )
        .route(
            "/companies/{company_id}/channels/{id}/simulate/thread",
            get(open_simulated_thread_get).post(open_simulated_thread_post),
        )
        .route(
            "/companies/{company_id}/channels/{id}/threads",
            get(list_channel_threads_page),
        )
        .route(
            "/companies/{company_id}/channels/{id}/threads/list",
            get(list_channel_threads_fragment),
        )
        .route(
            "/api/companies/{company_id}/channels",
            get(list_channels_json).post(create_channel_json),
        )
        .route(
            "/api/companies/{company_id}/channels/{id}",
            get(get_channel_json)
                .put(update_channel_json)
                .delete(delete_channel_json),
        )
        .route(
            "/api/companies/{company_id}/channels/{id}/threads",
            get(list_channel_threads_json),
        )
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelForm {
    pub name: String,
    pub description: Option<String>,
    pub slug: Option<String>,
    pub alias_slugs: Option<String>,
    pub participant_emails: Option<String>,
    pub confirm_spam_disabled: Option<String>,
    pub enabled: Option<String>,
    pub add_3rd_party: Option<String>,
}

impl ChannelForm {
    /// Whether the "Channel enabled" box was ticked.
    ///
    /// An unchecked checkbox submits no key at all, and every channel form posts its whole field
    /// set, so an absent value genuinely means the user unticked it.
    pub fn enabled(&self) -> bool {
        checkbox_ticked(self.enabled.as_deref())
    }

    /// Whether the "Add CC'd outsiders to threads" box was ticked. Same absent-means-unticked
    /// reasoning as [`ChannelForm::enabled`].
    pub fn add_3rd_party(&self) -> bool {
        checkbox_ticked(self.add_3rd_party.as_deref())
    }

    pub fn confirm_spam_disabled(&self) -> bool {
        checkbox_ticked(self.confirm_spam_disabled.as_deref())
    }
}

/// The two shapes a browser sends for a ticked checkbox, in one place.
pub(super) fn checkbox_ticked(value: Option<&str>) -> bool {
    matches!(value, Some("true") | Some("on"))
}

pub fn slugify(input: &str) -> String {
    let clean: String = input
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    let mut result = String::new();
    let mut last_was_hyphen = false;
    for c in clean.chars() {
        if c == '-' {
            if !last_was_hyphen {
                result.push('-');
                last_was_hyphen = true;
            }
        } else {
            result.push(c);
            last_was_hyphen = false;
        }
    }
    result.trim_matches('-').to_string()
}

/// The agent ids a form submitted, in order; `None` only when the field is absent.
pub(super) fn parse_agent_ids_form(input: Option<String>) -> Option<Vec<Uuid>> {
    input.map(|s| {
        s.split(&[',', ' ', ';', '\n'][..])
            .map(|e| e.trim())
            .filter(|e| !e.is_empty())
            .filter_map(|e| Uuid::parse_str(e).ok())
            .collect()
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelJsonPayload {
    pub name: String,
    pub description: Option<String>,
    pub slug: Option<String>,
    /// A PUT replaces the alias set wholesale, like every other field here.
    pub alias_slugs: Option<Vec<String>>,
    pub participant_emails: Option<Vec<String>>,
    pub confirm_spam_disabled: Option<bool>,
    /// Omitted means enabled: like every other field here, a PUT replaces rather than patches.
    pub enabled: Option<bool>,
    /// Omitted means on, for the same reason as `enabled`.
    pub add_3rd_party: Option<bool>,
    /// Unlike every field above, ownership is kept when absent: `null` detaches the desk from its
    /// department, an id assigns it, and leaving the key out changes nothing.
    #[serde(default, deserialize_with = "present")]
    pub department_id: Option<Option<Uuid>>,
    /// Kept when absent, like `department_id`.
    pub members_only: Option<bool>,
}

/// A field that is present, even as `null`, is `Some`; an absent one stays `None` via `default`.
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

impl ChannelJsonPayload {
    /// The ownership change this payload asks for, if it names either field.
    fn ownership(&self) -> Option<ChannelOwnershipWrite> {
        (self.department_id.is_some() || self.members_only.is_some()).then(|| {
            ChannelOwnershipWrite {
                department_id: self
                    .department_id
                    .map(|department| department.map(DepartmentId::new)),
                members_only: self.members_only,
            }
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelResponse {
    pub success: bool,
    pub channel: Channel,
}

const DEFAULT_THREAD_PAGE_SIZE: usize = 50;
const MAX_THREAD_PAGE_SIZE: usize = 100;

/// One page of a channel's threads, newest first.
///
/// Shared with the `/ui` mailbox so both thread lists page the same way.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThreadListQuery {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadListResponse {
    pub threads: Vec<Thread>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

fn parse_thread_cursor(cursor: &str) -> AppResult<ThreadCursor> {
    cursor
        .parse()
        .map_err(|_| AppError::BadRequest("Invalid thread cursor".into()))
}

pub(crate) async fn load_thread_page(
    thread_use_cases: &ThreadUseCases,
    viewer: &crate::entities::user::Viewer,
    channel: &Channel,
    query: &ThreadListQuery,
) -> AppResult<ThreadListResponse> {
    let limit = query.limit.unwrap_or(DEFAULT_THREAD_PAGE_SIZE);
    if !(1..=MAX_THREAD_PAGE_SIZE).contains(&limit) {
        return Err(AppError::BadRequest(format!(
            "limit must be between 1 and {MAX_THREAD_PAGE_SIZE}"
        )));
    }
    let before = query
        .cursor
        .as_deref()
        .map(parse_thread_cursor)
        .transpose()?;

    let mut threads = thread_use_cases
        .list_channel_threads(viewer, channel.into(), before, limit + 1)
        .await?;
    let has_more = threads.len() > limit;
    threads.truncate(limit);
    let next_cursor = has_more.then(|| threads[threads.len() - 1].cursor().to_string());

    Ok(ThreadListResponse {
        threads,
        next_cursor,
        has_more,
    })
}

pub(super) fn parse_emails_form(input: Option<String>) -> Option<Vec<String>> {
    let list = parse_list_form(input);
    if list.is_empty() { None } else { Some(list) }
}

/// Trim one free-text field, treating a blank one as absent.
///
/// A form always posts every field, so an untouched textarea arrives as `Some("")`. Storing that
/// would make "no description" two distinct values in the database.
pub(super) fn parse_text_form(input: Option<String>) -> Option<String> {
    input
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Split one comma/newline/semicolon-separated textarea into its non-blank entries.
pub(super) fn parse_list_form(input: Option<String>) -> Vec<String> {
    input
        .map(|s| {
            s.split(&[',', '\n', ';'][..])
                .map(|e| e.trim().to_string())
                .filter(|e| !e.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Which form the page should arrive with open — set by the mailbox's channel buttons.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChannelsPageQuery {
    /// `?new=1` opens the create-channel card.
    pub new: Option<String>,
    /// `?edit={channel_id}` renders that channel's row as its edit form.
    pub edit: Option<Uuid>,
}

/// GET /companies/{company_id}/channels - Full HTML page listing channels (Protected).
#[instrument(skip_all)]
async fn list_channels_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ChannelsPageQuery>,
) -> AppResult<Html<String>> {
    let company = company_use_cases
        .managed_company(user.id, company_id)
        .await?
        .company;
    let channels = channel_use_cases
        .list_company_channels(user.id, company_id)
        .await?;

    Ok(Html(pages::channels_page(&pages::ChannelsPage {
        company: &company,
        app_domain_name: &config.app_domain_name,
        channels: &channels,
        spam_scan_enabled: config.is_spam_scan_enabled(),
        focus: pages::ChannelsPageFocus {
            create_form_open: matches!(query.new.as_deref(), Some("1") | Some("true")),
            editing_channel_id: query.edit,
        },
    })))
}

/// POST /companies/{company_id}/channels - HTMX create channel (Protected).
#[instrument(skip_all)]
async fn create_channel_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<ChannelForm>,
) -> AppResult<Html<String>> {
    let company = company_use_cases
        .managed_company(user.id, company_id)
        .await?
        .company;
    let view = ChannelListView {
        channel_use_cases: &channel_use_cases,
        app_domain_name: &config.app_domain_name,
        user_id: user.id,
        company: &company,
    };

    let slug = form
        .slug
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| slugify(&form.name));

    let confirm_spam_disabled = form.confirm_spam_disabled();
    let enabled = form.enabled();
    let add_3rd_party = form.add_3rd_party();
    let write = ChannelWrite {
        enabled,
        add_3rd_party,
        name: form.name,
        description: parse_text_form(form.description),
        slug,
        alias_slugs: parse_list_form(form.alias_slugs),
        participant_emails: parse_emails_form(form.participant_emails),
        created_by: None,
        ownership: None,
    };
    if let Err(err) = channel_use_cases
        .create_channel(user.id, company_id, write, confirm_spam_disabled)
        .await
    {
        return match err {
            error @ (AppError::BadRequest(_) | AppError::Conflict(_)) => {
                view.render(Some(format!("Failed to create desk: {error}")))
                    .await
            }
            error => Err(error),
        };
    }
    view.render(None).await
}

/// Everything needed to re-render the channel list after any outcome.
///
/// Every exit from the create/update handlers ends in the same fragment, optionally headed by an
/// error alert, so the reload lives here instead of at each `return`.
struct ChannelListView<'a> {
    channel_use_cases: &'a ChannelUseCases,
    app_domain_name: &'a str,
    user_id: Uuid,
    company: &'a crate::entities::company::Company,
}

impl ChannelListView<'_> {
    async fn render(&self, error: Option<String>) -> AppResult<Html<String>> {
        let company_id = self.company.id;
        let channels = self
            .channel_use_cases
            .list_company_channels(self.user_id, company_id)
            .await?;
        let list = pages::channel_list_fragment(self.company, self.app_domain_name, &channels);
        Ok(Html(match error {
            Some(message) => format!("{}{}", pages::error_alert(&message), list),
            None => list,
        }))
    }
}

/// The company and one of its channels, for the management fragments: 404 for someone else's
/// company or a channel that is not this company's, 403 for a member, an error for a failed read.
async fn managed_channel(
    company_use_cases: &CompanyUseCases,
    channel_use_cases: &ChannelUseCases,
    user_id: Uuid,
    company_id: Uuid,
    channel_id: Uuid,
) -> AppResult<(crate::entities::company::Company, Channel)> {
    let company = company_use_cases
        .managed_company(user_id, company_id)
        .await?
        .company;
    let channel = channel_use_cases
        .get_company_channel(user_id, company_id, channel_id)
        .await?
        .ok_or_else(channel_not_found)?;
    Ok((company, channel))
}

fn channel_not_found() -> AppError {
    AppError::NotFound("Channel not found".into())
}

/// GET /companies/{company_id}/channels/{id}/edit - HTMX edit channel form fragment (Protected).
#[instrument(skip_all)]
async fn edit_channel_form(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    let (company, channel) = managed_channel(
        &company_use_cases,
        &channel_use_cases,
        user.id,
        company_id,
        channel_id,
    )
    .await?;
    Ok(Html(pages::channel_edit_fragment(
        &company,
        &config.app_domain_name,
        &channel,
        config.is_spam_scan_enabled(),
    )))
}

/// GET /companies/{company_id}/channels/{id}/cancel - Cancel channel edit fragment (Protected).
#[instrument(skip_all)]
async fn cancel_channel_edit(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    let (company, channel) = managed_channel(
        &company_use_cases,
        &channel_use_cases,
        user.id,
        company_id,
        channel_id,
    )
    .await?;
    Ok(Html(pages::channel_row_fragment(
        &company,
        &config.app_domain_name,
        &channel,
    )))
}

/// PUT /companies/{company_id}/channels/{id} - HTMX update channel (Protected).
#[instrument(skip_all)]
async fn update_channel_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<ChannelForm>,
) -> AppResult<Html<String>> {
    let (company, _) = managed_channel(
        &company_use_cases,
        &channel_use_cases,
        user.id,
        company_id,
        channel_id,
    )
    .await?;

    let slug = form
        .slug
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| slugify(&form.name));

    let confirm_spam_disabled = form.confirm_spam_disabled();
    let enabled = form.enabled();
    let add_3rd_party = form.add_3rd_party();
    let emails = parse_emails_form(form.participant_emails);

    let write = ChannelWrite {
        name: form.name,
        description: parse_text_form(form.description),
        slug,
        alias_slugs: parse_list_form(form.alias_slugs),
        participant_emails: emails,
        enabled,
        add_3rd_party,
        created_by: None,
        ownership: None,
    };

    let updated = match channel_use_cases
        .update_channel(
            user.id,
            company_id,
            channel_id,
            write,
            confirm_spam_disabled,
        )
        .await
    {
        Ok(channel) => channel,
        Err(err @ (AppError::BadRequest(_) | AppError::Conflict(_))) => {
            return Ok(Html(pages::error_alert(&format!("Update failed: {err}"))));
        }
        Err(error) => return Err(error),
    };
    Ok(Html(pages::channel_row_fragment(
        &company,
        &config.app_domain_name,
        &updated,
    )))
}

/// DELETE /companies/{company_id}/channels/{id} - HTMX delete channel (Protected).
#[instrument(skip_all)]
async fn delete_channel_handler(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    channel_use_cases
        .delete_channel(user.id, company_id, channel_id)
        .await?;
    Ok(Html(String::new()))
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimulationForm {
    pub to: String,
    pub from: Option<String>,
    pub subject: Option<String>,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub simulation_mode: Option<String>,
    pub in_reply_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimulateQuery {
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenThreadParams {
    pub thread_id: Option<String>,
}

/// Threading headers that make a simulated message a reply to an existing one.
///
/// Shared with the `/ui` mailbox so a reply sent there threads exactly like a simulated one.
pub(crate) fn reply_headers(in_reply_to: Option<&str>) -> Option<String> {
    let reply_to = in_reply_to.map(str::trim).filter(|s| !s.is_empty())?;
    Some(format!(
        "In-Reply-To: {}\nReferences: {}\n",
        reply_to, reply_to
    ))
}

/// Threading headers that target an explicit thread while keeping any RFC reply-to reference.
#[cfg_attr(not(test), expect(dead_code))]
pub(crate) fn reply_headers_for_thread(
    thread_id: Uuid,
    in_reply_to: Option<&str>,
) -> Option<String> {
    let mut headers = format!("X-MailAgents-Thread-Id: {thread_id}\n");
    if let Some(reply_to) = in_reply_to.map(str::trim).filter(|s| !s.is_empty()) {
        headers.push_str(&format!(
            "In-Reply-To: {reply_to}\nReferences: {reply_to}\n"
        ));
    }
    Some(headers)
}

/// GET /companies/{company_id}/channels/{id}/threads - Paginated thread list page (Protected).
async fn list_channel_threads_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    viewer: crate::entities::user::Viewer,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    let (company, channel) = managed_channel(
        &company_use_cases,
        &channel_use_cases,
        viewer.user_id,
        company_id,
        channel_id,
    )
    .await?;
    let page = load_thread_page(
        &thread_use_cases,
        &viewer,
        &channel,
        &ThreadListQuery {
            limit: None,
            cursor: None,
        },
    )
    .await?;

    Ok(Html(pages::channel_threads_page(
        &company,
        &channel,
        &config.app_domain_name,
        &page.threads,
        page.next_cursor.as_deref(),
    )))
}

/// GET /companies/{company_id}/channels/{id}/threads/list - Next HTMX page (Protected).
async fn list_channel_threads_fragment(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    viewer: crate::entities::user::Viewer,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ThreadListQuery>,
) -> AppResult<Html<String>> {
    let channel = channel_use_cases
        .get_company_channel(viewer.user_id, company_id, channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;
    let page = load_thread_page(&thread_use_cases, &viewer, &channel, &query).await?;

    Ok(Html(pages::channel_thread_list_fragment(
        company_id,
        channel_id,
        &config.app_domain_name,
        &page.threads,
        page.next_cursor.as_deref(),
        true,
    )))
}

/// JSON API: List company channels (Protected).
async fn list_channels_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let channels = channel_use_cases
        .list_company_channels(user.id, company_id)
        .await?;
    Ok((StatusCode::OK, Json(channels)))
}

/// JSON API: List channel threads newest-first with keyset pagination (Protected).
async fn list_channel_threads_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    viewer: crate::entities::user::Viewer,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ThreadListQuery>,
) -> AppResult<impl IntoResponse> {
    let channel = channel_use_cases
        .get_company_channel(viewer.user_id, company_id, channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;

    let page = load_thread_page(&thread_use_cases, &viewer, &channel, &query).await?;

    Ok((StatusCode::OK, Json(page)))
}

/// JSON API: Create company channel (Protected).
async fn create_channel_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Json(payload): Json<ChannelJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let confirm_spam_disabled = payload.confirm_spam_disabled.unwrap_or(false);
    let slug = payload
        .slug
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| slugify(&payload.name));

    let ownership = payload.ownership();
    let write = ChannelWrite {
        name: payload.name,
        description: parse_text_form(payload.description),
        slug,
        alias_slugs: payload.alias_slugs.unwrap_or_default(),
        participant_emails: payload.participant_emails,
        enabled: payload.enabled.unwrap_or(true),
        add_3rd_party: payload.add_3rd_party.unwrap_or(true),
        created_by: None,
        ownership,
    };

    let channel = channel_use_cases
        .create_channel(user.id, company_id, write, confirm_spam_disabled)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(ChannelResponse {
            success: true,
            channel,
        }),
    ))
}

/// JSON API: Get company channel details (Protected).
async fn get_channel_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let channel = channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await?
        .ok_or_else(crate::use_cases::channel::channel_not_found)?;

    Ok((StatusCode::OK, Json(channel)))
}

/// JSON API: Update company channel (Protected).
pub(super) async fn update_channel_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<ChannelJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let confirm_spam_disabled = payload.confirm_spam_disabled.unwrap_or(false);
    let slug = payload
        .slug
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| slugify(&payload.name));

    let ownership = payload.ownership();
    let write = ChannelWrite {
        name: payload.name,
        description: parse_text_form(payload.description),
        slug,
        alias_slugs: payload.alias_slugs.unwrap_or_default(),
        participant_emails: payload.participant_emails,
        enabled: payload.enabled.unwrap_or(true),
        add_3rd_party: payload.add_3rd_party.unwrap_or(true),
        created_by: None,
        ownership,
    };

    let channel = channel_use_cases
        .update_channel(
            user.id,
            company_id,
            channel_id,
            write,
            confirm_spam_disabled,
        )
        .await?;

    Ok((
        StatusCode::OK,
        Json(ChannelResponse {
            success: true,
            channel,
        }),
    ))
}

/// JSON API: Delete company channel (Protected).
async fn delete_channel_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    channel_use_cases
        .delete_channel(user.id, company_id, channel_id)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use crate::entities::{company::Company, thread::Thread};
    use crate::use_cases::thread::test_support::{EmailMessageDraft, stored_email_view};
    use chrono::Utc;

    use super::*;

    #[test]
    fn thread_cursor_round_trips() {
        let thread = Thread {
            id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            subject: "Subject".into(),
            participant_principal_ids: Vec::new(),
            participant_projection: crate::entities::thread::ThreadParticipantProjection::default(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let cursor = thread.cursor().to_string();
        assert_eq!(parse_thread_cursor(&cursor).unwrap(), thread.cursor());
        assert!(matches!(
            parse_thread_cursor("invalid"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn channel_pages_and_fragments_render_correctly() {
        let company = Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        };

        let channel = Channel {
            enabled: true,
            add_3rd_party: true,
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Auto Dispatcher".to_string(),
            description: None,
            slug: "auto-dispatcher".into(),
            alias_slugs: Vec::new(),
            participant_emails: Some(vec!["agent@test.com".into()]),
            access_mode: crate::entities::channel::ChannelAccessMode::Allowlist,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
            department_id: None,
            members_only: false,
        };

        let channels = [channel.clone()];
        let page = pages::ChannelsPage {
            company: &company,
            app_domain_name: "example.com",
            channels: &channels,
            spam_scan_enabled: true,
            focus: pages::ChannelsPageFocus::default(),
        };
        let page_html = pages::channels_page(&page);
        assert!(!page_html.contains("LLM Provider (Optional Override)"));
        assert!(!page_html.contains("LLM Model (Optional Override)"));
        assert!(!page_html.contains("LLM API Key (Optional Override)"));
        assert!(!page_html.contains("Channel Config (JSON, Optional)"));
        assert!(page_html.contains("id=\"channel-form-toggle\""));
        assert!(page_html.contains("id=\"channel-form-card\" class=\"hidden"));
        assert!(page_html.contains("aria-controls=\"channel-form-card\""));
        // p08 D4.7: the page creates plain channels; agents get theirs from the agent page.
        assert!(!page_html.contains("Agent Instructions"));
        assert!(!page_html.contains("form_mode"));
        assert!(!page_html.contains("target_id\": \"simple_system_prompt\""));
        assert!(!page_html.contains("id=\"simple_prompt_gen_box\""));
        assert!(page_html.contains("hx-disabled-elt=\"find button[type='submit']\""));

        let row_html = pages::channel_row_fragment(&company, "example.com", &channel);
        assert!(row_html.contains("Auto Dispatcher"));
        assert!(row_html.contains("auto-dispatcher@acme.example.com"));
        assert!(row_html.contains("agent@test.com"));
        assert!(!row_html.contains("async"));
        // The task pages are gone (p08 P08-Q3), and so is the row's link to them.
        assert!(!row_html.contains("Task Executions"));
        assert!(row_html.contains("New Thread"));
        assert!(row_html.contains(&format!(
            "/companies/{}/channels/{}/threads",
            company.id, channel.id
        )));

        let thread = Thread {
            id: Uuid::new_v4(),
            channel_id: channel.id,
            subject: "Question <script>".into(),
            participant_principal_ids: Vec::new(),
            participant_projection: crate::entities::thread::ThreadParticipantProjection {
                identities: vec![
                    crate::adapters::protocols::email::EmailIdentity::parse(
                        "person@example.com".into(),
                    )
                    .unwrap()
                    .qualify_default(),
                ],
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let threads_page = pages::channel_threads_page(
            &company,
            &channel,
            "example.com",
            std::slice::from_ref(&thread),
            Some("next_cursor"),
        );
        assert!(threads_page.contains("Auto Dispatcher Threads"));
        assert!(threads_page.contains("Question &lt;script&gt;"));
        assert!(threads_page.contains(&format!("thread_id={}", thread.id)));
        assert!(threads_page.contains("hx-swap=\"beforeend\""));
        assert!(threads_page.contains("threads/list?cursor=next_cursor"));

        let next_threads = pages::channel_thread_list_fragment(
            company.id,
            channel.id,
            "example.com",
            &[thread],
            None,
            true,
        );
        assert!(next_threads.contains("hx-swap-oob=\"outerHTML\""));

        let edit_html = pages::channel_edit_fragment(&company, "example.com", &channel, true);
        assert!(edit_html.contains("hx-put="));
        assert!(edit_html.contains("value=\"Auto Dispatcher\""));
        assert!(!edit_html.contains("Custom Channel Agent"));

        let sim_html = pages::channel_simulation_page(
            &company,
            "example.com",
            &channel,
            "logged-in@test.com",
            None,
            None,
        );
        assert!(sim_html.contains("Simulate Webhook: Auto Dispatcher"));
        assert!(sim_html.contains("auto-dispatcher@acme.example.com"));
        assert!(sim_html.contains("value=\"verify\""));
        assert!(sim_html.contains("value=\"run_test\" checked"));
        assert!(sim_html.contains("value=\"run\""));
        assert!(sim_html.contains("data-server-sender=\"logged-in@test.com\" required disabled"));
        assert!(sim_html.contains("Trigger Webhook Simulation"));
        assert!(sim_html.contains("Simulating..."));
        assert!(sim_html.contains("Open Existing Thread by ID"));
        assert!(sim_html.contains(r##"hx-sync="#simulation-result:replace""##));
        assert!(sim_html.contains("Simulated Webhook Payload"));
        assert!(sim_html.contains("value=\"logged-in@test.com\""));

        let loaded_thread_id = Uuid::parse_str("0f5421b8-9e78-4f21-ac52-3af494c3f344").unwrap();
        let live_thread = pages::simulation_live_view(
            company.id,
            channel.id,
            loaded_thread_id,
            "<p>Loaded thread</p>",
        );
        let sim_html_with_thread = pages::channel_simulation_page(
            &company,
            "example.com",
            &channel,
            "logged-in@test.com",
            Some("0f5421b8-9e78-4f21-ac52-3af494c3f344"),
            Some(&live_thread),
        );
        assert!(sim_html_with_thread.contains("Thread Loaded & Active"));
        assert!(sim_html_with_thread.contains("0f5421b8-9e78-4f21-ac52-3af494c3f344"));
        assert!(!sim_html_with_thread.contains("Simulated Webhook Payload"));
        assert!(sim_html_with_thread.contains("id=\"simulation-live\""));
        assert!(sim_html_with_thread.contains("sse-swap=\"simulation\""));

        let sim_result = pages::EmailSimulationResult {
            resolved: true,
            sender_authorized: true,
            company_slug: Some("acme".into()),
            channel_slug: Some("auto-dispatcher".into()),
            company: Some(company.clone()),
            channel: Some(channel.clone()),
            to: "auto-dispatcher@acme.example.com".to_string(),
            from: "agent@test.com".to_string(),
            subject: Some("Test".to_string()),
            text_body: Some("Body text".to_string()),
        };
        let sim_result_html =
            pages::channel_simulation_result_fragment(company.id, channel.id, &sim_result);
        assert!(sim_result_html.contains("Webhook Triggered &amp; Desk Resolved Successfully!"));

        let test_message = stored_email_view(EmailMessageDraft {
            id: Uuid::new_v4(),
            thread_id: Uuid::new_v4(),
            message_id: "<msg1@test>".into(),
            in_reply_to: None,
            references_list: vec![],
            sender: "agent@test.com".into(),
            recipients_to: vec!["auto-dispatcher@acme.example.com".into()],
            recipients_cc: vec![],
            subject: "Test".to_string(),
            clean_text_body: "Body text".to_string(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: crate::entities::message::MessageDirection::Inbound,
            role: crate::entities::message::MessageRole::Human,
            thread_index: None,
            created_at: Utc::now(),
        });
        let mut test_agent_message = stored_email_view(EmailMessageDraft {
            id: Uuid::new_v4(),
            thread_id: test_message.thread_id,
            message_id: "<out1@test>".into(),
            in_reply_to: None,
            references_list: vec![],
            sender: "auto-dispatcher@acme.example.com".into(),
            recipients_to: vec!["agent@test.com".into()],
            recipients_cc: vec![],
            subject: "Re: Test".to_string(),
            clean_text_body: "### Thread Result\n\n- **First** item\n- Second item".to_string(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: crate::entities::message::MessageDirection::Outbound,
            role: crate::entities::message::MessageRole::Agent,
            thread_index: None,
            created_at: Utc::now(),
        });

        let fail_html =
            pages::channel_simulation_failure_fragment(&pages::ChannelSimulationFailure {
                company_id: company.id,
                channel_id: channel.id,
                company: Some(&company),
                channel: Some(&channel),
                to: "test@recip.com",
                from: "sender@test.com",
                subject: "Test Subject",
                error: "Anthropic API key is missing",
            });
        assert!(fail_html.contains("Simulation Execution Error"));
        assert!(fail_html.contains("Anthropic API key is missing"));
        assert!(fail_html.contains("LLM Provider:"));
        assert!(fail_html.contains("LLM Model:"));
        assert!(fail_html.contains("API Key Status:"));
        assert!(fail_html.contains("Simulate New Thread"));

        let sample_thread = crate::entities::thread::Thread {
            id: Uuid::new_v4(),
            channel_id: channel.id,
            subject: "Existing Thread Subject".to_string(),
            participant_principal_ids: Vec::new(),
            participant_projection: crate::entities::thread::ThreadParticipantProjection {
                identities: vec![
                    crate::adapters::protocols::email::EmailIdentity::parse("user@test.com".into())
                        .unwrap()
                        .qualify_default(),
                ],
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        // p08 D4.6: every binding's outcome under the inbound message, the writing run under the
        // reply.
        use crate::{
            domain::workflow::{RunState, WaitingReason},
            entities::message_outcome::{
                AdmittedRun, BindingOutcome, MessageBindingOutcome, RunExecutionSummary,
                RunModelUsage,
            },
        };
        let run_id = Uuid::new_v4();
        test_agent_message.run_ids = vec![run_id];
        let outcomes = [
            MessageBindingOutcome {
                message_id: test_message.canonical_id.as_uuid(),
                binding_id: Uuid::new_v4(),
                workflow_title: "Triage".into(),
                outcome: BindingOutcome::Admitted(AdmittedRun {
                    payload_restricted: false,
                    run_id,
                    state: RunState::Waiting(WaitingReason::Decision),
                    executions: vec![RunExecutionSummary {
                        step_id: "draft".into(),
                        activation: 1,
                        route: Some("ready".into()),
                        created_at: Utc::now(),
                        activated_at: Some(Utc::now()),
                        completed_at: Some(Utc::now()),
                    }],
                    model_usage: vec![RunModelUsage {
                        provider: "anthropic".into(),
                        model: "claude-test".into(),
                        calls: 2,
                        input_tokens: 120,
                        output_tokens: 30,
                    }],
                }),
            },
            MessageBindingOutcome {
                message_id: test_message.canonical_id.as_uuid(),
                binding_id: Uuid::new_v4(),
                workflow_title: "<b>Escalation</b>".into(),
                outcome: BindingOutcome::Refused {
                    reason: "unavailable".into(),
                },
            },
        ];
        let loaded_thread_html =
            pages::channel_simulation_loaded_thread_fragment(&pages::SimulationThreadView {
                company: &company,
                channel: &channel,
                app_domain_name: "example.com",
                thread: &sample_thread,
                messages: &[test_message, test_agent_message],
                reply_context: None,
                outcomes: &outcomes,
                include_oob: true,
            });
        assert!(loaded_thread_html.contains("Thread Loaded & Active"));
        assert!(loaded_thread_html.contains("Existing Thread Subject"));
        assert!(!loaded_thread_html.contains("Task Execution Parameters"));
        assert!(loaded_thread_html.contains("Workflow outcomes"));
        assert!(loaded_thread_html.contains("Waiting on a decision"));
        assert!(loaded_thread_html.contains("draft #1"));
        assert!(
            loaded_thread_html
                .contains("anthropic / claude-test · 2 calls · 120 in / 30 out tokens")
        );
        assert!(loaded_thread_html.contains("Refused"));
        assert!(loaded_thread_html.contains("unavailable"));
        assert!(loaded_thread_html.contains("&lt;b&gt;Escalation&lt;/b&gt;"));
        assert!(loaded_thread_html.contains(&format!(
            "Written by run <span class=\"text-primary\">{}</span>",
            &run_id.to_string()[..8]
        )));
        assert!(loaded_thread_html.contains("Simulate Reply Webhook Call"));
        assert!(loaded_thread_html.contains("Trigger Reply Webhook Simulation"));
        assert!(loaded_thread_html.contains("Simulating..."));
        assert!(loaded_thread_html.contains("hx-disabled-elt=\"find button[type='submit']\""));
        assert!(loaded_thread_html.contains("<h3>Thread Result</h3>"));

        let error_thread_html = pages::channel_simulation_thread_error_fragment(
            company.id,
            channel.id,
            "invalid-uuid",
            "Thread not found",
            true,
        );
        assert!(error_thread_html.contains("Failed to Load Thread"));
        assert!(error_thread_html.contains("Thread not found"));
    }

    #[tokio::test]
    async fn test_slugify() {
        assert_eq!(slugify("Support Team"), "support-team");
        assert_eq!(
            slugify("  Inbound Email Handler!  "),
            "inbound-email-handler"
        );
        assert_eq!(slugify("Customer_Service 101"), "customer-service-101");
        assert_eq!(slugify("---"), "");
    }

    #[tokio::test]
    async fn test_channel_form_deserialization() {
        use axum::body::Body;
        use axum::extract::FromRequest;
        use axum::http::{Request, header};

        let req_named = Request::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("name=Inbound+Support"))
            .unwrap();
        let form_named = Form::<ChannelForm>::from_request(req_named, &())
            .await
            .unwrap()
            .0;
        assert_eq!(form_named.name, "Inbound Support");
        assert_eq!(form_named.slug, None);
        assert_eq!(slugify(&form_named.name), "inbound-support");

        assert_eq!(parse_agent_ids_form(None), None, "an absent field is none");
        assert_eq!(parse_agent_ids_form(Some(String::new())), Some(Vec::new()));
        let one = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let two = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        assert_eq!(parse_agent_ids_form(Some(one.to_string())), Some(vec![one]));
        assert_eq!(
            parse_agent_ids_form(Some(format!("{one},{two}"))),
            Some(vec![one, two])
        );
    }
}

#[cfg(test)]
#[path = "channel_authorization_tests.rs"]
mod authorization_tests;

#[cfg(test)]
#[path = "channel_department_tests.rs"]
mod department_tests;
