//! `/ui/companies` — the Companies workspace: the `/ui` shell with the companies themselves in
//! the sidebar.
//!
//! It is the workspace the other three are scoped by, so it is also the only one whose sidebar is
//! not company-scoped: there is no company switcher, because picking a company *is* the sidebar.
//! Everything else is shared — the chrome comes from [`crate::adapters::http::pages::ui_shell`]
//! and every write goes through the same [`CompanyUseCases`] the classic `/companies` page uses,
//! so the two UIs cannot drift on what a submitted company means.
//!
//! The selected company's pane has two tabs: its own settings, and its team, which
//! [`super::ui_team`] renders into it — a team is only ever the team *of* a company, so it is
//! shown inside the one it belongs to rather than in a workspace of its own.

use std::{collections::HashMap, sync::Arc};

use axum::{
    Form, Router,
    extract::{FromRequestParts, Path, Query},
    http::request::Parts,
    response::{Html, IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Deserialize;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{
        app_state::AppState,
        auth::{AuthError, AuthenticatedUser},
        pages::{self, CompanyCounts, CompanyTab, SpamGuardrail},
    },
    app_error::{AppError, AppResult},
    entities::{
        company::{Company, CompanyAccess},
        company_member::CompanyMembership,
        value_objects::{AvatarUrl, EmailAddress},
    },
    infra::config::AppConfig,
    services::memory_provider::{ConfiguredMemoryProviders, SelectedMemoryProvider},
    use_cases::{
        agent::AgentUseCases,
        channel::ChannelUseCases,
        company::{CompanyModelConnectionWrite, CompanyUseCases, CompanyWrite},
        company_invite::CompanyInviteUseCases,
        company_resend_api::{CompanyResendApiUseCases, SubmittedResendApiIntegration},
        memory::MemoryUseCases,
        user::UserUseCases,
    },
};

use super::{
    channel::slugify,
    ui::{load_account, load_readable_company, workspace_user},
    ui_team::{TeamRequest, team_tab_body},
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
        .route(
            "/ui/companies/{company_id}/memory/retry",
            post(retry_memory),
        )
        .route(
            "/ui/companies/{company_id}/resend_api",
            put(save_resend_api).delete(disconnect_resend_api),
        )
        .route(
            "/ui/companies/{company_id}/resend_api/token",
            post(rotate_resend_api_token),
        )
}

/// What the workspace has selected, all optional so `/ui/companies` alone is a valid entry point.
///
/// The Team tab's own selection rides along in here rather than in a query of its own: the tab is
/// part of the company's pane, so it is part of the same page and the same URL.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkspaceQuery {
    pub company_id: Option<Uuid>,
    /// `?tab=team` opens the company's people instead of its settings.
    pub tab: Option<String>,
    /// `?new=1` opens a create form: a new company on the Settings tab, a new invite on Team.
    pub new: Option<String>,
    /// The Team tab's selected member, keyed by `user_id`.
    pub member_id: Option<Uuid>,
    /// The Team tab's selected invite.
    pub invite_id: Option<Uuid>,
}

/// A submitted company, before it is a [`Company`].
///
/// The guardrail arrives as text rather than the `Option<bool>` the classic form takes: a checkbox
/// can only say "on" or "absent", and absent already means "leave it to the server", so the third
/// state needs a `<select>` — see [`SpamGuardrail`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CompanyForm {
    pub name: String,
    pub slug: Option<String>,
    pub enable_llm_spam_guardrail: Option<String>,
    /// What the pane's picker is holding: an uploaded picture's URL, or blank for the letter.
    pub avatar_url: Option<String>,
    pub memory_provider: Option<String>,
    pub default_add_3rd_party: Option<bool>,
    pub default_participant_emails: Option<String>,
    pub default_retrieve_company_memory: Option<bool>,
    pub default_retrieve_agent_memory: Option<bool>,
    pub default_retrieve_user_memory: Option<bool>,
    pub default_persist_company_memory: Option<bool>,
    pub default_persist_agent_memory: Option<bool>,
    pub default_persist_user_memory: Option<bool>,
    pub default_model_provider: Option<usize>,
    #[serde(flatten)]
    pub connection_fields: HashMap<String, String>,
}

const NO_SELECTION: &str = "Select a company to configure it, or create a new one.";

/// Whether a write changed the company the shell's chrome is drawn for.
///
/// Two pieces speak for that company -- the picture the rail ends on and the name at the centre of
/// the top bar -- and no pane swap touches either, so a save that renames a company or gives it a
/// new picture has to send both back out of band. Which company the chrome is showing is not in
/// the request, so it is read off the write instead: the sidebar's entries are ordinary links, so
/// the pane and the chrome always hold the same company on an edit, while a company that has only
/// just been created is not yet the one the chrome is drawn for.
enum ChromeRefresh {
    SameCompany,
    OtherCompany,
}

/// The use cases and the caller every Companies handler starts from.
///
/// Extracted as one value rather than five `State`s per handler: each of these routes needs the
/// same set, and a handler's own parameters should be what makes it different from its siblings.
struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    channel_use_cases: Arc<ChannelUseCases>,
    agent_use_cases: Arc<AgentUseCases>,
    invite_use_cases: Arc<CompanyInviteUseCases>,
    resend_api_use_cases: Arc<CompanyResendApiUseCases>,
    user_use_cases: Arc<UserUseCases>,
    memory_use_cases: Arc<MemoryUseCases>,
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
            invite_use_cases: state.company_invite_use_cases.clone(),
            resend_api_use_cases: state.company_resend_api_use_cases.clone(),
            user_use_cases: state.user_use_cases.clone(),
            memory_use_cases: state.memory_use_cases.clone(),
            config: state.config.clone(),
            user_id: user.id,
        })
    }
}

impl Workspace {
    /// Every company the caller can open, including companies whose invitations they accepted.
    async fn companies(&self) -> AppResult<Vec<Company>> {
        self.company_use_cases
            .list_accessible_companies(self.user_id)
            .await
            .map(|companies| companies.into_iter().map(|access| access.company).collect())
    }

    /// One company the caller may read, together with whether they own it.
    async fn scoped_company(&self, company_id: Uuid) -> AppResult<CompanyAccess> {
        self.company_use_cases
            .company_access(self.user_id, company_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Company not found".into()))
    }

    /// What the company holds, for the summary above its form.
    async fn counts(&self, company_id: Uuid, editable: bool) -> AppResult<CompanyCounts> {
        // These use cases intentionally list configuration records for owners only. A member's
        // company pane does not render those administration counts, so do not turn a valid read
        // into an ownership error merely to populate hidden data.
        if !editable {
            return Ok(CompanyCounts::default());
        }

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

    /// The Settings tab: the company's own form, with whatever was rejected still in it.
    async fn settings_pane(
        &self,
        company: &Company,
        counts: CompanyCounts,
        draft: Option<&pages::CompanyDraft<'_>>,
        error: Option<&str>,
        editable: bool,
    ) -> AppResult<String> {
        let memory = if editable {
            self.memory_use_cases
                .status(self.user_id, company.id)
                .await?
        } else {
            None
        };
        let model_connections = if editable {
            self.company_use_cases
                .list_model_connections(self.user_id, company.id)
                .await?
        } else {
            Vec::new()
        };
        // Owners only, on the same terms as the model credentials above: the panel is where two
        // provider secrets are entered, and a member's pane renders none of that.
        let resend_api_section = if editable {
            self.resend_api_section(company.id, None, None, None)
                .await?
        } else {
            String::new()
        };
        Ok(pages::company_edit_pane_with_memory(
            &pages::CompanyEditPane {
                company,
                model_connections: &model_connections,
                app_domain_name: &self.config.app_domain_name,
                counts,
                draft,
                error,
                editable,
                resend_api_section: &resend_api_section,
                body: pages::CompanyPaneBody::Settings,
            },
            memory.as_ref(),
            self.memory_use_cases.configured(),
        ))
    }

    /// The Resend panel as it stands right now, with whatever the last write left to say.
    ///
    /// One renderer for all four responses that draw it -- the pane, a save, a rotation and a
    /// disconnect -- so the panel cannot show one thing on a full load and another after a write.
    async fn resend_api_section(
        &self,
        company_id: Uuid,
        draft: Option<&pages::CompanyResendApiDraft<'_>>,
        error: Option<&str>,
        notice: Option<&str>,
    ) -> AppResult<String> {
        let integration = self
            .resend_api_use_cases
            .integration(self.user_id, company_id)
            .await?;
        Ok(pages::company_resend_api_section(
            &pages::CompanyResendApiSection {
                company_id,
                integration: integration.as_ref(),
                base_url: &self.config.public_base_url(),
                draft,
                error,
                notice,
            },
        ))
    }

    /// The Team tab: the same pane, with the company's people in it instead of its settings.
    async fn team_pane(&self, company: &Company, request: TeamRequest) -> AppResult<String> {
        let editable = company.user_id == self.user_id;
        let counts = self.counts(company.id, editable).await?;
        let team = team_tab_body(&self.invite_use_cases, self.user_id, company, request).await?;

        Ok(pages::company_edit_pane(&pages::CompanyEditPane {
            company,
            model_connections: &[],
            app_domain_name: &self.config.app_domain_name,
            counts,
            draft: None,
            error: None,
            editable,
            // The Team tab renders no settings at all, and the Resend panel is settings.
            resend_api_section: "",
            body: pages::CompanyPaneBody::Team(&team),
        }))
    }

    /// The sidebar with nothing lit, out of band beside a pane that belongs to no company —
    /// the placeholder, or the create form.
    async fn deselected_list(&self) -> AppResult<String> {
        let companies = self.companies().await?;

        Ok(pages::company_settings_list(
            &pages::CompanySettingsList {
                companies: &companies,
                selected_company_id: None,
            },
            pages::FragmentSwap::OutOfBand,
        ))
    }

    /// The empty pane plus a sidebar with nothing selected — what cancelling a form and deleting
    /// a company both leave behind.
    async fn cleared_response(&self) -> AppResult<Response> {
        let pane = pages::company_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline);
        let list = self.deselected_list().await?;

        Ok(Html(format!("{pane}{list}")).into_response())
    }

    /// What every successful write returns: the saved company's pane, with the sidebar list
    /// refreshed beside it so a create or a rename shows up immediately.
    async fn saved_response(
        &self,
        company: &Company,
        chrome: ChromeRefresh,
    ) -> AppResult<Response> {
        let counts = self.counts(company.id, true).await?;
        let pane = self
            .settings_pane(company, counts, None, None, true)
            .await?;
        let companies = self.companies().await?;
        let list = pages::company_settings_list(
            &pages::CompanySettingsList {
                companies: &companies,
                selected_company_id: Some(company.id),
            },
            pages::FragmentSwap::OutOfBand,
        );
        let chrome_html = match chrome {
            ChromeRefresh::SameCompany => format!(
                "{}{}",
                pages::rail_company_badge(company, pages::FragmentSwap::OutOfBand),
                pages::topbar_company(company, pages::FragmentSwap::OutOfBand),
            ),
            ChromeRefresh::OtherCompany => String::new(),
        };

        Ok((
            [(
                "HX-Push-Url",
                format!("/ui/companies?company_id={}", company.id),
            )],
            Html(format!("{pane}{list}{chrome_html}")),
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
    let workspace_user = workspace_user(&account, &account_email, &workspace.config);

    let (companies, access) = load_readable_company(
        &workspace.company_use_cases,
        workspace.user_id,
        query.company_id,
    )
    .await?;
    let workspace_user = workspace_user.with_company_membership(
        access
            .as_ref()
            .map(|access| access.membership)
            .unwrap_or(CompanyMembership::None),
    );

    let tab = CompanyTab::from_query(query.tab.as_deref());
    let creating = matches!(query.new.as_deref(), Some("1") | Some("true"));
    // `?new=1` means a different thing on each tab, and only the Settings one deselects the
    // company: an invite is created *for* the company whose pane is open.
    let creating_company = creating && tab == CompanyTab::Settings;
    let selected = if creating_company {
        None
    } else {
        access.as_ref().map(|access| &access.company)
    };

    let pane_html = match selected {
        Some(company) if tab == CompanyTab::Team => {
            workspace
                .team_pane(
                    company,
                    TeamRequest {
                        member_id: query.member_id,
                        invite_id: query.invite_id,
                        creating,
                    },
                )
                .await?
        }
        Some(company) => {
            let editable = access
                .as_ref()
                .is_some_and(|access| access.membership.is_owner());
            let counts = workspace.counts(company.id, editable).await?;
            workspace
                .settings_pane(company, counts, None, None, editable)
                .await?
        }
        None if creating_company => pages::company_create_pane_with_memory(
            &pages::CompanyCreatePane {
                draft: &pages::CompanyDraft::default(),
                error: None,
            },
            workspace.memory_use_cases.configured(),
        ),
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
            rail_company: access.as_ref().map(|access| &access.company),
            pane_html: &pane_html,
        },
    )))
}

/// GET /ui/companies/new - The create-company form for the pane (Protected).
///
/// The sidebar comes with it, deselected: the form belongs to no company, so leaving the entry
/// that was open before it lit would say the pane is showing a company it is not.
#[instrument(skip(workspace))]
async fn create_pane(workspace: Workspace) -> AppResult<Response> {
    let pane = pages::company_create_pane_with_memory(
        &pages::CompanyCreatePane {
            draft: &pages::CompanyDraft::default(),
            error: None,
        },
        workspace.memory_use_cases.configured(),
    );
    let list = workspace.deselected_list().await?;

    Ok(Html(format!("{pane}{list}")).into_response())
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
    let access = workspace.scoped_company(company_id).await?;
    let editable = access.membership.is_owner();
    let counts = workspace.counts(access.company.id, editable).await?;

    Ok(Html(
        workspace
            .settings_pane(&access.company, counts, None, None, editable)
            .await?,
    ))
}

/// POST /ui/companies - Create a company from the pane's form (Protected).
#[instrument(skip(workspace, form))]
async fn create_company(workspace: Workspace, Form(form): Form<CompanyForm>) -> Response {
    let submitted = SubmittedCompany::new(form);

    let created = match submitted
        .write(workspace.config.is_spam_scan_enabled())
        .and_then(|write| {
            submitted.selected_memory(workspace.memory_use_cases.configured())?;
            let connections = submitted
                .model_connections()
                .map_err(|error| error.to_string())?;
            // A company may be created before any provider is configured. Only an *update* to an
            // empty set is a wipe, and `validate_model_connections` refuses that one.
            if !connections.is_empty() {
                CompanyUseCases::validate_model_connections(&connections)
                    .map_err(|error| error.to_string())?;
                if connections
                    .iter()
                    .any(|connection| connection.api_key.is_none())
                {
                    return Err(
                        "An API key is required for every provider when creating a company."
                            .to_string(),
                    );
                }
            }
            Ok(write)
        }) {
        Ok(write) => {
            workspace
                .company_use_cases
                .create_company(workspace.user_id, write)
                .await
        }
        Err(refusal) => Err(AppError::BadRequest(refusal)),
    };

    match created {
        Ok(company) => {
            let applied = match workspace
                .apply_submitted_model_connections(&submitted, company.id)
                .await
            {
                Ok(()) => workspace.apply_memory(&submitted, company.id).await,
                Err(err) => Err(err),
            };
            let response = match applied {
                Ok(()) => {
                    workspace
                        .saved_response(&company, ChromeRefresh::OtherCompany)
                        .await
                }
                Err(err) => Err(err),
            };
            response.unwrap_or_else(IntoResponse::into_response)
        }
        Err(err) => Html(pages::company_create_pane_with_memory(
            &pages::CompanyCreatePane {
                draft: &submitted.draft(),
                error: Some(&format!("Failed to create company: {err}")),
            },
            workspace.memory_use_cases.configured(),
        ))
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
    let stored = workspace
        .company_use_cases
        .owned_company(workspace.user_id, company_id)
        .await?;
    let counts = workspace.counts(company_id, true).await?;
    let submitted = SubmittedCompany::new(form);

    let saved = match submitted
        .write(workspace.config.is_spam_scan_enabled())
        .and_then(|write| {
            submitted.selected_memory(workspace.memory_use_cases.configured())?;
            let connections = submitted
                .model_connections()
                .map_err(|error| error.to_string())?;
            CompanyUseCases::validate_model_connections(&connections)
                .map_err(|error| error.to_string())?;
            Ok(write)
        }) {
        Ok(write) => {
            workspace
                .company_use_cases
                .update_company_for_user(workspace.user_id, company_id, write)
                .await
        }
        Err(refusal) => Err(AppError::BadRequest(refusal)),
    };

    // The company row is already written by this point, so a failure below leaves the two halves
    // out of step. Everything that could be checked before that write already has been, so what
    // is left is reported through the pane the way a rejected save is -- not as a bare 500.
    let saved = match saved {
        Ok(company) => match workspace
            .apply_model_connections(&submitted, company.id)
            .await
        {
            Ok(()) => workspace
                .apply_memory(&submitted, company.id)
                .await
                .map(|()| company),
            Err(err) => Err(err),
        },
        Err(err) => Err(err),
    };

    match saved {
        Ok(company) => {
            workspace
                .saved_response(&company, ChromeRefresh::SameCompany)
                .await
        }
        Err(err) => Ok(Html(
            workspace
                .settings_pane(
                    &stored,
                    counts,
                    Some(&submitted.draft()),
                    Some(&format!("Failed to save company: {err}")),
                    true,
                )
                .await?,
        )
        .into_response()),
    }
}

#[instrument(skip(workspace))]
async fn retry_memory(
    workspace: Workspace,
    Path(company_id): Path<Uuid>,
) -> AppResult<Html<String>> {
    workspace
        .memory_use_cases
        .retry(workspace.user_id, company_id)
        .await?;
    let company = workspace
        .company_use_cases
        .owned_company(workspace.user_id, company_id)
        .await?;
    let counts = workspace.counts(company_id, true).await?;
    Ok(Html(
        workspace
            .settings_pane(&company, counts, None, None, true)
            .await?,
    ))
}

/// What the Resend panel submits. Both secrets are optional because blank means "keep the stored
/// one", and `enabled` is absent entirely when its checkbox is cleared.
#[derive(Debug, Deserialize)]
pub struct ResendApiForm {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub signing_secret: Option<String>,
    #[serde(default)]
    pub authserv_id: String,
    #[serde(default)]
    pub enabled: Option<String>,
}

impl ResendApiForm {
    fn submitted(self) -> SubmittedResendApiIntegration {
        SubmittedResendApiIntegration {
            api_key: self.api_key,
            signing_secret: self.signing_secret,
            authserv_id: self.authserv_id,
            // An unchecked checkbox submits nothing at all, so absence is the "off" it means.
            enabled: self.enabled.is_some(),
        }
    }
}

/// PUT /ui/companies/{company_id}/resend_api - Connect or update this company's Resend account.
///
/// A refusal comes back as the panel with the message above it and the typed `authserv-id` still
/// in place, never as a bare error: the two credential fields are already blank by design, so
/// re-rendering costs the operator nothing but the two secrets they were entering anyway.
#[instrument(skip(workspace, form))]
async fn save_resend_api(
    workspace: Workspace,
    Path(company_id): Path<Uuid>,
    Form(form): Form<ResendApiForm>,
) -> AppResult<Html<String>> {
    // Taken before the form is consumed, so a refusal can put back what was typed. Neither
    // credential appears here: those fields render blank whatever happens.
    let authserv_id = form.authserv_id.trim().to_string();
    let draft = pages::CompanyResendApiDraft {
        authserv_id: &authserv_id,
        enabled: form.enabled.is_some(),
    };
    match workspace
        .resend_api_use_cases
        .save(workspace.user_id, company_id, form.submitted())
        .await
    {
        Ok(_) => Ok(Html(
            workspace
                .resend_api_section(company_id, None, None, Some("Resend settings saved."))
                .await?,
        )),
        Err(error) => Ok(Html(
            workspace
                .resend_api_section(company_id, Some(&draft), Some(&error.to_string()), None)
                .await?,
        )),
    }
}

/// POST /ui/companies/{company_id}/resend_api/token - Issue a fresh webhook URL for this company.
#[instrument(skip(workspace))]
async fn rotate_resend_api_token(
    workspace: Workspace,
    Path(company_id): Path<Uuid>,
) -> AppResult<Html<String>> {
    workspace
        .resend_api_use_cases
        .rotate_webhook_token(workspace.user_id, company_id)
        .await?;
    Ok(Html(
        workspace
            .resend_api_section(
                company_id,
                None,
                None,
                Some("A new webhook URL was issued. Update the endpoint in Resend now: deliveries to the old URL are already being refused."),
            )
            .await?,
    ))
}

/// DELETE /ui/companies/{company_id}/resend_api - Forget this company's Resend account.
#[instrument(skip(workspace))]
async fn disconnect_resend_api(
    workspace: Workspace,
    Path(company_id): Path<Uuid>,
) -> AppResult<Html<String>> {
    let removed = workspace
        .resend_api_use_cases
        .disconnect(workspace.user_id, company_id)
        .await?;
    let notice = removed.then_some(
        "Resend was disconnected. This company's mail now goes out over the deployment relay.",
    );
    Ok(Html(
        workspace
            .resend_api_section(company_id, None, None, notice)
            .await?,
    ))
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
            spam_guardrail: self.spam_guardrail,
            avatar_url: self.form.avatar_url.as_deref().unwrap_or(""),
            memory_provider: self.form.memory_provider.as_deref().unwrap_or(""),
            default_add_3rd_party: self.form.default_add_3rd_party.unwrap_or(false),
            default_participant_emails: self
                .form
                .default_participant_emails
                .clone()
                .unwrap_or_default(),
            default_retrieve_company_memory: self
                .form
                .default_retrieve_company_memory
                .unwrap_or(false),
            default_retrieve_agent_memory: self.form.default_retrieve_agent_memory.unwrap_or(false),
            default_retrieve_user_memory: self.form.default_retrieve_user_memory.unwrap_or(false),
            default_persist_company_memory: self
                .form
                .default_persist_company_memory
                .unwrap_or(false),
            default_persist_agent_memory: self.form.default_persist_agent_memory.unwrap_or(false),
            default_persist_user_memory: self.form.default_persist_user_memory.unwrap_or(false),
            // This view model deliberately has no credential field. Preserve only safe submitted
            // metadata so even a rejected request cannot echo its API key into the response.
            model_connections: self.model_connection_drafts(),
        }
    }

    fn model_connection_drafts(&self) -> Vec<pages::CompanyModelConnectionDraft> {
        (0..crate::use_cases::company::MAX_COMPANY_MODEL_CONNECTIONS)
            .filter_map(|index| {
                let provider = self
                    .form
                    .connection_fields
                    .get(&format!("connection_{index}_provider"))?
                    .trim();
                if provider.is_empty() {
                    return None;
                }
                Some(pages::CompanyModelConnectionDraft {
                    provider: provider.to_string(),
                    models: self
                        .form
                        .connection_fields
                        .get(&format!("connection_{index}_models"))
                        .cloned()
                        .unwrap_or_default(),
                    is_default: self.form.default_model_provider == Some(index),
                    has_api_key: false,
                    remove: self.connection_removed(index),
                })
            })
            .collect()
    }

    fn connection_removed(&self, index: usize) -> bool {
        self.form
            .connection_fields
            .get(&format!("connection_{index}_remove"))
            .is_some_and(|value| matches!(value.as_str(), "true" | "on"))
    }

    /// The submitted company as a write. The picture is parsed rather than taken as typed: it
    /// ends up in an `<img src>` on the rail of every page this company scopes.
    fn write(&self, spam_scanning: bool) -> Result<CompanyWrite, String> {
        if !spam_scanning
            && self
                .form
                .default_participant_emails
                .as_deref()
                .is_some_and(|values| {
                    values.split([',', '\n']).any(|value| {
                        value
                            .trim()
                            .eq_ignore_ascii_case(crate::entities::channel::PUBLIC_PARTICIPANT)
                    })
                })
        {
            return Err(
                "Public new-agent channel defaults require spam scanning to be enabled.".into(),
            );
        }
        Ok(CompanyWrite {
            name: self.form.name.clone(),
            slug: self.slug.clone(),
            enable_llm_spam_guardrail: self.spam_guardrail.stored(),
            memory_provider: None,
            channel_defaults: crate::entities::company::CompanyChannelDefaults {
                add_3rd_party: self.form.default_add_3rd_party.unwrap_or(false),
                participant_emails: self
                    .form
                    .default_participant_emails
                    .as_deref()
                    .map(|values| {
                        values
                            .split([',', '\n'])
                            .map(crate::entities::value_objects::EmailAddress::from)
                            .collect()
                    }),
                retrieve_company_memory: self.form.default_retrieve_company_memory.unwrap_or(false),
                retrieve_agent_memory: self.form.default_retrieve_agent_memory.unwrap_or(false),
                retrieve_user_memory: self.form.default_retrieve_user_memory.unwrap_or(false),
                persist_company_memory: self.form.default_persist_company_memory.unwrap_or(false),
                persist_agent_memory: self.form.default_persist_agent_memory.unwrap_or(false),
                persist_user_memory: self.form.default_persist_user_memory.unwrap_or(false),
            },
            avatar_url: AvatarUrl::parse(self.form.avatar_url.as_deref().unwrap_or(""))?,
        })
    }

    fn model_connections(&self) -> AppResult<Vec<CompanyModelConnectionWrite>> {
        // `connection_fields` is a flattened catch-all, so an input named `connection_0_modles`
        // would otherwise be dropped in silence and read as "this provider enabled no models".
        // Anything claiming to be a connection field has to be one this loop actually reads.
        for key in self.form.connection_fields.keys() {
            let Some(rest) = key.strip_prefix("connection_") else {
                continue;
            };
            let recognized = rest.split_once('_').is_some_and(|(index, field)| {
                index.parse::<usize>().is_ok_and(|index| {
                    index < crate::use_cases::company::MAX_COMPANY_MODEL_CONNECTIONS
                }) && matches!(field, "provider" | "models" | "api_key" | "remove")
            });
            if !recognized {
                return Err(AppError::BadRequest(format!(
                    "Unrecognized model connection field '{key}'."
                )));
            }
        }

        let mut connections = Vec::new();
        for index in 0..crate::use_cases::company::MAX_COMPANY_MODEL_CONNECTIONS {
            if self.connection_removed(index) {
                continue;
            }
            let provider = self
                .form
                .connection_fields
                .get(&format!("connection_{index}_provider"))
                .map(String::as_str)
                .unwrap_or_default()
                .trim();
            if provider.is_empty() {
                continue;
            }
            let models = self
                .form
                .connection_fields
                .get(&format!("connection_{index}_models"))
                .map(String::as_str)
                .unwrap_or_default()
                .split([',', '\n'])
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(String::from)
                .collect();
            let api_key = self
                .form
                .connection_fields
                .get(&format!("connection_{index}_api_key"))
                .cloned();
            connections.push(CompanyModelConnectionWrite::new(
                provider,
                api_key,
                models,
                self.form.default_model_provider == Some(index),
            )?);
        }
        Ok(connections)
    }

    fn selected_memory(
        &self,
        configured: &ConfiguredMemoryProviders,
    ) -> Result<SelectedMemoryProvider, String> {
        configured.select(self.form.memory_provider.as_deref())
    }
}

impl Workspace {
    async fn apply_model_connections(
        &self,
        submitted: &SubmittedCompany,
        company_id: Uuid,
    ) -> AppResult<()> {
        self.company_use_cases
            .replace_model_connections(self.user_id, company_id, submitted.model_connections()?)
            .await
    }

    /// Creation counterpart to [`Self::apply_model_connections`]: a company with nothing submitted
    /// simply starts without providers, rather than being refused for an empty replace.
    async fn apply_submitted_model_connections(
        &self,
        submitted: &SubmittedCompany,
        company_id: Uuid,
    ) -> AppResult<()> {
        let connections = submitted.model_connections()?;
        if connections.is_empty() {
            return Ok(());
        }
        self.company_use_cases
            .replace_model_connections(self.user_id, company_id, connections)
            .await
    }

    async fn apply_memory(&self, submitted: &SubmittedCompany, company_id: Uuid) -> AppResult<()> {
        match submitted
            .selected_memory(self.memory_use_cases.configured())
            .map_err(AppError::BadRequest)?
        {
            SelectedMemoryProvider::Provider(kind) => {
                self.memory_use_cases
                    .select_provider(self.user_id, company_id, kind)
                    .await?;
            }
            SelectedMemoryProvider::Disabled => {
                self.memory_use_cases
                    .disable(self.user_id, company_id)
                    .await?
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(fields: &[(&str, &str)], default_model_provider: Option<usize>) -> CompanyForm {
        CompanyForm {
            name: "Acme".into(),
            slug: Some("acme".into()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            default_model_provider,
            connection_fields: fields
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            ..CompanyForm::default()
        }
    }

    #[test]
    fn a_blank_submitted_key_becomes_preserve_at_the_form_boundary() {
        let submitted = SubmittedCompany::new(form(
            &[
                ("connection_0_provider", "openai"),
                ("connection_0_models", "gpt-5.6-terra"),
                ("connection_0_api_key", "   "),
            ],
            Some(0),
        ));

        let connections = submitted.model_connections().unwrap();
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].provider.as_str(), "openai");
        assert_eq!(connections[0].api_key, None);
    }

    #[test]
    fn an_explicit_remove_omits_only_that_provider_from_the_replacement() {
        let submitted = SubmittedCompany::new(form(
            &[
                ("connection_0_provider", "openai"),
                ("connection_0_models", "gpt-5.6-terra"),
                ("connection_0_api_key", ""),
                ("connection_0_remove", "true"),
                ("connection_1_provider", "anthropic"),
                ("connection_1_models", "claude-sonnet-4-5"),
                ("connection_1_api_key", ""),
            ],
            Some(1),
        ));

        let connections = submitted.model_connections().unwrap();
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].provider.as_str(), "anthropic");
        assert!(connections[0].is_default);

        let draft = submitted.draft();
        assert!(draft.model_connections[0].remove);
    }

    #[test]
    fn a_rejected_form_never_renders_its_submitted_api_key() {
        let submitted = SubmittedCompany::new(form(
            &[
                ("connection_0_provider", "openai"),
                ("connection_0_models", "gpt-5.6-terra"),
                ("connection_0_api_key", "sk-do-not-render"),
            ],
            Some(0),
        ));
        let draft = submitted.draft();
        let html = pages::company_create_pane(&pages::CompanyCreatePane {
            draft: &draft,
            error: Some("save refused"),
        });

        assert!(!html.contains("sk-do-not-render"));
        assert!(html.contains(r#"name="connection_0_api_key" value="""#));
        assert!(html.contains("Required for a new provider"));
    }
}
