use super::*;
use crate::{
    entities::access_level::AccessLevel,
    use_cases::company_invite::{InviteLevelChange, MemberLevelChange},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LevelForm {
    pub access_level: AccessLevel,
}

#[instrument(skip_all)]
pub(super) async fn set_member_level(
    State(invites): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, member_user_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<LevelForm>,
) -> AppResult<Html<String>> {
    let member = invites
        .set_member_access_level(MemberLevelChange {
            actor_id: user.id,
            company_id,
            member_user_id,
            access_level: form.access_level,
        })
        .await?;
    let access = invites.level_manager(user.id, company_id).await?;
    Ok(Html(pages::company_team_row_fragment(
        company_id,
        &member,
        pages::team_level_actor(&access),
    )))
}
#[instrument(skip_all)]
pub(super) async fn set_invite_level(
    State(invites): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<LevelForm>,
) -> AppResult<Html<String>> {
    let invite = invites
        .set_invite_access_level(InviteLevelChange {
            actor_id: user.id,
            company_id,
            invite_id,
            access_level: form.access_level,
        })
        .await?;
    let access = invites.level_manager(user.id, company_id).await?;
    Ok(Html(pages::company_invite_row_fragment(
        company_id,
        &invite,
        pages::team_level_actor(&access),
    )))
}
#[instrument(skip_all)]
pub(super) async fn set_member_level_json(
    State(invites): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, member_user_id)): Path<(Uuid, Uuid)>,
    Json(form): Json<LevelForm>,
) -> AppResult<impl IntoResponse> {
    let member = invites
        .set_member_access_level(MemberLevelChange {
            actor_id: user.id,
            company_id,
            member_user_id,
            access_level: form.access_level,
        })
        .await?;
    Ok(Json(member))
}
#[instrument(skip_all)]
pub(super) async fn set_invite_level_json(
    State(invites): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
    Json(form): Json<LevelForm>,
) -> AppResult<impl IntoResponse> {
    let invite = invites
        .set_invite_access_level(InviteLevelChange {
            actor_id: user.id,
            company_id,
            invite_id,
            access_level: form.access_level,
        })
        .await?;
    Ok(Json(invite))
}

#[cfg(test)]
#[path = "company_invite_level_route_tests.rs"]
mod route_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, extract::FromRequest, http::Request};
    #[test]
    fn access_level_json_is_numeric_bounded_and_rejects_role_injection() {
        assert_eq!(
            serde_json::from_str::<LevelForm>(r#"{"access_level":4}"#)
                .unwrap()
                .access_level,
            AccessLevel::L4
        );
        for json in [
            r#"{"access_level":10}"#,
            r#"{"access_level":-1}"#,
            r#"{"access_level":4,"role":"admin"}"#,
            r#"{}"#,
        ] {
            assert!(serde_json::from_str::<LevelForm>(json).is_err());
        }
    }
    #[tokio::test]
    async fn access_level_browser_form_accepts_integer_strings_and_rejects_extra_fields() {
        for body in [
            "access_level=4",
            "access_level=4&role=admin",
            "access_level=10",
        ] {
            let request = Request::builder()
                .method("PUT")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap();
            let parsed = Form::<LevelForm>::from_request(request, &()).await;
            if body == "access_level=4" {
                assert_eq!(parsed.unwrap().0.access_level, AccessLevel::L4);
            } else {
                assert!(parsed.is_err());
            }
        }
    }
}
