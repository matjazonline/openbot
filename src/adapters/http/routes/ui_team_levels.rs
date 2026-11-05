use super::super::company_invite::LevelForm;
use super::*;
use crate::use_cases::company_invite::{InviteLevelChange, MemberLevelChange};

#[instrument(skip_all)]
pub(super) async fn set_member_level(
    workspace: Workspace,
    Path((company_id, member_user_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<LevelForm>,
) -> AppResult<Response> {
    workspace.scoped_company(company_id).await?;
    let saved = workspace
        .invite_use_cases
        .set_member_access_level(MemberLevelChange {
            actor_id: workspace.user_id,
            company_id,
            member_user_id,
            access_level: form.access_level,
        })
        .await?;
    // A self-demotion changes the options in the response immediately.
    let access = workspace.scoped_company(company_id).await?;
    let view = workspace.view(&access);
    let pane = view.member_pane(&saved, None).await?;
    view.saved_response(TeamSelection::Member(member_user_id), pane)
        .await
}

#[instrument(skip_all)]
pub(super) async fn set_invite_level(
    workspace: Workspace,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<LevelForm>,
) -> AppResult<Response> {
    workspace.scoped_company(company_id).await?;
    let saved = workspace
        .invite_use_cases
        .set_invite_access_level(InviteLevelChange {
            actor_id: workspace.user_id,
            company_id,
            invite_id,
            access_level: form.access_level,
        })
        .await?;
    let access = workspace.scoped_company(company_id).await?;
    let view = workspace.view(&access);
    let pane = view.invite_pane(&saved, None, None, None);
    view.saved_response(TeamSelection::Invite(invite_id), pane)
        .await
}
