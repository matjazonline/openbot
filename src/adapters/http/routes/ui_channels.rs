//! `/ui/channels` — the Channels workspace: the mailbox shell with the channel list configured
//! rather than read.
//!
//! The shell, the company scoping and the form parsing are all shared: chrome comes from
//! [`crate::adapters::http::pages::ui_shell`], the company from [`super::ui::load_scoped_company`],
//! and every field is parsed by the same helpers the classic Channels page uses, so the two UIs
//! cannot drift on what a submitted channel means.

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
    entities::{agent::Agent, channel::Channel, company::Company, value_objects::EmailAddress},
    infra::config::AppConfig,
    use_cases::{
        agent::AgentUseCases, channel::ChannelUseCases, company::CompanyUseCases,
        user::UserUseCases,
    },
};

use super::{
    channel::{
        ChannelForm, parse_agent_ids_form, parse_config_form, parse_emails_form,
        resolve_channel_agents, slugify,
    },
    ui::{load_account, load_scoped_company},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/channels", get(channels_page).post(create_channel))
        .route("/ui/channels/new", get(create_pane))
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

const NO_SELECTION: &str = "Select a channel to configure it, or create a new one.";

/// The use cases and the caller every Channels handler starts from.
///
/// Extracted as one value rather than five `State`s per handler: each of these routes needs the
/// same set, and a handler's own parameters should be what makes it different from its siblings.
struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    channel_use_cases: Arc<ChannelUseCases>,
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
            agent_use_cases: state.agent_use_cases.clone(),
            user_use_cases: state.user_use_cases.clone(),
            config: state.config.clone(),
            user_id: user.id,
        })
    }
}

impl Workspace {
    /// The company a request is scoped to, always picked from the caller's own companies so a
    /// guessed `company_id` cannot reach another user's channels.
    async fn scoped_company(&self, company_id: Uuid) -> AppResult<Company> {
        let (_, company) =
            load_scoped_company(&self.company_use_cases, self.user_id, Some(company_id)).await?;
        company.ok_or_else(|| AppError::NotFound("Company not found".into()))
    }

    fn view<'a>(&'a self, company: &'a Company) -> ChannelSettingsView<'a> {
        ChannelSettingsView {
            channel_use_cases: &self.channel_use_cases,
            agent_use_cases: &self.agent_use_cases,
            config: &self.config,
            user_id: self.user_id,
            company,
        }
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
    let workspace_user = pages::MailboxUser {
        username: &account.username,
        email: &account_email,
    };

    let (companies, company) = load_scoped_company(
        &workspace.company_use_cases,
        workspace.user_id,
        query.company_id,
    )
    .await?;
    let Some(company) = company else {
        return Ok(Html(pages::mailbox_no_company_page(&workspace_user)));
    };

    let view = workspace.view(&company);
    let channels = view.channels().await?;
    let agents = view.agents().await?;
    let selected = query
        .channel_id
        .and_then(|id| channels.iter().find(|channel| channel.id == id));

    let creating = matches!(query.new.as_deref(), Some("1") | Some("true"));
    let pane_html = match (creating, selected) {
        (true, _) => view.create_pane(&agents, &pages::ChannelDraft::default(), None),
        (false, Some(channel)) => view.edit_pane(channel, &agents, None, None),
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
    let view = workspace.view(&company);
    let agents = view.agents().await?;

    Ok(Html(view.create_pane(
        &agents,
        &pages::ChannelDraft::default(),
        None,
    )))
}

/// GET /ui/channels/{channel_id} - One channel's settings for the pane (Protected).
#[instrument(skip(workspace))]
async fn edit_pane(
    workspace: Workspace,
    Path(channel_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let channel = view.channel(channel_id).await?;
    let agents = view.agents().await?;

    Ok(Html(view.edit_pane(&channel, &agents, None, None)))
}

/// POST /ui/channels - Create a channel from the pane's Simple or Advanced form (Protected).
#[instrument(skip(workspace, form))]
async fn create_channel(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<ChannelForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
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

    let created = workspace
        .channel_use_cases
        .create_channel(
            workspace.user_id,
            company.id,
            &submitted.form.name,
            &submitted.slug,
            submitted.form.api_key.as_deref(),
            submitted.form.provider.as_deref(),
            submitted.form.model.as_deref(),
            parse_emails_form(submitted.form.participant_emails.clone()),
            agent_ids,
            channel_config,
            submitted.confirm_spam_disabled(),
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

/// PUT /ui/channels/{channel_id} - Save one channel's settings (Protected).
#[instrument(skip(workspace, form))]
async fn update_channel(
    workspace: Workspace,
    Path(channel_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<ChannelForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let stored = view.channel(channel_id).await?;
    let agents = view.agents().await?;
    let submitted = SubmittedChannel::new(form);

    let rejected = |message: String| {
        Ok(
            Html(view.edit_pane(&stored, &agents, Some(&submitted.draft()), Some(&message)))
                .into_response(),
        )
    };

    let channel_config = match parse_config_form(submitted.form.channel_config.clone()) {
        Ok(config) => config,
        Err(message) => return rejected(message),
    };

    let saved = workspace
        .channel_use_cases
        .update_channel(
            workspace.user_id,
            company.id,
            channel_id,
            &submitted.form.name,
            &submitted.slug,
            submitted.form.api_key.as_deref(),
            submitted.form.provider.as_deref(),
            submitted.form.model.as_deref(),
            parse_emails_form(submitted.form.participant_emails.clone()),
            Some(submitted.agent_ids.clone()),
            channel_config,
            submitted.confirm_spam_disabled(),
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

    let view = workspace.view(&company);
    let channels = view.channels().await?;
    let pane = pages::channel_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline);
    let list =
        pages::channel_settings_list(&view.list(&channels, None), pages::FragmentSwap::OutOfBand);

    Ok(Html(format!("{pane}{list}")).into_response())
}

/// Everything the workspace renders from, so each handler names its data once.
struct ChannelSettingsView<'a> {
    channel_use_cases: &'a ChannelUseCases,
    agent_use_cases: &'a AgentUseCases,
    config: &'a AppConfig,
    user_id: Uuid,
    company: &'a Company,
}

impl ChannelSettingsView<'_> {
    async fn channels(&self) -> AppResult<Vec<Channel>> {
        self.channel_use_cases
            .list_company_channels(self.user_id, self.company.id)
            .await
    }

    async fn agents(&self) -> AppResult<Vec<Agent>> {
        self.agent_use_cases
            .list_company_agents(self.user_id, self.company.id)
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
        pages::channel_create_pane(&pages::ChannelCreatePane {
            company: self.company,
            app_domain_name: &self.config.app_domain_name,
            agents,
            spam_scan_enabled: self.config.is_spam_scan_enabled(),
            draft,
            error,
        })
    }

    fn edit_pane(
        &self,
        channel: &Channel,
        agents: &[Agent],
        draft: Option<&pages::ChannelDraft<'_>>,
        error: Option<&str>,
    ) -> String {
        pages::channel_edit_pane(&pages::ChannelEditPane {
            company: self.company,
            app_domain_name: &self.config.app_domain_name,
            channel,
            agents,
            spam_scan_enabled: self.config.is_spam_scan_enabled(),
            draft,
            error,
        })
    }

    /// What every successful write returns: the saved channel's pane, with the sidebar list
    /// refreshed beside it so a create, rename or slug change shows up immediately.
    async fn saved_response(&self, channel: &Channel, agents: &[Agent]) -> AppResult<Response> {
        let pane = self.edit_pane(channel, agents, None, None);
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
            slug: &self.slug,
            system_prompt: self.form.system_prompt.as_deref().unwrap_or(""),
            participant_emails: self.form.participant_emails.as_deref().unwrap_or(""),
            agent_ids: &self.agent_ids,
            provider: self.form.provider.as_deref().unwrap_or(""),
            model: self.form.model.as_deref().unwrap_or(""),
            api_key: self.form.api_key.as_deref().unwrap_or(""),
            channel_config: self.form.channel_config.as_deref().unwrap_or(""),
            advanced: self.form.form_mode.as_deref() != Some("simple"),
        }
    }

    fn confirm_spam_disabled(&self) -> bool {
        matches!(
            self.form.confirm_spam_disabled.as_deref(),
            Some("true") | Some("on")
        )
    }
}
