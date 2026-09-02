//! `/ui/agents` — the Agents workspace: the mailbox shell with the company's agents configured
//! rather than read.
//!
//! The shell, the company scoping and the form parsing are all shared: chrome comes from
//! [`crate::adapters::http::pages::ui_shell`], the company from [`super::ui::load_managed_company`],
//! and every field is parsed by the same helpers the classic Agents page uses, so the two UIs
//! cannot drift on what a submitted agent means.

use std::sync::Arc;

use crate::use_cases::agent::{AgentWrite, PersonalChannelPlan};

use axum::{
    Form, Router,
    extract::{FromRequestParts, Path, Query},
    http::request::Parts,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
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
        agent::Agent,
        channel::Channel,
        company::Company,
        memory::{MemoryPersistenceMode, MemoryRecallMode},
        schedule::{ChannelSchedule, ScheduleRunAsChoices},
        value_objects::{AvatarUrl, EmailAddress},
    },
    infra::config::AppConfig,
    use_cases::{
        agent::AgentUseCases, channel::ChannelUseCases, company::CompanyUseCases,
        schedule::ScheduleUseCases, user::UserUseCases,
    },
};

use super::{
    agent::{AgentForm, AgentInstructionRequest, ModelOverrides, create_agent_from_instructions},
    channel::{ChannelForm, checkbox_ticked, parse_agent_ids_form, parse_config_form},
    task::deserialize_empty_string_as_none,
    ui::{load_account, load_managed_company, managed_company_membership, workspace_user},
    ui_channels::SubmittedChannel,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/agents", get(agents_page).post(create_agent))
        .route("/ui/agents/new", get(create_pane))
        .route("/ui/agents/new/channel", post(channel_step))
        .route("/ui/agents/new/agent", post(agent_step))
        .route("/ui/agents/new/create", post(create_agent_with_channel))
        .route("/ui/agents/from-library", post(create_from_library))
        .route("/ui/agents/generate-prompt", post(generate_prompt))
        .route(
            "/ui/agents/{agent_id}",
            get(edit_pane).put(update_agent).delete(delete_agent),
        )
        .route(
            "/ui/agents/{agent_id}/channel",
            axum::routing::put(update_owned_channel),
        )
}

/// What the workspace has selected, all optional so `/ui/agents` alone is a valid entry point.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceQuery {
    pub company_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    /// `?new=1` opens the create form instead of an agent's settings.
    pub new: Option<String>,
    /// Which half of the selected agent to open on; see [`pages::AgentTab`].
    pub tab: Option<String>,
}

/// The company scope every fragment and every write carries, in the URL rather than the body so
/// the form itself stays exactly [`AgentForm`].
#[derive(Debug, Clone, Deserialize)]
pub struct CompanyQuery {
    pub company_id: Uuid,
}

/// The same scope, plus the tab a pane fragment is for.
///
/// Separate from [`CompanyQuery`] rather than an optional field on it: only the edit pane and its
/// channel write have a tab, and the create and generator routes should not appear to accept one.
#[derive(Debug, Clone, Deserialize)]
pub struct EditPaneQuery {
    pub company_id: Uuid,
    pub tab: Option<String>,
}

/// What the "Generate with AI" box sends: the instructions, plus whichever model overrides the
/// form is currently showing.
#[derive(Debug, Clone, Deserialize)]
pub struct PromptGeneratorForm {
    pub instructions: Option<String>,
    /// Which pane asked, as the agent being edited — absent means the create pane. Typed rather
    /// than the raw id-prefix string, so nothing a caller sends can shape the element id it
    /// answers into.
    pub id_prefix: Option<Uuid>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// What the create form's channel step sends: the channel as filled in, and the agent from the
/// first step that came along with it.
///
/// The channel half is [`ChannelForm`] itself rather than a copy of its fields, so the step and the
/// Channels workspace cannot drift on what a submitted channel means.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentChannelStepForm {
    #[serde(flatten)]
    pub agent: CarriedAgent,
    #[serde(flatten)]
    pub channel: ChannelForm,
}

/// The create form's first step, as the channel step submits it back.
///
/// Every field is prefixed because the channel form beside it owns the unprefixed `name`, `slug`,
/// `description` and `system_prompt`. It converts into [`AgentForm`] rather than being handled
/// separately, so both steps go through one parser — and because that conversion writes an
/// exhaustive struct literal, a field added to `AgentForm` cannot be quietly dropped here.
#[derive(Debug, Clone, Deserialize)]
pub struct CarriedAgent {
    #[serde(rename = "agent_name")]
    pub name: String,
    #[serde(rename = "agent_slug")]
    pub slug: Option<String>,
    #[serde(rename = "agent_provider")]
    pub provider: Option<String>,
    #[serde(rename = "agent_model")]
    pub model: Option<String>,
    #[serde(
        rename = "agent_run_timeout_secs",
        default,
        deserialize_with = "deserialize_empty_string_as_none"
    )]
    pub run_timeout_secs: Option<u32>,
    #[serde(rename = "agent_system_prompt")]
    pub system_prompt: Option<String>,
    #[serde(rename = "agent_description")]
    pub description: Option<String>,
    #[serde(rename = "agent_config_json")]
    pub config_json: Option<String>,
    #[serde(rename = "agent_avatar_url")]
    pub avatar_url: Option<String>,
    /// A ticked box as its raw value: a flattened field is handed to its type as the string the
    /// browser sent (see the note on `deserialize_interval_seconds` in `routes/schedule.rs`), and
    /// a bare `bool` refuses `"true"`. An absent key is an unticked box, as everywhere else here.
    #[serde(rename = "agent_memory_enabled")]
    pub memory_enabled: Option<String>,
    #[serde(rename = "agent_memory_persistence_mode")]
    pub memory_persistence_mode: Option<MemoryPersistenceMode>,
    #[serde(rename = "agent_memory_recall_mode")]
    pub memory_recall_mode: Option<MemoryRecallMode>,
    #[serde(
        rename = "agent_memory_max_results",
        default,
        deserialize_with = "deserialize_empty_string_as_none"
    )]
    pub memory_max_results: Option<u8>,
}

impl From<CarriedAgent> for AgentForm {
    fn from(carried: CarriedAgent) -> Self {
        Self {
            name: carried.name,
            slug: carried.slug,
            provider: carried.provider,
            model: carried.model,
            run_timeout_secs: carried.run_timeout_secs,
            system_prompt: carried.system_prompt,
            description: carried.description,
            config_json: carried.config_json,
            avatar_url: carried.avatar_url,
            memory_enabled: checkbox_ticked(carried.memory_enabled.as_deref()),
            memory_persistence_mode: carried.memory_persistence_mode,
            memory_recall_mode: carried.memory_recall_mode,
            memory_max_results: carried.memory_max_results,
            // Only the Advanced tab has a channel step, and its prompt is written rather than
            // expanded.
            form_mode: Some("advanced".to_string()),
        }
    }
}

/// What the Easy tab sends: the library definitions to copy into this company, as the one
/// comma-separated field a urlencoded form can carry a set in.
#[derive(Debug, Clone, Deserialize)]
pub struct LibraryPickForm {
    pub library_agent_ids: Option<String>,
}

impl PromptGeneratorForm {
    /// The element-id namespace of the pane that asked, matching what the pane rendered with.
    fn id_prefix(&self) -> String {
        self.id_prefix
            .map(|id| id.to_string())
            .unwrap_or_else(|| "new".to_string())
    }
}

const NO_SELECTION: &str = "Select an agent to configure it, or create a new one.";

/// The channel this agent owns, out of the company's channels.
///
/// An agent owns at most one — `channels.owner_agent_id` is unique — so this is a `find` over the
/// list every handler has already loaded rather than a query of its own.
fn owned_channel(agent_id: Uuid, channels: &[Channel]) -> Option<&Channel> {
    channels
        .iter()
        .find(|channel| channel.is_owned_by(agent_id))
}

/// The use cases and the caller every Agents handler starts from.
///
/// Extracted as one value rather than five `State`s per handler: each of these routes needs the
/// same set, and a handler's own parameters should be what makes it different from its siblings.
struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    agent_use_cases: Arc<AgentUseCases>,
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
            agent_use_cases: state.agent_use_cases.clone(),
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
    /// so a guessed `company_id` cannot reach another company's agents.
    async fn scoped_company(&self, company_id: Uuid) -> AppResult<Company> {
        let (_, company) =
            load_managed_company(&self.company_use_cases, self.user_id, Some(company_id)).await?;
        company.ok_or_else(|| AppError::NotFound("Company not found".into()))
    }

    fn view<'a>(&'a self, company: &'a Company) -> AgentSettingsView<'a> {
        AgentSettingsView {
            agent_use_cases: &self.agent_use_cases,
            channel_use_cases: &self.channel_use_cases,
            schedule_use_cases: &self.schedule_use_cases,
            config: &self.config,
            user_id: self.user_id,
            company,
            app_domain_name: &self.config.app_domain_name,
        }
    }
}

/// GET /ui/agents - The Agents workspace for the selected company / agent (Protected).
#[instrument(skip(workspace))]
async fn agents_page(
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

    let view = workspace.view(&company);
    let agents = view.agents().await?;
    let channels = view.channels().await?;
    // Landing on `/ui/agents` with no `agent_id` opens the first agent rather than an empty pane,
    // so the workspace is never a blank screen when there is something to show.
    let selected = query
        .agent_id
        .and_then(|id| agents.iter().find(|agent| agent.id == id))
        .or_else(|| agents.first());

    let creating = matches!(query.new.as_deref(), Some("1") | Some("true"));
    let pane_html = match (creating, selected) {
        (true, _) => {
            view.create_pane(CreateForm {
                draft: &pages::AgentDraft::default(),
                error: None,
                selected_library_agent_ids: &[],
                tab: None,
            })
            .await?
        }
        (false, Some(agent)) => {
            view.tab_pane(
                agent,
                &channels,
                pages::AgentTab::from_query(query.tab.as_deref()),
            )
            .await?
        }
        (false, None) => {
            pages::agent_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline)
        }
    };

    let list = view.list(&agents, &channels, selected.map(|agent| agent.id));
    Ok(Html(pages::agent_settings_page(
        &pages::AgentSettingsPage {
            user: &workspace_user,
            companies: &companies,
            list: &list,
            pane_html: &pane_html,
        },
    )))
}

/// GET /ui/agents/new - The create-agent form for the pane (Protected).
#[instrument(skip(workspace))]
async fn create_pane(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);

    Ok(Html(
        view.create_pane(CreateForm {
            draft: &pages::AgentDraft::default(),
            error: None,
            selected_library_agent_ids: &[],
            tab: None,
        })
        .await?,
    ))
}

/// GET /ui/agents/{agent_id} - One agent's settings for the pane (Protected).
#[instrument(skip(workspace))]
async fn edit_pane(
    workspace: Workspace,
    Path(agent_id): Path<Uuid>,
    Query(query): Query<EditPaneQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let agent = view.agent(agent_id).await?;
    let channels = view.channels().await?;

    Ok(Html(
        view.tab_pane(
            &agent,
            &channels,
            pages::AgentTab::from_query(query.tab.as_deref()),
        )
        .await?,
    ))
}

/// POST /ui/agents/generate-prompt - Expand instructions into a system prompt for the pane's form
/// (Protected).
///
/// Answers with the status line and the filled-in field, rather than a redirect or a new pane: the
/// rest of the form is whatever the user has already typed and must survive untouched.
#[instrument(skip(workspace, form))]
async fn generate_prompt(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<PromptGeneratorForm>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let instructions = form.instructions.as_deref().unwrap_or_default().trim();

    if instructions.is_empty() {
        return Ok(Html(pages::agent_prompt_failed(
            "Describe what the agent should do first.",
        )));
    }

    let overrides = ModelOverrides {
        provider: form.provider.as_deref(),
        model: form.model.as_deref(),
    };

    let generated = workspace
        .agent_use_cases
        .generate_system_prompt(
            workspace.user_id,
            company.id,
            instructions,
            overrides.provider,
            overrides.model,
        )
        .await;

    Ok(Html(match generated {
        Ok(system_prompt) => pages::agent_prompt_generated(&form.id_prefix(), &system_prompt),
        Err(err) => pages::agent_prompt_failed(&format!("Prompt generation failed: {err}")),
    }))
}

/// POST /ui/agents - Create an agent from the pane's Simple or Advanced form (Protected).
#[instrument(skip(workspace, form))]
async fn create_agent(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<AgentForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let submitted = SubmittedAgent::new(form);

    let created = if submitted.is_simple() {
        let avatar_url = match &submitted.avatar_url {
            Ok(avatar) => avatar.clone(),
            Err(message) => return rejected(&view, &submitted, message).await,
        };

        create_agent_from_instructions(
            &workspace.agent_use_cases,
            AgentInstructionRequest {
                user_id: workspace.user_id,
                company_id: company.id,
                name: &submitted.form.name,
                slug: &submitted.slug,
                instructions: submitted.form.system_prompt.as_deref().unwrap_or_default(),
                overrides: submitted.overrides(),
                run_timeout_secs: submitted.form.run_timeout_secs,
                avatar_url: avatar_url.as_ref(),
            },
        )
        .await
        .map(|provisioned| (provisioned.agent, provisioned.warnings))
    } else {
        let write = match submitted.agent_write() {
            Ok(write) => write,
            Err(message) => return rejected(&view, &submitted, &message).await,
        };

        workspace
            .agent_use_cases
            .create_addressable_agent(workspace.user_id, company.id, write)
            .await
            .map(|provisioned| (provisioned.agent, provisioned.warnings))
            .map_err(|err| format!("Failed to create agent: {err}"))
    };

    match created {
        Ok((agent, warnings)) => {
            view.saved_response(&agent, pages::AgentTab::Settings, &warnings)
                .await
        }
        Err(message) => rejected(&view, &submitted, &message).await,
    }
}

/// POST /ui/agents/new/channel - The Advanced create form's second step (Protected).
///
/// Nothing is written here. The agent is checked first so a bad handle, model pair or config JSON
/// is refused while the field carrying it is still on screen, and the channel step then opens on
/// the values `personal_channel_write` would otherwise have chosen without showing them.
#[instrument(skip(workspace, form))]
async fn channel_step(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<AgentForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let submitted = SubmittedAgent::new(form);

    let write = match submitted.agent_write() {
        Ok(write) => write,
        Err(message) => return rejected(&view, &submitted, &message).await,
    };
    let write = match workspace
        .agent_use_cases
        .validate_new_agent(workspace.user_id, company.id, write)
        .await
    {
        Ok(write) => write,
        Err(err) => return rejected(&view, &submitted, &err.to_string()).await,
    };
    if let Some(message) = view.address_taken(&write.slug).await? {
        return rejected(&view, &submitted, &message).await;
    }

    // What was typed, with the handle in the form it will be stored in -- the channel address is
    // built from it, so the step must not preview one the create would not produce.
    let agent = pages::AgentDraft {
        slug: &write.slug,
        ..submitted.draft()
    };
    let participants = default_participants(&company);
    let draft = default_channel_draft(&company, &agent, &participants);

    Ok(Html(view.channel_step(&agent, &draft, None).await?).into_response())
}

/// POST /ui/agents/new/agent - Back from the channel step to the Advanced form (Protected).
#[instrument(skip(workspace, form))]
async fn agent_step(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<AgentChannelStepForm>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let submitted = SubmittedAgent::new(form.agent.into());

    Ok(Html(
        view.create_pane(CreateForm {
            draft: &submitted.draft(),
            error: None,
            selected_library_agent_ids: &[],
            tab: Some(pages::AgentCreateTab::Advanced),
        })
        .await?,
    ))
}

/// POST /ui/agents/new/create - Create the agent and the channel it answers on (Protected).
#[instrument(skip(workspace, form))]
async fn create_agent_with_channel(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<AgentChannelStepForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let agent = SubmittedAgent::new(form.agent.into());
    let channel = SubmittedChannel::new(form.channel);

    let agent_write = match agent.agent_write() {
        Ok(write) => write,
        Err(message) => return view.channel_step_rejected(&agent, &channel, &message).await,
    };
    // The position-0 assignment arrives with the transaction that creates the pair, so the step
    // never submits an agent list of its own.
    let channel_write = match channel.write(None) {
        Ok(write) => write,
        Err(message) => return view.channel_step_rejected(&agent, &channel, &message).await,
    };

    let created = workspace
        .agent_use_cases
        .create_addressable_agent_with(
            workspace.user_id,
            company.id,
            agent_write,
            PersonalChannelPlan::configured(channel_write, channel.form.confirm_spam_disabled()),
        )
        .await;

    match created {
        Ok(provisioned) => {
            view.saved_response(
                &provisioned.agent,
                pages::AgentTab::Settings,
                &provisioned.warnings,
            )
            .await
        }
        Err(err) => {
            view.channel_step_rejected(&agent, &channel, &format!("Failed to create agent: {err}"))
                .await
        }
    }
}

/// The company's default participants as the comma-separated list the channel step's field holds.
///
/// Kept out of [`default_channel_draft`] because [`pages::ChannelDraft`] borrows every string it
/// renders, so the joined list has to outlive the draft.
fn default_participants(company: &Company) -> String {
    company
        .channel_defaults
        .participant_emails
        .as_ref()
        .map(|emails| {
            emails
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// The channel step as it first opens: the company's channel defaults, named after the agent.
///
/// Mirrors `personal_channel_write`, so a step submitted untouched creates exactly what the
/// one-step Advanced tab created. The one thing it does not copy is that function's stripping of
/// `@public` on a server without spam scanning -- here the interlock's confirmation is on screen,
/// so the choice is offered rather than made.
fn default_channel_draft<'a>(
    company: &'a Company,
    agent: &'a pages::AgentDraft<'a>,
    participant_emails: &'a str,
) -> pages::ChannelDraft<'a> {
    let defaults = &company.channel_defaults;
    pages::ChannelDraft {
        name: agent.name,
        description: agent.description,
        slug: agent.slug,
        participant_emails,
        advanced: true,
        enabled: true,
        add_3rd_party: defaults.add_3rd_party,
        retrieve_company_memory: defaults.retrieve_company_memory,
        retrieve_agent_memory: defaults.retrieve_agent_memory,
        retrieve_user_memory: defaults.retrieve_user_memory,
        persist_company_memory: defaults.persist_company_memory,
        persist_agent_memory: defaults.persist_agent_memory,
        persist_user_memory: defaults.persist_user_memory,
        ..pages::ChannelDraft::default()
    }
}

/// A refused create, as the tab it was submitted from, filled back in with what was typed.
async fn rejected(
    view: &AgentSettingsView<'_>,
    submitted: &SubmittedAgent,
    message: &str,
) -> AppResult<Response> {
    let pane = view
        .create_pane(CreateForm {
            draft: &submitted.draft(),
            error: Some(message),
            selected_library_agent_ids: &[],
            tab: Some(if submitted.is_simple() {
                pages::AgentCreateTab::Simple
            } else {
                pages::AgentCreateTab::Advanced
            }),
        })
        .await?;
    Ok(Html(pane).into_response())
}

/// POST /ui/agents/from-library - Create one company agent per picked library definition
/// (Protected).
///
/// Each pick is copied and provisioned on its own, so one that collides with an agent this company
/// already has does not cost the others: the pane comes back showing what was created, with a line
/// per pick that could not be.
#[instrument(skip(workspace, form))]
async fn create_from_library(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<LibraryPickForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let picked = parse_agent_ids_form(form.library_agent_ids).unwrap_or_default();
    if picked.is_empty() {
        return view
            .library_rejected(&picked, "Pick at least one agent from the library.")
            .await;
    }

    let mut created = Vec::new();
    let mut warnings = Vec::new();
    let mut refusals = Vec::new();
    for agent_id in &picked {
        match workspace
            .agent_use_cases
            .create_agent_from_library(workspace.user_id, company.id, *agent_id)
            .await
        {
            Ok(provisioned) => {
                warnings.extend(provisioned.warnings);
                created.push(provisioned.agent);
            }
            Err(err) => refusals.push(crate::use_cases::agent::ProvisioningWarning {
                code: "library_agent_not_created".into(),
                message: format!("A picked library agent was not created: {err}"),
            }),
        }
    }

    let Some(agent) = created.first() else {
        let message = refusals
            .iter()
            .map(|refusal| refusal.message.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        return view.library_rejected(&picked, &message).await;
    };

    warnings.extend(refusals);
    view.saved_response(agent, pages::AgentTab::Settings, &warnings)
        .await
}

/// PUT /ui/agents/{agent_id} - Save one agent's settings (Protected).
#[instrument(skip(workspace, form))]
async fn update_agent(
    workspace: Workspace,
    Path(agent_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<AgentForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let stored = view.agent(agent_id).await?;
    let channels = view.channels().await?;
    let submitted = SubmittedAgent::new(form);

    let fields = parse_config_form(submitted.form.config_json.clone())
        .and_then(|config_json| Ok((config_json, submitted.avatar_url.clone()?)));

    let (config_json, avatar_url) = match fields {
        Ok(fields) => fields,
        Err(message) => {
            return Ok(Html(
                view.edit_pane(&stored, &channels, Some(&submitted.draft()), Some(&message))
                    .await?,
            )
            .into_response());
        }
    };

    let saved = workspace
        .agent_use_cases
        .update_agent(
            workspace.user_id,
            company.id,
            agent_id,
            AgentWrite {
                name: submitted.form.name.clone(),
                slug: submitted.slug.clone(),
                provider: submitted.form.provider.clone(),
                model: submitted.form.model.clone(),
                run_timeout_secs: submitted.form.run_timeout_secs,
                system_prompt: submitted.form.system_prompt.clone(),
                description: submitted.form.description.clone(),
                config_json,
                memory_enabled: submitted.form.memory_enabled,
                memory_persistence_mode: submitted.form.memory_persistence_mode.unwrap_or_default(),
                memory_recall_mode: submitted.form.memory_recall_mode.unwrap_or_default(),
                memory_max_results: submitted
                    .form
                    .memory_max_results
                    .unwrap_or_else(crate::entities::memory::default_memory_max_results),
                avatar_url,
                created_by: None,
            },
        )
        .await;

    match saved {
        Ok(agent) => {
            view.saved_response(&agent, pages::AgentTab::Settings, &[])
                .await
        }
        Err(err) => Ok(Html(
            view.edit_pane(
                &stored,
                &channels,
                Some(&submitted.draft()),
                Some(&format!("Failed to save agent: {err}")),
            )
            .await?,
        )
        .into_response()),
    }
}

/// PUT /ui/agents/{agent_id}/channel - Save the agent's personal channel (Protected).
///
/// The channel half of the Agents workspace, and deliberately not a second implementation of it:
/// the submission is parsed by [`SubmittedChannel`] and written by
/// [`ChannelUseCases::update_channel`], which is also what pins an owned channel's address and
/// keeps its owner at position 0 — so nothing about ownership has to be re-decided here.
#[instrument(skip(workspace, form))]
async fn update_owned_channel(
    workspace: Workspace,
    Path(agent_id): Path<Uuid>,
    Query(query): Query<EditPaneQuery>,
    Form(form): Form<ChannelForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let agent = view.agent(agent_id).await?;
    let channels = view.channels().await?;
    let stored = owned_channel(agent_id, &channels)
        .ok_or_else(|| AppError::NotFound("Agent has no personal channel".into()))?;
    let submitted = SubmittedChannel::new(form);

    // Both ways of refusing the save end in the same re-render, so they meet as one message here
    // rather than as two calls into the pane.
    let refused = match submitted.write(Some(submitted.agent_ids())) {
        Err(message) => Some(message),
        Ok(write) => workspace
            .channel_use_cases
            .update_channel(
                workspace.user_id,
                company.id,
                stored.id,
                write,
                submitted.form.confirm_spam_disabled(),
            )
            .await
            .err()
            .map(|err| format!("Failed to save channel: {err}")),
    };

    let Some(message) = refused else {
        return view
            .saved_response(&agent, pages::AgentTab::Channel, &[])
            .await;
    };

    Ok(Html(
        view.channel_pane(
            &agent,
            &channels,
            stored,
            Some(&submitted.draft()),
            Some(&message),
        )
        .await?,
    )
    .into_response())
}

/// DELETE /ui/agents/{agent_id} - Delete an agent and clear the pane (Protected).
#[instrument(skip(workspace))]
async fn delete_agent(
    workspace: Workspace,
    Path(agent_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    workspace
        .agent_use_cases
        .delete_agent(workspace.user_id, company.id, agent_id)
        .await?;

    let view = workspace.view(&company);
    let agents = view.agents().await?;
    let channels = view.channels().await?;
    let pane = pages::agent_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline);
    let list = pages::agent_settings_list(
        &view.list(&agents, &channels, None),
        pages::FragmentSwap::OutOfBand,
    );

    Ok(Html(format!("{pane}{list}")).into_response())
}

/// What the create pane is rendered from, beyond what the view can load itself.
struct CreateForm<'a> {
    draft: &'a pages::AgentDraft<'a>,
    error: Option<&'a str>,
    /// The library definitions ticked on the Easy tab, so a refused pick comes back intact.
    selected_library_agent_ids: &'a [Uuid],
    /// The tab that opens; `None` opens Easy whenever the library has anything to offer.
    tab: Option<pages::AgentCreateTab>,
}

/// Everything the workspace renders from, so each handler names its data once.
struct AgentSettingsView<'a> {
    agent_use_cases: &'a AgentUseCases,
    channel_use_cases: &'a ChannelUseCases,
    schedule_use_cases: &'a ScheduleUseCases,
    config: &'a AppConfig,
    user_id: Uuid,
    company: &'a Company,
    app_domain_name: &'a str,
}

impl AgentSettingsView<'_> {
    async fn model_connections(
        &self,
    ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
        self.agent_use_cases
            .list_company_model_connections(self.user_id, self.company.id)
            .await
    }

    async fn agents(&self) -> AppResult<Vec<Agent>> {
        self.agent_use_cases
            .list_company_agents(self.user_id, self.company.id)
            .await
    }

    async fn agent(&self, agent_id: Uuid) -> AppResult<Agent> {
        self.agent_use_cases
            .get_company_agent(self.user_id, self.company.id, agent_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Agent not found".into()))
    }

    async fn channels(&self) -> AppResult<Vec<Channel>> {
        self.channel_use_cases
            .list_company_channels(self.user_id, self.company.id)
            .await
    }

    async fn schedules(&self, channel_id: Uuid) -> AppResult<Vec<ChannelSchedule>> {
        self.schedule_use_cases
            .list_channel_schedules(self.user_id, self.company.id, channel_id)
            .await
    }

    /// Whom this caller may make the channel's scheduled runs act as.
    async fn run_as_choices(&self) -> AppResult<ScheduleRunAsChoices> {
        self.schedule_use_cases
            .run_as_choices(self.user_id, self.company.id)
            .await
    }

    /// Whether the company's memory provider is usable, which is what decides whether the channel
    /// form's memory grants are described as effective or as stored-but-inactive.
    async fn memory_ready(&self) -> AppResult<bool> {
        self.channel_use_cases
            .memory_ready(self.user_id, self.company.id)
            .await
    }

    fn used_by<'a>(&self, agent_id: Uuid, channels: &'a [Channel]) -> Vec<&'a Channel> {
        channels
            .iter()
            .filter(|channel| {
                channel
                    .agent_ids
                    .as_deref()
                    .is_some_and(|ids| ids.contains(&agent_id))
            })
            .collect()
    }

    fn list<'a>(
        &'a self,
        agents: &'a [Agent],
        channels: &'a [Channel],
        selected_agent_id: Option<Uuid>,
    ) -> pages::AgentSettingsList<'a> {
        pages::AgentSettingsList {
            company: self.company,
            app_domain_name: self.app_domain_name,
            agents,
            channels,
            selected_agent_id,
        }
    }

    /// The create pane, on whichever tab the request means.
    ///
    /// It loads its own model connections and library definitions the way [`Self::edit_pane`]
    /// does, so no caller has to fetch what only this pane reads.
    async fn create_pane(&self, form: CreateForm<'_>) -> AppResult<String> {
        let model_connections = self.model_connections().await?;
        let library_agents = self.agent_use_cases.list_library_agents().await?;
        // Nothing to start from means no Easy tab, so a request that asked for it lands on Simple.
        let tab = match form.tab {
            Some(pages::AgentCreateTab::Easy) | None if library_agents.is_empty() => {
                pages::AgentCreateTab::Simple
            }
            Some(tab) => tab,
            None => pages::AgentCreateTab::Easy,
        };

        Ok(pages::agent_create_pane(&pages::AgentCreatePane {
            company: self.company,
            app_domain_name: self.app_domain_name,
            model_connections: &model_connections,
            library_agents: &library_agents,
            selected_library_agent_ids: form.selected_library_agent_ids,
            tab,
            draft: form.draft,
            error: form.error,
        }))
    }

    /// The create form's channel step, for an agent that has not been written yet.
    async fn channel_step(
        &self,
        agent: &pages::AgentDraft<'_>,
        draft: &pages::ChannelDraft<'_>,
        error: Option<&str>,
    ) -> AppResult<String> {
        let memory_ready = self.memory_ready().await?;

        Ok(pages::agent_channel_step_pane(
            &pages::AgentChannelStepPane {
                company: self.company,
                app_domain_name: self.app_domain_name,
                agent,
                draft,
                spam_scan_enabled: self.config.is_spam_scan_enabled(),
                memory_ready,
                error,
            },
        ))
    }

    /// The channel step, filled back in with both halves and why the create was refused.
    async fn channel_step_rejected(
        &self,
        agent: &SubmittedAgent,
        channel: &SubmittedChannel,
        message: &str,
    ) -> AppResult<Response> {
        let pane = self
            .channel_step(&agent.draft(), &channel.draft(), Some(message))
            .await?;
        Ok(Html(pane).into_response())
    }

    /// Why this handle cannot be used, when something already answers on it.
    ///
    /// The unique constraints on `agents.slug` and `channel_slugs` are what actually hold this —
    /// the check exists so the create form's first step can say so while the handle is still on
    /// screen, rather than after the channel step has been filled in.
    async fn address_taken(&self, slug: &str) -> AppResult<Option<String>> {
        if self
            .agents()
            .await?
            .iter()
            .any(|agent| agent.slug.eq_ignore_ascii_case(slug))
        {
            return Ok(Some(format!(
                "This company already has an agent with the handle '{slug}'."
            )));
        }

        let taken = self.channels().await?.iter().any(|channel| {
            channel.slug.eq_ignore_ascii_case(slug)
                || channel
                    .alias_slugs
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(slug))
        });
        Ok(taken.then(|| {
            format!("The address '{slug}' is already in use by another channel in this company.")
        }))
    }

    /// The Easy tab, filled back in with what was picked and why it was refused.
    async fn library_rejected(&self, picked: &[Uuid], message: &str) -> AppResult<Response> {
        let pane = self
            .create_pane(CreateForm {
                draft: &pages::AgentDraft::default(),
                error: Some(message),
                selected_library_agent_ids: picked,
                tab: Some(pages::AgentCreateTab::Easy),
            })
            .await?;
        Ok(Html(pane).into_response())
    }

    /// The Settings tab, which is the tab every agent write comes back on.
    async fn edit_pane(
        &self,
        agent: &Agent,
        channels: &[Channel],
        draft: Option<&pages::AgentDraft<'_>>,
        error: Option<&str>,
    ) -> AppResult<String> {
        let used_by = self.used_by(agent.id, channels);
        let model_connections = self.model_connections().await?;

        Ok(pages::agent_edit_pane(&pages::AgentEditPane {
            company: self.company,
            app_domain_name: self.app_domain_name,
            model_connections: &model_connections,
            agent,
            used_by: &used_by,
            draft,
            error,
            body: pages::AgentPaneBody::Settings,
        }))
    }

    /// The Channel tab: the agent's personal channel, with its schedules.
    ///
    /// Takes the channel rather than looking it up, because every caller has already found it in
    /// the `channels` it must pass anyway.
    async fn channel_pane(
        &self,
        agent: &Agent,
        channels: &[Channel],
        channel: &Channel,
        draft: Option<&pages::ChannelDraft<'_>>,
        error: Option<&str>,
    ) -> AppResult<String> {
        let used_by = self.used_by(agent.id, channels);
        let model_connections = self.model_connections().await?;
        let schedules = self.schedules(channel.id).await?;
        let run_as = self.run_as_choices().await?;
        let memory_ready = self.memory_ready().await?;
        let tab = pages::AgentChannelTab {
            channel,
            schedules: &schedules,
            run_as: &run_as,
            spam_scan_enabled: self.config.is_spam_scan_enabled(),
            memory_ready,
            draft,
            error,
        };

        Ok(pages::agent_edit_pane(&pages::AgentEditPane {
            company: self.company,
            app_domain_name: self.app_domain_name,
            model_connections: &model_connections,
            agent,
            used_by: &used_by,
            draft: None,
            error: None,
            body: pages::AgentPaneBody::Channel(&tab),
        }))
    }

    /// Whichever tab was asked for, falling back to Settings when the agent owns no channel for
    /// the Channel tab to be about.
    async fn tab_pane(
        &self,
        agent: &Agent,
        channels: &[Channel],
        tab: pages::AgentTab,
    ) -> AppResult<String> {
        match (tab, owned_channel(agent.id, channels)) {
            (pages::AgentTab::Channel, Some(channel)) => {
                self.channel_pane(agent, channels, channel, None, None)
                    .await
            }
            _ => self.edit_pane(agent, channels, None, None).await,
        }
    }

    /// What every successful write returns: the saved agent's pane on the tab the write was made
    /// from, with the sidebar list refreshed beside it so a create, rename or handle change shows
    /// up immediately — a channel write included, since the sidebar row carries the owned
    /// channel's address.
    async fn saved_response(
        &self,
        agent: &Agent,
        tab: pages::AgentTab,
        warnings: &[crate::use_cases::agent::ProvisioningWarning],
    ) -> AppResult<Response> {
        let warning = warnings
            .iter()
            .map(|warning| pages::error_alert(&warning.message))
            .collect::<String>();
        let channels = self.channels().await?;
        let pane = self.tab_pane(agent, &channels, tab).await?;
        let agents = self.agents().await?;
        let list = pages::agent_settings_list(
            &self.list(&agents, &channels, Some(agent.id)),
            pages::FragmentSwap::OutOfBand,
        );

        Ok((
            [(
                "HX-Push-Url",
                format!(
                    "/ui/agents?company_id={}&agent_id={}&tab={}",
                    self.company.id,
                    agent.id,
                    tab.as_query()
                ),
            )],
            Html(format!("{warning}{pane}{list}")),
        )
            .into_response())
    }
}

/// A submitted agent form, kept whole so a rejected save can be re-rendered with what was typed.
///
/// It also owns the one derivation every write needs — the slug the name falls back to — so it is
/// not re-derived at a call site.
struct SubmittedAgent {
    form: AgentForm,
    slug: String,
    /// The avatar as parsed, or why it was refused. Kept as the `Result` so a bad URL comes back
    /// as the filled-in form with the reason on top, the way a bad config JSON does.
    avatar_url: Result<Option<AvatarUrl>, String>,
}

impl SubmittedAgent {
    fn new(form: AgentForm) -> Self {
        let slug = form.slug();
        let avatar_url = form.avatar_url();
        Self {
            form,
            slug,
            avatar_url,
        }
    }

    /// Whether `system_prompt` holds instructions to expand rather than the prompt itself.
    fn is_simple(&self) -> bool {
        self.form.form_mode.as_deref() == Some("simple")
    }

    /// The write this submission asks for, or why it cannot be built.
    ///
    /// One builder for the three places an Advanced submit becomes a write — the one-step create,
    /// the channel step's check, and the create that ends it — so none of them can drift on what
    /// the form means.
    fn agent_write(&self) -> Result<AgentWrite, String> {
        Ok(AgentWrite {
            name: self.form.name.clone(),
            slug: self.slug.clone(),
            provider: self.form.provider.clone(),
            model: self.form.model.clone(),
            run_timeout_secs: self.form.run_timeout_secs,
            system_prompt: self.form.system_prompt.clone(),
            description: self.form.description.clone(),
            config_json: parse_config_form(self.form.config_json.clone())?,
            memory_enabled: self.form.memory_enabled,
            memory_persistence_mode: self.form.memory_persistence_mode.unwrap_or_default(),
            memory_recall_mode: self.form.memory_recall_mode.unwrap_or_default(),
            memory_max_results: self
                .form
                .memory_max_results
                .unwrap_or_else(crate::entities::memory::default_memory_max_results),
            avatar_url: self.avatar_url.clone()?,
            created_by: None,
        })
    }

    fn overrides(&self) -> ModelOverrides<'_> {
        ModelOverrides {
            provider: self.form.provider.as_deref(),
            model: self.form.model.as_deref(),
        }
    }

    fn draft(&self) -> pages::AgentDraft<'_> {
        pages::AgentDraft {
            name: &self.form.name,
            slug: &self.slug,
            system_prompt: self.form.system_prompt.as_deref().unwrap_or(""),
            description: self.form.description.as_deref().unwrap_or(""),
            provider: self.form.provider.as_deref().unwrap_or(""),
            model: self.form.model.as_deref().unwrap_or(""),
            run_timeout_secs: self.form.run_timeout_secs,
            memory_enabled: self.form.memory_enabled,
            memory_persistence_mode: self
                .form
                .memory_persistence_mode
                .as_ref()
                .map(|mode| mode.as_str())
                .unwrap_or("audience_only"),
            memory_recall_mode: self
                .form
                .memory_recall_mode
                .as_ref()
                .map(|mode| mode.as_str())
                .unwrap_or("fast"),
            memory_max_results: self
                .form
                .memory_max_results
                .unwrap_or_else(crate::entities::memory::default_memory_max_results),
            config_json: self.form.config_json.as_deref().unwrap_or(""),
            avatar_url: self.form.avatar_url.as_deref().unwrap_or(""),
            advanced: !self.is_simple(),
        }
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

    async fn channel_step_form(body: &str) -> AgentChannelStepForm {
        let request = Request::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string()))
            .unwrap();
        Form::<AgentChannelStepForm>::from_request(request, &())
            .await
            .expect("the channel step's body parses")
            .0
    }

    /// The two halves of the channel step share a body and several field names, so the prefix is
    /// the only thing keeping the agent's name out of the channel's.
    #[tokio::test]
    async fn the_channel_step_parses_both_halves_of_its_body() {
        let form = channel_step_form(
            "agent_name=Support+Triage&agent_slug=support-triage\
             &agent_system_prompt=You+are+support&agent_description=Answers+billing\
             &agent_provider=anthropic&agent_model=claude&agent_run_timeout_secs=120\
             &agent_memory_enabled=true&agent_memory_persistence_mode=scope_specific_facts\
             &agent_memory_recall_mode=thinking&agent_memory_max_results=7\
             &agent_config_json=&agent_avatar_url=\
             &name=Support+Inbox&slug=support-triage&description=Where+support+mail+lands\
             &alias_slugs=help,+sales&participant_emails=%40public&enabled=true&add_3rd_party=true\
             &retrieve_company_memory=true&confirm_spam_disabled=true",
        )
        .await;

        assert_eq!(form.agent.name, "Support Triage");
        assert_eq!(form.agent.slug.as_deref(), Some("support-triage"));
        assert_eq!(form.agent.run_timeout_secs, Some(120));
        assert_eq!(form.agent.memory_max_results, Some(7));
        assert_eq!(form.agent.memory_enabled.as_deref(), Some("true"));
        assert_eq!(
            form.agent.memory_persistence_mode,
            Some(MemoryPersistenceMode::ScopeSpecificFacts)
        );
        assert_eq!(
            form.agent.memory_recall_mode,
            Some(MemoryRecallMode::Thinking)
        );

        assert_eq!(form.channel.name, "Support Inbox");
        assert_eq!(
            form.channel.description.as_deref(),
            Some("Where support mail lands")
        );
        assert_eq!(form.channel.alias_slugs.as_deref(), Some("help, sales"));
        assert!(form.channel.confirm_spam_disabled());

        // The carried half becomes the ordinary agent form, on the tab it came from.
        let agent = AgentForm::from(form.agent);
        assert_eq!(agent.form_mode.as_deref(), Some("advanced"));
        assert_eq!(agent.system_prompt.as_deref(), Some("You are support"));
        let submitted = SubmittedAgent::new(agent);
        assert!(!submitted.is_simple());
        assert_eq!(
            submitted.agent_write().expect("the write builds").slug,
            "support-triage"
        );
    }

    /// An unticked memory switch submits no key at all, the same way the agent form itself reads
    /// one, and a blank number field must not 422 the whole submit.
    #[tokio::test]
    async fn the_channel_step_reads_an_absent_memory_switch_as_off() {
        let form = channel_step_form(
            "agent_name=Triage&agent_slug=triage&agent_system_prompt=Hi&agent_description=\
             &agent_provider=&agent_model=&agent_run_timeout_secs=&agent_memory_max_results=5\
             &agent_memory_persistence_mode=audience_only&agent_memory_recall_mode=fast\
             &agent_config_json=&agent_avatar_url=&name=Triage&slug=triage",
        )
        .await;

        assert_eq!(form.agent.memory_enabled, None);
        assert_eq!(form.agent.run_timeout_secs, None);
        assert!(!form.channel.enabled());
    }

    fn owned(company_id: Uuid, agent_id: Option<Uuid>) -> Channel {
        Channel {
            owner_agent_id: agent_id,
            id: Uuid::new_v4(),
            company_id,
            name: "Triage".to_string(),
            description: None,
            slug: "triage".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            agent_ids: agent_id.map(|id| vec![id]),
            enabled: true,
            add_3rd_party: true,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        }
    }

    /// Ownership is what the Channel tab is about, and it is not the same question as assignment:
    /// a channel this agent merely runs on is not the one the tab may edit.
    #[test]
    fn owned_channel_finds_the_agents_own_and_never_a_borrowed_one() {
        let company_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let mine = owned(company_id, Some(agent_id));
        let theirs = owned(company_id, Some(Uuid::new_v4()));
        let mut shared = owned(company_id, None);
        shared.agent_ids = Some(vec![agent_id]);

        let channels = vec![shared, theirs, mine.clone()];
        assert_eq!(
            owned_channel(agent_id, &channels).map(|channel| channel.id),
            Some(mine.id)
        );
        assert!(owned_channel(Uuid::new_v4(), &channels).is_none());
    }

    /// The Channel tab renders `channel_fields` with `ChannelOwner::Existing`, whose only agent
    /// input is one hidden `agent_ids` carrying the owner. The write has to come back out of that
    /// body with the owner still in it, or the save would drop the position-0 assignment an owned
    /// channel is required to have.
    #[tokio::test]
    async fn the_channel_tab_body_writes_the_owner_back() {
        let agent_id = Uuid::new_v4();
        let request = Request::builder()
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "form_mode=advanced&name=Triage&slug=triage\
                 &description=Answers+billing&alias_slugs=help,+sales\
                 &participant_emails=%40public&agent_ids={agent_id}\
                 &enabled=true&add_3rd_party=true&confirm_spam_disabled=true"
            )))
            .unwrap();
        let form = Form::<ChannelForm>::from_request(request, &())
            .await
            .expect("the channel tab's body parses")
            .0;

        let submitted = SubmittedChannel::new(form);
        assert_eq!(submitted.agent_ids(), vec![agent_id]);

        let write = submitted
            .write(Some(submitted.agent_ids()))
            .expect("the write builds");
        assert_eq!(write.slug, "triage");
        assert_eq!(write.agent_ids, Some(vec![agent_id]));
        assert_eq!(
            write.alias_slugs,
            vec!["help".to_string(), "sales".to_string()]
        );
        assert!(submitted.form.confirm_spam_disabled());
    }
}
