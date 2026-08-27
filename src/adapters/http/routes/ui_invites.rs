//! `/ui/invites` — invitations addressed to the signed-in user.

use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    response::Html,
    routing::{get, post},
};
use serde::Deserialize;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::AppResult,
    entities::{company_member::CompanyMembership, value_objects::EmailAddress},
    infra::config::AppConfig,
    use_cases::{
        company::CompanyUseCases, company_invite::CompanyInviteUseCases, user::UserUseCases,
    },
};

use super::ui::{load_account, load_readable_company, workspace_user};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/invites", get(invites_page))
        .route("/ui/invites/{invite_id}/accept", post(accept_invite))
        .route("/ui/invites/{invite_id}/decline", post(decline_invite))
}

#[derive(Debug, Deserialize)]
struct InviteQuery {
    company_id: Option<Uuid>,
}

#[instrument(skip(company_use_cases, invite_use_cases, user_use_cases, config, user))]
async fn invites_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Query(query): Query<InviteQuery>,
) -> AppResult<Html<String>> {
    let account = load_account(&user_use_cases, user.id).await?;
    let (_, company) = load_readable_company(&company_use_cases, user.id, query.company_id).await?;
    let invites = invite_use_cases.list_user_invites(&account.email).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let mailbox_user = workspace_user(&account, &account_email, &config).with_company_membership(
        company
            .as_ref()
            .map(|access| access.membership)
            .unwrap_or(CompanyMembership::None),
    );

    Ok(Html(pages::invite_settings_page(
        &pages::InviteSettingsPage {
            user: &mailbox_user,
            company: company.as_ref().map(|access| &access.company),
            invites: &invites,
        },
    )))
}

#[instrument(skip(invite_use_cases, user_use_cases, user))]
async fn accept_invite(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    user: AuthenticatedUser,
    Path(invite_id): Path<Uuid>,
) -> AppResult<Html<String>> {
    let account = load_account(&user_use_cases, user.id).await?;
    let invite = invite_use_cases.accept_invite(&account, invite_id).await?;
    Ok(Html(pages::invite_settings_row(&invite)))
}

#[instrument(skip(invite_use_cases, user_use_cases, user))]
async fn decline_invite(
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    user: AuthenticatedUser,
    Path(invite_id): Path<Uuid>,
) -> AppResult<Html<String>> {
    let account = load_account(&user_use_cases, user.id).await?;
    let invite = invite_use_cases.decline_invite(&account, invite_id).await?;
    Ok(Html(pages::invite_settings_row(&invite)))
}
