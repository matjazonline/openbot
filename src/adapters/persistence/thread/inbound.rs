//! The one transaction an accepted inbound message goes through.
//!
//! Every row an accepted message produces is written here, in one transaction, in dependency
//! order: the threads it lands in, the provider conversation mappings that reach them, the
//! canonical payload, its participants and email extension, one association per thread, one
//! binding-qualified message mapping per association, one workflow-admission intent per
//! association (p08 D1.3), and the delivery fan-out. They become visible together or not at all.
//!
//! That property is the point. The shape this replaces created the thread in one statement, the
//! message in a second transaction and the task in a third, so a crash between them left a stored
//! message that no redelivery would deduplicate and that no agent would ever answer -- and the
//! database trigger published the message before either of the other two rows existed.
//!
//! Three concurrency rules do the rest of the work:
//!
//! * The channel gates come first, shared, and each channel's settings revision is re-read once
//!   they are held. A commit routed against settings an edit has since replaced is refused as
//!   superseded rather than stored under them.
//! * Transaction-scoped advisory locks cover every message and thread key on every association
//!   binding, in deterministic order. The mappings are re-read only after those locks are held.
//! * A provider conversation mapping wins over a caller's stale create decision. A reply that
//!   arrives before its root can therefore create the conversation and the root joins it.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{email_metadata, external, message};
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        channel_gate::{ChannelGateAccess, ChannelGateKey, acquire_channel_gates_on},
        delivery::enqueue::insert_delivery_on,
    },
    app_error::{AppError, AppResult},
    entities::{
        email_message::EmailMessageMetadata,
        message::{CanonicalMessageId, MessageDirection, MessageParticipantKind, MessageRole},
        participant::{IdentityClaimMetadata, IdentityProvenance},
        transport::{ChannelBindingId, DeliveryId, QualifiedIdentity},
    },
    transport::{
        CommitDisposition, InboundCommitOutcome, InboundCommitRequest, InboundEnvelope,
        InboundMessageCommitter, ProtocolExtension, ReplyDelivery, ThreadAssociation, ThreadTarget,
    },
    use_cases::{
        participant::IdentityObservation,
        thread::{MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite, MessageWrite},
    },
};

/// Every thread one association resolved to, once the commit has created the ones it had to.
struct ResolvedThread {
    thread_id: Uuid,
    channel_id: Uuid,
    binding_id: ChannelBindingId,
}

struct StoredMessage {
    id: CanonicalMessageId,
    content_hash: Vec<u8>,
}

#[async_trait]
impl InboundMessageCommitter for PostgresPersistence {
    async fn commit_inbound(
        &self,
        request: InboundCommitRequest,
    ) -> AppResult<InboundCommitOutcome> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let outcome = commit_on(&mut tx, &request).await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(outcome)
    }
}

/// The commit body, on a caller-supplied transaction so a failure at any statement rolls the whole
/// set back rather than part of it.
pub(super) async fn commit_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<InboundCommitOutcome> {
    if request.associations.is_empty() {
        return Err(AppError::BadRequest(
            "An inbound commit must name at least one thread association".into(),
        ));
    }
    hold_channel_settings(tx, request).await?;
    let binding_ids = verify_association_bindings(tx, request).await?;
    lock_provider_keys(tx, request, &binding_ids).await?;

    // Asked first, and under every relevant lock. A redelivery must open no thread: the old path
    // created the thread before it knew whether the message was new, so a provider retry left an
    // empty conversation behind and then failed associating the message it already held.
    let existing = existing_message_mappings(tx, request, &binding_ids).await?;
    if !existing.is_empty() {
        return recognise_redelivery(tx, request, existing).await;
    }

    let threads = resolve_threads(tx, request).await?;
    bind_conversations(tx, request, &threads).await?;

    let write = message_write(request, threads[0].thread_id)?;
    let stored = store_canonical(tx, request, &write).await?;

    associate_threads(tx, request, &threads, stored.id).await?;
    map_provider_message(tx, request, &threads, &stored).await?;
    // After the associations, whose rows the intents' foreign key names, and before the fan-out:
    // keeping the statement order of the whole commit fixed is what stops two concurrent inbound
    // commits on one thread taking their locks in opposite orders.
    let intent_ids = create_intents(tx, request, &threads, stored.id).await?;
    let delivery_ids = create_deliveries(tx, request).await?;
    complete_claimed_event(tx, request).await?;

    Ok(InboundCommitOutcome {
        disposition: CommitDisposition::Created,
        message_id: stored.id,
        thread_ids: threads.iter().map(|thread| thread.thread_id).collect(),
        intent_ids,
        delivery_ids,
    })
}

/// Take the gate of every channel this commit admits into, then check the settings it was routed
/// under still stand. No settings edit can land on those channels for the rest of the commit.
///
/// The gates come before every row lock the commit takes, all of them in one ordered call, so
/// channel settings writers and this commit lock in one order.
pub(super) async fn hold_channel_settings(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<()> {
    acquire_channel_gates_on(
        tx,
        request
            .intents
            .iter()
            .map(|intent| ChannelGateKey::new(request.company_id, intent.channel_id)),
        ChannelGateAccess::Admit,
    )
    .await?;
    verify_channel_settings(tx, request).await
}

/// Refuse the commit if any channel it admits into has moved past the settings revision routing
/// accepted it under -- or no longer exists.
///
/// After the gates, in a statement of its own, so it reads with a snapshot taken once they are
/// held: an edit that landed first is seen here, and one that comes later waits for this commit
/// ([`crate::adapters::persistence::channel_gate`]).
async fn verify_channel_settings(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<()> {
    let (channels, revisions): (Vec<Uuid>, Vec<i64>) = request
        .intents
        .iter()
        .map(|intent| (intent.channel_id, intent.settings_revision))
        .unzip();
    let superseded: Option<Uuid> = sqlx::query_scalar(
        "SELECT seen.channel_id FROM unnest($2::uuid[], $3::bigint[]) AS seen(channel_id, revision) \
         WHERE NOT EXISTS (SELECT 1 FROM channels AS channel WHERE channel.company_id = $1 \
           AND channel.id = seen.channel_id AND channel.settings_revision = seen.revision) \
         ORDER BY seen.channel_id LIMIT 1",
    )
    .bind(request.company_id)
    .bind(&channels)
    .bind(&revisions)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    match superseded {
        Some(channel_id) => Err(AppError::Superseded(format!(
            "Channel {channel_id} changed its settings while this message was being routed, so \
             nothing was stored; route it again"
        ))),
        None => Ok(()),
    }
}

/// Return what the first delivery of this message already made durable.
///
/// The content hash is checked before anything is returned: a repeated provider key carrying
/// *different* content is not a redelivery, and answering it with the stored message would hide a
/// provider or adapter fault behind a successful-looking commit.
///
/// Nothing is written and nothing is enqueued -- which is the property the caller needs, because
/// the alternative is a second workflow admission for the same turn.
async fn recognise_redelivery(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    existing: Vec<(ChannelBindingId, external::ExistingExternalMessage)>,
) -> AppResult<InboundCommitOutcome> {
    let message_id = existing[0].1.message_id;
    for (binding_id, mapping) in &existing[1..] {
        if mapping.message_id != message_id {
            return Err(crate::entities::message::ExternalMessageCollision {
                binding_id: *binding_id,
                external_message_key: request.envelope.source.message_key.clone(),
                existing_message_id: mapping.message_id,
            }
            .into());
        }
    }
    let thread_ids = threads_holding(tx, request.company_id, message_id).await?;
    let first_thread = *thread_ids.first().ok_or_else(|| {
        AppError::Internal(format!("Stored message {message_id} belongs to no thread"))
    })?;

    let write = message_write(request, first_thread)?;
    let author = message::resolve_author(tx, request.company_id, &write.author).await?;
    let participants = message::resolve_participants(tx, request.company_id, &write).await?;
    let attachments = message::encode_attachments(&write.attachments)?;
    let content_hash =
        message::canonical_message_hash(&write, &author, &participants, attachments.as_ref());
    for (binding_id, mapping) in existing {
        external::reuse_or_reject(
            mapping,
            binding_id,
            &request.envelope.source.message_key,
            &content_hash,
        )?;
    }

    let intent_ids = intents_for_message(tx, request.company_id, message_id, &thread_ids).await?;
    complete_claimed_event(tx, request).await?;
    Ok(InboundCommitOutcome {
        disposition: CommitDisposition::Duplicate,
        message_id,
        thread_ids,
        intent_ids,
        // A redelivery fans out nothing: the first delivery's intents are already durable.
        delivery_ids: Vec::new(),
    })
}

/// The conversations a stored message already belongs to, in the order it joined them.
async fn threads_holding(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    message_id: CanonicalMessageId,
) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        r#"SELECT association.thread_id
           FROM thread_messages AS association
           WHERE association.company_id = $1 AND association.message_id = $2
           ORDER BY association.created_at, association.id"#,
    )
    .bind(company_id)
    .bind(message_id.as_uuid())
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)
}

/// The runs this message already caused, ordered by their original thread associations.
async fn intents_for_message(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    message_id: CanonicalMessageId,
    thread_ids: &[Uuid],
) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        r#"SELECT id FROM workflow_message_intents
           WHERE company_id = $1 AND message_id = $2
           ORDER BY array_position($3::uuid[], thread_id), id"#,
    )
    .bind(company_id)
    .bind(message_id.as_uuid())
    .bind(thread_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)
}

/// Verify every association binding belongs to the stated tenant/channel and return each once.
async fn verify_association_bindings(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<Vec<ChannelBindingId>> {
    if let Some((binding_id, channel_id)) = first_unbound_association(tx, request).await? {
        return Err(AppError::NotFound(format!(
            "Active binding {binding_id} does not belong to channel {channel_id}"
        )));
    }
    let mut bindings = Vec::with_capacity(request.associations.len());
    for association in &request.associations {
        if !bindings.contains(&association.binding_id) {
            bindings.push(association.binding_id);
        }
    }
    if !bindings.contains(&request.envelope.source.binding_id) {
        return Err(AppError::BadRequest(
            "The source binding has no thread association".into(),
        ));
    }
    bindings.sort_unstable();
    Ok(bindings)
}

/// The first association whose binding is not an active binding of its channel in this company,
/// asked once for the whole list rather than once per recipient channel.
///
/// "First" is the request's own order, as it was when each association was its own round trip, so
/// a commit naming several bad pairs is refused naming the same one it always was.
async fn first_unbound_association(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<Option<(ChannelBindingId, Uuid)>> {
    let binding_ids: Vec<Uuid> = request
        .associations
        .iter()
        .map(|association| association.binding_id.as_uuid())
        .collect();
    let channel_ids: Vec<Uuid> = request
        .associations
        .iter()
        .map(|association| association.channel_id)
        .collect();
    let unbound: Option<(Uuid, Uuid)> = sqlx::query_as(
        r#"SELECT association.binding_id, association.channel_id
           FROM unnest($1::uuid[], $2::uuid[]) WITH ORDINALITY
                AS association (binding_id, channel_id, ordinal)
           WHERE NOT EXISTS (
               SELECT 1 FROM channel_bindings AS binding
               WHERE binding.id = association.binding_id AND binding.company_id = $3
                 AND binding.channel_id = association.channel_id AND binding.status = 'active'
           )
           ORDER BY association.ordinal
           LIMIT 1"#,
    )
    .bind(&binding_ids)
    .bind(&channel_ids)
    .bind(request.company_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(unbound.map(|(binding_id, channel_id)| (ChannelBindingId::new(binding_id), channel_id)))
}

/// Serialize every provider mapping this transaction may read or create.
///
/// Transaction-scoped, so it is released by commit or rollback with nothing to clean up, and keyed
/// on pairs rather than rows: the rows may not exist yet, which is why a row lock cannot do this
/// job. Sorting the complete set prevents two multi-binding commits from deadlocking each other.
async fn lock_provider_keys(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    binding_ids: &[ChannelBindingId],
) -> AppResult<()> {
    let mut keys = Vec::with_capacity(binding_ids.len() * 2);
    for binding_id in binding_ids {
        keys.push(format!(
            "message:{binding_id}:{}",
            request.envelope.source.message_key.as_str()
        ));
        keys.push(format!(
            "thread:{binding_id}:{}",
            request.envelope.source.thread_key.as_str()
        ));
    }
    keys.sort_unstable();
    keys.dedup();
    for key in keys {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(key)
            .execute(&mut **tx)
            .await
            .map_err(AppError::from)?;
    }
    Ok(())
}

async fn existing_message_mappings(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    binding_ids: &[ChannelBindingId],
) -> AppResult<Vec<(ChannelBindingId, external::ExistingExternalMessage)>> {
    let mut existing = Vec::new();
    for binding_id in binding_ids {
        if let Some(mapping) =
            external::find_external_message(tx, *binding_id, &request.envelope.source.message_key)
                .await?
        {
            existing.push((*binding_id, mapping));
        }
    }
    Ok(existing)
}

/// Resolve every association's thread, creating the ones this commit opens.
///
/// Creation happens before the message exists on purpose: a thread with no message is an empty
/// conversation a later delivery joins, whereas a message with no thread is unreachable.
struct ThreadResolution<'a> {
    association: &'a ThreadAssociation,
    target: ThreadTarget,
}

#[derive(sqlx::FromRow)]
struct LockedThread {
    id: Uuid,
    channel_id: Uuid,
}

async fn resolve_threads(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<Vec<ResolvedThread>> {
    let resolutions = plan_thread_resolution(tx, request).await?;
    lock_existing_threads(tx, request.company_id, &resolutions).await?;
    let mut threads = Vec::with_capacity(resolutions.len());
    let mut resolved_by_binding = HashMap::new();
    for resolution in resolutions {
        let association = resolution.association;
        let thread_id = match resolved_by_binding.get(&association.binding_id) {
            Some(thread_id) => *thread_id,
            None => match resolution.target {
                ThreadTarget::Existing(thread_id) => thread_id,
                ThreadTarget::Create { subject } => {
                    open_thread(tx, request.company_id, association.channel_id, &subject).await?
                }
            },
        };
        add_thread_principals(tx, request, association, thread_id).await?;
        resolved_by_binding.insert(association.binding_id, thread_id);
        threads.push(ResolvedThread {
            thread_id,
            channel_id: association.channel_id,
            binding_id: association.binding_id,
        });
    }
    Ok(threads)
}

async fn plan_thread_resolution<'a>(
    tx: &mut Transaction<'_, Postgres>,
    request: &'a InboundCommitRequest,
) -> AppResult<Vec<ThreadResolution<'a>>> {
    let mut planned_by_binding: HashMap<ChannelBindingId, ThreadTarget> = HashMap::new();
    let mut resolutions = Vec::with_capacity(request.associations.len());
    for association in &request.associations {
        let target = match planned_by_binding.get(&association.binding_id) {
            Some(target) => target.clone(),
            None => {
                let target = external::find_external_thread(
                    tx,
                    association.binding_id,
                    &request.envelope.source.thread_key,
                )
                .await?
                .map(ThreadTarget::Existing)
                .unwrap_or_else(|| association.target.clone());
                planned_by_binding.insert(association.binding_id, target.clone());
                target
            }
        };
        resolutions.push(ThreadResolution {
            association,
            target,
        });
    }
    Ok(resolutions)
}

async fn lock_existing_threads(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    resolutions: &[ThreadResolution<'_>],
) -> AppResult<()> {
    let ids: Vec<Uuid> = resolutions
        .iter()
        .filter_map(|resolution| match resolution.target {
            ThreadTarget::Existing(id) => Some(id),
            ThreadTarget::Create { .. } => None,
        })
        .collect();
    // Publications lock this same set in UUID order. Address order remains the presentation
    // order, but cannot determine the lock order when a follow-up races a sibling reply.
    let locked: Vec<LockedThread> = sqlx::query_as(
        "SELECT id, channel_id FROM threads WHERE company_id = $1 AND id = ANY($2) \
         ORDER BY id FOR UPDATE",
    )
    .bind(company_id)
    .bind(ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    for resolution in resolutions {
        let ThreadTarget::Existing(id) = resolution.target else {
            continue;
        };
        if !locked
            .iter()
            .any(|thread| thread.id == id && thread.channel_id == resolution.association.channel_id)
        {
            return Err(AppError::Internal(format!(
                "Thread {id} does not belong to channel {} of company {company_id}",
                resolution.association.channel_id,
            )));
        }
    }
    Ok(())
}

async fn open_thread(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    subject: &str,
) -> AppResult<Uuid> {
    let id = Uuid::new_v4();
    let created: Option<Uuid> = sqlx::query_scalar(
        r#"INSERT INTO threads (id, company_id, channel_id, subject)
           SELECT $1, channel.company_id, channel.id, $4
           FROM channels AS channel
           WHERE channel.id = $3 AND channel.company_id = $2
           RETURNING id"#,
    )
    .bind(id)
    .bind(company_id)
    .bind(channel_id)
    .bind(subject)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;

    created.ok_or_else(|| {
        AppError::Internal(format!(
            "Channel {channel_id} of company {company_id} was not found"
        ))
    })
}

/// Add the handles this message brings to a thread, and touch it so readers see the change.
///
/// The author's own handle is marked as such only when this message opens the thread; on an
/// existing conversation everyone the message adds is a participant.
async fn add_thread_principals(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    association: &ThreadAssociation,
    thread_id: Uuid,
) -> AppResult<()> {
    super::insert_thread_principals(
        tx,
        request.company_id,
        association.channel_id,
        thread_id,
        &association.principals,
    )
    .await?;
    sqlx::query(
        "UPDATE threads SET updated_at = GREATEST(updated_at, clock_timestamp()) WHERE id = $1 AND company_id = $2",
    )
    .bind(thread_id)
    .bind(request.company_id)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// Bind each interface's provider conversation key to the thread it reached.
///
/// Before the message is materialized so every later write uses the locked, winning conversation.
/// A later failure rolls this mapping back with the rest of the transaction.
async fn bind_conversations(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    threads: &[ResolvedThread],
) -> AppResult<()> {
    let thread_key = &request.envelope.source.thread_key;
    for thread in threads {
        external::upsert_external_thread(
            tx,
            request.company_id,
            thread.binding_id,
            thread_key,
            thread.thread_id,
        )
        .await?;
    }
    Ok(())
}

/// Insert the canonical payload, its participant projection and its email extension.
///
/// Only ever reached for a message this binding has not carried before: the redelivery it would
/// otherwise collide with was recognised under the lock at the top of the commit.
async fn store_canonical(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    write: &MessageWrite,
) -> AppResult<StoredMessage> {
    let author = message::resolve_author(tx, request.company_id, &write.author).await?;
    let participants = message::resolve_participants(tx, request.company_id, write).await?;
    let attachments = message::encode_attachments(&write.attachments)?;
    let content_hash =
        message::canonical_message_hash(write, &author, &participants, attachments.as_ref());

    let message_id = message::insert_canonical_message(
        tx,
        request.company_id,
        write,
        &author,
        attachments.as_ref(),
        &content_hash,
    )
    .await?;
    message::insert_participants(tx, request.company_id, message_id, &participants).await?;
    if let Some(metadata) = write.email_metadata() {
        email_metadata::insert_email_metadata_on(
            tx,
            request.company_id,
            message_id.as_uuid(),
            metadata,
        )
        .await?;
    }
    Ok(StoredMessage {
        id: message_id,
        content_hash,
    })
}

/// One canonical payload, one association per conversation it landed in.
async fn associate_threads(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    threads: &[ResolvedThread],
    message_id: CanonicalMessageId,
) -> AppResult<()> {
    for thread in threads {
        message::insert_thread_association(
            tx,
            message::ThreadScope {
                company_id: request.company_id,
                channel_id: thread.channel_id,
            },
            message::AssociationWrite {
                thread_id: thread.thread_id,
                created_at: Utc::now(),
                entry_kind: if request.envelope.directives.source_channel_id.is_some() {
                    crate::entities::message::ThreadEntryKind::Delegation
                } else {
                    crate::entities::message::ThreadEntryKind::Conversation
                },
            },
            message_id,
        )
        .await?;
    }
    Ok(())
}

/// Record the provider key under every interface the message arrived on.
///
/// One mapping per binding, deduplicated because a channel addressed twice in one pipeline shares
/// an interface. Every insert verifies an existing row still names this canonical message and
/// content hash, so a secondary binding cannot silently hide a conflicting mapping.
async fn map_provider_message(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    threads: &[ResolvedThread],
    stored: &StoredMessage,
) -> AppResult<()> {
    let mut written = Vec::with_capacity(threads.len());
    for thread in threads {
        if written.contains(&thread.binding_id) {
            continue;
        }
        written.push(thread.binding_id);
        external::insert_or_verify_external_message(
            tx,
            request.company_id,
            thread.binding_id,
            &request.envelope.source.message_key,
            stored.id,
            &stored.content_hash,
        )
        .await?;
    }
    Ok(())
}

/// One workflow-admission intent per association, in association order (p08 D1.3).
///
/// Identifiers and ingress facts only: the intent worker reads the stored message when it admits.
/// The responder is the stored message's author principal, and the relay trace recorded here is
/// the one a workflow reply continues (p08 D1.6).
async fn create_intents(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    threads: &[ResolvedThread],
    message_id: CanonicalMessageId,
) -> AppResult<Vec<Uuid>> {
    let thread_of: HashMap<Uuid, Uuid> = threads
        .iter()
        .map(|thread| (thread.channel_id, thread.thread_id))
        .collect();
    let directives = &request.envelope.directives;
    let trace: Vec<Uuid> = directives.trace_channels.iter().copied().collect();
    let policy = match request.reply_delivery {
        ReplyDelivery::Send => "deliver",
        ReplyDelivery::InAppOnly => "in_app_only",
    };
    let mut ids = Vec::with_capacity(request.intents.len());
    for intent in &request.intents {
        let thread_id = thread_of.get(&intent.channel_id).copied().ok_or_else(|| {
            AppError::Internal(format!(
                "Admission intent channel {} has no association in this commit",
                intent.channel_id
            ))
        })?;
        let id: Uuid = sqlx::query_scalar(
            r#"INSERT INTO workflow_message_intents
                   (company_id, id, message_id, channel_id, thread_id, role, mentioned, admissible,
                    origin, reply_to_message_id, responder_principal_id, hop_count, trace_channels,
                    delivery_policy, correlation_id)
               SELECT $1, $2, message.id, $3, $4, $5, $6, $7, $8, $9, message.author_principal_id,
                    $10, $11, $12, $13
               FROM messages AS message WHERE message.company_id = $1 AND message.id = $14
               RETURNING id"#,
        )
        .bind(request.company_id)
        .bind(Uuid::new_v4())
        .bind(intent.channel_id)
        .bind(thread_id)
        .bind(intent.role.as_str())
        .bind(intent.mentioned)
        .bind(request.admissible)
        .bind(request.origin.as_str())
        .bind(intent.reply_to.map(|parent| parent.as_uuid()))
        .bind(
            i32::try_from(directives.hop_count)
                .map_err(|_| AppError::BadRequest("An inbound hop count is out of range".into()))?,
        )
        .bind(&trace)
        .bind(policy)
        .bind(request.envelope.correlation_id.as_uuid())
        .bind(message_id.as_uuid())
        .fetch_one(&mut **tx)
        .await
        .map_err(AppError::from)?;
        ids.push(id);
    }
    Ok(ids)
}

async fn create_deliveries(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<Vec<DeliveryId>> {
    let mut created = Vec::with_capacity(request.deliveries.len());
    for delivery in &request.deliveries {
        if delivery.company_id != request.company_id {
            return Err(AppError::Internal(format!(
                "Delivery '{}' belongs to another company than the message it carries",
                delivery.idempotency_key
            )));
        }
        created.push(insert_delivery_on(tx, delivery).await?.delivery_id());
    }
    Ok(created)
}

/// Complete the durable event under the same transaction and execution fence as its canonical
/// rows. If the lease lapsed during this commit, the zero-row update turns the whole transaction
/// into a rollback; a replacement execution can then retry without observing partial effects.
async fn complete_claimed_event(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<()> {
    let Some(fence) = request.claimed_event.as_ref() else {
        // SMTP holds its request open through this commit and therefore has no inbox claim.
        return Ok(());
    };
    if crate::adapters::persistence::inbound_event::complete_inbound_event_on(tx, fence).await? {
        return Ok(());
    }
    Err(AppError::Conflict(format!(
        "Inbound event {} lost execution ownership before canonical commit",
        fence.row
    )))
}

/// The canonical message, as the producer vocabulary states it.
///
/// The envelope is already validated, so this is a projection rather than a second parse: the
/// author and every addressed handle become observations that the commit resolves to principals,
/// and the protocol extension becomes the message's correlation.
fn message_write(
    request: &InboundCommitRequest,
    primary_thread_id: Uuid,
) -> AppResult<MessageWrite> {
    let envelope = &request.envelope;
    let author = envelope.author.clone();

    let mut participants = vec![MessageParticipantWrite::new(
        MessageParticipantKind::Sender,
        author.clone(),
    )];
    for addressed in &envelope.addressed {
        participants.push(MessageParticipantWrite::new(
            addressed.role.participant_kind(),
            addressed.identity.clone(),
        ));
    }

    // A relayed message is delegated evidence: another channel talking, kept private.
    let is_delegation = envelope.directives.source_channel_id.is_some();
    let is_note = envelope.directives.disposition == crate::transport::MessageDisposition::FileOnly;
    Ok(MessageWrite {
        structured: None,
        id: CanonicalMessageId::random(),
        thread_id: primary_thread_id,
        author: MessageAuthorWrite::Observed(observation(author)),
        subject: envelope.content.subject().to_string(),
        clean_text_body: envelope.content.body_text().to_string(),
        attachments: envelope.attachments.to_vec(),
        direction: MessageDirection::Inbound,
        // A relayed message is another agent talking, not a person.
        role: if is_delegation {
            MessageRole::Agent
        } else {
            MessageRole::Human
        },
        audience: if is_delegation || is_note {
            crate::entities::message::MessageAudience::InternalOnly
        } else {
            crate::entities::message::MessageAudience::ExternalConversation
        },
        entry_kind: if is_delegation {
            crate::entities::message::ThreadEntryKind::Delegation
        } else if is_note {
            crate::entities::message::ThreadEntryKind::Note
        } else {
            crate::entities::message::ThreadEntryKind::Conversation
        },
        correlation_id: envelope.correlation_id,
        participants,
        correlation: correlation_of(envelope),
        created_at: Utc::now(),
    })
}

fn correlation_of(envelope: &InboundEnvelope) -> MessageCorrelation {
    match &envelope.extension {
        ProtocolExtension::Email { metadata, .. } => {
            MessageCorrelation::Email(EmailMessageMetadata::clone(metadata))
        }
        ProtocolExtension::StoredEvent { .. } | ProtocolExtension::None { .. } => {
            MessageCorrelation::Internal
        }
    }
}

/// A sighting of a handle on an arriving message. It confers no grant; it only fixes which
/// principal every later decision about that handle will name.
fn observation(identity: QualifiedIdentity) -> IdentityObservation {
    let provenance = match identity.transport() {
        crate::entities::transport::TransportKind::Email
        | crate::entities::transport::TransportKind::Slack => IdentityProvenance::TransportIngress,
    };
    IdentityObservation {
        identity,
        display_label: None,
        claim_metadata: IdentityClaimMetadata::observation(),
        provenance,
    }
}
