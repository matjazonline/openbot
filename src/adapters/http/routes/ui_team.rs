//! `/ui/companies/{company_id}/team` — the Team tab of the Companies workspace: who is in the
//! selected company, and who has been invited to it.
//!
//! There is no Team workspace any more; a team only means anything as the team *of* a company, so
//! its list and its panes are drawn inside that company's own pane by
//! [`super::ui_companies::companies_page`], which calls [`team_tab_body`] here. Every fragment
//! and every write is nested under the company it belongs to, and all of them go through the same
//! [`CompanyInviteUseCases`] the classic `/companies/{id}/invites` page uses, so the two UIs
//! cannot drift on who may change a team.

use std::sync::Arc;

use axum::{
    Form, Router,
    extract::{FromRequestParts, Path, Query},
    http::request::Parts,
    response::{Html, IntoResponse, Redirect, Response},
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
    infra::config::AppConfig,
    use_cases::{
        company::CompanyUseCases, company_invite::CompanyInviteUseCases, user::UserUseCases,
    },
};

use super::ui::{load_scoped_company, workspace_user};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/team", get(team_redirect))
        .route("/ui/companies/{company_id}/team/new", get(invite_form_pane))
        .route("/ui/companies/{company_id}/team/close", get(close_pane))
        .route(
            "/ui/companies/{company_id}/team/invites",
            post(create_invite),
        )
        .route(
            "/ui/companies/{company_id}/team/invites/{invite_id}",
            get(invite_pane).put(update_invite).delete(delete_invite),
        )
        .route(
            "/ui/companies/{company_id}/team/members/{user_id}",
            get(member_pane).delete(remove_member),
        )
        .route(
            "/ui/companies/{company_id}/team/members/{user_id}/avatar",
            put(set_member_avatar),
        )
}

/// What the Team tab has selected, all optional so the tab alone is a valid entry point.
///
/// It arrives as part of [`super::ui_companies::WorkspaceQuery`] rather than being parsed here:
/// the tab and the company it sits in are one page, and so are one query.
#[derive(Debug, Clone, Copy, Default)]
pub struct TeamRequest {
    /// The selected member's `user_id`, which is what removing one takes.
    pub member_id: Option<Uuid>,
    pub invite_id: Option<Uuid>,
    /// Whether the pane should hold the invite form instead of a person.
    pub creating: bool,
}

/// The company a stale `/ui/team` link named, if it named one at all.
#[derive(Debug, Clone, Deserialize)]
pub struct LegacyTeamQuery {
    pub company_id: Option<Uuid>,
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
            invite_use_cases: state.company_invite_use_cases.clone(),
            user_use_cases: state.user_use_cases.clone(),
            config: state.config.clone(),
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

/// The Team tab's body for one company, as the Companies workspace embeds it.
///
/// It builds from the same [`TeamView`] the fragments below do, so the list's entries and the
/// panes they click through to cannot drift on what a member or an invite is allowed to do.
pub(super) async fn team_tab_body(
    invite_use_cases: &CompanyInviteUseCases,
    user_id: Uuid,
    company: &Company,
    request: TeamRequest,
) -> AppResult<String> {
    let view = TeamView {
        invite_use_cases,
        user_id,
        company,
    };
    let members = view.members().await?;
    let invites = view.invites().await?;

    // Only the owner has an invite form to open, so a member's `?new=1` is an ordinary empty tab
    // rather than a pane full of buttons the server would refuse.
    let creating = request.creating && view.role().manages();
    let selected = if creating {
        TeamSelection::None
    } else {
        view.selection(&members, &invites, request.member_id, request.invite_id)
    };

    let pane_html = if creating {
        view.invite_create_pane("", None)
    } else {
        view.selected_pane(&members, &invites, selected)
    };

    Ok(pages::team_tab(
        &view.list(&members, &invites, selected),
        &pane_html,
    ))
}

/// GET /ui/team - Where the Team workspace used to be (Protected).
///
/// The team lives in its company's pane now, so a stale link is answered with the company it named
/// rather than a 404; without one there is nothing to show, and the Companies workspace is where a
/// company gets picked.
#[instrument]
async fn team_redirect(Query(query): Query<LegacyTeamQuery>) -> Redirect {
    match query.company_id {
        Some(company_id) => Redirect::to(&pages::team_url(company_id, TeamSelection::None)),
        None => Redirect::to("/ui/companies"),
    }
}

/// GET /ui/companies/{company_id}/team/new - The invite form for the pane (Protected).
///
/// The list comes with it, deselected: the form belongs to nobody, so leaving whoever was open
/// before it lit would say the pane is showing a person it is not.
#[instrument(skip(workspace))]
async fn invite_form_pane(
    workspace: Workspace,
    Path(company_id): Path<Uuid>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(company_id).await?;
    let view = workspace.view(&company);
    view.require_owner()?;

    view.invite_form_response().await
}

/// GET /ui/companies/{company_id}/team/close - Dismiss whichever form the pane holds (Protected).
///
/// What Cancel does: the pane goes back to its placeholder and the list loses its highlight, so
/// cancelling a form and closing a person both leave the tab in the same state as arriving at it
/// with nothing selected.
#[instrument(skip(workspace))]
async fn close_pane(workspace: Workspace, Path(company_id): Path<Uuid>) -> AppResult<Response> {
    let company = workspace.scoped_company(company_id).await?;
    workspace.view(&company).cleared_response().await
}

/// GET /ui/companies/{company_id}/team/members/{user_id} - One member's access (Protected).
#[instrument(skip(workspace))]
async fn member_pane(
    workspace: Workspace,
    Path((company_id, user_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(company_id).await?;
    let view = workspace.view(&company);
    let member = view.member(user_id).await?;

    Ok(Html(view.member_pane(&member, None)))
}

/// PUT /ui/companies/{company_id}/team/members/{user_id}/avatar - Save your own picture (Protected).
///
/// An avatar belongs to the account, not to the membership, so this refuses any `user_id` but the
/// caller's -- being the company owner does not make somebody else's picture yours to set.
#[instrument(skip(workspace, form))]
async fn set_member_avatar(
    workspace: Workspace,
    Path((company_id, user_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<AvatarForm>,
) -> AppResult<Response> {
    if user_id != workspace.user_id {
        return Err(AppError::BadRequest(
            "You can only change your own avatar.".into(),
        ));
    }

    let company = workspace.scoped_company(company_id).await?;
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

    // Re-read the member so the pane and the list both show what was just stored, and send the
    // top bar's own avatar along -- it is chrome no pane swap would otherwise touch.
    let member = view.member(user_id).await?;
    let saved_email = EmailAddress::from(saved.email.as_str());
    let pane = format!(
        "{}{}",
        view.member_pane(&member, None),
        pages::account_chip(
            &workspace_user(&saved, &saved_email, &workspace.config),
            pages::FragmentSwap::OutOfBand,
        ),
    );

    view.saved_response(TeamSelection::Member(user_id), pane)
        .await
}

/// GET /ui/companies/{company_id}/team/invites/{invite_id} - One invite for the pane (Protected).
#[instrument(skip(workspace))]
async fn invite_pane(
    workspace: Workspace,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(company_id).await?;
    let view = workspace.view(&company);
    let invite = view.invite(invite_id).await?;

    Ok(Html(view.invite_pane(&invite, None, None)))
}

/// POST /ui/companies/{company_id}/team/invites - Invite someone from the form (Protected).
#[instrument(skip(workspace, form))]
async fn create_invite(
    workspace: Workspace,
    Path(company_id): Path<Uuid>,
    Form(form): Form<InviteForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(company_id).await?;
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

/// PUT /ui/companies/{company_id}/team/invites/{invite_id} - Save one address (Protected).
#[instrument(skip(workspace, form))]
async fn update_invite(
    workspace: Workspace,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<InviteForm>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(company_id).await?;
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

/// DELETE /ui/companies/{company_id}/team/invites/{invite_id} - Withdraw an invite (Protected).
#[instrument(skip(workspace))]
async fn delete_invite(
    workspace: Workspace,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(company_id).await?;
    let view = workspace.view(&company);
    view.invite_use_cases
        .delete_company_invite(view.user_id, company.id, invite_id)
        .await?;

    view.cleared_response().await
}

/// DELETE /ui/companies/{company_id}/team/members/{user_id} - Remove someone (Protected).
#[instrument(skip(workspace))]
async fn remove_member(
    workspace: Workspace,
    Path((company_id, user_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(company_id).await?;
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

/// Everything the tab renders from, so each handler names its data once.
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

    /// Only the owner may see invites, and the list hides the section for everyone else, so a
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

    /// Which list entry the query names, keeping only a selection that actually exists.
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

    /// The pane one selection opens, or the placeholder when it opens nothing.
    fn selected_pane(
        &self,
        members: &[CompanyMember],
        invites: &[CompanyInvite],
        selected: TeamSelection,
    ) -> String {
        let pane = match selected {
            TeamSelection::Member(user_id) => members
                .iter()
                .find(|member| member.user_id == user_id)
                .map(|member| self.member_pane(member, None)),
            TeamSelection::Invite(invite_id) => invites
                .iter()
                .find(|invite| invite.id == invite_id)
                .map(|invite| self.invite_pane(invite, None, None)),
            TeamSelection::None => None,
        };

        pane.unwrap_or_else(|| {
            pages::team_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline)
        })
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

    /// The empty pane plus a list with nothing selected — what cancelling a form, closing a
    /// person, and every removal all leave behind.
    async fn cleared_response(&self) -> AppResult<Response> {
        self.refreshed_response(
            TeamSelection::None,
            pages::team_settings_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline),
            pages::team_url(self.company.id, TeamSelection::None),
        )
        .await
    }

    /// The invite form plus a list with nothing lit — an invite is not somebody yet, so it is not
    /// a selection, which is why this cannot go through [`Self::saved_response`].
    async fn invite_form_response(&self) -> AppResult<Response> {
        self.refreshed_response(
            TeamSelection::None,
            self.invite_create_pane("", None),
            pages::team_invite_form_url(self.company.id),
        )
        .await
    }

    /// What every successful write returns: the saved person's pane, with the list refreshed
    /// beside it so a new invite or a changed address shows up immediately.
    async fn saved_response(&self, selected: TeamSelection, pane: String) -> AppResult<Response> {
        let push_url = pages::team_url(self.company.id, selected);
        self.refreshed_response(selected, pane, push_url).await
    }

    /// One pane plus the list it belongs to, swapped out of band beside it, with the address bar
    /// moved to where that pane is reachable.
    ///
    /// The URL is passed in rather than derived from `selected`: the invite form lights nothing
    /// and still has an address of its own, so the two are not the same fact.
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
