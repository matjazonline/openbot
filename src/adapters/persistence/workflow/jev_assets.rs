//! Authorize immutable private attachment references before storage reads.
use super::*;
use crate::application::workflow::{
    actions::ActionScope, jev::*, lease::ClaimedWorkflow, polling::WorkflowStepKind,
};
use crate::domain::entities::message::AttachmentMetadata;
use std::{collections::BTreeMap, time::Duration};

#[async_trait]
impl JevAssetDirectory for PostgresPersistence {
    async fn locate_jev_assets(
        &self,
        claim: &ClaimedWorkflow,
    ) -> AppResult<BTreeMap<AssetName, JevAssetLocation>> {
        tokio::time::timeout(Duration::from_secs(5), locate(self, claim))
            .await
            .map_err(|_| AppError::Internal("Jev image authorization timed out".into()))?
    }
}
async fn locate(
    p: &PostgresPersistence,
    claim: &ClaimedWorkflow,
) -> AppResult<BTreeMap<AssetName, JevAssetLocation>> {
    let refs = parse_assets(claim.activation.inputs.get("assets"))
        .map_err(|s| AppError::BadRequest(s.into()))?;
    let scope = claim.fence.scope;
    let started = tokio::time::Instant::now();
    let mut tx = http::begin_http(p).await?;
    let actor = action_authority::prelock_on(
        &mut tx,
        ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        },
    )
    .await?
    .ok_or_else(missing)?;
    let locations = locate_on(&mut tx, scope.company, actor, refs).await?;
    if !lease::lock_fence(&mut tx, claim.fence, Duration::from_secs(2)).await? {
        return Err(missing());
    }
    let facts = http::runtime::facts(
        &mut tx,
        ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        },
    )
    .await?;
    if facts.actor != actor {
        return Err(missing());
    }
    http::runtime::frozen_inputs(
        &mut tx,
        &facts,
        claim,
        &WorkflowStepKind("jev.evaluate".into()),
    )
    .await?;
    if lease::live_window(&mut tx, claim.fence, started)
        .await?
        .is_none()
    {
        return Err(missing());
    }
    tx.commit().await?;
    Ok(locations)
}

/// Caller holds company authority; retain all file grants/membership through subsequent
/// credential/fence waits in the SAME transaction. No storage or network reads here.
pub(super) async fn locate_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    refs: JevAssetRefs,
) -> AppResult<BTreeMap<AssetName, JevAssetLocation>> {
    let mut locations = BTreeMap::new();
    // Acquire every file's authority before workflow-local fence locks.
    for (name, reference) in refs {
        association::authorize_association(
            tx,
            company,
            actor,
            RelatedAssociation::Thread {
                channel_id: reference.channel_id,
                thread_id: reference.thread_id,
            },
        )
        .await?;
        let metadata: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT attachment.value FROM thread_messages AS membership \
             JOIN messages AS message ON message.company_id=membership.company_id AND message.id=membership.message_id \
             CROSS JOIN LATERAL jsonb_array_elements(message.attachments->'items') AS attachment(value) \
             WHERE membership.company_id=$1 AND membership.channel_id=$2 AND membership.thread_id=$3 \
               AND attachment.value->>'sha256_hash'=$4 LIMIT 1 FOR SHARE OF membership, message"
        ).bind(company.as_uuid()).bind(reference.channel_id.as_uuid()).bind(reference.thread_id.as_uuid()).bind(reference.sha256.as_str()).fetch_optional(&mut **tx).await?;
        let metadata: AttachmentMetadata =
            serde_json::from_value(metadata.ok_or_else(missing)?).map_err(|_| invalid())?;
        let key = metadata.storage_key.ok_or_else(missing)?;
        locations.insert(
            name,
            JevAssetLocation {
                reference,
                key,
                content_type: metadata.content_type,
                size_bytes: metadata.size_bytes,
            },
        );
    }
    Ok(locations)
}
