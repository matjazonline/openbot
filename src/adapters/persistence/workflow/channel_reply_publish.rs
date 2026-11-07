//! Atomic final publication under the saved decision and its independent current claim.
use super::channel_reply_publication::{
    CLAIM_FILTER, bind_claim, bounds, settle_on, storage_timeout,
};
use super::channel_reply_publish_authority::{FinalAuthority, RunAuthority};
use super::channel_reply_publish_read::{PublicationRow, PublicationState, ReadLock, read_on};
use super::*;
use crate::application::workflow::actions::ActionScope;
use crate::application::workflow::channel_reply::*;
use crate::application::workflow::message::{MessagePlan, PlannedDestination};
use sqlx::{Postgres, Transaction};
use std::time::Duration;

#[async_trait]
impl ReplyPublicationStore for PostgresPersistence {
    async fn load_reply_publication(
        &self,
        claim: ReplyPublicationClaim,
    ) -> AppResult<ReplyPublicationLoad> {
        tokio::time::timeout(Duration::from_secs(7), async {
            let mut tx = self.pool().begin().await?;
            bounds(&mut tx).await?;
            let loaded = match read_on(&mut tx, claim.into(), ReadLock::Observe).await? {
                Some(row) => load(&row, claim)?,
                None => ReplyPublicationLoad::Lost,
            };
            tx.rollback().await?;
            Ok(loaded)
        })
        .await
        .map_err(|_| storage_timeout())?
    }

    async fn commit_reply_publication(
        &self,
        claim: ReplyPublicationClaim,
        plan: &MessagePlan,
    ) -> AppResult<ReplyPublicationOutcome> {
        tokio::time::timeout(
            Duration::from_secs(7),
            self.publish_final_reply(claim, plan),
        )
        .await
        .map_err(|_| storage_timeout())?
    }
}

fn load(row: &PublicationRow, claim: ReplyPublicationClaim) -> AppResult<ReplyPublicationLoad> {
    Ok(match row.state {
        PublicationState::Published => ReplyPublicationLoad::Published(row.receipt()?),
        PublicationState::Failed => ReplyPublicationLoad::Failed(row.failure()?),
        _ if !row.owned_by(claim) => ReplyPublicationLoad::Lost,
        _ => match row.draft(claim.into()) {
            Ok(draft) => ReplyPublicationLoad::Ready(Box::new(draft)),
            Err(_) => ReplyPublicationLoad::Refused(ReplyPublicationFailure::InvalidProposal),
        },
    })
}

impl PostgresPersistence {
    async fn publish_final_reply(
        &self,
        claim: ReplyPublicationClaim,
        plan: &MessagePlan,
    ) -> AppResult<ReplyPublicationOutcome> {
        let mut tx = self.pool().begin().await?;
        bounds(&mut tx).await?;
        let Some(hint) = read_on(&mut tx, claim.into(), ReadLock::Observe).await? else {
            return Ok(ReplyPublicationOutcome::Lost);
        };
        if let Some(outcome) = stored(&hint)? {
            return Ok(outcome);
        }
        if !hint.owned_by(claim) {
            return Ok(ReplyPublicationOutcome::Lost);
        }
        let run = channel_reply_publish_authority::lock_run_on(&mut tx, &hint).await?;
        let Some(row) = read_on(&mut tx, claim.into(), ReadLock::Publish).await? else {
            return Ok(ReplyPublicationOutcome::Lost);
        };
        if let Some(outcome) = stored(&row)? {
            return Ok(outcome);
        }
        if !row.owned_by(claim) {
            return Ok(ReplyPublicationOutcome::Lost);
        }
        let refusal = match run {
            RunAuthority::Available => None,
            RunAuthority::Revoked => Some(ReplyPublicationFailure::AuthorityRevoked),
            RunAuthority::Unavailable => Some(ReplyPublicationFailure::InvalidProposal),
        };
        if let Some(reason) = refusal {
            return refuse(tx, claim, reason).await;
        }
        let artifact = match Box::pin(channel_reply_publish_authority::validate_on(
            &mut tx,
            &row,
            claim.into(),
        ))
        .await?
        {
            FinalAuthority::Allowed(artifact) => artifact,
            FinalAuthority::Refused(reason) => return refuse(tx, claim, reason).await,
        };
        if !channel_reply_publish_plan::matches(&artifact, plan) {
            return refuse(tx, claim, ReplyPublicationFailure::InvalidProposal).await;
        }
        message::writer::recheck_routes_on(&mut tx, claim.company, plan).await?;
        // The artifact validator requires exactly one destination before any write.
        Box::pin(write_publication(
            tx,
            row.scope(),
            claim,
            &plan.destinations[0],
        ))
        .await
    }
}

async fn write_publication(
    mut tx: Transaction<'_, Postgres>,
    scope: ActionScope,
    claim: ReplyPublicationClaim,
    destination: &PlannedDestination,
) -> AppResult<ReplyPublicationOutcome> {
    if let Some(delivery) = &destination.delivery {
        let binding = channel_reply_binding_authority::BindingScope {
            company: scope.company.as_uuid(),
            channel: destination.channel_id,
            binding: delivery.source_binding_id.as_uuid(),
            transport: delivery.transport,
        };
        if !channel_reply_binding_authority::active_on(&mut tx, binding).await? {
            return refuse(tx, claim, ReplyPublicationFailure::SourceUnavailable).await;
        }
    }
    let written = Box::pin(message::writer::write_destination_on(
        &mut tx,
        scope,
        destination,
    ))
    .await?;
    let query=format!("UPDATE channel_reply_decisions SET state='published',message_id=$9,delivery_id=$10,
        generation=NULL,worker_id=NULL,lease_expires_at=NULL,updated_at=clock_timestamp() WHERE {CLAIM_FILTER}");
    let changed = bind_claim(sqlx::query(&query), claim)
        .bind(written.message.as_uuid())
        .bind(written.delivery.map(|id| id.as_uuid()))
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if changed != 1 {
        tx.rollback().await?;
        return Ok(ReplyPublicationOutcome::Lost);
    }
    tx.commit().await?;
    Ok(ReplyPublicationOutcome::Published(
        ReplyPublicationReceipt {
            message: written.message,
            delivery: written.delivery,
        },
    ))
}

fn stored(row: &PublicationRow) -> AppResult<Option<ReplyPublicationOutcome>> {
    Ok(match row.state {
        PublicationState::Published => Some(ReplyPublicationOutcome::Published(row.receipt()?)),
        PublicationState::Failed => Some(ReplyPublicationOutcome::Refused(row.failure()?)),
        _ => None,
    })
}

async fn refuse(
    mut tx: Transaction<'_, Postgres>,
    claim: ReplyPublicationClaim,
    reason: ReplyPublicationFailure,
) -> AppResult<ReplyPublicationOutcome> {
    let changed = settle_on(&mut tx, claim, ReplyPublicationRetry::Terminal(reason)).await?;
    tx.commit().await?;
    Ok(if changed {
        ReplyPublicationOutcome::Refused(reason)
    } else {
        ReplyPublicationOutcome::Lost
    })
}
