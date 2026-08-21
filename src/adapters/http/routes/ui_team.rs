//! `/ui/team` — the Team workspace: the `/ui` shell showing who is in the company and who has
//! been invited to it.
//!
//! The shell, the company scoping and the authorization are all shared: chrome comes from
//! [`crate::adapters::http::pages::ui_shell`], the company from [`super::ui::load_scoped_company`],
//! and every write goes through the same [`CompanyInviteUseCases`] the classic
//! `/companies/{id}/invites` page uses, so the two UIs cannot drift on who may change a team.

use std::sync::Arc;

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
        pages::{self, TeamRole, TeamSelection},
    },
    app_error::{AppError, AppResult},
    entities::{
        company::Company,
        company_invite::CompanyInvite,
        company_member::CompanyMember,
        value_objects::{AvatarUrl, EmailAddress},
    },
    use_cases::{
        company::CompanyUseCases, company_invite::CompanyInviteUseCases, user::UserUseCases,
    },
};

use super::ui::{load_account, load_scoped_company, workspace_user};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/team", get(team_page))
        .route("/ui/team/new", get(invite_form_pane))
        .route("/ui/team/close", get(close_pane))
        .route("/ui/team/invites", post(create_invite))
        .route(
            "/ui/team/invites/{invite_id}",
            get(invite_pane).put(update_invite).delete(delete_invite),
        )
        .route(
            "/ui/team/members/{user_id}",
            get(member_pane).delete(remove_member),
        )
        .route("/ui/team/members/{user_id}/avatar", put(set_member_avatar))
}

/// What the workspace has selected, all optional so `/ui/team` alone is a valid entry point.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceQuery {
    pub company_id: Option<Uuid>,
    /// The selected member's `user_id`, which is what removing one takes.
    pub member_id: Option<Uuid>,
    pub invite_id: Option<Uuid>,
    /// `?new=1` opens the invite form instead of a person.
    pub new: Option<String>,
}

/// The company scope every fragment and every write carries, in the URL rather than the body so
/// the form itself stays exactly [`InviteForm`].
#[derive(Debug, Clone, Deserialize)]
pub struct CompanyQuery {
    pub company_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InviteForm {
    pub email: String,
}

/// The avatar field from a member's own pane. Blank means "back to my initial", which is why it is
/// a plain `String` rather than an `Option`.
#[derive(Debug, Clone, Deserialize)]
pub struct AvatarForm {
    pub avatar_url: String,
}

const NO_SELECTION: &str = "Select someone to see their access, or invite a new person.";

/// The use cases and the caller every Team handler starts from.
///
/// Extracted as one value rather than four `State`s per handler: each of these routes needs the
/// same set, and a handler's own parameters should be what makes it different from its siblings.
struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    invite_use_cases: Arc<CompanyInviteUseCases>,
    user_use_cases: Arc<UserUseCases>,
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
            invite_use_cases: state.company_invite_use_cases.clone(),
            user_use_cases: state.user_use_cases.clone(),
            user_id: user.id,
        })
    }
}

impl Workspace {
    /// The company a request is scoped to, always picked from the caller's own companies so a
    /// guessed `company_id` cannot reach another user's team.
    async fn scoped_company(&self, company_id: Uuid) -> AppResult<Company> {
        let (_, company) =
            load_scoped_company(&self.company_use_cases, self.user_id, Some(company_id)).await?;
        company.ok_or_else(|| AppError::NotFound("Company not found".into()))
    }

    fn view<'a>(&'a self, company: &'a Company) -> TeamView<'a> {
        TeamView {
            invite_use_cases: &self.invite_use_cases,
            user_id: self.user_id,
            company,
        }
    }
}

/// GET /ui/team - The Team workspace for the selected company / person (Protected).
#[instrument(skip(workspace))]
async fn team_page(
    workspace: Workspace,
    Query(query): Query<WorkspaceQuery>,
) -> AppResult<Html<String>> {
    let account = load_account(&workspace.user_use_cases, workspace.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let workspace_user = workspace_user(&account, &account_email);

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
    let members = view.members().await?;
    let invites = view.invites().await?;

    let creating = matches!(query.new.as_deref(), Some("1") | Some("true"));
    let selected = if creating {
        TeamSelection::None
    } else {
        view.selection(&members, &invites, query.member_id, query.invite_id)
    };

    let pane_html = match selected {
        _ if creating && view.role().manages() => view.invite_create_pane("", None),
        TeamSelection::Member(user_id) => match members.iter().find(|m| m.user_id == user_id) {
            Some(member) => view.member_pane(member, None),
            None => pages::team_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline),
        },
        TeamSelection::Invite(invite_id) => match invites.iter().find(|i| i.id == invite_id) {
            Some(invite) => view.invite_pane(invite, None, None),
            None => pages::team_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline),
        },
        TeamSelection::None => {
            pages::team_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline)
        }
    };

    Ok(Html(pages::team_settings_page(&pages::TeamSettingsPage {
        user: &workspace_user,
        companies: &companies,
        list: &view.list(&members, &invites, selected),
        pane_html: &pane_html,
    })))
}

/// GET /ui/team/new - The invite form for the pane (Protected).
#[instrument(skip(workspace))]
async fn invite_form_pane(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    view.require_owner()?;

    Ok(Html(view.invite_create_pane("", None)))
}

/// GET /ui/team/close - Dismiss whichever form the pane holds (Protected).
///
/// What Cancel does: the pane goes back to its placeholder and the sidebar loses its highlight,
/// so cancelling a form and closing a person both leave the workspace in the same state as
/// arriving at `/ui/team` with nothing selected.
#[instrument(skip(workspace))]
async fn close_pane(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    workspace.view(&company).cleared_response().await
}

/// GET /ui/team/members/{user_id} - One member's access for the pane (Protected).
#[instrument(skip(workspace))]
async fn member_pane(
    workspace: Workspace,
    Path(user_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let member = view.member(user_id).await?;

    Ok(Html(view.member_pane(&member, None)))
}

/// PUT /ui/team/members/{user_id}/avatar - Save your own profile picture (Protected).
///
/// An avatar belongs to the account, not to the membership, so this refuses any `user_id` but the
/// caller's -- being the company owner does not make somebody else's picture yours to set.
#[instrument(skip(workspace, form))]
async fn set_member_avatar(
    workspace: Workspace,
    Path(user_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<AvatarForm>,
) -> AppResult<Response> {
    if user_id != workspace.user_id {
        return Err(AppError::BadRequest(
            "You can only change your own avatar.".into(),
        ));
    }

    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);

    let avatar_url = match AvatarUrl::parse(&form.avatar_url) {
        Ok(avatar_url) => avatar_url,
        Err(message) => {
            let member = view.member(user_id).await?;
            return Ok(Html(view.member_pane_with_draft(
                &member,
                Some(&form.avatar_url),
                Some(&message),
            ))
            .into_response());
        }
    };

    let saved = match workspace
        .user_use_cases
        .set_avatar(user_id, avatar_url.as_ref())
        .await
    {
        Ok(saved) => saved,
        Err(err) => {
            let member = view.member(user_id).await?;
            return Ok(Html(view.member_pane_with_draft(
                &member,
                Some(&form.avatar_url),
                Some(&format!("Failed to save avatar: {err}")),
            ))
            .into_response());
        }
    };

    // Re-read the member so the pane and the sidebar both show what was just stored, and send the
    // top bar's own avatar along -- it is chrome no pane swap would otherwise touch.
    let member = view.member(user_id).await?;
    let saved_email = EmailAddress::from(saved.email.as_str());
    let pane = format!(
        "{}{}",
        view.member_pane(&member, None),
        pages::account_chip(
            &workspace_user(&saved, &saved_email),
            pages::FragmentSwap::OutOfBand,
        ),
    );

    view.saved_response(TeamSelection::Member(user_id), pane)
        .await
}

/// GET /ui/team/invites/{invite_id} - One invite for the pane (Protected).
#[instrument(skip(workspace))]
async fn invite_pane(
    workspace: Workspace,
    Path(invite_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let invite = view.invite(invite_id).await?;

    Ok(Html(view.invite_pane(&invite, None, None)))
}

/// POST /ui/team/invites - Invite someone from the pane's form (Protected).
#[instrument(skip(workspace, form))]
async fn create_invite(
    workspace: Workspace,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<InviteForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);

    let created = view
        .invite_use_cases
        .create_company_invite(view.user_id, company.id, &form.email)
        .await;

    match created {
        Ok(invite) => {
            let pane = view.invite_pane(&invite, None, None);
            view.saved_response(TeamSelection::Invite(invite.id), pane)
                .await
        }
        Err(err) => Ok(Html(
            view.invite_create_pane(&form.email, Some(&format!("Failed to send invite: {err}"))),
        )
        .into_response()),
    }
}

/// PUT /ui/team/invites/{invite_id} - Save one invite's address (Protected).
#[instrument(skip(workspace, form))]
async fn update_invite(
    workspace: Workspace,
    Path(invite_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<InviteForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let stored = view.invite(invite_id).await?;

    let saved = view
        .invite_use_cases
        .update_company_invite_email(view.user_id, company.id, invite_id, &form.email)
        .await;

    match saved {
        Ok(invite) => {
            let pane = view.invite_pane(&invite, None, None);
            view.saved_response(TeamSelection::Invite(invite.id), pane)
                .await
        }
        Err(err) => Ok(Html(view.invite_pane(
            &stored,
            Some(&form.email),
            Some(&format!("Failed to save invite: {err}")),
        ))
        .into_response()),
    }
}

/// DELETE /ui/team/invites/{invite_id} - Withdraw an invite and clear the pane (Protected).
#[instrument(skip(workspace))]
async fn delete_invite(
    workspace: Workspace,
    Path(invite_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    view.invite_use_cases
        .delete_company_invite(view.user_id, company.id, invite_id)
        .await?;

    view.cleared_response().await
}

/// DELETE /ui/team/members/{user_id} - Remove someone from the team and clear the pane (Protected).
#[instrument(skip(workspace))]
async fn remove_member(
    workspace: Workspace,
    Path(user_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);

    // A refused removal is the pane's own error, not a 500: the owner row is the case that hits
    // it, and the pane is what explains why.
    if let Err(err) = view
        .invite_use_cases
        .remove_company_team_member(view.user_id, company.id, user_id)
        .await
    {
        let member = view.member(user_id).await?;
        return Ok(Html(
            view.member_pane(&member, Some(&format!("Failed to remove member: {err}"))),
        )
        .into_response());
    }

    view.cleared_response().await
}

/// Everything the workspace renders from, so each handler names its data once.
struct TeamView<'a> {
    invite_use_cases: &'a CompanyInviteUseCases,
    user_id: Uuid,
    company: &'a Company,
}

impl TeamView<'_> {
    /// What the caller may do here.
    ///
    /// Decided from the company's own `user_id` rather than by letting an unauthorized use-case
    /// call fail: the invite calls reject a non-owner outright, so asking them first would turn
    /// an ordinary member's page load into an error.
    fn role(&self) -> TeamRole {
        if self.company.user_id == self.user_id {
            TeamRole::Owner
        } else {
            TeamRole::Member
        }
    }

    fn require_owner(&self) -> AppResult<()> {
        match self.role() {
            TeamRole::Owner => Ok(()),
            TeamRole::Member => Err(AppError::Internal(
                "Unauthorized: only the company owner can manage invites and team members.".into(),
            )),
        }
    }

    async fn members(&self) -> AppResult<Vec<CompanyMember>> {
        self.invite_use_cases
            .list_company_team_members(self.user_id, self.company.id)
            .await
    }

    /// Only the owner may see invites, and the sidebar hides the section for everyone else, so a
    /// member's list is empty rather than an error.
    async fn invites(&self) -> AppResult<Vec<CompanyInvite>> {
        match self.role() {
            TeamRole::Owner => {
                self.invite_use_cases
                    .list_company_invites(self.user_id, self.company.id)
                    .await
            }
            TeamRole::Member => Ok(Vec::new()),
        }
    }

    async fn member(&self, user_id: Uuid) -> AppResult<CompanyMember> {
        self.members()
            .await?
            .into_iter()
            .find(|member| member.user_id == user_id)
            .ok_or_else(|| AppError::NotFound("Team member not found".into()))
    }

    async fn invite(&self, invite_id: Uuid) -> AppResult<CompanyInvite> {
        self.invite_use_cases
            .get_company_invite(self.user_id, self.company.id, invite_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Invite not found".into()))
    }

    /// Which sidebar entry the query names, keeping only a selection that actually exists.
    fn selection(
        &self,
        members: &[CompanyMember],
        invites: &[CompanyInvite],
        member_id: Option<Uuid>,
        invite_id: Option<Uuid>,
    ) -> TeamSelection {
        if let Some(user_id) = member_id
            && members.iter().any(|member| member.user_id == user_id)
        {
            return TeamSelection::Member(user_id);
        }
        if let Some(invite_id) = invite_id
            && invites.iter().any(|invite| invite.id == invite_id)
        {
            return TeamSelection::Invite(invite_id);
        }
        TeamSelection::None
    }

    fn list<'l>(
        &'l self,
        members: &'l [CompanyMember],
        invites: &'l [CompanyInvite],
        selected: TeamSelection,
    ) -> pages::TeamSettingsList<'l> {
        pages::TeamSettingsList {
            company: self.company,
            members,
            invites,
            selected,
            role: self.role(),
        }
    }

    fn member_pane(&self, member: &CompanyMember, error: Option<&str>) -> String {
        self.member_pane_with_draft(member, None, error)
    }

    /// The same pane with the avatar field kept as typed, for a save that was rejected.
    fn member_pane_with_draft(
        &self,
        member: &CompanyMember,
        avatar_draft: Option<&str>,
        error: Option<&str>,
    ) -> String {
        pages::member_pane(&pages::MemberPane {
            company: self.company,
            member,
            role: self.role(),
            viewer_id: self.user_id,
            avatar_draft,
            error,
        })
    }

    fn invite_pane(
        &self,
        invite: &CompanyInvite,
        draft: Option<&str>,
        error: Option<&str>,
    ) -> String {
        pages::invite_pane(&pages::InvitePane {
            company: self.company,
            invite,
            role: self.role(),
            draft,
            error,
        })
    }

    fn invite_create_pane(&self, draft: &str, error: Option<&str>) -> String {
        pages::invite_create_pane(&pages::InviteCreatePane {
            company: self.company,
            draft,
            error,
        })
    }

    /// The empty pane plus a sidebar with nothing selected — what cancelling a form, closing a
    /// person, and every removal all leave behind.
    async fn cleared_response(&self) -> AppResult<Response> {
        self.refreshed_response(
            TeamSelection::None,
            pages::team_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline),
            format!("/ui/team?company_id={}", self.company.id),
        )
        .await
    }

    /// What every successful write returns: the saved person's pane, with the sidebar list
    /// refreshed beside it so a new invite or a changed address shows up immediately.
    async fn saved_response(&self, selected: TeamSelection, pane: String) -> AppResult<Response> {
        let push_url = match selected {
            TeamSelection::Member(user_id) => format!(
                "/ui/team?company_id={}&member_id={user_id}",
                self.company.id
            ),
            TeamSelection::Invite(invite_id) => format!(
                "/ui/team?company_id={}&invite_id={invite_id}",
                self.company.id
            ),
            TeamSelection::None => format!("/ui/team?company_id={}", self.company.id),
        };

        self.refreshed_response(selected, pane, push_url).await
    }

    /// One pane plus the sidebar it belongs to, swapped out of band beside it.
    async fn refreshed_response(
        &self,
        selected: TeamSelection,
        pane: String,
        push_url: String,
    ) -> AppResult<Response> {
        let members = self.members().await?;
        let invites = self.invites().await?;
        let list = pages::team_settings_list(
            &self.list(&members, &invites, selected),
            pages::FragmentSwap::OutOfBand,
        );

        Ok(([("HX-Push-Url", push_url)], Html(format!("{pane}{list}"))).into_response())
    }
}
