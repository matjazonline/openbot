use crate::use_cases::agent::AgentWrite;
use std::{convert::Infallible, sync::Arc};

use axum::{
    Form, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        Html, IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use tokio_stream::{Stream, StreamExt};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::{
        channel::Channel,
        cursor::ThreadCursor,
        memory::{MemoryPersistenceMode, MemoryRecallMode, default_memory_max_results},
        thread::Thread,
    },
    infra::{config::AppConfig, events::MailboxEvents},
    services::email_parser::RawInboundPayload,
    use_cases::{
        agent::AgentUseCases,
        channel::{ChannelUseCases, ChannelWrite, parse_recipient_address_pipeline},
        company::CompanyUseCases,
        thread::{ReplyDelivery, SimulationMode, ThreadUseCases},
        user::UserUseCases,
    },
};

use super::agent::{ModelOverrides, create_agent_from_instructions};
use super::ui::wake_ups;

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
    pub system_prompt: Option<String>,
    pub form_mode: Option<String>,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<String>,
    pub agent_ids: Option<String>,
    pub channel_config: Option<String>,
    pub confirm_spam_disabled: Option<String>,
    pub enabled: Option<String>,
    pub add_3rd_party: Option<String>,
    pub retrieve_company_memory: Option<String>,
    pub retrieve_agent_memory: Option<String>,
    pub retrieve_user_memory: Option<String>,
    pub persist_company_memory: Option<String>,
    pub persist_agent_memory: Option<String>,
    pub persist_user_memory: Option<String>,
    pub memory_persistence_mode: Option<String>,
    pub memory_recall_mode: Option<String>,
    pub memory_max_results: Option<String>,
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

    pub fn memory_settings(&self) -> Result<SubmittedMemorySettings, String> {
        let persistence_mode = match self
            .memory_persistence_mode
            .as_deref()
            .unwrap_or("audience_only")
        {
            "audience_only" => MemoryPersistenceMode::AudienceOnly,
            "scope_specific_facts" => MemoryPersistenceMode::ScopeSpecificFacts,
            _ => {
                return Err(
                    "Memory persistence mode must be audience_only or scope_specific_facts.".into(),
                );
            }
        };
        let recall_mode = match self.memory_recall_mode.as_deref().unwrap_or("fast") {
            "fast" => MemoryRecallMode::Fast,
            "thinking" => MemoryRecallMode::Thinking,
            _ => return Err("Memory recall mode must be fast or thinking.".into()),
        };
        let max_results = self
            .memory_max_results
            .as_deref()
            .unwrap_or("5")
            .parse::<u8>()
            .map_err(|_| "Memory result limit must be between 1 and 20.".to_string())?;
        if !(1..=20).contains(&max_results) {
            return Err("Memory result limit must be between 1 and 20.".into());
        }
        Ok(SubmittedMemorySettings {
            retrieve_company: checkbox_ticked(self.retrieve_company_memory.as_deref()),
            retrieve_agent: checkbox_ticked(self.retrieve_agent_memory.as_deref()),
            retrieve_user: checkbox_ticked(self.retrieve_user_memory.as_deref()),
            persist_company: checkbox_ticked(self.persist_company_memory.as_deref()),
            persist_agent: checkbox_ticked(self.persist_agent_memory.as_deref()),
            persist_user: checkbox_ticked(self.persist_user_memory.as_deref()),
            persistence_mode,
            recall_mode,
            max_results,
        })
    }
}

pub struct SubmittedMemorySettings {
    pub retrieve_company: bool,
    pub retrieve_agent: bool,
    pub retrieve_user: bool,
    pub persist_company: bool,
    pub persist_agent: bool,
    pub persist_user: bool,
    pub persistence_mode: MemoryPersistenceMode,
    pub recall_mode: MemoryRecallMode,
    pub max_results: u8,
}

/// The two shapes a browser sends for a ticked checkbox, in one place.
fn checkbox_ticked(value: Option<&str>) -> bool {
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

pub(super) fn parse_agent_ids_form(input: Option<String>) -> Option<Vec<Uuid>> {
    input.and_then(|s| {
        let list: Vec<Uuid> = s
            .split(&[',', ' ', ';', '\n'][..])
            .map(|e| e.trim())
            .filter(|e| !e.is_empty())
            .filter_map(|e| Uuid::parse_str(e).ok())
            .collect();
        if list.is_empty() { None } else { Some(list) }
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelJsonPayload {
    pub name: String,
    pub description: Option<String>,
    pub slug: Option<String>,
    /// A PUT replaces the alias set wholesale, like every other field here.
    pub alias_slugs: Option<Vec<String>>,
    pub system_prompt: Option<String>,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<Vec<String>>,
    pub agent_ids: Option<Vec<Uuid>>,
    pub channel_config: Option<serde_json::Value>,
    pub confirm_spam_disabled: Option<bool>,
    /// Omitted means enabled: like every other field here, a PUT replaces rather than patches.
    pub enabled: Option<bool>,
    /// Omitted means on, for the same reason as `enabled`.
    pub add_3rd_party: Option<bool>,
    #[serde(default)]
    pub retrieve_company_memory: bool,
    #[serde(default)]
    pub retrieve_agent_memory: bool,
    #[serde(default)]
    pub retrieve_user_memory: bool,
    #[serde(default)]
    pub persist_company_memory: bool,
    #[serde(default)]
    pub persist_agent_memory: bool,
    #[serde(default)]
    pub persist_user_memory: bool,
    #[serde(default)]
    pub memory_persistence_mode: MemoryPersistenceMode,
    #[serde(default)]
    pub memory_recall_mode: MemoryRecallMode,
    #[serde(default = "default_memory_max_results")]
    pub memory_max_results: u8,
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
    channel_id: Uuid,
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
        .list_channel_threads(channel_id, before, limit + 1)
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

pub(super) fn parse_config_form(
    input: Option<String>,
) -> Result<Option<serde_json::Value>, String> {
    match input {
        Some(ref s) if !s.trim().is_empty() => serde_json::from_str(s.trim())
            .map(Some)
            .map_err(|e| format!("Invalid JSON config: {e}")),
        _ => Ok(None),
    }
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
#[instrument(skip(company_use_cases, channel_use_cases, agent_use_cases, config, user))]
async fn list_channels_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ChannelsPageQuery>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let channels = channel_use_cases
        .list_company_channels(user.id, company_id)
        .await
        .unwrap_or_default();

    let agents = agent_use_cases
        .list_assignable_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    Html(pages::channels_page(&pages::ChannelsPage {
        company: &company,
        app_domain_name: &config.app_domain_name,
        channels: &channels,
        agents: &agents,
        spam_scan_enabled: config.is_spam_scan_enabled(),
        focus: pages::ChannelsPageFocus {
            create_form_open: matches!(query.new.as_deref(), Some("1") | Some("true")),
            editing_channel_id: query.edit,
        },
    }))
}

/// POST /companies/{company_id}/channels - HTMX create channel (Protected).
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    agent_use_cases,
    config,
    user,
    form
))]
/// Everything needed to re-render the channel list after any outcome.
///
/// Every exit from the create/update handlers ends in the same fragment, optionally headed by an
/// error alert, so the reload lives here instead of at each `return`.
async fn create_channel_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<ChannelForm>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };
    let view = ChannelListView {
        channel_use_cases: &channel_use_cases,
        agent_use_cases: &agent_use_cases,
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

    let agent_ids =
        match resolve_channel_agents(&agent_use_cases, &form, &slug, user.id, company_id).await {
            Ok(agent_ids) => agent_ids,
            Err(message) => return view.render(Some(message)).await,
        };

    let confirm_spam_disabled = form.confirm_spam_disabled();
    let enabled = form.enabled();
    let add_3rd_party = form.add_3rd_party();
    let memory = match form.memory_settings() {
        Ok(memory) => memory,
        Err(error) => return view.render(Some(error)).await,
    };

    let channel_config = match parse_config_form(form.channel_config) {
        Ok(c) => c,
        Err(err) => return view.render(Some(err)).await,
    };

    let write = ChannelWrite {
        enabled,
        add_3rd_party,
        name: form.name,
        description: parse_text_form(form.description),
        slug,
        alias_slugs: parse_list_form(form.alias_slugs),
        api_key: form.api_key,
        provider: form.provider,
        model: form.model,
        participant_emails: parse_emails_form(form.participant_emails),
        agent_ids,
        channel_config,
        retrieve_company_memory: memory.retrieve_company,
        retrieve_agent_memory: memory.retrieve_agent,
        retrieve_user_memory: memory.retrieve_user,
        persist_company_memory: memory.persist_company,
        persist_agent_memory: memory.persist_agent,
        persist_user_memory: memory.persist_user,
        memory_persistence_mode: memory.persistence_mode,
        memory_recall_mode: memory.recall_mode,
        memory_max_results: memory.max_results,
        created_by: None,
    };
    match channel_use_cases
        .create_channel(user.id, company_id, write, confirm_spam_disabled)
        .await
    {
        Ok(_) => view.render(None).await,
        Err(err) => {
            view.render(Some(format!("Failed to create channel: {err}")))
                .await
        }
    }
}

/// Everything needed to re-render the channel list after any outcome.
///
/// Every exit from the create/update handlers ends in the same fragment, optionally headed by an
/// error alert, so the reload lives here instead of at each `return`.
struct ChannelListView<'a> {
    channel_use_cases: &'a ChannelUseCases,
    agent_use_cases: &'a AgentUseCases,
    app_domain_name: &'a str,
    user_id: Uuid,
    company: &'a crate::entities::company::Company,
}

impl ChannelListView<'_> {
    async fn render(&self, error: Option<String>) -> Html<String> {
        let company_id = self.company.id;
        let agents = self
            .agent_use_cases
            .list_assignable_agents(self.user_id, company_id)
            .await
            .unwrap_or_default();
        let channels = self
            .channel_use_cases
            .list_company_channels(self.user_id, company_id)
            .await
            .unwrap_or_default();
        let list =
            pages::channel_list_fragment(self.company, self.app_domain_name, &channels, &agents);
        Html(match error {
            Some(message) => format!("{}{}", pages::error_alert(&message), list),
            None => list,
        })
    }
}

/// The agents this channel should run.
///
/// In "simple" mode the submitted instructions are expanded into a system prompt and saved as a
/// brand new agent, which is then appended to any explicitly selected ones.
pub(super) async fn resolve_channel_agents(
    agent_use_cases: &AgentUseCases,
    form: &ChannelForm,
    slug: &str,
    user_id: Uuid,
    company_id: Uuid,
) -> Result<Option<Vec<Uuid>>, String> {
    let agent_ids = parse_agent_ids_form(form.agent_ids.clone());
    let instructions = form
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let Some(instructions) = instructions else {
        return Ok(agent_ids);
    };

    let agent = if form.form_mode.as_deref() == Some("simple") {
        create_agent_from_instructions(
            agent_use_cases,
            user_id,
            company_id,
            &form.name,
            slug,
            instructions,
            ModelOverrides {
                provider: form.provider.as_deref(),
                model: form.model.as_deref(),
                api_key: form.api_key.as_deref(),
            },
            None,
        )
        .await?
    } else {
        agent_use_cases
            .create_agent(
                user_id,
                company_id,
                AgentWrite {
                    name: form.name.clone(),
                    slug: slug.to_string(),
                    provider: form.provider.clone(),
                    model: form.model.clone(),
                    api_key: form.api_key.clone(),
                    system_prompt: Some(instructions.to_string()),
                    ..AgentWrite::default()
                },
            )
            .await
            .map_err(|err| format!("Failed to create agent for channel: {err}"))?
    };

    let mut ids = agent_ids.unwrap_or_default();
    ids.push(agent.id);
    Ok(Some(ids))
}

/// GET /companies/{company_id}/channels/{id}/edit - HTMX edit channel form fragment (Protected).
#[instrument(skip(company_use_cases, channel_use_cases, agent_use_cases, config, user))]
async fn edit_channel_form(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let agents = agent_use_cases
        .list_assignable_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    if let Ok(Some(wf)) = channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await
    {
        Html(pages::channel_edit_fragment(
            &company,
            &config.app_domain_name,
            &wf,
            &agents,
            config.is_spam_scan_enabled(),
        ))
    } else {
        Html(pages::error_alert("Channel not found."))
    }
}

/// GET /companies/{company_id}/channels/{id}/cancel - Cancel channel edit fragment (Protected).
#[instrument(skip(company_use_cases, channel_use_cases, agent_use_cases, config, user))]
async fn cancel_channel_edit(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let agents = agent_use_cases
        .list_assignable_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    if let Ok(Some(wf)) = channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await
    {
        Html(pages::channel_row_fragment(
            &company,
            &config.app_domain_name,
            &wf,
            &agents,
        ))
    } else {
        Html(String::new())
    }
}

/// PUT /companies/{company_id}/channels/{id} - HTMX update channel (Protected).
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    agent_use_cases,
    config,
    user,
    form
))]
async fn update_channel_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<ChannelForm>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let agents = agent_use_cases
        .list_assignable_agents(user.id, company_id)
        .await
        .unwrap_or_default();

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
    let memory = match form.memory_settings() {
        Ok(memory) => memory,
        Err(error) => return Html(pages::error_alert(&error)),
    };
    let emails = parse_emails_form(form.participant_emails);
    let agent_ids = parse_agent_ids_form(form.agent_ids);

    let channel_config = match parse_config_form(form.channel_config) {
        Ok(c) => c,
        Err(err) => return Html(pages::error_alert(&err)),
    };

    let write = ChannelWrite {
        name: form.name,
        description: parse_text_form(form.description),
        slug,
        alias_slugs: parse_list_form(form.alias_slugs),
        api_key: form.api_key,
        provider: form.provider,
        model: form.model,
        participant_emails: emails,
        agent_ids,
        channel_config,
        enabled,
        add_3rd_party,
        retrieve_company_memory: memory.retrieve_company,
        retrieve_agent_memory: memory.retrieve_agent,
        retrieve_user_memory: memory.retrieve_user,
        persist_company_memory: memory.persist_company,
        persist_agent_memory: memory.persist_agent,
        persist_user_memory: memory.persist_user,
        memory_persistence_mode: memory.persistence_mode,
        memory_recall_mode: memory.recall_mode,
        memory_max_results: memory.max_results,
        created_by: None,
    };

    match channel_use_cases
        .update_channel(
            user.id,
            company_id,
            channel_id,
            write,
            confirm_spam_disabled,
        )
        .await
    {
        Ok(wf) => Html(pages::channel_row_fragment(
            &company,
            &config.app_domain_name,
            &wf,
            &agents,
        )),
        Err(err) => Html(pages::error_alert(&format!("Update failed: {err}"))),
    }
}

/// DELETE /companies/{company_id}/channels/{id} - HTMX delete channel (Protected).
#[instrument(skip(channel_use_cases, user))]
async fn delete_channel_handler(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let _ = channel_use_cases
        .delete_channel(user.id, company_id, channel_id)
        .await;
    Html(String::new())
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

/// GET /companies/{company_id}/channels/{id}/simulate - Simulation page (Protected).
#[instrument(skip(
    user_use_cases,
    company_use_cases,
    channel_use_cases,
    thread_use_cases,
    config,
    user
))]
async fn simulate_channel_page(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<SimulateQuery>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")).into_response(),
    };
    let channel = match channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await
    {
        Ok(Some(wf)) => wf,
        _ => return Html(pages::error_alert("Channel not found.")).into_response(),
    };
    let active_user = match user_use_cases.get_user_by_id(user.id).await {
        Ok(Some(active_user)) => active_user,
        _ => return Html(pages::error_alert("User not found.")).into_response(),
    };

    // A `?thread_id=` on the page URL pre-loads that conversation into the result pane.
    let requested_thread = query
        .thread_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let initial_result_html = match requested_thread {
        Some(thread_id) => Some(
            load_simulation_thread_fragment(
                &thread_use_cases,
                &company,
                &channel,
                &config.app_domain_name,
                thread_id,
                false,
            )
            .await
            .map(|(_, fragment)| fragment)
            .unwrap_or_else(|error_fragment| error_fragment),
        ),
        None => None,
    };

    Html(pages::channel_simulation_page(
        &company,
        &config.app_domain_name,
        &channel,
        &active_user.email,
        requested_thread,
        initial_result_html.as_deref(),
    ))
    .into_response()
}

/// Render the thread a simulation page was asked to open.
///
/// `Ok` carries the loaded thread and its fragment; `Err` carries the fragment explaining why the
/// id could not be opened, so both paths still have something to show the user.
async fn load_simulation_thread_fragment(
    thread_use_cases: &ThreadUseCases,
    company: &crate::entities::company::Company,
    channel: &Channel,
    app_domain_name: &str,
    thread_id_input: &str,
    include_oob: bool,
) -> Result<(Thread, String), String> {
    let company_id = company.id;
    let channel_id = channel.id;
    let trimmed = thread_id_input.trim();
    let error_fragment = |reason: &str| {
        pages::channel_simulation_thread_error_fragment(
            company_id,
            channel_id,
            trimmed,
            reason,
            include_oob,
        )
    };

    if trimmed.is_empty() {
        return Err(error_fragment("Thread ID cannot be empty"));
    }
    let Ok(thread_id) = Uuid::parse_str(trimmed) else {
        return Err(error_fragment(
            "Invalid Thread ID format (must be a valid UUID)",
        ));
    };

    let thread = match thread_use_cases.get_thread(thread_id).await {
        Ok(Some(thread)) => thread,
        Ok(None) => return Err(error_fragment("Thread not found")),
        Err(err) => return Err(error_fragment(&format!("Failed to retrieve thread: {err}"))),
    };
    if thread.channel_id != channel_id {
        return Err(error_fragment("Thread does not belong to this channel"));
    }

    let messages = thread_use_cases
        .get_thread_history(thread.id)
        .await
        .unwrap_or_default();
    let tasks = thread_use_cases
        .list_company_tasks(company_id, Some(channel_id), None, true)
        .await
        .unwrap_or_default();

    let fragment = pages::channel_simulation_loaded_thread_fragment(
        company,
        channel,
        app_domain_name,
        &thread,
        &messages,
        &tasks,
        include_oob,
    );
    Ok((thread, fragment))
}

/// URL the simulation page should show once a thread is open, so a refresh reloads the same thread.
fn simulation_thread_url(company_id: Uuid, channel_id: Uuid, thread_id: Uuid) -> String {
    format!("/companies/{company_id}/channels/{channel_id}/simulate?thread_id={thread_id}")
}

/// POST /companies/{company_id}/channels/{id}/simulate - Submit simulation form (Protected).
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    thread_use_cases,
    user_use_cases,
    config,
    user,
    form
))]
async fn simulate_channel_handler(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<SimulationForm>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(company)) if company.user_id == user.id => company,
        _ => return Html(pages::error_alert("Company not found.")).into_response(),
    };
    let channel = match channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await
    {
        Ok(Some(channel)) => channel,
        _ => return Html(pages::error_alert("Channel not found.")).into_response(),
    };

    // The simulated envelope must address the very channel being simulated — otherwise the run
    // would exercise a different channel's agent from inside this one's page.
    if !simulation_recipient_matches(&form.to, &company, &channel, &config.app_domain_name) {
        return Html(pages::error_alert(
            "Simulation recipient must match the selected company and channel.",
        ))
        .into_response();
    }

    let mode = match form
        .simulation_mode
        .as_deref()
        .unwrap_or("run_test")
        .to_lowercase()
        .as_str()
    {
        "run_test" => SimulationMode::RunTest,
        "run" => SimulationMode::Run,
        _ => SimulationMode::Verify,
    };

    let failure_fragment = |sender: &str, message: String| {
        Html(pages::channel_simulation_failure_fragment(
            company_id,
            channel_id,
            Some(&company),
            Some(&channel),
            &form.to,
            sender,
            form.subject.as_deref().unwrap_or("(No subject)"),
            &message,
        ))
        .into_response()
    };

    match mode {
        // Verify stops at routing: it resolves the address without running an agent.
        SimulationMode::Verify => {
            let sender = form.from.clone().unwrap_or_default();
            let inbound_email = crate::use_cases::channel::InboundEmail {
                to: form.to.clone(),
                from: sender.clone(),
                subject: form.subject.clone(),
                text_body: form.text_body.clone(),
                html_body: form.html_body.clone(),
                raw_payload: None,
            };

            match channel_use_cases
                .process_inbound_email("simulation", inbound_email, &config.app_domain_name)
                .await
            {
                Ok(result) => Html(pages::channel_simulation_result_fragment(
                    company_id, channel_id, &result,
                ))
                .into_response(),
                Err(err) => failure_fragment(&sender, format!("Simulation failed: {err}")),
            }
        }
        SimulationMode::RunTest | SimulationMode::Run => {
            let sender = match user_use_cases.get_user_by_id(user.id).await {
                Ok(Some(active_user)) => active_user.email,
                _ => return Html(pages::error_alert("User not found.")).into_response(),
            };

            let raw_payload = RawInboundPayload {
                to: form.to.clone(),
                from: sender.clone(),
                subject: form.subject.clone(),
                text: form.text_body.clone(),
                html: form.html_body.clone(),
                headers: reply_headers(form.in_reply_to.as_deref()),
                ..Default::default()
            };

            // Queued like any other message: the worker runs the agent and the view below fills
            // itself in over its own stream.
            let ingest = match thread_use_cases
                .queue_authenticated_inbound_for_agent(raw_payload, delivery_for(mode))
                .await
            {
                Ok(ingest) => ingest,
                Err(err) => {
                    return failure_fragment(
                        &sender,
                        format!("Simulation execution failed: {err}"),
                    );
                }
            };
            let Some(thread) = ingest.thread.as_ref() else {
                let reason = ingest
                    .reason
                    .unwrap_or_else(|| "The channel rejected this message.".to_string());
                return failure_fragment(&sender, reason);
            };

            match load_simulation_thread_fragment(
                &thread_use_cases,
                &company,
                &channel,
                &config.app_domain_name,
                &thread.id.to_string(),
                true,
            )
            .await
            {
                Ok((thread, fragment)) => (
                    [(
                        "HX-Push-Url",
                        simulation_thread_url(company_id, channel_id, thread.id),
                    )],
                    Html(pages::simulation_live_view(
                        company_id, channel_id, thread.id, &fragment,
                    )),
                )
                    .into_response(),
                Err(error_fragment) => Html(error_fragment).into_response(),
            }
        }
    }
}

/// GET /companies/{company_id}/channels/{id}/simulate/events - A simulated thread as the worker
/// works through it (Protected).
///
/// The simulation page used to run its agent inside the POST and print the result once. Now the run
/// is queued like every other message, so the page starts out showing only what was sent and fills
/// in from here. Every commit or status change on the thread re-renders the view whole: this is a
/// debugging page, and re-reading it in full is simpler and more honest than patching it in pieces.
#[instrument(skip(company_use_cases, channel_use_cases, thread_use_cases, events, user))]
async fn simulation_stream(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(events): State<MailboxEvents>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<OpenThreadParams>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // Authorize once, here: the stream that follows filters on ids alone, so nothing past this
    // point re-checks ownership.
    let company = company_use_cases
        .get_company(company_id)
        .await?
        .filter(|company| company.user_id == user.id)
        .ok_or_else(|| AppError::NotFound("Company not found".into()))?;
    let channel = channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;
    let thread_id = params
        .thread_id
        .as_deref()
        .unwrap_or_default()
        .trim()
        .parse::<Uuid>()
        .map_err(|_| AppError::NotFound("Thread not found".into()))?;
    let thread = thread_use_cases
        .get_thread(thread_id)
        .await?
        .filter(|thread| thread.channel_id == channel_id)
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;

    let mut wake_ups = Box::pin(wake_ups(&events, "simulation", move |event| {
        event.is_message_in_thread(thread_id) || event.is_activity_in_thread(thread_id)
    }));
    let app_domain_name = thread_use_cases.config().app_domain_name.clone();

    let stream = async_stream::stream! {
        // A lagged subscriber needs no special handling: the next render reads current state.
        while wake_ups.next().await.is_some() {
            // `include_oob: false` -- the out-of-band form reset belongs to the POST that started
            // this run, not to every update after it.
            let rendered = load_simulation_thread_fragment(
                &thread_use_cases,
                &company,
                &channel,
                &app_domain_name,
                &thread.id.to_string(),
                false,
            )
            .await;
            match rendered {
                Ok((_, fragment)) => {
                    yield Ok(Event::default().event("simulation").data(fragment));
                }
                Err(error_fragment) => {
                    yield Ok(Event::default().event("simulation").data(error_fragment));
                    return;
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// A simulation asks for a real send only in `Run` mode; `RunTest` keeps the answer in-app.
fn delivery_for(mode: SimulationMode) -> ReplyDelivery {
    if mode == SimulationMode::Run {
        ReplyDelivery::Send
    } else {
        ReplyDelivery::InAppOnly
    }
}

/// Does the simulated `To:` address resolve to exactly the channel being simulated?
fn simulation_recipient_matches(
    to: &str,
    company: &crate::entities::company::Company,
    channel: &Channel,
    app_domain_name: &str,
) -> bool {
    parse_recipient_address_pipeline(to, app_domain_name).is_some_and(
        |(company_slug, channel_slugs, _)| {
            company_slug.eq_ignore_ascii_case(&company.slug)
                && !channel_slugs.is_empty()
                && channel_slugs
                    .iter()
                    .all(|slug| slug.eq_ignore_ascii_case(&channel.slug))
        },
    )
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

async fn open_simulated_thread_logic(
    company_use_cases: Arc<CompanyUseCases>,
    channel_use_cases: Arc<ChannelUseCases>,
    thread_use_cases: Arc<ThreadUseCases>,
    config: Arc<AppConfig>,
    user: AuthenticatedUser,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id_input: &str,
) -> axum::response::Response {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")).into_response(),
    };
    let channel = match channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await
    {
        Ok(Some(wf)) => wf,
        _ => return Html(pages::error_alert("Channel not found.")).into_response(),
    };

    match load_simulation_thread_fragment(
        &thread_use_cases,
        &company,
        &channel,
        &config.app_domain_name,
        thread_id_input,
        true,
    )
    .await
    {
        Ok((thread, fragment)) => (
            [(
                "HX-Push-Url",
                simulation_thread_url(company_id, channel_id, thread.id),
            )],
            Html(pages::simulation_live_view(
                company_id, channel_id, thread.id, &fragment,
            )),
        )
            .into_response(),
        Err(error_fragment) => Html(error_fragment).into_response(),
    }
}

#[instrument(skip(company_use_cases, channel_use_cases, thread_use_cases, config, user))]
async fn open_simulated_thread_get(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<OpenThreadParams>,
) -> impl IntoResponse {
    let tid_str = params.thread_id.as_deref().unwrap_or("");
    open_simulated_thread_logic(
        company_use_cases,
        channel_use_cases,
        thread_use_cases,
        config,
        user,
        company_id,
        channel_id,
        tid_str,
    )
    .await
}

#[instrument(skip(company_use_cases, channel_use_cases, thread_use_cases, config, user))]
async fn open_simulated_thread_post(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Form(params): Form<OpenThreadParams>,
) -> impl IntoResponse {
    let tid_str = params.thread_id.as_deref().unwrap_or("");
    open_simulated_thread_logic(
        company_use_cases,
        channel_use_cases,
        thread_use_cases,
        config,
        user,
        company_id,
        channel_id,
        tid_str,
    )
    .await
}

/// GET /companies/{company_id}/channels/{id}/threads - Paginated thread list page (Protected).
async fn list_channel_threads_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    let company = company_use_cases
        .get_company(company_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Company not found".into()))?;
    let channel = channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;
    let page = load_thread_page(
        &thread_use_cases,
        channel_id,
        &ThreadListQuery {
            limit: None,
            cursor: None,
        },
    )
    .await?;

    Ok(Html(pages::channel_threads_page(
        &company,
        &channel,
        &page.threads,
        page.next_cursor.as_deref(),
    )))
}

/// GET /companies/{company_id}/channels/{id}/threads/list - Next HTMX page (Protected).
async fn list_channel_threads_fragment(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ThreadListQuery>,
) -> AppResult<Html<String>> {
    channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;
    let page = load_thread_page(&thread_use_cases, channel_id, &query).await?;

    Ok(Html(pages::channel_thread_list_fragment(
        company_id,
        channel_id,
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
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ThreadListQuery>,
) -> AppResult<impl IntoResponse> {
    channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found".into()))?;

    let page = load_thread_page(&thread_use_cases, channel_id, &query).await?;

    Ok((StatusCode::OK, Json(page)))
}

/// JSON API: Create company channel (Protected).
async fn create_channel_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
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

    let mut agent_ids = payload.agent_ids;

    if let Some(ref prompt) = payload.system_prompt {
        let prompt_trimmed = prompt.trim();
        if !prompt_trimmed.is_empty() {
            let agent = agent_use_cases
                .create_agent(
                    user.id,
                    company_id,
                    AgentWrite {
                        name: payload.name.clone(),
                        slug: slug.clone(),
                        provider: payload.provider.clone(),
                        model: payload.model.clone(),
                        api_key: payload.api_key.clone(),
                        system_prompt: Some(prompt_trimmed.to_string()),
                        ..AgentWrite::default()
                    },
                )
                .await?;
            let mut ids = agent_ids.unwrap_or_default();
            ids.push(agent.id);
            agent_ids = Some(ids);
        }
    }

    let write = ChannelWrite {
        name: payload.name,
        description: parse_text_form(payload.description),
        slug,
        alias_slugs: payload.alias_slugs.unwrap_or_default(),
        api_key: payload.api_key,
        provider: payload.provider,
        model: payload.model,
        participant_emails: payload.participant_emails,
        agent_ids,
        channel_config: payload.channel_config,
        enabled: payload.enabled.unwrap_or(true),
        add_3rd_party: payload.add_3rd_party.unwrap_or(true),
        retrieve_company_memory: payload.retrieve_company_memory,
        retrieve_agent_memory: payload.retrieve_agent_memory,
        retrieve_user_memory: payload.retrieve_user_memory,
        persist_company_memory: payload.persist_company_memory,
        persist_agent_memory: payload.persist_agent_memory,
        persist_user_memory: payload.persist_user_memory,
        memory_persistence_mode: payload.memory_persistence_mode,
        memory_recall_mode: payload.memory_recall_mode,
        memory_max_results: payload.memory_max_results,
        created_by: None,
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
async fn update_channel_json(
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

    let write = ChannelWrite {
        name: payload.name,
        description: parse_text_form(payload.description),
        slug,
        alias_slugs: payload.alias_slugs.unwrap_or_default(),
        api_key: payload.api_key,
        provider: payload.provider,
        model: payload.model,
        participant_emails: payload.participant_emails,
        agent_ids: payload.agent_ids,
        channel_config: payload.channel_config,
        enabled: payload.enabled.unwrap_or(true),
        add_3rd_party: payload.add_3rd_party.unwrap_or(true),
        retrieve_company_memory: payload.retrieve_company_memory,
        retrieve_agent_memory: payload.retrieve_agent_memory,
        retrieve_user_memory: payload.retrieve_user_memory,
        persist_company_memory: payload.persist_company_memory,
        persist_agent_memory: payload.persist_agent_memory,
        persist_user_memory: payload.persist_user_memory,
        memory_persistence_mode: payload.memory_persistence_mode,
        memory_recall_mode: payload.memory_recall_mode,
        memory_max_results: payload.memory_max_results,
        created_by: None,
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
    use chrono::Utc;
    use serde_json::json;

    use super::*;

    #[test]
    fn thread_cursor_round_trips() {
        let thread = Thread {
            id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            subject: "Subject".into(),
            participant_emails: vec![],
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
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
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
            api_key: None,
            provider: None,
            model: None,
            participant_emails: Some(vec!["agent@test.com".into()]),
            agent_ids: None,
            channel_config: Some(json!({ "mode": "async" })),
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            memory_persistence_mode: crate::entities::memory::MemoryPersistenceMode::AudienceOnly,
            memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
            memory_max_results: 5,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        };

        let channels = [channel.clone()];
        let page = pages::ChannelsPage {
            company: &company,
            app_domain_name: "example.com",
            channels: &channels,
            agents: &[],
            spam_scan_enabled: true,
            focus: pages::ChannelsPageFocus::default(),
        };
        let page_html = pages::channels_page(&page);
        assert!(page_html.contains("Custom Channel Agent"));
        assert!(page_html.contains("LLM Provider (Optional Override)"));
        assert!(page_html.contains("LLM Model (Optional Override)"));
        assert!(page_html.contains("LLM API Key (Optional Override)"));
        assert!(page_html.contains("Channel Config (JSON, Optional)"));
        assert!(page_html.contains("id=\"channel-form-toggle\""));
        assert!(page_html.contains("id=\"channel-form-card\" class=\"hidden"));
        assert!(page_html.contains("aria-controls=\"channel-form-card\""));
        assert!(page_html.contains("Agent Instructions"));
        assert!(!page_html.contains("target_id\": \"simple_system_prompt\""));
        assert!(!page_html.contains("id=\"simple_prompt_gen_box\""));
        assert!(page_html.contains("Creating..."));
        assert!(page_html.contains("animate-spin h-4 w-4"));
        assert!(page_html.contains("hx-disabled-elt=\"find button[type='submit']\""));

        let row_html = pages::channel_row_fragment(&company, "example.com", &channel, &[]);
        assert!(row_html.contains("Auto Dispatcher"));
        assert!(row_html.contains("auto-dispatcher@acme.example.com"));
        assert!(row_html.contains("agent@test.com"));
        assert!(row_html.contains("async"));
        assert!(row_html.contains("Task Executions"));
        assert!(row_html.contains("New Thread"));
        assert!(row_html.contains(&format!(
            "/companies/{}/channels/{}/threads",
            company.id, channel.id
        )));

        let thread = Thread {
            id: Uuid::new_v4(),
            channel_id: channel.id,
            subject: "Question <script>".into(),
            participant_emails: vec!["person@example.com".into()],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let threads_page = pages::channel_threads_page(
            &company,
            &channel,
            std::slice::from_ref(&thread),
            Some("next_cursor"),
        );
        assert!(threads_page.contains("Auto Dispatcher Threads"));
        assert!(threads_page.contains("Question &lt;script&gt;"));
        assert!(threads_page.contains(&format!("thread_id={}", thread.id)));
        assert!(threads_page.contains("hx-swap=\"beforeend\""));
        assert!(threads_page.contains("threads/list?cursor=next_cursor"));

        let next_threads =
            pages::channel_thread_list_fragment(company.id, channel.id, &[thread], None, true);
        assert!(next_threads.contains("hx-swap-oob=\"outerHTML\""));

        let edit_html = pages::channel_edit_fragment(&company, "example.com", &channel, &[], true);
        assert!(edit_html.contains("hx-put="));
        assert!(edit_html.contains("value=\"Auto Dispatcher\""));
        assert!(edit_html.contains("Custom Channel Agent"));

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
        assert!(sim_html.contains("Simulated Webhook Payload"));
        assert!(sim_html.contains("value=\"logged-in@test.com\""));

        let sim_html_with_thread = pages::channel_simulation_page(
            &company,
            "example.com",
            &channel,
            "logged-in@test.com",
            Some("0f5421b8-9e78-4f21-ac52-3af494c3f344"),
            None,
        );
        assert!(sim_html_with_thread.contains("Thread Loaded & Active"));
        assert!(sim_html_with_thread.contains("0f5421b8-9e78-4f21-ac52-3af494c3f344"));
        assert!(!sim_html_with_thread.contains("Simulated Webhook Payload"));

        let sim_result = crate::use_cases::channel::InboundEmailResult {
            resolved: true,
            sender_authorized: true,
            company_slug: Some("acme".into()),
            channel_slug: Some("auto-dispatcher".into()),
            company: Some(company.clone()),
            channel: Some(channel.clone()),
            email: crate::use_cases::channel::InboundEmail {
                to: "auto-dispatcher@acme.example.com".to_string(),
                from: "agent@test.com".to_string(),
                subject: Some("Test".to_string()),
                text_body: Some("Body text".to_string()),
                html_body: None,
                raw_payload: None,
            },
        };
        let sim_result_html =
            pages::channel_simulation_result_fragment(company.id, channel.id, &sim_result);
        assert!(sim_result_html.contains("Webhook Triggered &amp; Channel Resolved Successfully!"));

        let test_message = crate::entities::message::Message {
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
        };
        let test_agent_message = crate::entities::message::Message {
            id: Uuid::new_v4(),
            thread_id: test_message.thread_id,
            message_id: "<out1@test>".into(),
            in_reply_to: Some(test_message.message_id.clone()),
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
        };

        let fail_html = pages::channel_simulation_failure_fragment(
            company.id,
            channel.id,
            Some(&company),
            Some(&channel),
            "test@recip.com",
            "sender@test.com",
            "Test Subject",
            "Anthropic API key is missing",
        );
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
            participant_emails: vec!["user@test.com".into()],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let loaded_thread_html = pages::channel_simulation_loaded_thread_fragment(
            &company,
            &channel,
            "example.com",
            &sample_thread,
            &[test_message, test_agent_message],
            &[],
            true,
        );
        assert!(loaded_thread_html.contains("Thread Loaded & Active"));
        assert!(loaded_thread_html.contains("Existing Thread Subject"));
        assert!(loaded_thread_html.contains("Task Execution Parameters"));
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

        let req_simple = Request::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(
                "name=Inbound+Support&system_prompt=You+are+a+support+agent.&form_mode=simple",
            ))
            .unwrap();
        let form_simple = Form::<ChannelForm>::from_request(req_simple, &())
            .await
            .unwrap()
            .0;
        assert_eq!(form_simple.name, "Inbound Support");
        assert_eq!(form_simple.slug, None);
        assert_eq!(
            form_simple.system_prompt,
            Some("You are a support agent.".to_string())
        );
        assert_eq!(form_simple.form_mode.as_deref(), Some("simple"));
        assert_eq!(slugify(&form_simple.name), "inbound-support");

        let req_omitted = Request::builder()
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("name=My+Channel&slug=my-channel&agent_ids="))
            .unwrap();
        let form_omitted = Form::<ChannelForm>::from_request(req_omitted, &())
            .await
            .unwrap()
            .0;
        assert_eq!(parse_agent_ids_form(form_omitted.agent_ids), None);

        let req_single = Request::builder()
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(
                "name=My+Channel&slug=my-channel&agent_ids=00000000-0000-0000-0000-000000000001",
            ))
            .unwrap();
        let form_single = Form::<ChannelForm>::from_request(req_single, &())
            .await
            .unwrap()
            .0;
        assert_eq!(
            parse_agent_ids_form(form_single.agent_ids),
            Some(vec![
                Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
            ])
        );

        let req_multiple = Request::builder()
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("name=My+Channel&slug=my-channel&agent_ids=00000000-0000-0000-0000-000000000001%2C00000000-0000-0000-0000-000000000002"))
            .unwrap();
        let form_multiple = Form::<ChannelForm>::from_request(req_multiple, &())
            .await
            .unwrap()
            .0;
        assert_eq!(
            parse_agent_ids_form(form_multiple.agent_ids),
            Some(vec![
                Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
                Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
            ])
        );
    }
}
