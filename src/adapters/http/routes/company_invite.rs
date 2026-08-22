use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::AppResult,
    entities::{company_invite::CompanyInvite, company_member::CompanyMember},
    use_cases::{
        company::CompanyUseCases, company_invite::CompanyInviteUseCases, user::UserUseCases,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        // Web / HTMX Company Invites & Team
        .route(
            "/companies/{company_id}/invites",
            get(company_invites_page).post(create_company_invite),
        )
        .route(
            "/companies/{company_id}/invites/{invite_id}/edit",
            get(edit_invite_form),
        )
        .route(
            "/companies/{company_id}/invites/{invite_id}/cancel",
            get(cancel_invite_edit),
        )
        .route(
            "/companies/{company_id}/invites/{invite_id}",
            put(update_company_invite).delete(delete_company_invite),
        )
        .route(
            "/companies/{company_id}/team/{user_id}",
            delete(remove_team_member),
        )
        // Web / HTMX User Invites
        .route("/invites", get(user_invites_page))
        .route("/invites/{invite_id}/accept", post(accept_user_invite))
        .route("/invites/{invite_id}/decline", post(decline_user_invite))
        // JSON API Company Invites & Team
        .route(
            "/api/companies/{company_id}/invites",
            get(list_company_invites_json).post(create_company_invite_json),
        )
        .route(
            "/api/companies/{company_id}/invites/{invite_id}",
            put(update_company_invite_json).delete(delete_company_invite_json),
        )
        .route(
            "/api/companies/{company_id}/team",
            get(list_team_members_json),
        )
        .route(
            "/api/companies/{company_id}/team/{user_id}",
            delete(remove_team_member_json),
        )
        // JSON API User Invites
        .route("/api/invites", get(list_user_invites_json))
        .route(
            "/api/invites/{invite_id}/accept",
            post(accept_user_invite_json),
        )
        .route(
            "/api/invites/{invite_id}/decline",
            post(decline_user_invite_json),
        )
}

#[derive(Debug, Clone, Deserialize)]
pub struct InviteForm {
    pub email: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InviteResponse {
    pub success: bool,
    pub invite: CompanyInvite,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamMembersResponse {
    pub success: bool,
    pub members: Vec<CompanyMember>,
}

/// GET /companies/{company_id}/invites - Full HTML page for company invites & team management.
#[instrument(skip(company_use_cases, invite_use_cases, user))]
async fn company_invites_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")).into_response(),
    };

    if company.user_id != user.id {
        return Html(pages::error_alert(
            "Unauthorized: Only company owner can manage invites.",
        ))
        .into_response();
    }

    let invites = invite_use_cases
        .list_company_invites(user.id, company_id)
        .await
        .unwrap_or_default();

    let members = invite_use_cases
        .list_company_team_members(user.id, company_id)
        .await
        .unwrap_or_default();

    Html(pages::company_invites_page(&company, &invites, &members)).into_response()
}

/// POST /companies/{company_id}/invites - Create email invite (HTMX).
#[instrument(skip(invite_use_cases, user, form))]
async fn create_company_invite(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<InviteForm>,
) -> impl IntoResponse {
    match invite_use_cases
        .create_company_invite(user.id, company_id, &form.email)
        .await
    {
        Ok(_) => {
            let invites = invite_use_cases
                .list_company_invites(user.id, company_id)
                .await
                .unwrap_or_default();
            Html(pages::company_invite_list_fragment(company_id, &invites))
        }
        Err(err) => {
            let error_html = pages::error_alert(&format!("Failed to create invite: {err}"));
            let invites = invite_use_cases
                .list_company_invites(user.id, company_id)
                .await
                .unwrap_or_default();
            Html(format!(
                "{}{}",
                error_html,
                pages::company_invite_list_fragment(company_id, &invites)
            ))
        }
    }
}

/// GET /companies/{company_id}/invites/{invite_id}/edit - Edit form fragment.
#[instrument(skip(invite_use_cases, user))]
async fn edit_invite_form(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    if let Ok(Some(invite)) = invite_use_cases
        .get_company_invite(user.id, company_id, invite_id)
        .await
    {
        Html(pages::company_invite_edit_fragment(company_id, &invite))
    } else {
        Html(pages::error_alert("Invite not found."))
    }
}

/// GET /companies/{company_id}/invites/{invite_id}/cancel - Cancel edit fragment.
#[instrument(skip(invite_use_cases, user))]
async fn cancel_invite_edit(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    if let Ok(Some(invite)) = invite_use_cases
        .get_company_invite(user.id, company_id, invite_id)
        .await
    {
        Html(pages::company_invite_row_fragment(company_id, &invite))
    } else {
        Html(String::new())
    }
}

/// PUT /companies/{company_id}/invites/{invite_id} - Update invite email (HTMX).
#[instrument(skip(invite_use_cases, user, form))]
async fn update_company_invite(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<InviteForm>,
) -> impl IntoResponse {
    match invite_use_cases
        .update_company_invite_email(user.id, company_id, invite_id, &form.email)
        .await
    {
        Ok(invite) => Html(pages::company_invite_row_fragment(company_id, &invite)),
        Err(err) => Html(pages::error_alert(&format!("Update failed: {err}"))),
    }
}

/// DELETE /companies/{company_id}/invites/{invite_id} - Delete invite (HTMX).
#[instrument(skip(invite_use_cases, user))]
async fn delete_company_invite(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let _ = invite_use_cases
        .delete_company_invite(user.id, company_id, invite_id)
        .await;
    Html(String::new())
}

/// DELETE /companies/{company_id}/team/{user_id} - Remove member from company team (HTMX).
#[instrument(skip(invite_use_cases, user))]
async fn remove_team_member(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, member_user_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    match invite_use_cases
        .remove_company_team_member(user.id, company_id, member_user_id)
        .await
    {
        Ok(_) => Html(String::new()),
        Err(err) => Html(pages::error_alert(&format!(
            "Failed to remove member: {err}"
        ))),
    }
}

/// GET /invites - Page listing user's invites.
#[instrument(skip(user_use_cases, invite_use_cases, user))]
async fn user_invites_page(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
) -> impl IntoResponse {
    let active_user = match user_use_cases.get_user_by_id(user.id).await {
        Ok(Some(u)) => u,
        _ => return Html(pages::error_alert("User not found.")).into_response(),
    };

    let invites = invite_use_cases
        .list_user_invites(&active_user.email)
        .await
        .unwrap_or_default();

    Html(pages::user_invites_page(&active_user.email, &invites)).into_response()
}

/// POST /invites/{invite_id}/accept - Accept invite (HTMX).
#[instrument(skip(user_use_cases, invite_use_cases, user))]
async fn accept_user_invite(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path(invite_id): Path<Uuid>,
) -> impl IntoResponse {
    let active_user = match user_use_cases.get_user_by_id(user.id).await {
        Ok(Some(u)) => u,
        _ => return Html(pages::error_alert("User not found.")).into_response(),
    };

    match invite_use_cases
        .accept_invite(&active_user, invite_id)
        .await
    {
        Ok(invite) => Html(pages::user_invite_row_fragment(&invite)).into_response(),
        Err(err) => Html(pages::error_alert(&format!("Failed to accept: {err}"))).into_response(),
    }
}

/// POST /invites/{invite_id}/decline - Decline invite (HTMX).
#[instrument(skip(user_use_cases, invite_use_cases, user))]
async fn decline_user_invite(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path(invite_id): Path<Uuid>,
) -> impl IntoResponse {
    let active_user = match user_use_cases.get_user_by_id(user.id).await {
        Ok(Some(u)) => u,
        _ => return Html(pages::error_alert("User not found.")).into_response(),
    };

    match invite_use_cases
        .decline_invite(&active_user, invite_id)
        .await
    {
        Ok(invite) => Html(pages::user_invite_row_fragment(&invite)).into_response(),
        Err(err) => Html(pages::error_alert(&format!("Failed to decline: {err}"))).into_response(),
    }
}

// ================= JSON API Handlers =================

/// GET /api/companies/{company_id}/invites - JSON list company invites.
async fn list_company_invites_json(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let invites = invite_use_cases
        .list_company_invites(user.id, company_id)
        .await?;
    Ok((StatusCode::OK, Json(invites)))
}

/// POST /api/companies/{company_id}/invites - JSON create company invite.
async fn create_company_invite_json(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Json(payload): Json<InviteForm>,
) -> AppResult<impl IntoResponse> {
    let invite = invite_use_cases
        .create_company_invite(user.id, company_id, &payload.email)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(InviteResponse {
            success: true,
            invite,
        }),
    ))
}

/// PUT /api/companies/{company_id}/invites/{invite_id} - JSON update company invite.
async fn update_company_invite_json(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<InviteForm>,
) -> AppResult<impl IntoResponse> {
    let invite = invite_use_cases
        .update_company_invite_email(user.id, company_id, invite_id, &payload.email)
        .await?;
    Ok((
        StatusCode::OK,
        Json(InviteResponse {
            success: true,
            invite,
        }),
    ))
}

/// DELETE /api/companies/{company_id}/invites/{invite_id} - JSON delete company invite.
async fn delete_company_invite_json(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, invite_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    invite_use_cases
        .delete_company_invite(user.id, company_id, invite_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/companies/{company_id}/team - JSON list company team members.
async fn list_team_members_json(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let members = invite_use_cases
        .list_company_team_members(user.id, company_id)
        .await?;
    Ok((
        StatusCode::OK,
        Json(TeamMembersResponse {
            success: true,
            members,
        }),
    ))
}

/// DELETE /api/companies/{company_id}/team/{user_id} - JSON remove member from team.
async fn remove_team_member_json(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, member_user_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    invite_use_cases
        .remove_company_team_member(user.id, company_id, member_user_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/invites - JSON list user's invites.
async fn list_user_invites_json(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
) -> AppResult<impl IntoResponse> {
    let active_user = user_use_cases
        .get_user_by_id(user.id)
        .await?
        .ok_or_else(|| crate::app_error::AppError::Internal("User not found.".into()))?;

    let invites = invite_use_cases
        .list_user_invites(&active_user.email)
        .await?;
    Ok((StatusCode::OK, Json(invites)))
}

/// POST /api/invites/{invite_id}/accept - JSON accept invite.
async fn accept_user_invite_json(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path(invite_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let active_user = user_use_cases
        .get_user_by_id(user.id)
        .await?
        .ok_or_else(|| crate::app_error::AppError::Internal("User not found.".into()))?;

    let invite = invite_use_cases
        .accept_invite(&active_user, invite_id)
        .await?;
    Ok((
        StatusCode::OK,
        Json(InviteResponse {
            success: true,
            invite,
        }),
    ))
}

/// POST /api/invites/{invite_id}/decline - JSON decline invite.
async fn decline_user_invite_json(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    user: AuthenticatedUser,
    Path(invite_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let active_user = user_use_cases
        .get_user_by_id(user.id)
        .await?
        .ok_or_else(|| crate::app_error::AppError::Internal("User not found.".into()))?;

    let invite = invite_use_cases
        .decline_invite(&active_user, invite_id)
        .await?;
    Ok((
        StatusCode::OK,
        Json(InviteResponse {
            success: true,
            invite,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn company_invites_page_renders_html_components() {
        let company = crate::entities::company::Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Test Company".to_string(),
            slug: "test-company".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };

        let invite = CompanyInvite {
            id: Uuid::new_v4(),
            company_id: company.id,
            company_name: Some(company.name.clone()),
            email: "invited@test.com".to_string(),
            status: "pending".to_string(),
            created_at: Utc::now(),
        };

        let member = CompanyMember {
            id: Uuid::new_v4(),
            company_id: company.id,
            user_id: Uuid::new_v4(),
            username: Some("member1".to_string()),
            email: Some("member1@test.com".to_string()),
            avatar_url: None,
            role: "member".to_string(),
            created_at: Utc::now(),
        };

        let page_html = pages::company_invites_page(&company, &[invite.clone()], &[member.clone()]);
        assert!(page_html.contains("Test Company Management"));
        assert!(page_html.contains("invited@test.com"));
        assert!(page_html.contains("member1"));

        let user_invites_html = pages::user_invites_page("invited@test.com", &[invite]);
        assert!(user_invites_html.contains("Your Invitations"));
        assert!(user_invites_html.contains("Accept"));
        assert!(user_invites_html.contains("Decline"));
    }
}
