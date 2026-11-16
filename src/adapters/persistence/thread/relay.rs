//! Authenticate a relay from its frozen source, both before routing and under commit gates.
use super::*;
use crate::application::transport::ports::{AuthenticatedRelaySource, InternalRelayMail};
use crate::entities::address::AddressTarget;
use crate::entities::transport::PrincipalId;

pub(super) struct RelayClaim<'a> {
    pub proof: crate::transport::InternalRelayProof,
    pub target: AddressTarget,
    pub from: &'a str,
    pub to: &'a str,
    pub message: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
    pub in_reply_to: Option<&'a str>,
    pub references: &'a [crate::entities::value_objects::MessageId],
}

impl<'a> RelayClaim<'a> {
    pub fn from_mail(mail: &'a InternalRelayMail<'_>) -> Self {
        Self {
            proof: mail.proof,
            target: mail.source_target,
            from: mail.from.as_str(),
            to: mail.recipient_to.as_str(),
            message: mail.message_id.as_str(),
            subject: mail.subject,
            body: mail.body_text,
            in_reply_to: mail.in_reply_to.map(|value| value.as_str()),
            references: mail.references,
        }
    }

    pub fn from_request(
        request: &'a crate::transport::InboundCommitRequest,
    ) -> AppResult<Option<Self>> {
        let Some(proof) = request.origin.relay_proof() else {
            return Ok(None);
        };
        let metadata = request
            .envelope
            .extension
            .email_metadata()
            .ok_or_else(refused)?;
        let [recipient] = &request.envelope.addressed[..] else {
            return Err(refused());
        };
        Ok(Some(Self {
            proof,
            target: request
                .envelope
                .directives
                .source_target
                .ok_or_else(refused)?,
            from: request.envelope.author.subject().as_str(),
            to: recipient.identity.subject().as_str(),
            message: metadata.rfc_message_id.as_str(),
            subject: request.envelope.content.subject(),
            body: metadata.raw_text_body.as_deref().ok_or_else(refused)?,
            in_reply_to: metadata.in_reply_to.as_ref().map(|value| value.as_str()),
            references: &metadata.references,
        }))
    }
}

#[derive(sqlx::FromRow)]
struct SourceRow {
    sender_principal_id: Option<Uuid>,
}

pub(super) async fn authenticate_on(
    db: &mut sqlx::PgConnection,
    claim: &RelayClaim<'_>,
) -> AppResult<Option<AuthenticatedRelaySource>> {
    let references: Vec<&str> = claim
        .references
        .iter()
        .map(|value| value.as_str())
        .collect();
    let row = sqlx::query_as::<_, SourceRow>(
        "SELECT sender_principal_id FROM internal_relay_source($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    ).bind(claim.proof.company_id()).bind(claim.proof.delivery_id().as_uuid())
        .bind(i32::from(claim.proof.part_index().get())).bind(claim.target.desk_id())
        .bind(claim.target.principal_id().map(PrincipalId::as_uuid))
        .bind(claim.from).bind(claim.to).bind(claim.message).bind(claim.subject).bind(claim.body)
        .bind(claim.in_reply_to).bind(serde_json::to_value(references).map_err(|error| AppError::Internal(error.to_string()))?)
        .fetch_optional(db).await?;
    Ok(row.map(|row| AuthenticatedRelaySource {
        company_id: claim.proof.company_id(),
        source_target: claim.target,
        agent_principal: row.sender_principal_id.map(PrincipalId::new),
    }))
}

pub(super) fn refused() -> AppError {
    AppError::Forbidden("Internal relay source is unavailable or its proof does not match".into())
}

pub(super) async fn validate_targets_on(
    db: &mut sqlx::PgConnection,
    request: &crate::transport::InboundCommitRequest,
    actor: Option<PrincipalId>,
) -> AppResult<()> {
    let channels: Vec<_> = request
        .intents
        .iter()
        .map(|intent| intent.target.desk_id())
        .collect();
    let principals: Vec<_> = request
        .intents
        .iter()
        .map(|intent| intent.target.principal_id().map(PrincipalId::as_uuid))
        .collect();
    let allowed: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM unnest($2::uuid[],$3::uuid[]) AS target(channel_id,principal_id) \
         WHERE CASE WHEN target.channel_id IS NOT NULL THEN relay_desk_can_participate($1,target.channel_id,$4) \
             ELSE principal_direct_can_participate($1,target.principal_id,$4) END IS NOT TRUE)",
    ).bind(request.company_id).bind(channels).bind(principals).bind(actor.map(PrincipalId::as_uuid))
        .fetch_one(db).await?;
    if allowed { Ok(()) } else { Err(refused()) }
}
