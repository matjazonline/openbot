//! `/ui/agents` — the Agents workspace: the mailbox shell with the company's agents configured
//! rather than read.
//!
//! The shell, the company scoping and the form parsing are all shared: chrome comes from
//! [`crate::adapters::http::pages::ui_shell`], the company from [`super::ui::load_managed_company`],
//! and every field is parsed by the same helpers the classic Agents page uses, so the two UIs
//! cannot drift on what a submitted agent means.

use std::sync::Arc;

use crate::use_cases::agent::AgentWrite;

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
        value_objects::{AvatarUrl, EmailAddress},
    },
    infra::config::AppConfig,
    use_cases::{
        agent::AgentUseCases, channel::ChannelUseCases, company::CompanyUseCases,
        user::UserUseCases,
    },
};

use super::{
    agent::{AgentForm, ModelOverrides, create_agent_from_instructions},
    channel::parse_config_form,
    ui::{load_account, load_managed_company, managed_company_membership, workspace_user},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/agents", get(agents_page).post(create_agent))
        .route("/ui/agents/new", get(create_pane))
        .route("/ui/agents/generate-prompt", post(generate_prompt))
        .route(
            "/ui/agents/{agent_id}",
            get(edit_pane).put(update_agent).delete(delete_agent),
        )
}

/// What the workspace has selected, all optional so `/ui/agents` alone is a valid entry point.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceQuery {
    pub company_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    /// `?new=1` opens the create form instead of an agent's settings.
    pub new: Option<String>,
}

/// The company scope every fragment and every write carries, in the URL rather than the body so
/// the form itself stays exactly [`AgentForm`].
#[derive(Debug, Clone, Deserialize)]
pub struct CompanyQuery {
    pub company_id: Uuid,
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
    pub api_key: Option<String>,
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

/// The use cases and the caller every Agents handler starts from.
///
/// Extracted as one value rather than five `State`s per handler: each of these routes needs the
/// same set, and a handler's own parameters should be what makes it different from its siblings.
struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    agent_use_cases: Arc<AgentUseCases>,
    channel_use_cases: Arc<ChannelUseCases>,
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
            user_id: self.user_id,
            company,
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
    // Landing on `/ui/agents` with no `agent_id` opens the first agent rather than an empty pane,
    // so the workspace is never a blank screen when there is something to show.
    let selected = query
        .agent_id
        .and_then(|id| agents.iter().find(|agent| agent.id == id))
        .or_else(|| agents.first());

    let creating = matches!(query.new.as_deref(), Some("1") | Some("true"));
    let pane_html = match (creating, selected) {
        (true, _) => view.create_pane(&pages::AgentDraft::default(), None),
        (false, Some(agent)) => view.edit_pane(agent, None, None).await?,
        (false, None) => {
            pages::agent_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline)
        }
    };

    let list = view.list(&agents, selected.map(|agent| agent.id));
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

    Ok(Html(view.create_pane(&pages::AgentDraft::default(), None)))
}

/// GET /ui/agents/{agent_id} - One agent's settings for the pane (Protected).
#[instrument(skip(workspace))]
async fn edit_pane(
    workspace: Workspace,
    Path(agent_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let agent = view.agent(agent_id).await?;

    Ok(Html(view.edit_pane(&agent, None, None).await?))
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
        api_key: form.api_key.as_deref(),
    };

    let generated = workspace
        .agent_use_cases
        .generate_system_prompt(
            workspace.user_id,
            company.id,
            instructions,
            overrides.provider,
            overrides.model,
            overrides.api_key,
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

    let rejected = |message: String| {
        Ok(Html(view.create_pane(&submitted.draft(), Some(&message))).into_response())
    };

    let avatar_url = match &submitted.avatar_url {
        Ok(avatar) => avatar.clone(),
        Err(message) => return rejected(message.clone()),
    };

    let created = if submitted.is_simple() {
        create_agent_from_instructions(
            &workspace.agent_use_cases,
            workspace.user_id,
            company.id,
            &submitted.form.name,
            &submitted.slug,
            submitted.form.system_prompt.as_deref().unwrap_or_default(),
            submitted.overrides(),
            avatar_url.as_ref(),
        )
        .await
    } else {
        let config_json = match parse_config_form(submitted.form.config_json.clone()) {
            Ok(config) => config,
            Err(message) => return rejected(message),
        };

        workspace
            .agent_use_cases
            .create_agent(
                workspace.user_id,
                company.id,
                AgentWrite {
                    name: submitted.form.name.clone(),
                    slug: submitted.slug.clone(),
                    provider: submitted.form.provider.clone(),
                    model: submitted.form.model.clone(),
                    api_key: submitted.form.api_key.clone(),
                    system_prompt: submitted.form.system_prompt.clone(),
                    description: submitted.form.description.clone(),
                    config_json,
                    avatar_url,
                    created_by: None,
                },
            )
            .await
            .map_err(|err| format!("Failed to create agent: {err}"))
    };

    match created {
        Ok(agent) => view.saved_response(&agent).await,
        Err(message) => rejected(message),
    }
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
    let submitted = SubmittedAgent::new(form);

    let fields = parse_config_form(submitted.form.config_json.clone())
        .and_then(|config_json| Ok((config_json, submitted.avatar_url.clone()?)));

    let (config_json, avatar_url) = match fields {
        Ok(fields) => fields,
        Err(message) => {
            return Ok(Html(
                view.edit_pane(&stored, Some(&submitted.draft()), Some(&message))
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
                api_key: submitted.form.api_key.clone(),
                system_prompt: submitted.form.system_prompt.clone(),
                description: submitted.form.description.clone(),
                config_json,
                avatar_url,
                created_by: None,
            },
        )
        .await;

    match saved {
        Ok(agent) => view.saved_response(&agent).await,
        Err(err) => Ok(Html(
            view.edit_pane(
                &stored,
                Some(&submitted.draft()),
                Some(&format!("Failed to save agent: {err}")),
            )
            .await?,
        )
        .into_response()),
    }
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
    let pane = pages::agent_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline);
    let list =
        pages::agent_settings_list(&view.list(&agents, None), pages::FragmentSwap::OutOfBand);

    Ok(Html(format!("{pane}{list}")).into_response())
}

/// Everything the workspace renders from, so each handler names its data once.
struct AgentSettingsView<'a> {
    agent_use_cases: &'a AgentUseCases,
    channel_use_cases: &'a ChannelUseCases,
    user_id: Uuid,
    company: &'a Company,
}

impl AgentSettingsView<'_> {
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

    /// The channels running this agent. Nothing enforces the reference, so the pane has to look
    /// it up to say what a delete would cost.
    async fn used_by(&self, agent_id: Uuid) -> AppResult<Vec<Channel>> {
        let channels = self
            .channel_use_cases
            .list_company_channels(self.user_id, self.company.id)
            .await?;

        Ok(channels
            .into_iter()
            .filter(|channel| {
                channel
                    .agent_ids
                    .as_deref()
                    .is_some_and(|ids| ids.contains(&agent_id))
            })
            .collect())
    }

    fn list<'a>(
        &'a self,
        agents: &'a [Agent],
        selected_agent_id: Option<Uuid>,
    ) -> pages::AgentSettingsList<'a> {
        pages::AgentSettingsList {
            company: self.company,
            agents,
            selected_agent_id,
        }
    }

    fn create_pane(&self, draft: &pages::AgentDraft<'_>, error: Option<&str>) -> String {
        pages::agent_create_pane(&pages::AgentCreatePane {
            company: self.company,
            draft,
            error,
        })
    }

    async fn edit_pane(
        &self,
        agent: &Agent,
        draft: Option<&pages::AgentDraft<'_>>,
        error: Option<&str>,
    ) -> AppResult<String> {
        let used_by = self.used_by(agent.id).await?;
        let used_by: Vec<&Channel> = used_by.iter().collect();

        Ok(pages::agent_edit_pane(&pages::AgentEditPane {
            company: self.company,
            agent,
            used_by: &used_by,
            draft,
            error,
        }))
    }

    /// What every successful write returns: the saved agent's pane, with the sidebar list
    /// refreshed beside it so a create, rename or handle change shows up immediately.
    async fn saved_response(&self, agent: &Agent) -> AppResult<Response> {
        let pane = self.edit_pane(agent, None, None).await?;
        let agents = self.agents().await?;
        let list = pages::agent_settings_list(
            &self.list(&agents, Some(agent.id)),
            pages::FragmentSwap::OutOfBand,
        );

        Ok((
            [(
                "HX-Push-Url",
                format!(
                    "/ui/agents?company_id={}&agent_id={}",
                    self.company.id, agent.id
                ),
            )],
            Html(format!("{pane}{list}")),
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

    fn overrides(&self) -> ModelOverrides<'_> {
        ModelOverrides {
            provider: self.form.provider.as_deref(),
            model: self.form.model.as_deref(),
            api_key: self.form.api_key.as_deref(),
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
            api_key: self.form.api_key.as_deref().unwrap_or(""),
            config_json: self.form.config_json.as_deref().unwrap_or(""),
            avatar_url: self.form.avatar_url.as_deref().unwrap_or(""),
            advanced: !self.is_simple(),
        }
    }
}
