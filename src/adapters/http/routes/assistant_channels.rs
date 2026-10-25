//! "Create assistant channel" (p08 D2.4): an owner or admin gives an existing agent a team
//! channel bound to a reply-mode workflow, through the same provisioning as the Agent Builder's
//! `create_agent_channel` tool. The caller is the requester and the binding's run-as.
use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::post,
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    application::workflow::{
        WorkflowActor,
        provisioning::{
            AssistantChannelRequest, AssistantChannels, ProvisioningRefusal, ReplyMode,
        },
    },
    domain::workflow::CompanyId,
    use_cases::agent::AgentUseCases,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/companies/{company_id}/assistant-channels",
            post(create_assistant_channel_json),
        )
        .route(
            "/ui/agents/{agent_id}/assistant-channel",
            post(ui_create_assistant_channel),
        )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantChannelBody {
    pub agent_slug: String,
    pub channel_slug: String,
    pub channel_name: String,
    pub reply_mode: ReplyMode,
    pub command_key: String,
}

/// 201 with the created agent, channel, workflow and binding (or the stored result of a replay).
pub async fn create_assistant_channel_json(
    State(channels): State<Arc<dyn AssistantChannels>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Json(body): Json<AssistantChannelBody>,
) -> AppResult<impl IntoResponse> {
    let created = create(
        channels.as_ref(),
        &user,
        company_id,
        AssistantChannelRequest {
            agent_slug: body.agent_slug,
            channel_slug: body.channel_slug,
            channel_name: body.channel_name,
            reply_mode: body.reply_mode,
            command_key: body.command_key,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Debug, Deserialize)]
pub struct CompanyQuery {
    pub company_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct UiAssistantChannelForm {
    pub channel_slug: String,
    pub channel_name: String,
    pub reply_mode: ReplyMode,
    pub command_key: Option<String>,
}

/// The agent page's card, re-rendered with what happened; a refusal is shown on the card.
pub async fn ui_create_assistant_channel(
    State(channels): State<Arc<dyn AssistantChannels>>,
    State(agents): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path(agent_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    Form(form): Form<UiAssistantChannelForm>,
) -> AppResult<Html<String>> {
    let agent = agents
        .get_company_agent(user.id, query.company_id, agent_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Agent not found".into()))?;
    let created = create(
        channels.as_ref(),
        &user,
        query.company_id,
        AssistantChannelRequest {
            agent_slug: agent.slug.clone(),
            channel_slug: form.channel_slug,
            channel_name: form.channel_name,
            reply_mode: form.reply_mode,
            command_key: crate::adapters::http::routes::schedule::run_now_key(form.command_key),
        },
    )
    .await;
    let card = match created {
        Ok(created) => pages::assistant_channel_card(
            query.company_id,
            &agent,
            Some(pages::AssistantChannelOutcome::Created {
                channel: created["channel"]["slug"].as_str().unwrap_or_default(),
                address: created["channel"]["address"].as_str().unwrap_or_default(),
            }),
        ),
        Err(AppError::BadRequest(message) | AppError::Conflict(message)) => {
            pages::assistant_channel_card(
                query.company_id,
                &agent,
                Some(pages::AssistantChannelOutcome::Refused(&message)),
            )
        }
        Err(error) => return Err(error),
    };
    Ok(Html(card))
}

/// The command, with refusals as the route's errors: forbidden is not found (the route's
/// convention for what the caller may not manage), a taken slug is 409, invalid input is 400.
async fn create(
    channels: &dyn AssistantChannels,
    user: &AuthenticatedUser,
    company_id: Uuid,
    request: AssistantChannelRequest,
) -> AppResult<Value> {
    let caller = WorkflowActor::authenticated(user.id)?;
    match channels
        .create_assistant_channel(CompanyId::new(company_id), caller, request)
        .await?
    {
        Ok(created) => Ok(created),
        Err(ProvisioningRefusal::Forbidden) => Err(AppError::NotFound("Company not found".into())),
        Err(ProvisioningRefusal::Conflict(message)) => Err(AppError::Conflict(message)),
        Err(ProvisioningRefusal::Invalid(message)) => Err(AppError::BadRequest(message)),
    }
}

#[cfg(test)]
#[path = "assistant_channels_tests.rs"]
mod tests;
