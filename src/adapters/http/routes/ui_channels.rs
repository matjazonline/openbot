//! `/ui/channels` — the Channels workspace: the mailbox shell with the channel list configured
//! rather than read.
//!
//! The shell, the company scoping and the form parsing are all shared: chrome comes from
//! [`crate::adapters::http::pages::ui_shell`], the company from [`super::ui::load_managed_company`],
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
    entities::{
        channel::Channel, company::Company, schedule::ChannelSchedule, value_objects::EmailAddress,
    },
    infra::config::AppConfig,
    use_cases::{
        channel::{ChannelUseCases, ChannelWrite},
        company::CompanyUseCases,
        schedule::ScheduleUseCases,
        user::UserUseCases,
    },
};

use super::{
    channel::{ChannelForm, parse_emails_form, parse_list_form, parse_text_form, slugify},
    ui::{load_account, load_managed_company, managed_company_membership, workspace_user},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/channels", get(channels_page).post(create_channel))
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
/// A channel's schedules and the workflow bindings they may name, which the pane's schedules
/// card needs together.
#[derive(Debug, Clone, Default)]
struct ChannelAutomation {
    schedules: Vec<ChannelSchedule>,
    bindings: Vec<crate::use_cases::schedule::ScheduleBindingChoice>,
}

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
    schedule_use_cases: Arc<ScheduleUseCases>,
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
        Ok(ChannelSettingsView {
            channel_use_cases: &self.channel_use_cases,
            schedule_use_cases: &self.schedule_use_cases,
            config: &self.config,
            user_id: self.user_id,
            company,
        })
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
    // Landing on `/ui/channels` with no `channel_id` opens the first channel rather than an empty
    // pane, so the workspace is never a blank screen when there is something to show.
    let selected = query
        .channel_id
        .and_then(|id| channels.iter().find(|channel| channel.id == id))
        .or_else(|| channels.first());

    let selected_automation = match selected {
        Some(channel) => view.automation(channel.id).await?,
        None => ChannelAutomation::default(),
    };

    let creating = matches!(query.new.as_deref(), Some("1") | Some("true"));
    let pane_html = match (creating, selected) {
        (true, _) => view.create_pane(&pages::ChannelDraft::default(), None),
        (false, Some(channel)) => view.edit_pane(channel, &selected_automation, None, None),
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

    Ok(Html(
        view.create_pane(&pages::ChannelDraft::default(), None),
    ))
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
    let automation = view.automation(channel.id).await?;

    Ok(Html(view.edit_pane(&channel, &automation, None, None)))
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
    let submitted = SubmittedChannel::new(form);

    let rejected = |message: String| {
        Ok(Html(view.create_pane(&submitted.draft(), Some(&message))).into_response())
    };

    let write = submitted.write();
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
        Ok(channel) => view.saved_response(&channel).await,
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
    let view = workspace.view(&company).await?;
    let stored = view.channel(channel_id).await?;
    let automation = view.automation(channel_id).await?;
    let submitted = SubmittedChannel::new(form);

    let rejected = |message: String| {
        Ok(Html(view.edit_pane(
            &stored,
            &automation,
            Some(&submitted.draft()),
            Some(&message),
        ))
        .into_response())
    };

    let write = submitted.write();
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
        Ok(channel) => view.saved_response(&channel).await,
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

    /// A channel's automation as its pane needs it, loaded in one step because the schedules card
    /// renders both halves: the schedules themselves, and the bindings they may name.
    async fn automation(&self, channel_id: Uuid) -> AppResult<ChannelAutomation> {
        Ok(ChannelAutomation {
            schedules: self
                .schedule_use_cases
                .list_channel_schedules(self.user_id, self.company.id, channel_id)
                .await?,
            bindings: self
                .schedule_use_cases
                .binding_choices(self.user_id, self.company.id, Some(channel_id))
                .await?,
        })
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

    fn create_pane(&self, draft: &pages::ChannelDraft<'_>, error: Option<&str>) -> String {
        pages::channel_create_pane(&pages::ChannelCreatePane {
            company: self.company,
            app_domain_name: &self.config.app_domain_name,
            spam_scan_enabled: self.config.is_spam_scan_enabled(),
            draft,
            error,
        })
    }

    fn edit_pane(
        &self,
        channel: &Channel,
        automation: &ChannelAutomation,
        draft: Option<&pages::ChannelDraft<'_>>,
        error: Option<&str>,
    ) -> String {
        pages::channel_edit_pane(&pages::ChannelEditPane {
            company: self.company,
            app_domain_name: &self.config.app_domain_name,
            channel,
            schedules: &automation.schedules,
            schedule_bindings: &automation.bindings,
            spam_scan_enabled: self.config.is_spam_scan_enabled(),
            draft,
            error,
        })
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
    async fn saved_response(&self, channel: &Channel) -> AppResult<Response> {
        let automation = self.automation(channel.id).await?;
        let pane = self.edit_pane(channel, &automation, None, None);
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
/// It also owns the one derivation every write needs -- the slug the name falls back to -- so it
/// is not re-derived at a call site.
struct SubmittedChannel {
    form: ChannelForm,
    slug: String,
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

        Self { form, slug }
    }

    fn draft(&self) -> pages::ChannelDraft<'_> {
        pages::ChannelDraft {
            name: &self.form.name,
            description: self.form.description.as_deref().unwrap_or(""),
            slug: &self.slug,
            alias_slugs: self.form.alias_slugs.as_deref().unwrap_or(""),
            participant_emails: self.form.participant_emails.as_deref().unwrap_or(""),
            enabled: self.form.enabled(),
            add_3rd_party: self.form.add_3rd_party(),
        }
    }

    /// The write this submission asks for.
    fn write(&self) -> ChannelWrite {
        ChannelWrite {
            name: self.form.name.clone(),
            description: parse_text_form(self.form.description.clone()),
            slug: self.slug.clone(),
            alias_slugs: parse_list_form(self.form.alias_slugs.clone()),
            participant_emails: parse_emails_form(self.form.participant_emails.clone()),
            enabled: self.form.enabled(),
            add_3rd_party: self.form.add_3rd_party(),
            created_by: None,
        }
    }
}
