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
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    adapters::{http::app_state::AppState, protocols::email::EmailIdentity},
    app_error::{AppError, AppResult},
    entities::{
        attention::BusinessPriority,
        internal_note::{AskOwnerToAct, StartAgentTask},
        response_draft::ResponseDraftId,
        task::TaskOwner,
        thread_handoff::{
            HandoffRunRequest, ThreadHandoff, ThreadHandoffCommand, ThreadHandoffDismiss,
            ThreadHandoffDraft, ThreadHandoffOperation,
        },
        user::Viewer,
        value_objects::EmailAddress,
    },
    use_cases::{
        channel::ChannelUseCases,
        company::CompanyUseCases,
        response_review::{ResponseReviewUseCases, ReviewAction, ReviewCommand},
        thread::{ResponseReviewEditDraft, ThreadUseCases},
        thread_handoff::ThreadHandoffUseCases,
    },
};

use super::attention::read_context;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/companies/{company_id}/thread-handoffs/{handoff_id}",
            post(change_thread_handoff).get(read_thread_handoff),
        )
        .route(
            "/api/companies/{company_id}/thread-handoffs/{handoff_id}/draft",
            post(request_draft),
        )
        .route(
            "/api/companies/{company_id}/thread-handoffs/{handoff_id}/send",
            post(send_draft),
        )
        .route(
            "/api/companies/{company_id}/thread-handoffs/{handoff_id}/send-edited",
            post(send_edited_draft),
        )
        .route(
            "/api/companies/{company_id}/thread-handoffs/{handoff_id}/dismiss",
            post(dismiss),
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

/// Both fences, on every action body, in one place.
///
/// Flattened into each body rather than nested, so the wire form stays one flat object and a
/// client that read a queue item can post its `version` and `generation` straight back.
#[derive(Debug, Deserialize)]
struct Fences {
    command_id: Uuid,
    expected_version: u64,
    expected_generation: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftBody {
    #[serde(flatten)]
    fences: Fences,
    /// The active internal notes the agent should read, exactly as `#ask-agent-form` posts them.
    #[serde(default)]
    note_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SendBody {
    #[serde(flatten)]
    fences: Fences,
    /// The draft version the sender is looking at. Checked against the run's own, never taken
    /// from the client as the thing to publish.
    draft_version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SendEditedBody {
    #[serde(flatten)]
    fences: Fences,
    draft_version: u32,
    subject: String,
    body: String,
    recipient_to: String,
    #[serde(default)]
    recipients_cc: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DismissBody {
    #[serde(flatten)]
    fences: Fences,
}

/// Load the handoff this action names and check both fences the body states.
///
/// **Never refreshes them.** A route that reloaded the current version and retried with it would
/// defeat the entire fencing design: the point of carrying a version and a generation is that an
/// action written against a customer message which has since been superseded is refused rather
/// than applied to whatever arrived after it. The conflict is returned verbatim from here and from
/// persistence, so the sentence the user reads names the generation the thread is actually on.
///
/// This check is the early, friendly one. The transactional fence is inside persistence -- for
/// **Generate draft** and **Dismiss** in the same statement that writes, and for **Send** in the
/// review command's own draft-version lock -- so a race that slips between this read and that
/// write is still refused there.
async fn fenced_handoff(
    handoffs: &ThreadHandoffUseCases,
    company_id: Uuid,
    handoff_id: Uuid,
    visible_channel_ids: &[Uuid],
    fences: &Fences,
) -> AppResult<ThreadHandoff> {
    let handoff = handoffs
        .get_thread_handoff(company_id, handoff_id, visible_channel_ids)
        .await?
        // The same sentence for another company's id, a channel the caller cannot view, and an id
        // that never existed: never a message that distinguishes them.
        .ok_or_else(|| AppError::NotFound("Thread handoff not found.".into()))?;
    if handoff.generation != fences.expected_generation {
        return Err(AppError::Conflict(format!(
            "This thread received a newer reply; the current handoff generation is {}. \
             Refresh and try again.",
            handoff.generation
        )));
    }
    if handoff.version != fences.expected_version {
        return Err(AppError::Conflict(format!(
            "Thread handoff changed from version {} to {}; refresh and try again.",
            fences.expected_version, handoff.version
        )));
    }
    Ok(handoff)
}

/// The handoff as it stands after an action, which is what every action route answers with.
async fn handoff_state(
    handoffs: &ThreadHandoffUseCases,
    company_id: Uuid,
    handoff_id: Uuid,
    visible_channel_ids: &[Uuid],
) -> AppResult<Json<serde_json::Value>> {
    let handoff = handoffs
        .get_thread_handoff(company_id, handoff_id, visible_channel_ids)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread handoff not found.".into()))?;
    Ok(Json(serde_json::json!({
        "version": handoff.version,
        "generation": handoff.generation,
        "state": handoff.state.as_str(),
    })))
}

/// POST …/draft - start the drafting run for this generation.
///
/// The note itself is not here: writing one is `AddInternalNote` through the existing
/// `/ui/internal-notes` route, and this only selects which active notes the run should read.
async fn request_draft(
    State(handoffs): State<Arc<ThreadHandoffUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, handoff_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<DraftBody>,
) -> AppResult<Json<serde_json::Value>> {
    let context = read_context(&companies, &channels, &threads, &viewer, company_id).await?;
    let handoff = fenced_handoff(
        &handoffs,
        company_id,
        handoff_id,
        &context.visible_channel_ids,
        &body.fences,
    )
    .await?;
    // The note use cases take the whole access context, not just the principal: they check the
    // actor's membership against the thread themselves.
    let actor = threads
        .principal_access_for_user(company_id, viewer.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread handoff not found.".into()))?;
    let request = HandoffRunRequest {
        handoff_id,
        generation: body.fences.expected_generation,
        expected_version: body.fences.expected_version,
    };
    // The same choice `internal_note_actions` makes in the mailbox: an agent-owned task in flight
    // is woken, and a thread with no active work starts one. Neither path is new machinery.
    let work = threads
        .thread_work_summary(&[handoff.thread_id])
        .await?
        .remove(&handoff.thread_id);
    match work {
        Some(work) if matches!(work.ownership.owner, TaskOwner::Agent(_)) => {
            threads
                .ask_owner_to_act(
                    AskOwnerToAct {
                        company_id,
                        channel_id: handoff.channel_id,
                        thread_id: handoff.thread_id,
                        task_id: work.task_id,
                        expected_ownership_version: work.ownership.version,
                        note_ids: body.note_ids,
                        command_id: body.fences.command_id,
                        handoff: Some(request),
                    },
                    actor,
                )
                .await?;
        }
        None => {
            threads
                .start_agent_task(
                    StartAgentTask {
                        company_id,
                        channel_id: handoff.channel_id,
                        thread_id: handoff.thread_id,
                        note_ids: body.note_ids,
                        command_id: body.fences.command_id,
                        handoff: Some(request),
                    },
                    actor,
                )
                .await?;
        }
        Some(_) => {
            return Err(AppError::Conflict(
                "This thread's task is owned by a person; take it over or ask its owner instead."
                    .into(),
            ));
        }
    }
    handoff_state(
        &handoffs,
        company_id,
        handoff_id,
        &context.visible_channel_ids,
    )
    .await
}

/// POST …/send - publish the drafted reply through the existing review machinery.
#[expect(
    clippy::too_many_arguments,
    reason = "Axum handlers receive request state and extractors as parameters"
)]
async fn send_draft(
    State(handoffs): State<Arc<ThreadHandoffUseCases>>,
    State(reviews): State<Arc<ResponseReviewUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, handoff_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SendBody>,
) -> AppResult<Json<serde_json::Value>> {
    let context = read_context(&companies, &channels, &threads, &viewer, company_id).await?;
    fenced_handoff(
        &handoffs,
        company_id,
        handoff_id,
        &context.visible_channel_ids,
        &body.fences,
    )
    .await?;
    let draft = handoff_draft(
        &handoffs,
        company_id,
        handoff_id,
        body.fences.expected_generation,
        &context.visible_channel_ids,
        body.draft_version,
    )
    .await?;
    let approved = reviews
        .execute(ReviewCommand {
            company_id,
            draft_id: ResponseDraftId::new(draft.draft_id),
            expected_draft_version: draft.draft_version,
            command_id: body.fences.command_id,
            actor_principal_id: context.principal_id,
            action: ReviewAction::Approve { rationale: None },
        })
        .await;
    expired_draft_returns_the_reply(&handoffs, company_id, draft.task_id, approved).await?;
    handoff_state(
        &handoffs,
        company_id,
        handoff_id,
        &context.visible_channel_ids,
    )
    .await
}

/// POST …/send-edited - edit the drafted reply, then send the edited version.
///
/// **Two transactions, deliberately.** This is the existing `Edit` command followed by the
/// existing `Approve`, which is the same pair the review UI performs across two requests; a
/// single-transaction edit-and-publish would have to duplicate `create_review_draft_on`'s
/// validation. If the edit commits and the approve fails, the draft is at version *n+1* pending
/// review and the handoff is still `draft_ready`, so the banner's Send simply targets the new
/// version. Both command ids are derived from the request's one, so a retry replays both steps
/// rather than creating a third draft version.
#[expect(
    clippy::too_many_arguments,
    reason = "Axum handlers receive request state and extractors as parameters"
)]
async fn send_edited_draft(
    State(handoffs): State<Arc<ThreadHandoffUseCases>>,
    State(reviews): State<Arc<ResponseReviewUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, handoff_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SendEditedBody>,
) -> AppResult<Json<serde_json::Value>> {
    let context = read_context(&companies, &channels, &threads, &viewer, company_id).await?;
    fenced_handoff(
        &handoffs,
        company_id,
        handoff_id,
        &context.visible_channel_ids,
        &body.fences,
    )
    .await?;
    let draft = handoff_draft(
        &handoffs,
        company_id,
        handoff_id,
        body.fences.expected_generation,
        &context.visible_channel_ids,
        body.draft_version,
    )
    .await?;
    let draft_id = ResponseDraftId::new(draft.draft_id);
    let actor = context.principal_id;
    let detail = reviews
        .get(company_id, draft_id, actor)
        .await?
        .filter(|detail| detail.draft.version == draft.draft_version)
        .ok_or_else(|| {
            AppError::Conflict("The draft version is stale; refresh and try again.".into())
        })?;
    let publication = reviews
        .publication(company_id, draft_id, draft.draft_version, actor)
        .await?
        .ok_or_else(|| AppError::Conflict("The draft is no longer editable.".into()))?;
    let replacement = threads
        .prepare_response_review_edit(ResponseReviewEditDraft {
            draft_id,
            current_version: draft.draft_version,
            company_id,
            channel_id: detail.draft.channel_id,
            thread_id: detail.draft.thread_id,
            task_id: detail.draft.task_id,
            // Carried forward untouched: this is the task-transfer event id, not a handoff
            // generation, and copying it across an edit is what the review path already does.
            source_handoff_generation: detail.draft.source_handoff_generation,
            actor_principal_id: actor,
            subject: &body.subject,
            text_body: &body.body,
            recipient_to: parse_recipient(&body.recipient_to)?,
            recipients_cc: body
                .recipients_cc
                .iter()
                .map(|value| parse_recipient(value))
                .collect::<AppResult<Vec<_>>>()?,
            evidence: detail
                .evidence
                .into_iter()
                .map(|item| item.evidence)
                .collect(),
            current_publication: &publication,
        })
        .await?;
    let edited_version = replacement.version;
    let edit = reviews
        .execute(ReviewCommand {
            company_id,
            draft_id,
            expected_draft_version: draft.draft_version,
            command_id: derived_command_id(body.fences.command_id, b"thread-handoff-edit"),
            actor_principal_id: actor,
            action: ReviewAction::Edit {
                replacement: Box::new(replacement),
            },
        })
        .await;
    expired_draft_returns_the_reply(&handoffs, company_id, draft.task_id, edit).await?;
    let approved = reviews
        .execute(ReviewCommand {
            company_id,
            draft_id,
            expected_draft_version: edited_version,
            command_id: derived_command_id(body.fences.command_id, b"thread-handoff-send"),
            actor_principal_id: actor,
            action: ReviewAction::Approve { rationale: None },
        })
        .await;
    expired_draft_returns_the_reply(&handoffs, company_id, draft.task_id, approved).await?;
    handoff_state(
        &handoffs,
        company_id,
        handoff_id,
        &context.visible_channel_ids,
    )
    .await
}

/// POST …/dismiss - close this generation without answering it.
async fn dismiss(
    State(handoffs): State<Arc<ThreadHandoffUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, handoff_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<DismissBody>,
) -> AppResult<Json<serde_json::Value>> {
    let context = read_context(&companies, &channels, &threads, &viewer, company_id).await?;
    let version = handoffs
        .dismiss_thread_handoff(ThreadHandoffDismiss {
            company_id,
            handoff_id,
            command_id: body.fences.command_id,
            expected_version: body.fences.expected_version,
            expected_generation: body.fences.expected_generation,
            actor_principal_id: context.principal_id,
            visible_channel_ids: context.visible_channel_ids,
        })
        .await?;
    Ok(Json(serde_json::json!({ "version": version })))
}

/// The draft this generation's run produced, refusing a body that names a different version.
async fn handoff_draft(
    handoffs: &ThreadHandoffUseCases,
    company_id: Uuid,
    handoff_id: Uuid,
    generation: Uuid,
    visible_channel_ids: &[Uuid],
    stated_version: u32,
) -> AppResult<ThreadHandoffDraft> {
    let draft = handoffs
        .thread_handoff_draft(company_id, handoff_id, generation, visible_channel_ids)
        .await?
        .ok_or_else(|| {
            AppError::Conflict("This reply has no drafted answer to send.".to_string())
        })?;
    if draft.draft_version != stated_version {
        return Err(AppError::Conflict(format!(
            "The draft moved from version {stated_version} to {}; refresh and try again.",
            draft.draft_version
        )));
    }
    Ok(draft)
}

/// Turn the review machinery's expiry conflict into a `draft_failed` transition.
///
/// Without this an expired review would leave a `draft_ready` handoff whose only button can never
/// succeed. Every other error is returned exactly as it came, including the conflicts that name a
/// version or a generation.
async fn expired_draft_returns_the_reply<T>(
    handoffs: &ThreadHandoffUseCases,
    company_id: Uuid,
    task_id: Uuid,
    result: AppResult<T>,
) -> AppResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(AppError::Conflict(message)) if message == "This review has expired." => {
            handoffs
                .expire_thread_handoff_draft(
                    company_id,
                    task_id,
                    "the drafted reply expired before it was sent",
                )
                .await?;
            Err(AppError::Conflict(
                "The drafted reply expired before it was sent; generate a new draft.".into(),
            ))
        }
        Err(error) => Err(error),
    }
}

/// Two stable command ids from the one the client sent, so a retry of **Edit and send** replays
/// both steps instead of writing a third draft version.
///
/// A digest rather than a UUID namespace scheme: the only properties needed are determinism and
/// not colliding with the caller's own id, and `uuid` is not built with `v5` here.
fn derived_command_id(command_id: Uuid, tag: &[u8]) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(command_id.as_bytes());
    digest.update(tag);
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.finalize()[..16]);
    Uuid::from_bytes(bytes)
}

fn parse_recipient(value: &str) -> AppResult<EmailAddress> {
    let identity = EmailIdentity::parse(EmailAddress::from(value.trim()))
        .map_err(|_| AppError::BadRequest(format!("Invalid email address: {value}")))?;
    Ok(EmailAddress::from(identity.subject().as_str()))
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
