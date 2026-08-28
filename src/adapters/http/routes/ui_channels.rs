//! `/ui/channels` — the Channels workspace: the mailbox shell with the channel list configured
//! rather than read.
//!
//! The shell, the company scoping and the form parsing are all shared: chrome comes from
//! [`crate::adapters::http::pages::ui_shell`], the company from [`super::ui::load_managed_company`],
//! and every field is parsed by the same helpers the classic Channels page uses, so the two UIs
//! cannot drift on what a submitted channel means.

use std::collections::HashSet;
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
    },
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent, channel::Channel, company::Company, schedule::ChannelSchedule,
        value_objects::EmailAddress,
    },
    infra::config::AppConfig,
    use_cases::{
        agent::AgentUseCases,
        channel::{ChannelUseCases, ChannelWrite},
        company::CompanyUseCases,
        schedule::ScheduleUseCases,
        user::UserUseCases,
    },
};

use super::{
    channel::{
        ChannelForm, parse_agent_ids_form, parse_config_form, parse_emails_form, parse_list_form,
        parse_text_form, resolve_channel_agents, slugify,
    },
    ui::{load_account, load_managed_company, managed_company_membership, workspace_user},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/channels", get(channels_page).post(create_channel))
        .route(
            "/ui/channels/easy",
            axum::routing::post(create_easy_channels),
        )
        .route("/ui/channels/new", get(create_pane))
        .route("/ui/channels/close", get(close_pane))
        .route(
            "/ui/channels/{channel_id}",
            get(edit_pane).put(update_channel).delete(delete_channel),
        )
}

/// What the workspace has selected, all optional so `/ui/channels` alone is a valid entry point.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceQuery {
    pub company_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    /// `?new=1` opens the create form instead of a channel's settings.
    pub new: Option<String>,
}

/// The company scope every fragment and every write carries, in the URL rather than the body so
/// the form itself stays exactly [`ChannelForm`].
#[derive(Debug, Clone, Deserialize)]
pub struct CompanyQuery {
    pub company_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct EasyChannelForm {
    library_agent_ids: Option<String>,
}

const NO_SELECTION: &str = "Select a channel to configure it, or create a new one.";

/// The use cases and the caller every Channels handler starts from.
///
/// Extracted as one value rather than five `State`s per handler: each of these routes needs the
/// same set, and a handler's own parameters should be what makes it different from its siblings.
struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    channel_use_cases: Arc<ChannelUseCases>,
    schedule_use_cases: Arc<ScheduleUseCases>,
    agent_use_cases: Arc<AgentUseCases>,
    user_use_cases: Arc<UserUseCases>,
    config: Arc<AppConfig>,
    user_id: Uuid,
}

impl FromRequestParts<AppState> for Workspace {
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
            user_use_cases: state.user_use_cases.clone(),
            config: state.config.clone(),
            user_id: user.id,
        })
    }
}

impl Workspace {
    /// The company a request is scoped to, always picked from those the caller owns or administers
    /// so a guessed `company_id` cannot reach another company's channels.
    async fn scoped_company(&self, company_id: Uuid) -> AppResult<Company> {
        let (_, company) =
            load_managed_company(&self.company_use_cases, self.user_id, Some(company_id)).await?;
        company.ok_or_else(|| AppError::NotFound("Company not found".into()))
    }

    async fn view<'a>(&'a self, company: &'a Company) -> AppResult<ChannelSettingsView<'a>> {
        let memory_ready = self
            .channel_use_cases
            .memory_ready(self.user_id, company.id)
            .await?;
        Ok(ChannelSettingsView {
            channel_use_cases: &self.channel_use_cases,
            schedule_use_cases: &self.schedule_use_cases,
            agent_use_cases: &self.agent_use_cases,
            config: &self.config,
            user_id: self.user_id,
            company,
            memory_ready,
        })
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        extract::{Form, FromRequest},
        http::{Request, header},
    };

    use super::*;

    #[tokio::test]
    async fn rejected_memory_limit_is_preserved_in_the_draft() {
        let request = Request::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(
                "name=Support&slug=support&memory_max_results=0&memory_recall_mode=fast",
            ))
            .unwrap();
        let form = Form::<ChannelForm>::from_request(request, &())
            .await
            .unwrap()
            .0;
        let submitted = SubmittedChannel::new(form);

        assert!(submitted.write(None, None).is_err());
        assert_eq!(submitted.draft().memory_max_results, "0");
    }
}

/// GET /ui/channels - The Channels workspace for the selected company / channel (Protected).
#[instrument(skip(workspace))]
async fn channels_page(
    workspace: Workspace,
    Query(query): Query<WorkspaceQuery>,
) -> AppResult<Html<String>> {
    let account = load_account(&workspace.user_use_cases, workspace.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let workspace_user = workspace_user(&account, &account_email, &workspace.config);

    let (companies, company) = load_managed_company(
        &workspace.company_use_cases,
        workspace.user_id,
        query.company_id,
    )
    .await?;
    let Some(company) = company else {
        return Ok(Html(pages::mailbox_no_company_page(&workspace_user)));
    };
    let workspace_user = workspace_user
        .with_company_membership(managed_company_membership(&company, workspace.user_id));

    let view = workspace.view(&company).await?;
    let channels = view.channels().await?;
    let agents = view.agents().await?;
    // Landing on `/ui/channels` with no `channel_id` opens the first channel rather than an empty
    // pane, so the workspace is never a blank screen when there is something to show.
    let selected = query
        .channel_id
        .and_then(|id| channels.iter().find(|channel| channel.id == id))
        .or_else(|| channels.first());

    let selected_schedules = match selected {
        Some(channel) => view.schedules(channel.id).await?,
        None => vec![],
    };

    let creating = matches!(query.new.as_deref(), Some("1") | Some("true"));
    let pane_html = match (creating, selected) {
        (true, _) => view.create_pane(&agents, &pages::ChannelDraft::default(), None),
        (false, Some(channel)) => view.edit_pane(channel, &agents, &selected_schedules, None, None),
        (false, None) => {
            pages::channel_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline)
        }
    };

    let list = view.list(&channels, selected.map(|channel| channel.id));
    Ok(Html(pages::channel_settings_page(
        &pages::ChannelSettingsPage {
            user: &workspace_user,
            companies: &companies,
            list: &list,
            pane_html: &pane_html,
        },
    )))
}

/// GET /ui/channels/new - The create-channel form for the pane (Protected).
#[instrument(skip(workspace))]
async fn create_pane(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company).await?;
    let agents = view.agents().await?;

    Ok(Html(view.create_pane(
        &agents,
        &pages::ChannelDraft::default(),
        None,
    )))
}

/// GET /ui/channels/close - Dismiss whichever form the pane holds (Protected).
///
/// What Cancel does: the pane goes back to its placeholder and the sidebar loses its highlight,
/// so cancelling a create and cancelling an edit both leave the workspace in the same state as
/// arriving at `/ui/channels` with nothing selected.
#[instrument(skip(workspace))]
async fn close_pane(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    workspace.view(&company).await?.cleared_response().await
}

/// GET /ui/channels/{channel_id} - One channel's settings for the pane (Protected).
#[instrument(skip(workspace))]
async fn edit_pane(
    workspace: Workspace,
    Path(channel_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company).await?;
    let channel = view.channel(channel_id).await?;
    let agents = view.agents().await?;
    let schedules = view.schedules(channel.id).await?;

    Ok(Html(
        view.edit_pane(&channel, &agents, &schedules, None, None),
    ))
}

/// POST /ui/channels - Create a channel from the pane's Simple or Advanced form (Protected).
#[instrument(skip(workspace, form))]
async fn create_channel(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<ChannelForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company).await?;
    let agents = view.agents().await?;
    let submitted = SubmittedChannel::new(form);

    let rejected = |message: String| {
        Ok(Html(view.create_pane(&agents, &submitted.draft(), Some(&message))).into_response())
    };

    let agent_ids = match resolve_channel_agents(
        &workspace.agent_use_cases,
        &submitted.form,
        &submitted.slug,
        workspace.user_id,
        company.id,
    )
    .await
    {
        Ok(agent_ids) => agent_ids,
        Err(message) => return rejected(message),
    };
    let channel_config = match parse_config_form(submitted.form.channel_config.clone()) {
        Ok(config) => config,
        Err(message) => return rejected(message),
    };

    let write = match submitted.write(agent_ids, channel_config) {
        Ok(write) => write,
        Err(message) => return rejected(message),
    };
    let created = workspace
        .channel_use_cases
        .create_channel(
            workspace.user_id,
            company.id,
            write,
            submitted.form.confirm_spam_disabled(),
        )
        .await;

    match created {
        Ok(channel) => {
            // Agents may have grown by one: "simple" mode creates the channel's agent for it.
            let agents = view.agents().await?;
            view.saved_response(&channel, &agents).await
        }
        Err(err) => rejected(format!("Failed to create channel: {err}")),
    }
}

/// POST /ui/channels/easy - Provision one same-named channel per selected library agent.
#[instrument(skip(workspace, form))]
async fn create_easy_channels(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<EasyChannelForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company).await?;
    let agents = view.agents().await?;
    let submitted_ids = parse_agent_ids_form(form.library_agent_ids).unwrap_or_default();
    let mut seen = HashSet::new();
    let selected_ids = submitted_ids
        .into_iter()
        .filter(|id| seen.insert(*id))
        .collect::<Vec<_>>();
    let draft = pages::ChannelDraft {
        agent_ids: &selected_ids,
        ..pages::ChannelDraft::default()
    };
    let rejected = |message: String| {
        Ok(Html(view.create_pane_mode(&agents, &draft, true, Some(&message))).into_response())
    };

    if selected_ids.is_empty() {
        return rejected("Select at least one library agent.".to_string());
    }
    let selected = selected_ids
        .iter()
        .map(|id| {
            agents
                .iter()
                .find(|agent| agent.id == *id && agent.is_library())
        })
        .collect::<Option<Vec<_>>>();
    let Some(selected) = selected else {
        return rejected(
            "One or more selected library agents are no longer available.".to_string(),
        );
    };

    let mut created = Vec::new();
    for agent in selected {
        let write = ChannelWrite {
            name: agent.name.clone(),
            slug: agent.slug.clone(),
            agent_ids: Some(vec![agent.id]),
            enabled: true,
            add_3rd_party: true,
            ..ChannelWrite::default()
        };
        match workspace
            .channel_use_cases
            .create_channel(workspace.user_id, company.id, write, false)
            .await
        {
            Ok(channel) => created.push(channel),
            Err(err) => {
                for channel in created {
                    let _ = workspace
                        .channel_use_cases
                        .delete_channel(workspace.user_id, company.id, channel.id)
                        .await;
                }
                return rejected(format!("Failed to create channels: {err}"));
            }
        }
    }

    let channel = created.first().expect("at least one channel was selected");
    view.saved_response(channel, &agents).await
}

/// PUT /ui/channels/{channel_id} - Save one channel's settings (Protected).
#[instrument(skip(workspace, form))]
async fn update_channel(
    workspace: Workspace,
    Path(channel_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<ChannelForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company).await?;
    let stored = view.channel(channel_id).await?;
    let agents = view.agents().await?;
    let schedules = view.schedules(channel_id).await?;
    let submitted = SubmittedChannel::new(form);

    let rejected = |message: String| {
        Ok(Html(view.edit_pane(
            &stored,
            &agents,
            &schedules,
            Some(&submitted.draft()),
            Some(&message),
        ))
        .into_response())
    };

    let channel_config = match parse_config_form(submitted.form.channel_config.clone()) {
        Ok(config) => config,
        Err(message) => return rejected(message),
    };

    let write = match submitted.write(Some(submitted.agent_ids.clone()), channel_config) {
        Ok(write) => write,
        Err(message) => return rejected(message),
    };
    let saved = workspace
        .channel_use_cases
        .update_channel(
            workspace.user_id,
            company.id,
            channel_id,
            write,
            submitted.form.confirm_spam_disabled(),
        )
        .await;

    match saved {
        Ok(channel) => view.saved_response(&channel, &agents).await,
        Err(err) => rejected(format!("Failed to save channel: {err}")),
    }
}

/// DELETE /ui/channels/{channel_id} - Delete a channel and clear the pane (Protected).
#[instrument(skip(workspace))]
async fn delete_channel(
    workspace: Workspace,
    Path(channel_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    workspace
        .channel_use_cases
        .delete_channel(workspace.user_id, company.id, channel_id)
        .await?;

    workspace.view(&company).await?.cleared_response().await
}

/// Everything the workspace renders from, so each handler names its data once.
struct ChannelSettingsView<'a> {
    channel_use_cases: &'a ChannelUseCases,
    schedule_use_cases: &'a ScheduleUseCases,
    agent_use_cases: &'a AgentUseCases,
    config: &'a AppConfig,
    user_id: Uuid,
    company: &'a Company,
    memory_ready: bool,
}

impl ChannelSettingsView<'_> {
    async fn channels(&self) -> AppResult<Vec<Channel>> {
        self.channel_use_cases
            .list_company_channels(self.user_id, self.company.id)
            .await
    }

    async fn agents(&self) -> AppResult<Vec<Agent>> {
        self.agent_use_cases
            .list_assignable_agents(self.user_id, self.company.id)
            .await
    }

    async fn schedules(&self, channel_id: Uuid) -> AppResult<Vec<ChannelSchedule>> {
        self.schedule_use_cases
            .list_channel_schedules(self.user_id, self.company.id, channel_id)
            .await
    }

    async fn channel(&self, channel_id: Uuid) -> AppResult<Channel> {
        self.channel_use_cases
            .get_company_channel(self.user_id, self.company.id, channel_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Channel not found".into()))
    }

    fn list<'c>(
        &'c self,
        channels: &'c [Channel],
        selected_channel_id: Option<Uuid>,
    ) -> pages::ChannelSettingsList<'c> {
        pages::ChannelSettingsList {
            company: self.company,
            app_domain_name: &self.config.app_domain_name,
            channels,
            selected_channel_id,
        }
    }

    fn create_pane(
        &self,
        agents: &[Agent],
        draft: &pages::ChannelDraft<'_>,
        error: Option<&str>,
    ) -> String {
        self.create_pane_mode(agents, draft, false, error)
    }

    fn create_pane_mode(
        &self,
        agents: &[Agent],
        draft: &pages::ChannelDraft<'_>,
        easy: bool,
        error: Option<&str>,
    ) -> String {
        pages::channel_create_pane_with_memory(
            &pages::ChannelCreatePane {
                company: self.company,
                app_domain_name: &self.config.app_domain_name,
                agents,
                spam_scan_enabled: self.config.is_spam_scan_enabled(),
                draft,
                easy,
                error,
            },
            self.memory_ready,
        )
    }

    fn edit_pane(
        &self,
        channel: &Channel,
        agents: &[Agent],
        schedules: &[ChannelSchedule],
        draft: Option<&pages::ChannelDraft<'_>>,
        error: Option<&str>,
    ) -> String {
        pages::channel_edit_pane_with_memory(
            &pages::ChannelEditPane {
                company: self.company,
                app_domain_name: &self.config.app_domain_name,
                channel,
                agents,
                schedules,
                spam_scan_enabled: self.config.is_spam_scan_enabled(),
                draft,
                error,
            },
            self.memory_ready,
        )
    }

    /// The empty pane plus a sidebar with nothing selected — what cancelling a form and deleting
    /// a channel both leave behind.
    async fn cleared_response(&self) -> AppResult<Response> {
        let channels = self.channels().await?;
        let pane = pages::channel_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline);
        let list = pages::channel_settings_list(
            &self.list(&channels, None),
            pages::FragmentSwap::OutOfBand,
        );

        Ok(Html(format!("{pane}{list}")).into_response())
    }

    /// What every successful write returns: the saved channel's pane, with the sidebar list
    /// refreshed beside it so a create, rename or slug change shows up immediately.
    async fn saved_response(&self, channel: &Channel, agents: &[Agent]) -> AppResult<Response> {
        let schedules = self.schedules(channel.id).await?;
        let pane = self.edit_pane(channel, agents, &schedules, None, None);
        let channels = self.channels().await?;
        let list = pages::channel_settings_list(
            &self.list(&channels, Some(channel.id)),
            pages::FragmentSwap::OutOfBand,
        );

        Ok((
            [(
                "HX-Push-Url",
                format!(
                    "/ui/channels?company_id={}&channel_id={}",
                    self.company.id, channel.id
                ),
            )],
            Html(format!("{pane}{list}")),
        )
            .into_response())
    }
}

/// A submitted channel form, kept whole so a rejected save can be re-rendered with what was typed.
///
/// It also owns the two derivations every write needs — the slug the name falls back to, and the
/// parsed agent list — so neither is re-derived at a call site.
struct SubmittedChannel {
    form: ChannelForm,
    slug: String,
    agent_ids: Vec<Uuid>,
}

impl SubmittedChannel {
    fn new(form: ChannelForm) -> Self {
        let slug = form
            .slug
            .as_deref()
            .map(str::trim)
            .filter(|slug| !slug.is_empty())
            .map(String::from)
            .unwrap_or_else(|| slugify(&form.name));
        let agent_ids = parse_agent_ids_form(form.agent_ids.clone()).unwrap_or_default();

        Self {
            form,
            slug,
            agent_ids,
        }
    }

    fn draft(&self) -> pages::ChannelDraft<'_> {
        pages::ChannelDraft {
            name: &self.form.name,
            description: self.form.description.as_deref().unwrap_or(""),
            slug: &self.slug,
            alias_slugs: self.form.alias_slugs.as_deref().unwrap_or(""),
            system_prompt: self.form.system_prompt.as_deref().unwrap_or(""),
            participant_emails: self.form.participant_emails.as_deref().unwrap_or(""),
            agent_ids: &self.agent_ids,
            provider: self.form.provider.as_deref().unwrap_or(""),
            model: self.form.model.as_deref().unwrap_or(""),
            api_key: self.form.api_key.as_deref().unwrap_or(""),
            channel_config: self.form.channel_config.as_deref().unwrap_or(""),
            advanced: self.form.form_mode.as_deref() != Some("simple"),
            enabled: self.form.enabled(),
            add_3rd_party: self.form.add_3rd_party(),
            retrieve_company_memory: self.form.retrieve_company_memory.is_some(),
            retrieve_agent_memory: self.form.retrieve_agent_memory.is_some(),
            retrieve_user_memory: self.form.retrieve_user_memory.is_some(),
            persist_company_memory: self.form.persist_company_memory.is_some(),
            persist_agent_memory: self.form.persist_agent_memory.is_some(),
            persist_user_memory: self.form.persist_user_memory.is_some(),
            memory_persistence_mode: self
                .form
                .memory_persistence_mode
                .as_deref()
                .unwrap_or("audience_only"),
            memory_recall_mode: self.form.memory_recall_mode.as_deref().unwrap_or("fast"),
            memory_max_results: self
                .form
                .memory_max_results
                .clone()
                .unwrap_or_else(|| "5".into()),
        }
    }

    /// The write this submission asks for. `agent_ids` and `channel_config` are passed in because
    /// each handler resolves them differently (create may mint an agent; update reuses the list).
    fn write(
        &self,
        agent_ids: Option<Vec<Uuid>>,
        channel_config: Option<serde_json::Value>,
    ) -> Result<ChannelWrite, String> {
        let memory = self.form.memory_settings()?;
        Ok(ChannelWrite {
            name: self.form.name.clone(),
            description: parse_text_form(self.form.description.clone()),
            slug: self.slug.clone(),
            alias_slugs: parse_list_form(self.form.alias_slugs.clone()),
            api_key: self.form.api_key.clone(),
            provider: self.form.provider.clone(),
            model: self.form.model.clone(),
            participant_emails: parse_emails_form(self.form.participant_emails.clone()),
            agent_ids,
            channel_config,
            enabled: self.form.enabled(),
            add_3rd_party: self.form.add_3rd_party(),
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
        })
    }
}
