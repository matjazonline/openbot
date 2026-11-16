//! Only a live, exact projector claim may admit a private system notice to an agent.
use super::*;
use crate::entities::{notification::NotificationEventId, transport::ExternalMessageKey};
use crate::transport::{ExecutionLease, InboundOrigin, RecipientRole};

pub(in crate::adapters::persistence) struct AgentNoticeAuthority<'a> {
    pub fence: &'a ExecutionLease<NotificationEventId>,
    pub notice_id: Uuid,
    pub principal_id: Uuid,
    pub agent_id: Uuid,
    pub transport_binding_id: Uuid,
    pub binding_id: Uuid,
    pub binding_revision: i64,
    pub settings_revision: i64,
    pub key: &'a ExternalMessageKey,
    pub recipient: &'a QualifiedIdentity,
}

pub(in crate::adapters::persistence) async fn commit_agent_notice_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    authority: &AgentNoticeAuthority<'_>,
) -> AppResult<InboundCommitOutcome> {
    validate_request(request, authority)?;
    // The caller already holds principal/accounts/company/source/event in that order.
    // Check the real source and live fence before provider keys or canonical effects.
    let allowed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM notification_events AS event \
         JOIN workflow_escalation_notices AS notice ON notice.company_id=event.company_id \
           AND notice.event_id=event.id AND notice.id=event.source_id \
         CROSS JOIN LATERAL workflow_escalation_agent_route(notice.company_id,notice.principal_id) AS route \
         WHERE event.company_id=$1 AND event.id=$2 AND event.execution_id=$3 AND event.owner_worker_id=$4 \
           AND event.status='processing' AND event.lock_expires_at>clock_timestamp() \
           AND notice.id=$5 AND notice.principal_id=$6 AND notice.principal_kind='agent' \
           AND workflow_escalation_notice_active(notice.company_id,notice.id) \
           AND workflow_escalation_agent_eligible(notice.company_id,notice.id) \
           AND NOT EXISTS(SELECT 1 FROM workflow_escalation_recipient_failures AS failure \
               WHERE failure.company_id=notice.company_id AND failure.notice_id=notice.id) \
           AND (route.agent_id,route.transport_binding_id,route.binding_id,route.binding_revision,route.settings_revision) \
               =($7,$8,$9,$10,$11))",
    ).bind(request.company_id).bind(authority.fence.row.as_uuid())
        .bind(authority.fence.execution.as_uuid()).bind(authority.fence.owner.as_uuid())
        .bind(authority.notice_id).bind(authority.principal_id).bind(authority.agent_id)
        .bind(authority.transport_binding_id).bind(authority.binding_id)
        .bind(authority.binding_revision).bind(authority.settings_revision)
        .fetch_one(&mut **tx).await?;
    if !allowed {
        return Err(refused());
    }
    commit_as_on(tx, request, InboundAuthor::AgentNotice).await
}

fn refused() -> AppError {
    AppError::Superseded("Private agent notice authority is no longer current".into())
}

fn validate_request(
    request: &InboundCommitRequest,
    authority: &AgentNoticeAuthority<'_>,
) -> AppResult<()> {
    let [association] = &request.associations[..] else {
        return Err(refused());
    };
    let [intent] = &request.intents[..] else {
        return Err(refused());
    };
    let [recipient] = &request.envelope.addressed[..] else {
        return Err(refused());
    };
    let target = AddressTarget::Principal(PrincipalId::new(authority.principal_id));
    let metadata = request
        .envelope
        .extension
        .email_metadata()
        .ok_or_else(refused)?;
    if request.origin != InboundOrigin::Application
        || !request.admissible
        || request.reply_delivery != ReplyDelivery::InAppOnly
        || request.claimed_event.is_some()
        || !request.deliveries.is_empty()
        || !request.envelope.attachments.is_empty()
        || request.envelope.directives != Default::default()
        || association.address_target != target
        || intent.target != target
        || association.binding_id.as_uuid() != authority.transport_binding_id
        || request.envelope.source.binding_id != association.binding_id
        || request.envelope.source.message_key != *authority.key
        || metadata.rfc_message_id.as_str() != authority.key.as_str()
        || metadata.in_reply_to.is_some()
        || !metadata.references.is_empty()
        || recipient.role != RecipientRole::To
        || recipient.identity != *authority.recipient
        || request.envelope.author != *authority.recipient
        || association.role != RecipientRole::To
        || !association.principals.is_empty()
        || !matches!(association.target, ThreadTarget::Create { .. })
        || intent.role != RecipientRole::To
        || !intent.mentioned
        || intent.reply_to.is_some()
        || intent.settings_revision != authority.settings_revision
    {
        return Err(refused());
    }
    Ok(())
}
