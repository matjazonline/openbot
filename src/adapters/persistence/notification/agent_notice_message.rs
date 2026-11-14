use super::route::Route;
use super::*;
use crate::application::ports::thread::{ProtocolExtension, qualified_email_identity};
use crate::entities::email_message::EmailMessageMetadata;
use crate::entities::transport::{
    ChannelBindingId, ExternalMessageKey, ExternalThreadKey, InboundSource, RecipientRole,
};
use crate::entities::value_objects::MessageId;
use crate::transport::{
    AddressedIdentity, BoundedVec, CanonicalContent, InboundAdmissionIntent, InboundCommitRequest,
    InboundEnvelope, InboundOrigin, IngressDirectives, IngressPolicyFacts, PipelineStep,
    ReplyDelivery, ThreadAssociation, ThreadTarget,
};

pub(super) async fn commit_on(
    tx: &mut Transaction<'_, Postgres>,
    event: &NotificationEvent,
    source: &NoticeSource,
    route: &Route,
    key: &ExternalMessageKey,
) -> AppResult<()> {
    let body: String=sqlx::query_scalar("SELECT format('A workflow needs action.\nReason: %s\nStep: %s\nOpen: /ui/workflow-decisions/escalations/%s',reason_code,COALESCE(step_id,'unavailable'),$3::uuid) \
        FROM workflow_escalation_cases WHERE company_id=$1 AND id=$2")
        .bind(event.company_id).bind(source.case_id).bind(source.decision_id).fetch_one(&mut **tx).await?;
    let request = request(event, route, body, key)?;
    // Canonical ingress retains its existing provider/thread key and dedup order.
    let stored = Box::pin(crate::adapters::persistence::thread::commit_platform_on(
        tx, &request,
    ))
    .await?;
    let [intent] = stored.intent_ids.as_slice() else {
        return Err(AppError::Internal(
            "An agent notice must commit exactly one intent".into(),
        ));
    };
    sqlx::query("INSERT INTO workflow_escalation_agent_receipts(company_id,notice_id,event_id,principal_id, \
        agent_id,desk_id,binding_id,binding_revision,message_id,intent_id) \
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT(company_id,notice_id) DO NOTHING")
        .bind(event.company_id).bind(event.source_id).bind(event.id.as_uuid()).bind(source.principal_id)
        .bind(route.agent_id).bind(route.desk_id).bind(route.binding_id).bind(route.binding_revision)
        .bind(stored.message_id.as_uuid()).bind(intent).execute(&mut **tx).await?;
    Ok(())
}

fn request(
    event: &NotificationEvent,
    route: &Route,
    body: String,
    key: &ExternalMessageKey,
) -> AppResult<InboundCommitRequest> {
    let endpoint = qualified_email_identity(route.address.to_string())?;
    let binding = ChannelBindingId::new(route.transport_binding_id);
    Ok(InboundCommitRequest {
        company_id: event.company_id,
        envelope: InboundEnvelope {
            source: InboundSource {
                binding_id: binding,
                event_key: None,
                message_key: key.clone(),
                thread_key: ExternalThreadKey::parse(key.as_str())
                    .map_err(|e| AppError::Internal(e.to_string()))?,
            },
            // DTO requires a transport identity, but the PRIVATE platform writer
            // ignores it and resolves no sender/recipient account or observed handle.
            author: endpoint.clone(),
            addressed: BoundedVec::parse(
                "notice recipients",
                vec![AddressedIdentity::new(RecipientRole::To, endpoint)],
            )?,
            content: CanonicalContent::parse("Workflow escalation needs action", body)?,
            attachments: BoundedVec::empty(),
            reply_candidates: Default::default(),
            directives: IngressDirectives::default(),
            policy: IngressPolicyFacts::TrustedApplication,
            correlation_id: event.id.as_uuid().into(),
            extension: ProtocolExtension::email(EmailMessageMetadata::new(MessageId::from(
                key.as_str(),
            ))),
        },
        claimed_event: None,
        associations: BoundedVec::parse(
            "notice desk",
            vec![ThreadAssociation {
                channel_id: route.desk_id,
                binding_id: binding,
                target: ThreadTarget::Create {
                    subject: "Workflow escalation needs action".into(),
                },
                role: RecipientRole::To,
                step: PipelineStep::only(),
                principals: BoundedVec::empty(),
            }],
        )?,
        intents: BoundedVec::parse(
            "notice intent",
            vec![InboundAdmissionIntent {
                channel_id: route.desk_id,
                role: RecipientRole::To,
                mentioned: true,
                reply_to: None,
                settings_revision: route.settings_revision,
            }],
        )?,
        admissible: true,
        origin: InboundOrigin::Application,
        deliveries: vec![],
        reply_delivery: ReplyDelivery::InAppOnly,
    })
}
