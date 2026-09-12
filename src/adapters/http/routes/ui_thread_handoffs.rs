//! The reply-handling settings page and its two saves.
//!
//! Not to be confused with `manual_handoffs`, whose routes live in [`super::attention`].

use std::sync::Arc;

use axum::{
    Form, Router,
    extract::{Query, State},
    response::Html,
    routing::get,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::{thread_handoff::ExternalReplyHandling, user::Viewer},
    use_cases::{
        channel::ChannelUseCases, company::CompanyUseCases, thread_handoff::ThreadHandoffUseCases,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/reply-handling", get(reply_handling_policy))
        .route(
            "/ui/reply-handling/company",
            axum::routing::post(update_company_reply_handling),
        )
        .route(
            "/ui/reply-handling/channel",
            axum::routing::post(update_channel_reply_handling),
        )
}

#[derive(Debug, Deserialize)]
struct PolicyQuery {
    company_id: Uuid,
    channel_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct CompanyPolicyForm {
    company_id: Uuid,
    channel_id: Uuid,
    policy: ExternalReplyHandling,
}

#[derive(Debug, Deserialize)]
struct ChannelPolicyForm {
    company_id: Uuid,
    channel_id: Uuid,
    policy_override: String,
}

/// Only a company manager may read or change reply handling.
///
/// A non-manager gets the company-not-found error rather than a refusal, so an id cannot be probed
/// to learn that a channel exists.
pub(super) async fn authorize_policy_manager(
    companies: &CompanyUseCases,
    user_id: Uuid,
    company_id: Uuid,
) -> AppResult<()> {
    let allowed = companies
        .company_access(user_id, company_id)
        .await?
        .is_some_and(|access| access.membership.manages_company_operations());
    if !allowed {
        return Err(crate::use_cases::company::company_not_found());
    }
    Ok(())
}

/// `"inherit"` and a blank value clear the override; anything else must name a policy.
///
/// Deliberately not [`super::channel::parse_review_override`]'s shape, which maps an unrecognised
/// string to `None` and so turns a typo into "inherit".
pub(super) fn parse_reply_handling_override(
    value: &str,
) -> AppResult<Option<ExternalReplyHandling>> {
    match value.trim() {
        "inherit" | "" => Ok(None),
        value => value.parse().map(Some).map_err(AppError::BadRequest),
    }
}

async fn reply_handling_policy(
    State(handoffs): State<Arc<ThreadHandoffUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    viewer: Viewer,
    Query(query): Query<PolicyQuery>,
) -> AppResult<Html<String>> {
    render_reply_handling_policy(&handoffs, &companies, &channels, &viewer, &query).await
}

async fn render_reply_handling_policy(
    handoffs: &ThreadHandoffUseCases,
    companies: &CompanyUseCases,
    channels: &ChannelUseCases,
    viewer: &Viewer,
    query: &PolicyQuery,
) -> AppResult<Html<String>> {
    authorize_policy_manager(companies, viewer.user_id, query.company_id).await?;
    // Managing the company is not the same as being allowed to read this channel: a restricted
    // channel answers as missing rather than rendering its policy.
    channels
        .get_readable_channel(viewer, query.company_id, query.channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found.".into()))?;
    let policy = handoffs
        .reply_handling_policy(query.company_id, query.channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found.".into()))?;
    Ok(Html(pages::reply_handling_policy_page(
        query.company_id,
        query.channel_id,
        &policy,
    )))
}

async fn update_company_reply_handling(
    State(handoffs): State<Arc<ThreadHandoffUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<CompanyPolicyForm>,
) -> AppResult<Html<String>> {
    save_company_reply_handling(&handoffs, &companies, user.id, &form).await
}

async fn save_company_reply_handling(
    handoffs: &ThreadHandoffUseCases,
    companies: &CompanyUseCases,
    user_id: Uuid,
    form: &CompanyPolicyForm,
) -> AppResult<Html<String>> {
    authorize_policy_manager(companies, user_id, form.company_id).await?;
    handoffs
        .set_company_reply_handling(form.company_id, form.policy)
        .await?;
    Ok(Html(pages::reply_handling_policy_saved(
        form.company_id,
        form.channel_id,
    )))
}

async fn update_channel_reply_handling(
    State(handoffs): State<Arc<ThreadHandoffUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<ChannelPolicyForm>,
) -> AppResult<Html<String>> {
    save_channel_reply_handling(&handoffs, &companies, user.id, &form).await
}

async fn save_channel_reply_handling(
    handoffs: &ThreadHandoffUseCases,
    companies: &CompanyUseCases,
    user_id: Uuid,
    form: &ChannelPolicyForm,
) -> AppResult<Html<String>> {
    authorize_policy_manager(companies, user_id, form.company_id).await?;
    let policy_override = parse_reply_handling_override(&form.policy_override)?;
    handoffs
        .set_channel_reply_handling_override(form.company_id, form.channel_id, policy_override)
        .await?;
    Ok(Html(pages::reply_handling_policy_saved(
        form.company_id,
        form.channel_id,
    )))
}

#[cfg(test)]
#[path = "ui_thread_handoffs_tests.rs"]
mod tests;
