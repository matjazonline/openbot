//! Final delivery entry uses the committed final grant and the delivery worker's real lease.
use super::channel_reply_publish_authority::{FinalAuthority, RunAuthority};
use super::channel_reply_publish_read::{FinalIdentity, PublicationState, ReadLock, read_on};
use super::message::receipts::WorkflowDeliveryAdmission;
use super::*;
use crate::domain::workflow::{ReplyIntegrity, ReplyProposalId};
use crate::entities::transport::DeliveryCancellationReason;
use sqlx::PgConnection;

#[derive(sqlx::FromRow)]
struct FinalDeliveryLink {
    company_id: Uuid,
    run_id: Uuid,
    proposal_id: Uuid,
    #[sqlx(try_from = "String")]
    integrity: ReplyIntegrity,
    channel_id: Uuid,
    source_binding_id: Uuid,
    destination_binding_id: Uuid,
}

pub(super) async fn admission_on(
    db: &mut PgConnection,
    delivery: Uuid,
) -> AppResult<Option<WorkflowDeliveryAdmission>> {
    let link: Option<FinalDeliveryLink> = sqlx::query_as(
        "SELECT final_reply.company_id,final_reply.run_id,final_reply.proposal_id,final_reply.integrity, \
        delivery.channel_id,delivery.source_binding_id,delivery.destination_binding_id \
        FROM message_deliveries AS delivery JOIN channel_reply_decisions AS final_reply \
            ON final_reply.company_id=delivery.company_id AND final_reply.message_id=delivery.message_id \
        WHERE delivery.id=$1 AND final_reply.state='published'",
    )
    .bind(delivery)
    .fetch_optional(&mut *db)
    .await?;
    let Some(link) = link else {
        return Ok(None);
    };
    // Keep final authority locks and the subsequent parent/part fence bounded in this transaction.
    super::channel_reply_publication::bounds(db).await?;
    let identity = FinalIdentity {
        company: CompanyId::new(link.company_id),
        run: RunId::new(link.run_id),
        proposal: ReplyProposalId::new(link.proposal_id),
        integrity: link.integrity,
    };
    let Some(row) = read_on(db, identity, ReadLock::Observe).await? else {
        return Ok(Some(refused()));
    };
    if row.state != PublicationState::Published || row.delivery_id != Some(delivery) {
        return Ok(Some(refused()));
    }
    match channel_reply_publish_authority::lock_run_on(db, &row).await? {
        RunAuthority::Available => {}
        RunAuthority::Revoked | RunAuthority::Unavailable => return Ok(Some(refused())),
    }
    // No execution deadline is applied after valid success; this call still has its real
    // delivery lease checked by begin_part after the company/run/source checks.
    let authorized = Box::pin(channel_reply_publish_authority::validate_on(
        db, &row, identity,
    ))
    .await?;
    if matches!(&authorized, FinalAuthority::Allowed(_)) {
        let binding = channel_reply_binding_authority::BindingScope {
            company: link.company_id,
            channel: link.channel_id,
            binding: link.source_binding_id,
            transport: crate::entities::transport::TransportKind::Email,
        };
        if link.source_binding_id != link.destination_binding_id
            || !channel_reply_binding_authority::active_on(db, binding).await?
        {
            return Ok(Some(refused()));
        }
    }
    Ok(Some(match authorized {
        FinalAuthority::Allowed(_) => WorkflowDeliveryAdmission::Proceed,
        FinalAuthority::Refused(_) => refused(),
    }))
}

fn refused() -> WorkflowDeliveryAdmission {
    WorkflowDeliveryAdmission::Refuse(DeliveryCancellationReason::WorkflowRevoked)
}
