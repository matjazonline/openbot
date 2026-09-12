//! The four thread-handoff responsibility commands, and the JSON read of one handoff.
//!
//! Not to be confused with [`super::attention`]'s `manual_handoffs` routes, nor with
//! [`super::ui_thread_handoffs`], which is the reply-handling *settings* page. The HTML surface
//! for these commands is Phase 5's: the attention queue links into the thread, and the thread is
//! where the banner with the buttons lives, so the version and the generation come from one place.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    adapters::http::app_state::AppState,
    app_error::{AppError, AppResult},
    entities::{
        attention::BusinessPriority,
        thread_handoff::{ThreadHandoff, ThreadHandoffCommand, ThreadHandoffOperation},
        user::Viewer,
    },
    use_cases::{
        channel::ChannelUseCases, company::CompanyUseCases, thread::ThreadUseCases,
        thread_handoff::ThreadHandoffUseCases,
    },
};

use super::attention::read_context;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/companies/{company_id}/thread-handoffs/{handoff_id}",
        post(change_thread_handoff).get(read_thread_handoff),
    )
}

/// One command, exactly as the queue item a client read gives it: both fences, the operation, and
/// the whole attribute pair.
#[derive(Debug, Deserialize)]
struct CommandBody {
    command_id: Uuid,
    expected_version: u64,
    expected_generation: Uuid,
    /// `{"kind": "claim"}` / `{"kind": "reassign", "to": "<uuid>"}`. An unrecognised kind is a
    /// deserialization error rather than a silent default -- deliberately not
    /// [`super::channel::parse_review_override`]'s shape, which turns a typo into "inherit".
    operation: ThreadHandoffOperation,
    priority: BusinessPriority,
    due_at: Option<DateTime<Utc>>,
}

async fn change_thread_handoff(
    State(handoffs): State<Arc<ThreadHandoffUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, handoff_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CommandBody>,
) -> AppResult<Json<serde_json::Value>> {
    let context = read_context(&companies, &channels, &threads, &viewer, company_id).await?;
    let version = handoffs
        .change_thread_handoff(ThreadHandoffCommand {
            company_id,
            handoff_id,
            command_id: body.command_id,
            expected_version: body.expected_version,
            expected_generation: body.expected_generation,
            operation: body.operation,
            priority: body.priority,
            due_at: body.due_at,
            actor_principal_id: context.principal_id,
            visible_channel_ids: context.visible_channel_ids,
        })
        .await?;
    Ok(Json(serde_json::json!({ "version": version })))
}

async fn read_thread_handoff(
    State(handoffs): State<Arc<ThreadHandoffUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, handoff_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<serde_json::Value>> {
    let context = read_context(&companies, &channels, &threads, &viewer, company_id).await?;
    let handoff = handoffs
        .get_thread_handoff(company_id, handoff_id, &context.visible_channel_ids)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread handoff not found.".into()))?;
    Ok(Json(handoff_json(&handoff)))
}

/// A handoff as this route answers it.
///
/// Built by hand because [`ThreadHandoff`] is deliberately not `Serialize`: nothing about a
/// handoff may reach a durable task payload, where a snapshotted generation could not notice that
/// a newer customer message had replaced it. Deriving it here would remove that guard.
fn handoff_json(handoff: &ThreadHandoff) -> serde_json::Value {
    serde_json::json!({
        "id": handoff.id,
        "company_id": handoff.company_id,
        "channel_id": handoff.channel_id,
        "thread_id": handoff.thread_id,
        "generation": handoff.generation,
        "state": handoff.state.as_str(),
        "source_message_id": handoff.source_message_id.as_uuid(),
        "responsible_principal_id": handoff.responsible_principal_id,
        "priority": handoff.priority.as_str(),
        "due_at": handoff.due_at,
        "version": handoff.version,
        "generation_opened_at": handoff.generation_opened_at,
        "created_at": handoff.created_at,
        "updated_at": handoff.updated_at,
    })
}

#[cfg(test)]
#[path = "thread_handoffs_tests.rs"]
mod tests;
