//! Thin settings adapters: the existing application port owns authorization and both CAS checks.
use super::*;
use crate::application::workflow::RelatedChannelId;
use crate::application::workflow::channel_reply::{
    ChannelReplyContract, ChannelReplySettings, ConfigureChannelReply, ConfigureResponder,
    ReplyApprovalPolicy, ReplySettingsRevision,
};
use crate::domain::workflow::{BindingStateRevision, WorkflowBindingId};
use axum::extract::{Path, State};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResponseBody {
    pub expected_revision: Option<u64>,
    pub responder: Option<ResponderBody>,
    pub approval_policy: Option<ReplyApprovalPolicy>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResponderBody {
    pub binding_id: Uuid,
    pub expected_binding_state_revision: u64,
    pub contract: ChannelReplyContract,
}

pub(super) fn settings_json(state: Option<&ChannelReplySettings>) -> Value {
    match state {
        None => Value::Null,
        Some(state) => json!({
            "revision":state.revision.get(),
            "responder":state.responder.as_ref().map(|selected|json!({
                "binding_id":selected.binding.as_uuid(),
                "configuration_revision":selected.configuration_revision.get(),
                "contract":selected.contract,
            })),
            "approval_policy":state.approval_policy,
        }),
    }
}

pub(super) async fn read(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, channel)): Path<(Uuid, Uuid)>,
) -> WorkflowResult<Json<Value>> {
    let state = engine
        .channel_replies()
        .channel_reply_settings(
            company(company_id),
            RelatedChannelId::new(channel),
            actor(&user)?,
        )
        .await?;
    Ok(ok(settings_json(state.as_ref())))
}

pub(super) async fn configure(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, channel)): Path<(Uuid, Uuid)>,
    Json(body): Json<ResponseBody>,
) -> WorkflowResult<Json<Value>> {
    let state = save(&engine, company_id, channel, actor(&user)?, body).await?;
    Ok(ok(settings_json(Some(&state))))
}

pub(super) async fn save(
    engine: &Engine,
    company_id: Uuid,
    channel: Uuid,
    actor: WorkflowActor,
    body: ResponseBody,
) -> AppResult<ChannelReplySettings> {
    let invalid = || AppError::BadRequest("Invalid response settings revision".into());
    let expected = body
        .expected_revision
        .map(|value| ReplySettingsRevision::try_from(value).map_err(|_| invalid()))
        .transpose()?;
    let responder = body
        .responder
        .map(|selected| {
            Ok::<_, AppError>(ConfigureResponder {
                binding: WorkflowBindingId::new(selected.binding_id),
                expected_binding_revision: BindingStateRevision::new(
                    selected.expected_binding_state_revision,
                )
                .ok_or_else(invalid)?,
                contract: selected.contract,
            })
        })
        .transpose()?;
    engine
        .channel_replies()
        .configure_channel_reply(ConfigureChannelReply {
            company: company(company_id),
            channel: RelatedChannelId::new(channel),
            actor,
            expected,
            responder,
            approval_policy: body.approval_policy,
        })
        .await
}
