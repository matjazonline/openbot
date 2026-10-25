//! First-class internal-note HTTP boundaries. The ask-agent and start-agent-task actions are gone
//! with the legacy task runtime (p08 B4a): a workflow binding answers a thread now.

use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    response::Html,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    adapters::http::app_state::AppState,
    app_error::{AppError, AppResult},
    application::thread_activity::ThreadActivityReader,
    entities::{
        channel::Channel,
        internal_note::{
            AddInternalNote, InternalNoteProvenance, MAX_INTERNAL_NOTE_BYTES, TombstoneInternalNote,
        },
        participant::PrincipalAccessContext,
        thread::Thread,
        user::Viewer,
    },
    use_cases::{channel::ChannelUseCases, company::CompanyUseCases, thread::ThreadUseCases},
};

use super::ui::{load_channel_thread, load_viewable_channel, render_message_pane};

const NOTE_BODY_LIMIT: usize = MAX_INTERNAL_NOTE_BYTES + 16 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/internal-notes", axum::routing::post(add_note_ui))
        .route(
            "/ui/internal-notes/{note_id}/tombstone",
            axum::routing::post(tombstone_note_ui),
        )
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/threads/{thread_id}/notes",
            axum::routing::post(add_note_api),
        )
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/threads/{thread_id}/notes/{note_id}/tombstone",
            axum::routing::post(tombstone_note_api),
        )
        .layer(DefaultBodyLimit::max(NOTE_BODY_LIMIT))
}

#[derive(Debug, Clone, Deserialize)]
struct InternalNoteForm {
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    command_id: Uuid,
    text_body: String,
    supersedes_note_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
struct TombstoneNoteForm {
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    command_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
struct NoteApiRequest {
    command_id: Uuid,
    text: String,
    supersedes_note_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandApiRequest {
    command_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
struct NoteApiResponse {
    note_id: Uuid,
    message_id: Option<Uuid>,
    active: bool,
}

struct NoteScope {
    company_id: Uuid,
    channel: Channel,
    thread: Thread,
    actor: PrincipalAccessContext,
}

async fn load_scope(
    company_use_cases: &CompanyUseCases,
    channel_use_cases: &ChannelUseCases,
    thread_use_cases: &ThreadUseCases,
    viewer: &Viewer,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
) -> AppResult<NoteScope> {
    let (_, channel) = load_viewable_channel(
        company_use_cases,
        channel_use_cases,
        viewer,
        company_id,
        channel_id,
    )
    .await?;
    let thread = load_channel_thread(thread_use_cases, channel.id, thread_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".into()))?;
    let actor = thread_use_cases
        .principal_access_for_user(company_id, viewer.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found.".into()))?;
    Ok(NoteScope {
        company_id,
        channel,
        thread,
        actor,
    })
}

async fn render_result(
    thread_use_cases: &ThreadUseCases,
    activity_reader: &dyn ThreadActivityReader,
    scope: &NoteScope,
    viewer: &Viewer,
    error: Option<&str>,
) -> AppResult<Html<String>> {
    Ok(Html(
        render_message_pane(
            thread_use_cases,
            activity_reader,
            scope.company_id,
            &scope.channel,
            &scope.thread,
            viewer,
            error,
        )
        .await?,
    ))
}

async fn add_note_ui(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(activity_reader): State<Arc<dyn ThreadActivityReader>>,
    viewer: Viewer,
    Form(form): Form<InternalNoteForm>,
) -> AppResult<Html<String>> {
    let scope = load_scope(
        &company_use_cases,
        &channel_use_cases,
        &thread_use_cases,
        &viewer,
        form.company_id,
        form.channel_id,
        form.thread_id,
    )
    .await?;
    let error = thread_use_cases
        .add_internal_note(
            AddInternalNote {
                company_id: form.company_id,
                channel_id: form.channel_id,
                thread_id: form.thread_id,
                text: form.text_body,
                command_id: form.command_id,
                supersedes_note_id: form.supersedes_note_id,
                provenance: InternalNoteProvenance::HumanUi,
            },
            scope.actor,
        )
        .await
        .err()
        .map(|error| error.to_string());
    render_result(
        &thread_use_cases,
        activity_reader.as_ref(),
        &scope,
        &viewer,
        error.as_deref(),
    )
    .await
}

async fn tombstone_note_ui(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(activity_reader): State<Arc<dyn ThreadActivityReader>>,
    viewer: Viewer,
    Path(note_id): Path<Uuid>,
    Form(form): Form<TombstoneNoteForm>,
) -> AppResult<Html<String>> {
    let scope = load_scope(
        &company_use_cases,
        &channel_use_cases,
        &thread_use_cases,
        &viewer,
        form.company_id,
        form.channel_id,
        form.thread_id,
    )
    .await?;
    let error = thread_use_cases
        .tombstone_internal_note(
            TombstoneInternalNote {
                company_id: form.company_id,
                channel_id: form.channel_id,
                thread_id: form.thread_id,
                note_id,
                command_id: form.command_id,
            },
            scope.actor,
        )
        .await
        .err()
        .map(|error| error.to_string());
    render_result(
        &thread_use_cases,
        activity_reader.as_ref(),
        &scope,
        &viewer,
        error.as_deref(),
    )
    .await
}

async fn add_note_api(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, channel_id, thread_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<NoteApiRequest>,
) -> AppResult<(StatusCode, Json<NoteApiResponse>)> {
    let scope = load_scope(
        &company_use_cases,
        &channel_use_cases,
        &thread_use_cases,
        &viewer,
        company_id,
        channel_id,
        thread_id,
    )
    .await?;
    let message = thread_use_cases
        .add_internal_note(
            AddInternalNote {
                company_id,
                channel_id,
                thread_id,
                text: body.text,
                command_id: body.command_id,
                supersedes_note_id: body.supersedes_note_id,
                provenance: InternalNoteProvenance::Api,
            },
            scope.actor,
        )
        .await?;
    let note = message
        .internal_note
        .ok_or_else(|| AppError::Internal("Created note metadata is missing.".into()))?;
    Ok((
        StatusCode::CREATED,
        Json(NoteApiResponse {
            note_id: note.id,
            message_id: Some(message.canonical_id.as_uuid()),
            active: note.is_active(),
        }),
    ))
}

async fn tombstone_note_api(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, channel_id, thread_id, note_id)): Path<(Uuid, Uuid, Uuid, Uuid)>,
    Json(body): Json<CommandApiRequest>,
) -> AppResult<Json<NoteApiResponse>> {
    let scope = load_scope(
        &company_use_cases,
        &channel_use_cases,
        &thread_use_cases,
        &viewer,
        company_id,
        channel_id,
        thread_id,
    )
    .await?;
    let note = thread_use_cases
        .tombstone_internal_note(
            TombstoneInternalNote {
                company_id,
                channel_id,
                thread_id,
                note_id,
                command_id: body.command_id,
            },
            scope.actor,
        )
        .await?;
    Ok(Json(NoteApiResponse {
        note_id: note.id,
        message_id: None,
        active: note.is_active(),
    }))
}
