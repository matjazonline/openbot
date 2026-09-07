//! First-class internal-note and explicit ask-agent HTTP boundaries.

use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    response::Html,
};
use axum_extra::extract::Form as HtmlForm;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    adapters::http::app_state::AppState,
    app_error::{AppError, AppResult},
    entities::{
        channel::Channel,
        internal_note::{
            AddInternalNote, AskOwnerOutcome, AskOwnerToAct, InternalNoteProvenance,
            MAX_INTERNAL_NOTE_BYTES, StartAgentTask, TombstoneInternalNote,
        },
        participant::PrincipalAccessContext,
        thread::Thread,
        user::Viewer,
    },
    use_cases::{
        agent::AgentUseCases, channel::ChannelUseCases, company::CompanyUseCases,
        thread::ThreadUseCases,
    },
};

use super::ui::{channel_agent, load_channel_thread, load_viewable_channel, render_message_pane};

const NOTE_BODY_LIMIT: usize = MAX_INTERNAL_NOTE_BYTES + 16 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/internal-notes", axum::routing::post(add_note_ui))
        .route(
            "/ui/internal-notes/{note_id}/tombstone",
            axum::routing::post(tombstone_note_ui),
        )
        .route("/ui/internal-notes/ask", axum::routing::post(ask_owner_ui))
        .route("/ui/internal-notes/start", axum::routing::post(start_task_ui))
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/threads/{thread_id}/notes",
            axum::routing::post(add_note_api),
        )
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/threads/{thread_id}/notes/{note_id}/tombstone",
            axum::routing::post(tombstone_note_api),
        )
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/threads/{thread_id}/tasks/{task_id}/ask-owner",
            axum::routing::post(ask_owner_api),
        )
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/threads/{thread_id}/agent-tasks",
            axum::routing::post(start_task_api),
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
struct NoteActionForm {
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    command_id: Uuid,
    task_id: Option<Uuid>,
    expected_ownership_version: Option<u64>,
    #[serde(default)]
    note_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
struct NoteApiRequest {
    command_id: Uuid,
    text: String,
    supersedes_note_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
struct NoteIdsApiRequest {
    command_id: Uuid,
    note_ids: Vec<Uuid>,
    expected_ownership_version: Option<u64>,
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

#[derive(Debug, Clone, Serialize)]
struct AgentActionApiResponse {
    task_id: Uuid,
    outcome: &'static str,
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
    agent_use_cases: &AgentUseCases,
    scope: &NoteScope,
    viewer: &Viewer,
    error: Option<&str>,
) -> AppResult<Html<String>> {
    let agent = channel_agent(agent_use_cases, viewer, &scope.channel).await?;
    Ok(Html(
        render_message_pane(
            thread_use_cases,
            scope.company_id,
            &scope.channel,
            &scope.thread,
            agent.as_ref(),
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
    State(agent_use_cases): State<Arc<AgentUseCases>>,
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
        &agent_use_cases,
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
    State(agent_use_cases): State<Arc<AgentUseCases>>,
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
        &agent_use_cases,
        &scope,
        &viewer,
        error.as_deref(),
    )
    .await
}

async fn ask_owner_ui(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    viewer: Viewer,
    HtmlForm(form): HtmlForm<NoteActionForm>,
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
    let result = match (form.task_id, form.expected_ownership_version) {
        (Some(task_id), Some(expected_ownership_version)) => thread_use_cases
            .ask_owner_to_act(
                AskOwnerToAct {
                    company_id: form.company_id,
                    channel_id: form.channel_id,
                    thread_id: form.thread_id,
                    task_id,
                    expected_ownership_version,
                    note_ids: form.note_ids,
                    command_id: form.command_id,
                },
                scope.actor,
            )
            .await
            .map(|_| ()),
        _ => Err(AppError::BadRequest(
            "The active task changed; refresh and try again.".into(),
        )),
    };
    let error = result.err().map(|error| error.to_string());
    render_result(
        &thread_use_cases,
        &agent_use_cases,
        &scope,
        &viewer,
        error.as_deref(),
    )
    .await
}

async fn start_task_ui(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    viewer: Viewer,
    HtmlForm(form): HtmlForm<NoteActionForm>,
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
        .start_agent_task(
            StartAgentTask {
                company_id: form.company_id,
                channel_id: form.channel_id,
                thread_id: form.thread_id,
                note_ids: form.note_ids,
                command_id: form.command_id,
            },
            scope.actor,
        )
        .await
        .err()
        .map(|error| error.to_string());
    render_result(
        &thread_use_cases,
        &agent_use_cases,
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

async fn ask_owner_api(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, channel_id, thread_id, task_id)): Path<(Uuid, Uuid, Uuid, Uuid)>,
    Json(body): Json<NoteIdsApiRequest>,
) -> AppResult<Json<AgentActionApiResponse>> {
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
    let expected_ownership_version = body
        .expected_ownership_version
        .ok_or_else(|| AppError::BadRequest("expected_ownership_version is required.".into()))?;
    let outcome = thread_use_cases
        .ask_owner_to_act(
            AskOwnerToAct {
                company_id,
                channel_id,
                thread_id,
                task_id,
                expected_ownership_version,
                note_ids: body.note_ids,
                command_id: body.command_id,
            },
            scope.actor,
        )
        .await?;
    let outcome = match outcome {
        AskOwnerOutcome::Queued => "queued",
        AskOwnerOutcome::Requeued => "requeued",
        AskOwnerOutcome::Parked => "parked",
    };
    Ok(Json(AgentActionApiResponse { task_id, outcome }))
}

async fn start_task_api(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, channel_id, thread_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<NoteIdsApiRequest>,
) -> AppResult<(StatusCode, Json<AgentActionApiResponse>)> {
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
    let task = thread_use_cases
        .start_agent_task(
            StartAgentTask {
                company_id,
                channel_id,
                thread_id,
                note_ids: body.note_ids,
                command_id: body.command_id,
            },
            scope.actor,
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(AgentActionApiResponse {
            task_id: task.id,
            outcome: if task.ownership.owner.is_agent() {
                "queued"
            } else {
                "unassigned"
            },
        }),
    ))
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        extract::FromRequest,
        http::{Request, header},
    };

    use super::{HtmlForm, NoteActionForm};

    #[tokio::test]
    async fn note_action_form_accepts_one_or_multiple_selected_notes() {
        let first_note = uuid::Uuid::new_v4();
        let second_note = uuid::Uuid::new_v4();
        let common_fields = format!(
            "company_id={}&channel_id={}&thread_id={}&command_id={}",
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4()
        );

        let one = extract_note_action_form(format!("{common_fields}&note_ids={first_note}")).await;
        assert_eq!(one.note_ids, vec![first_note]);

        let multiple = extract_note_action_form(format!(
            "{common_fields}&note_ids={first_note}&note_ids={second_note}"
        ))
        .await;
        assert_eq!(multiple.note_ids, vec![first_note, second_note]);
    }

    async fn extract_note_action_form(body: String) -> NoteActionForm {
        let request = Request::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();
        HtmlForm::<NoteActionForm>::from_request(request, &())
            .await
            .unwrap()
            .0
    }
}
