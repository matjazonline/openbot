//! Lock all identity keys before direct target rows, then prove current routing authority.
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    adapters::persistence::participant::lock_identity_key,
    app_error::{AppError, AppResult},
    entities::transport::{ChannelBindingId, PrincipalId, QualifiedIdentity},
    transport::{InboundCommitRequest, ThreadPrincipalReference},
};

pub(super) fn validate_target_alignment(request: &InboundCommitRequest) -> AppResult<()> {
    let targets: std::collections::HashSet<_> = request
        .associations
        .iter()
        .map(|association| association.address_target)
        .collect();
    let intents: std::collections::HashSet<_> =
        request.intents.iter().map(|intent| intent.target).collect();
    if targets.len() != request.associations.len()
        || intents.len() != request.intents.len()
        || targets != intents
    {
        return Err(AppError::BadRequest(
            "Every inbound target requires exactly one matching admission intent".into(),
        ));
    }
    Ok(())
}

fn observed_identities(
    request: &InboundCommitRequest,
    author: super::InboundAuthor,
) -> Vec<&QualifiedIdentity> {
    let mut identities = Vec::new();
    if !matches!(author, super::InboundAuthor::AgentNotice) {
        identities.push(&request.envelope.author);
        identities.extend(
            request
                .envelope
                .addressed
                .iter()
                .map(|addressed| &addressed.identity),
        );
    }
    identities.extend(
        request
            .associations
            .iter()
            .flat_map(|association| association.principals.iter())
            .filter_map(|intent| match &intent.reference {
                ThreadPrincipalReference::Observed(identity) => Some(identity),
                ThreadPrincipalReference::Known(_) => None,
            }),
    );
    identities.sort_by(|left, right| {
        left.transport()
            .as_str()
            .cmp(right.transport().as_str())
            .then_with(|| left.namespace().as_str().cmp(right.namespace().as_str()))
            .then_with(|| left.subject().as_str().cmp(right.subject().as_str()))
    });
    identities.dedup();
    identities
}

pub(super) async fn hold_principal_settings(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    author: super::InboundAuthor,
) -> AppResult<()> {
    let identities = observed_identities(request, author);
    for identity in identities {
        lock_identity_key(tx, request.company_id, identity).await?;
    }
    let mut ids: Vec<_> = request
        .intents
        .iter()
        .filter_map(|intent| intent.target.principal_id())
        .map(PrincipalId::as_uuid)
        .collect();
    if let super::InboundAuthor::Relay(principal) = author {
        ids.extend(principal.map(PrincipalId::as_uuid));
        ids.extend(
            request
                .envelope
                .directives
                .source_target
                .and_then(|target| target.principal_id())
                .map(PrincipalId::as_uuid),
        );
    }
    ids.sort();
    ids.dedup();
    // One ordered acquisition, before binding/provider/thread locks or identity observations.
    sqlx::query(
        "SELECT id FROM principals WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE",
    )
    .bind(request.company_id)
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    for intent in &request.intents {
        let Some(principal) = intent.target.principal_id() else {
            continue;
        };
        let current: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM principals AS target WHERE target.company_id=$1 AND target.id=$2 \
             AND target.settings_revision=$3 AND EXISTS(SELECT 1 FROM addresses AS address \
                 WHERE address.company_id=$1 AND address.principal_id=target.id) \
             AND ($9 OR principal_direct_can_participate($1,target.id,CASE WHEN $4::text='relay' THEN $5::uuid ELSE \
                 (SELECT identity.principal_id FROM participant_identities AS identity \
                  WHERE identity.company_id=$1 AND identity.transport=$6 \
                  AND identity.namespace=$7 AND identity.subject=$8 AND identity.status='verified') END)))",
        ).bind(request.company_id).bind(principal.as_uuid()).bind(intent.settings_revision)
            .bind(if matches!(author,super::InboundAuthor::Relay(_)) { "relay" } else { "observed" })
            .bind(match author { super::InboundAuthor::Relay(principal) => principal.map(PrincipalId::as_uuid), _ => None })
            .bind(request.envelope.author.transport().as_str()).bind(request.envelope.author.namespace().as_str())
            .bind(request.envelope.author.subject().as_str())
            .bind(matches!(author, super::InboundAuthor::AgentNotice)).fetch_one(&mut **tx).await?;
        if !current {
            return Err(AppError::Superseded(
                "Direct routing authority changed; route the message again".into(),
            ));
        }
    }
    Ok(())
}

pub(super) async fn verify_bindings(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<()> {
    #[derive(sqlx::FromRow)]
    struct Binding {
        id: Uuid,
        channel_id: Option<Uuid>,
        principal_id: Option<Uuid>,
        status: String,
    }
    let ids: Vec<_> = request
        .associations
        .iter()
        .map(|association| association.binding_id.as_uuid())
        .collect();
    let rows: Vec<Binding> = sqlx::query_as(
        "SELECT id,channel_id,principal_id,status FROM channel_bindings \
         WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE",
    )
    .bind(request.company_id)
    .bind(ids)
    .fetch_all(&mut **tx)
    .await?;
    for association in &request.associations {
        let stored = rows
            .iter()
            .find(|row| ChannelBindingId::new(row.id) == association.binding_id);
        let target_matches = stored.is_some_and(|row| {
            row.channel_id == association.address_target.desk_id()
                && row.principal_id
                    == association
                        .address_target
                        .principal_id()
                        .map(PrincipalId::as_uuid)
        });
        if !target_matches {
            return Err(AppError::NotFound(match association.address_target {
                crate::entities::address::AddressTarget::Desk(id) => format!(
                    "Active binding {} does not belong to channel {id}",
                    association.binding_id
                ),
                crate::entities::address::AddressTarget::Principal(id) => format!(
                    "Active binding {} does not belong to principal {id}",
                    association.binding_id
                ),
            }));
        }
        if stored.is_none_or(|row| row.status != "active") {
            return Err(AppError::Superseded(
                "Inbound transport binding changed; route the message again".into(),
            ));
        }
    }
    Ok(())
}
