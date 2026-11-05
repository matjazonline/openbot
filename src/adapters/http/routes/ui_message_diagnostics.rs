//! The one place a provider identifier is shown.
//!
//! Every ordinary read of a message goes through a projection that has no provider keys in it at
//! all -- see [`crate::entities::message_view`] -- so a Message-ID or a Slack timestamp cannot
//! reach a page by accident. An operator does sometimes need one: to match a thread against a
//! mail server's logs, or to ask a provider what happened to a delivery. That is what this is.
//!
//! It is authorized exactly like an attachment download, through the same two guards the mailbox
//! opens a thread with, and then scoped a third time by company inside the query itself. A
//! diagnostic pane is not a weaker door onto the same data.

use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    response::Html,
    routing::get,
};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::{http::app_state::AppState, http::pages},
    app_error::{AppError, AppResult},
    entities::user::Viewer,
    use_cases::{channel::ChannelUseCases, company::CompanyUseCases, thread::ThreadUseCases},
};

use super::ui::load_viewable_channel;
use super::ui_attachments::AttachmentQuery;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/ui/threads/{thread_id}/messages/{association_id}/diagnostics",
        get(message_diagnostics),
    )
}

/// GET /ui/threads/{thread_id}/messages/{association_id}/diagnostics (Protected).
///
/// Scoped four times over, deliberately: the viewer must be able to read the channel, the channel
/// must own the thread, the thread must hold the message, and the message's own company must match
/// the one the read is made under. Three of those are guards a caller could not satisfy by
/// guessing an id; the fourth is what makes a guessed id from another tenant a plain "not found".
#[instrument(skip_all)]
async fn message_diagnostics(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((thread_id, association_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<AttachmentQuery>,
) -> AppResult<Html<String>> {
    let (_, channel) = load_viewable_channel(
        &company_use_cases,
        &channel_use_cases,
        &viewer,
        query.company_id,
        query.channel_id,
    )
    .await?;

    let thread = thread_use_cases
        .readable_thread(&viewer, channel.company_id, channel.id, thread_id)
        .await?
        .ok_or_else(missing)?;

    let audit = thread_use_cases
        .get_message_audit(
            &viewer,
            crate::use_cases::thread::ThreadReadScope::new(&channel, thread.id),
            association_id,
        )
        .await?
        .filter(|audit| audit.thread_id == thread.id && audit.channel_id == channel.id)
        .ok_or_else(missing)?;

    Ok(Html(pages::message_diagnostics_pane(&audit)))
}

/// A message that is not this viewer's, does not exist, or is not on the thread they named: the
/// same answer in all three cases, so the endpoint tells nobody which of them it was.
fn missing() -> AppError {
    AppError::NotFound("Message not found".into())
}
