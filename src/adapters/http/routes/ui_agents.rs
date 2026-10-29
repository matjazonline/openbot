//! `/ui/agents` — the Agents workspace: the mailbox shell with the company's agents configured
//! rather than read.
//!
//! The shell, the company scoping and the form parsing are all shared: chrome comes from
//! [`crate::adapters::http::pages::ui_shell`], the company from [`super::ui::load_managed_company`],
//! and every field is parsed by the same helpers the classic Agents page uses, so the two UIs
//! cannot drift on what a submitted agent means.

use std::sync::Arc;

use crate::use_cases::agent::{AgentWrite, LibraryCopyModel};

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
        company::Company,
        value_objects::{AvatarUrl, EmailAddress},
    },
    infra::config::AppConfig,
    use_cases::{
        agent::AgentUseCases,
        company::CompanyUseCases,
        skill::{MAX_SKILL_PAGE_SIZE, SkillPageRequest, SkillUseCases},
        user::UserUseCases,
    },
};

use super::{
    agent::{AgentForm, AgentInstructionRequest, ModelOverrides, agent_write_from_instructions},
    channel::parse_agent_ids_form,
    ui::{load_account, load_managed_company, managed_company_membership, workspace_user},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/agents", get(agents_page).post(create_agent))
        .route("/ui/agents/new", get(create_pane))
        .route("/ui/agents/from-library", post(create_from_library))
        .route("/ui/agents/generate-prompt", post(generate_prompt))
        .route(
            "/ui/agents/{agent_id}",
            get(edit_pane)
                .put(update_agent)
                .post(update_agent)
                .delete(delete_agent),
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

/// The use cases and the caller every Agents handler starts from.
///
/// Extracted as one value rather than five `State`s per handler: each of these routes needs the
/// same set, and a handler's own parameters should be what makes it different from its siblings.
struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    agent_use_cases: Arc<AgentUseCases>,
    skill_use_cases: Arc<SkillUseCases>,
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
            skill_use_cases: state.skill_use_cases.clone(),
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
            skill_use_cases: &self.skill_use_cases,
            user_id: self.user_id,
            company,
            app_domain_name: &self.config.app_domain_name,
        }
    }
}

/// GET /ui/agents - The Agents workspace for the selected company / agent (Protected).
#[instrument(skip_all)]
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
        (true, _) => {
            view.create_pane(CreateForm {
                draft: &pages::AgentDraft::default(),
                error: None,
                selected_library_agent_ids: &[],
                tab: None,
            })
            .await?
        }
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
#[instrument(skip_all)]
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
#[instrument(skip_all)]
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
#[instrument(skip_all)]
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
#[instrument(skip_all)]
async fn create_agent(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<AgentForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let submitted = SubmittedAgent::new(form);

    let mut write = match submitted.agent_write() {
        Ok(write) => write,
        Err(message) => return rejected(&view, &submitted, &message).await,
    };
    if submitted.is_simple() {
        if let Err(error) = workspace
            .agent_use_cases
            .validate_new_agent(workspace.user_id, company.id, write.clone())
            .await
        {
            return rejected(&view, &submitted, &error.to_string()).await;
        }
        let generated = agent_write_from_instructions(
            &workspace.agent_use_cases,
            AgentInstructionRequest {
                user_id: workspace.user_id,
                company_id: company.id,
                name: &submitted.form.name,
                slug: &submitted.slug,
                instructions: submitted.form.system_prompt.as_deref().unwrap_or_default(),
                overrides: submitted.overrides(),
                avatar_url: write.avatar_url.as_ref(),
            },
        )
        .await;
        match generated {
            Ok(generated) => write.system_prompt = generated.system_prompt,
            Err(message) => return rejected(&view, &submitted, &message).await,
        }
    }
    let created = workspace
        .agent_use_cases
        .create_agent(workspace.user_id, company.id, write)
        .await
        .map(|agent| (agent, Vec::new()))
        .map_err(|err| format!("Failed to create agent: {err}"));

    match created {
        Ok((agent, warnings)) => view.saved_response(&agent, &warnings).await,
        Err(message) => rejected(&view, &submitted, &message).await,
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
#[instrument(skip_all)]
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
            .create_agent_from_library(
                workspace.user_id,
                company.id,
                *agent_id,
                LibraryCopyModel::FollowDefault,
            )
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
    view.saved_response(agent, &warnings).await
}

/// PUT /ui/agents/{agent_id} - Save one agent's settings (Protected).
#[instrument(skip_all)]
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

    let write = match submitted.agent_write() {
        Ok(write) => write,
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
        .update_agent(workspace.user_id, company.id, agent_id, write)
        .await;

    match saved {
        Ok(agent) => view.saved_response(&agent, &[]).await,
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
#[instrument(skip_all)]
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
    skill_use_cases: &'a SkillUseCases,
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

    async fn skills(&self) -> AppResult<Vec<crate::entities::skill::Skill>> {
        Ok(self
            .skill_use_cases
            .list_company_page(
                self.user_id,
                self.company.id,
                SkillPageRequest {
                    before: None,
                    limit: MAX_SKILL_PAGE_SIZE,
                },
            )
            .await?
            .items)
    }

    async fn agent(&self, agent_id: Uuid) -> AppResult<Agent> {
        self.agent_use_cases
            .get_company_agent(self.user_id, self.company.id, agent_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Agent not found".into()))
    }

    fn list<'a>(
        &'a self,
        agents: &'a [Agent],
        selected_agent_id: Option<Uuid>,
    ) -> pages::AgentSettingsList<'a> {
        pages::AgentSettingsList {
            company: self.company,
            app_domain_name: self.app_domain_name,
            agents,
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
        let mut skills = self.skills().await?;
        for skill_id in &form.draft.skill_ids {
            if !skills.iter().any(|skill| skill.id == *skill_id)
                && let Some(skill) = self
                    .skill_use_cases
                    .get_company(self.user_id, self.company.id, *skill_id)
                    .await?
            {
                skills.push(skill);
            }
        }
        // Nothing to start from means no Easy tab, so a request that asked for it lands on Simple.
        let tab = match form.tab {
            Some(pages::AgentCreateTab::Easy) | None if library_agents.is_empty() => {
                pages::AgentCreateTab::Simple
            }
            Some(tab) => tab,
            None => pages::AgentCreateTab::Easy,
        };

        let draft = form.draft.clone();
        Ok(pages::agent_create_pane(&pages::AgentCreatePane {
            company: self.company,
            app_domain_name: self.app_domain_name,
            model_connections: &model_connections,
            library_agents: &library_agents,
            selected_library_agent_ids: form.selected_library_agent_ids,
            tab,
            draft: &draft,
            error: form.error,
            capability_options: pages::AgentCapabilityOptions { skills: &skills },
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

    /// The agent's settings, which every agent write comes back on.
    async fn edit_pane(
        &self,
        agent: &Agent,
        draft: Option<&pages::AgentDraft<'_>>,
        error: Option<&str>,
    ) -> AppResult<String> {
        let model_connections = self.model_connections().await?;
        let capabilities = self
            .skill_use_cases
            .company_agent_capabilities(self.user_id, self.company.id, agent.id)
            .await?
            .ok_or_else(|| AppError::NotFound("Agent not found".into()))?;
        let mut skills = self.skills().await?;
        for selected in &capabilities.skills {
            if !skills.iter().any(|skill| skill.id == selected.id) {
                skills.push(selected.clone());
            }
        }
        let stored = pages::AgentDraft {
            name: &agent.name,
            slug: &agent.slug,
            system_prompt: agent.system_prompt.as_deref().unwrap_or(""),
            description: agent.description.as_deref().unwrap_or(""),
            provider: agent.provider.as_deref().unwrap_or(""),
            model: agent.model.as_deref().unwrap_or(""),
            skill_ids: capabilities.skills.iter().map(|skill| skill.id).collect(),
            avatar_url: agent
                .avatar_url
                .as_ref()
                .map(AvatarUrl::as_str)
                .unwrap_or(""),
            advanced: true,
        };
        let shown = draft.cloned().unwrap_or(stored);

        Ok(pages::agent_edit_pane(&pages::AgentEditPane {
            company: self.company,
            app_domain_name: self.app_domain_name,
            model_connections: &model_connections,
            agent,
            draft: Some(&shown),
            error,
            capability_options: pages::AgentCapabilityOptions { skills: &skills },
        }))
    }

    /// What every successful write returns: the saved agent's pane, with the sidebar list
    /// refreshed beside it so a create, rename or handle change shows up immediately.
    async fn saved_response(
        &self,
        agent: &Agent,
        warnings: &[crate::use_cases::agent::ProvisioningWarning],
    ) -> AppResult<Response> {
        let warning = warnings
            .iter()
            .map(|warning| pages::error_alert(&warning.message))
            .collect::<String>();
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
            Html(format!("{warning}{pane}{list}")),
        )
            .into_response())
    }
}

/// A submitted agent form, kept whole so a rejected save can be re-rendered with what was typed.
///
/// It also owns the one derivation every write needs — the slug the name falls back to — so it is
/// not re-derived at a call site.
pub(super) struct SubmittedAgent {
    form: AgentForm,
    slug: String,
    /// The avatar as parsed, or why it was refused. Kept as the `Result` so a bad URL comes back
    /// as the filled-in form with the reason on top, the way a bad config JSON does.
    avatar_url: Result<Option<AvatarUrl>, String>,
}

impl SubmittedAgent {
    pub(super) fn new(form: AgentForm) -> Self {
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
    /// One builder for every place a submit becomes a write -- the check before a Simple create
    /// expands its prompt, and the create itself -- so none of them can drift on what the form
    /// means.
    pub(super) fn agent_write(&self) -> Result<AgentWrite, String> {
        let capabilities = self.form.capabilities()?;
        Ok(AgentWrite {
            workflow_tools: None,
            name: self.form.name.clone(),
            slug: self.slug.clone(),
            provider: self.form.provider.clone(),
            model: self.form.model.clone(),
            system_prompt: self.form.system_prompt.clone(),
            description: self.form.description.clone(),
            skill_ids: capabilities.skill_ids,
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

    pub(super) fn draft(&self) -> pages::AgentDraft<'_> {
        let capabilities =
            self.form
                .capabilities()
                .unwrap_or_else(|_| super::agent::SubmittedCapabilities {
                    skill_ids: Vec::new(),
                });
        pages::AgentDraft {
            name: &self.form.name,
            slug: &self.slug,
            system_prompt: self.form.system_prompt.as_deref().unwrap_or(""),
            description: self.form.description.as_deref().unwrap_or(""),
            provider: self.form.provider.as_deref().unwrap_or(""),
            model: self.form.model.as_deref().unwrap_or(""),
            skill_ids: capabilities.skill_ids,
            avatar_url: self.form.avatar_url.as_deref().unwrap_or(""),
            advanced: !self.is_simple(),
        }
    }
}

#[cfg(test)]
#[path = "agent_form_tests.rs"]
mod form_tests;
