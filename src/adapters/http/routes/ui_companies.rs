//! `/ui/companies` — the Companies workspace: the `/ui` shell with the companies themselves in
//! the sidebar.
//!
//! It is the workspace the other three are scoped by, so it is also the only one whose sidebar is
//! not company-scoped: there is no company switcher, because picking a company *is* the sidebar.
//! Everything else is shared — the chrome comes from [`crate::adapters::http::pages::ui_shell`]
//! and every write goes through the same [`CompanyUseCases`] the classic `/companies` page uses,
//! so the two UIs cannot drift on what a submitted company means.

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
        pages::{self, CompanyCounts, SpamGuardrail},
    },
    app_error::{AppError, AppResult},
    entities::{company::Company, value_objects::EmailAddress},
    infra::config::AppConfig,
    use_cases::{
        agent::AgentUseCases, channel::ChannelUseCases, company::CompanyUseCases,
        user::UserUseCases,
    },
};

use super::{
    channel::slugify,
    ui::{load_account, load_scoped_company},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/companies", get(companies_page).post(create_company))
        .route("/ui/companies/new", get(create_pane))
        .route("/ui/companies/close", get(close_pane))
        .route(
            "/ui/companies/{company_id}",
            get(edit_pane).put(update_company).delete(delete_company),
        )
}

/// What the workspace has selected, both optional so `/ui/companies` alone is a valid entry point.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceQuery {
    pub company_id: Option<Uuid>,
    /// `?new=1` opens the create form instead of a company's settings.
    pub new: Option<String>,
}

/// A submitted company, before it is a [`Company`].
///
/// The guardrail arrives as text rather than the `Option<bool>` the classic form takes: a checkbox
/// can only say "on" or "absent", and absent already means "leave it to the server", so the third
/// state needs a `<select>` — see [`SpamGuardrail`].
#[derive(Debug, Clone, Deserialize)]
pub struct CompanyForm {
    pub name: String,
    pub slug: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub enable_llm_spam_guardrail: Option<String>,
}

const NO_SELECTION: &str = "Select a company to configure it, or create a new one.";

/// The use cases and the caller every Companies handler starts from.
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
    /// The caller's own companies — the sidebar's contents, and the only companies any handler
    /// here will act on.
    async fn companies(&self) -> AppResult<Vec<Company>> {
        self.company_use_cases
            .list_user_companies(self.user_id)
            .await
    }

    /// One company, always picked from the caller's own so a guessed `company_id` cannot reach
    /// another user's settings.
    async fn scoped_company(&self, company_id: Uuid) -> AppResult<Company> {
        let (_, company) =
            load_scoped_company(&self.company_use_cases, self.user_id, Some(company_id)).await?;
        company.ok_or_else(|| AppError::NotFound("Company not found".into()))
    }

    /// What the company holds, for the summary above its form.
    async fn counts(&self, company_id: Uuid) -> AppResult<CompanyCounts> {
        let channels = self
            .channel_use_cases
            .list_company_channels(self.user_id, company_id)
            .await?;
        let agents = self
            .agent_use_cases
            .list_company_agents(self.user_id, company_id)
            .await?;

        Ok(CompanyCounts {
            channels: channels.len(),
            agents: agents.len(),
        })
    }

    fn edit_pane(
        &self,
        company: &Company,
        counts: CompanyCounts,
        draft: Option<&pages::CompanyDraft<'_>>,
        error: Option<&str>,
    ) -> String {
        pages::company_edit_pane(&pages::CompanyEditPane {
            company,
            app_domain_name: &self.config.app_domain_name,
            counts,
            draft,
            error,
        })
    }

    /// The empty pane plus a sidebar with nothing selected — what cancelling a form and deleting
    /// a company both leave behind.
    async fn cleared_response(&self) -> AppResult<Response> {
        let companies = self.companies().await?;
        let pane = pages::company_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline);
        let list = pages::company_settings_list(
            &pages::CompanySettingsList {
                companies: &companies,
                selected_company_id: None,
            },
            pages::FragmentSwap::OutOfBand,
        );

        Ok(Html(format!("{pane}{list}")).into_response())
    }

    /// What every successful write returns: the saved company's pane, with the sidebar list
    /// refreshed beside it so a create or a rename shows up immediately.
    async fn saved_response(&self, company: &Company) -> AppResult<Response> {
        let counts = self.counts(company.id).await?;
        let pane = self.edit_pane(company, counts, None, None);
        let companies = self.companies().await?;
        let list = pages::company_settings_list(
            &pages::CompanySettingsList {
                companies: &companies,
                selected_company_id: Some(company.id),
            },
            pages::FragmentSwap::OutOfBand,
        );

        Ok((
            [(
                "HX-Push-Url",
                format!("/ui/companies?company_id={}", company.id),
            )],
            Html(format!("{pane}{list}")),
        )
            .into_response())
    }
}

/// GET /ui/companies - The Companies workspace for the selected company (Protected).
#[instrument(skip(workspace))]
async fn companies_page(
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

    let creating = matches!(query.new.as_deref(), Some("1") | Some("true"));
    let selected = if creating { None } else { company.as_ref() };

    let pane_html = match selected {
        Some(company) => {
            let counts = workspace.counts(company.id).await?;
            workspace.edit_pane(company, counts, None, None)
        }
        None if creating => pages::company_create_pane(&pages::CompanyCreatePane {
            draft: &pages::CompanyDraft::default(),
            error: None,
        }),
        None => pages::company_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline),
    };

    Ok(Html(pages::company_settings_page(
        &pages::CompanySettingsPage {
            user: &workspace_user,
            list: &pages::CompanySettingsList {
                companies: &companies,
                selected_company_id: selected.map(|company| company.id),
            },
            // The create form deselects the list, but the rail keeps pointing at the company the
            // request was scoped to, so the other workspaces stay one click away.
            rail_company_id: company.as_ref().map(|company| company.id),
            pane_html: &pane_html,
        },
    )))
}

/// GET /ui/companies/new - The create-company form for the pane (Protected).
#[instrument(skip(_workspace))]
async fn create_pane(_workspace: Workspace) -> Html<String> {
    Html(pages::company_create_pane(&pages::CompanyCreatePane {
        draft: &pages::CompanyDraft::default(),
        error: None,
    }))
}

/// GET /ui/companies/close - Dismiss whichever form the pane holds (Protected).
///
/// What Cancel does: the pane goes back to its placeholder and the sidebar loses its highlight,
/// so cancelling a create and cancelling an edit both leave the workspace in the same state as
/// arriving at `/ui/companies` with nothing selected.
#[instrument(skip(workspace))]
async fn close_pane(workspace: Workspace) -> AppResult<Response> {
    workspace.cleared_response().await
}

/// GET /ui/companies/{company_id} - One company's settings for the pane (Protected).
#[instrument(skip(workspace))]
async fn edit_pane(workspace: Workspace, Path(company_id): Path<Uuid>) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(company_id).await?;
    let counts = workspace.counts(company.id).await?;

    Ok(Html(workspace.edit_pane(&company, counts, None, None)))
}

/// POST /ui/companies - Create a company from the pane's form (Protected).
#[instrument(skip(workspace, form))]
async fn create_company(workspace: Workspace, Form(form): Form<CompanyForm>) -> Response {
    let submitted = SubmittedCompany::new(form);

    let created = workspace
        .company_use_cases
        .create_company(
            workspace.user_id,
            &submitted.form.name,
            &submitted.slug,
            submitted.form.api_key.as_deref(),
            submitted.form.provider.as_deref(),
            submitted.form.model.as_deref(),
            submitted.spam_guardrail.stored(),
        )
        .await;

    match created {
        Ok(company) => match workspace.saved_response(&company).await {
            Ok(response) => response,
            Err(err) => err.into_response(),
        },
        Err(err) => Html(pages::company_create_pane(&pages::CompanyCreatePane {
            draft: &submitted.draft(),
            error: Some(&format!("Failed to create company: {err}")),
        }))
        .into_response(),
    }
}

/// PUT /ui/companies/{company_id} - Save one company's settings (Protected).
#[instrument(skip(workspace, form))]
async fn update_company(
    workspace: Workspace,
    Path(company_id): Path<Uuid>,
    Form(form): Form<CompanyForm>,
) -> AppResult<Response> {
    let stored = workspace.scoped_company(company_id).await?;
    let counts = workspace.counts(company_id).await?;
    let submitted = SubmittedCompany::new(form);

    let saved = workspace
        .company_use_cases
        .update_company_for_user(
            workspace.user_id,
            company_id,
            &submitted.form.name,
            &submitted.slug,
            submitted.form.api_key.as_deref(),
            submitted.form.provider.as_deref(),
            submitted.form.model.as_deref(),
            submitted.spam_guardrail.stored(),
        )
        .await;

    match saved {
        Ok(company) => workspace.saved_response(&company).await,
        Err(err) => Ok(Html(workspace.edit_pane(
            &stored,
            counts,
            Some(&submitted.draft()),
            Some(&format!("Failed to save company: {err}")),
        ))
        .into_response()),
    }
}

/// DELETE /ui/companies/{company_id} - Delete a company and clear the pane (Protected).
#[instrument(skip(workspace))]
async fn delete_company(workspace: Workspace, Path(company_id): Path<Uuid>) -> AppResult<Response> {
    workspace
        .company_use_cases
        .delete_company_for_user(workspace.user_id, company_id)
        .await?;

    workspace.cleared_response().await
}

/// A submitted company form, kept whole so a rejected save can be re-rendered with what was typed.
///
/// It also owns the two derivations every write needs — the slug the name falls back to, and the
/// parsed guardrail choice — so neither is re-derived at a call site.
struct SubmittedCompany {
    form: CompanyForm,
    slug: String,
    spam_guardrail: SpamGuardrail,
}

impl SubmittedCompany {
    fn new(form: CompanyForm) -> Self {
        let slug = form
            .slug
            .as_deref()
            .map(str::trim)
            .filter(|slug| !slug.is_empty())
            .map(String::from)
            .unwrap_or_else(|| slugify(&form.name));
        let spam_guardrail = SpamGuardrail::from_form(form.enable_llm_spam_guardrail.as_deref());

        Self {
            form,
            slug,
            spam_guardrail,
        }
    }

    fn draft(&self) -> pages::CompanyDraft<'_> {
        pages::CompanyDraft {
            name: &self.form.name,
            slug: &self.slug,
            provider: self.form.provider.as_deref().unwrap_or(""),
            model: self.form.model.as_deref().unwrap_or(""),
            api_key: self.form.api_key.as_deref().unwrap_or(""),
            spam_guardrail: self.spam_guardrail,
        }
    }
}
