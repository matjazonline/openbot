//! The one transaction an accepted inbound message goes through.
//!
//! Every row an accepted message produces is written here, in one transaction, in dependency
//! order: the threads it lands in, the provider conversation mappings that reach them, the
//! canonical payload, its participants and email extension, one association per thread, one
//! binding-qualified message mapping per association, the agent-dispatch task keyed on the message,
//! and the delivery fan-out. They become visible together or not at all.
//!
//! That property is the point. The shape this replaces created the thread in one statement, the
//! message in a second transaction and the task in a third, so a crash between them left a stored
//! message that no redelivery would deduplicate and that no agent would ever answer -- and the
//! database trigger published the message before either of the other two rows existed.
//!
//! Two concurrency rules do the rest of the work:
//!
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
        delivery::enqueue::insert_delivery_on,
        task::{insert_task, record_outreach_reply_on},
    },
    app_error::{AppError, AppResult},
    entities::{
        email_message::EmailMessageMetadata,
        message::{CanonicalMessageId, MessageDirection, MessageParticipantKind, MessageRole},
        participant::{IdentityClaimMetadata, IdentityProvenance},
        task::{NewTask, TaskSource},
        transport::{ChannelBindingId, DeliveryId, QualifiedIdentity},
    },
    transport::{
        CommitDisposition, InboundCommitOutcome, InboundCommitRequest, InboundEnvelope,
        InboundMessageCommitter, InboundTaskPayload, InboundTaskPayloadV1, InboundTaskRequest,
        ProtocolExtension, ThreadAssociation, ThreadTarget,
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
async fn commit_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<InboundCommitOutcome> {
    if request.associations.is_empty() {
        return Err(AppError::BadRequest(
            "An inbound commit must name at least one thread association".into(),
        ));
    }
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
    apply_outreach_transitions(tx, request, stored.id).await?;

    let task_id = create_task(tx, request, &threads, stored.id).await?;
    let delivery_ids = create_deliveries(tx, request).await?;
    complete_claimed_event(tx, request).await?;

    Ok(InboundCommitOutcome {
        disposition: CommitDisposition::Created,
        message_id: stored.id,
        thread_ids: threads.iter().map(|thread| thread.thread_id).collect(),
        task_id,
        delivery_ids,
    })
}

/// Return what the first delivery of this message already made durable.
///
/// The content hash is checked before anything is returned: a repeated provider key carrying
/// *different* content is not a redelivery, and answering it with the stored message would hide a
/// provider or adapter fault behind a successful-looking commit.
///
/// Nothing is written and nothing is enqueued -- which is the property the caller needs, because
/// the alternative is a second agent run on the same turn.
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

    complete_claimed_event(tx, request).await?;
    Ok(InboundCommitOutcome {
        disposition: CommitDisposition::Duplicate,
        message_id,
        thread_ids,
        task_id: task_for_message(tx, request.company_id, message_id).await?,
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

/// The run this message already caused, if it caused one.
async fn task_for_message(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    message_id: CanonicalMessageId,
) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM background_tasks WHERE company_id = $1 AND source_message_uuid = $2",
    )
    .bind(company_id)
    .bind(message_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)
}

/// Verify every association binding belongs to the stated tenant/channel and return each once.
async fn verify_association_bindings(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<Vec<ChannelBindingId>> {
    let mut bindings = Vec::with_capacity(request.associations.len());
    for association in &request.associations {
        let valid: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                   SELECT 1 FROM channel_bindings AS binding
                   WHERE binding.id = $1 AND binding.company_id = $2
                     AND binding.channel_id = $3 AND binding.status = 'active'
               )"#,
        )
        .bind(association.binding_id.as_uuid())
        .bind(request.company_id)
        .bind(association.channel_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(AppError::from)?;
        if !valid {
            return Err(AppError::NotFound(format!(
                "Active binding {} does not belong to channel {}",
                association.binding_id, association.channel_id
            )));
        }
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
async fn resolve_threads(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
) -> AppResult<Vec<ResolvedThread>> {
    let mut threads = Vec::with_capacity(request.associations.len());
    let mut resolved_by_binding = HashMap::new();
    for association in &request.associations {
        let mapped = match resolved_by_binding.get(&association.binding_id) {
            Some(thread_id) => Some(*thread_id),
            None => {
                external::find_external_thread(
                    tx,
                    association.binding_id,
                    &request.envelope.source.thread_key,
                )
                .await?
            }
        };
        let thread_id = match mapped {
            Some(thread_id) => {
                verify_thread_scope(tx, request.company_id, association, thread_id).await?;
                thread_id
            }
            None => match &association.target {
                ThreadTarget::Existing(thread_id) => {
                    verify_thread_scope(tx, request.company_id, association, *thread_id).await?;
                    *thread_id
                }
                ThreadTarget::Create { subject } => {
                    open_thread(tx, request.company_id, association.channel_id, subject).await?
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

async fn verify_thread_scope(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    association: &ThreadAssociation,
    thread_id: Uuid,
) -> AppResult<()> {
    let scope: Option<(Uuid, Uuid)> =
        sqlx::query_as("SELECT company_id, channel_id FROM threads WHERE id = $1 FOR UPDATE")
            .bind(thread_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(AppError::from)?;
    match scope {
        Some((thread_company, thread_channel))
            if thread_company == company_id && thread_channel == association.channel_id =>
        {
            Ok(())
        }
        Some(_) => Err(AppError::Internal(format!(
            "Thread {thread_id} does not belong to channel {}",
            association.channel_id
        ))),
        None => Err(AppError::NotFound(format!(
            "Thread {thread_id} was not found"
        ))),
    }
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
    sqlx::query("UPDATE threads SET updated_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(thread_id)
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

/// Associate outreach responses and wake any satisfied waiting task inside this commit.
async fn apply_outreach_transitions(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    message_id: CanonicalMessageId,
) -> AppResult<()> {
    for transition in &request.outreach_transitions {
        let association_id: Option<Uuid> = sqlx::query_scalar(
            r#"SELECT association.id
               FROM thread_messages AS association
               WHERE association.company_id = $1 AND association.channel_id = $2
                 AND association.message_id = $3"#,
        )
        .bind(request.company_id)
        .bind(transition.channel_id)
        .bind(message_id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(AppError::from)?;
        let association_id = association_id.ok_or_else(|| {
            AppError::BadRequest(format!(
                "Outreach transition channel {} has no message association",
                transition.channel_id
            ))
        })?;
        Box::pin(record_outreach_reply_on(
            tx,
            &transition.matched,
            association_id,
        ))
        .await?;
    }
    Ok(())
}

/// Create the agent-dispatch task, or return the one this message already has.
///
/// Keyed on the canonical message through `TaskSource::Message`, whose unique index absorbs a
/// redelivery: the second delivery of a message resolves to the first delivery's run rather than
/// starting a second one on the same turn.
async fn create_task(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    threads: &[ResolvedThread],
    message_id: CanonicalMessageId,
) -> AppResult<Option<Uuid>> {
    let Some(task) = request.task.as_ref() else {
        return Ok(None);
    };
    let thread_of: HashMap<Uuid, Uuid> = threads
        .iter()
        .map(|thread| (thread.channel_id, thread.thread_id))
        .collect();

    let targets = task_targets(task, &thread_of)?;
    let primary = *targets
        .first()
        .ok_or_else(|| AppError::Internal("An inbound task names no channel".into()))?;

    let payload = InboundTaskPayload::v1(InboundTaskPayloadV1 {
        company_id: request.company_id,
        channel_id: primary.channel_id,
        thread_id: primary.thread_id,
        source_message_id: message_id,
        correlation_id: request.envelope.correlation_id,
        hop_count: request.envelope.directives.hop_count,
        trace_channels: request.envelope.directives.trace_channels.clone(),
        is_forwarded: request.envelope.directives.is_forwarded,
        reply_delivery: request.reply_delivery,
    })
    .encode()?;

    let created = insert_task(
        tx,
        NewTask {
            company_id: request.company_id,
            channel_id: primary.channel_id,
            thread_id: Some(primary.thread_id),
            task_type: task.task_type.clone(),
            payload,
            source: TaskSource::Message(message_id),
            // The chain starts at the message, not here: a relayed reply carrying a correlation id
            // stays on the chain its sender was already on.
            correlation_id: request.envelope.correlation_id,
            targets,
        },
    )
    .await?;
    Ok(Some(created.id))
}

/// Pair every task target with the thread this commit put the message in.
///
/// A target naming a channel with no association would be a task pointing at a conversation the
/// message never joined, so it is refused here rather than written and discovered by the worker.
fn task_targets(
    task: &InboundTaskRequest,
    thread_of: &HashMap<Uuid, Uuid>,
) -> AppResult<Vec<crate::entities::task::TaskTarget>> {
    task.targets
        .iter()
        .map(|target| {
            let thread_id = thread_of.get(&target.channel_id).copied().ok_or_else(|| {
                AppError::Internal(format!(
                    "Task target channel {} has no association in this commit",
                    target.channel_id
                ))
            })?;
            Ok(crate::entities::task::TaskTarget {
                channel_id: target.channel_id,
                thread_id,
                recipient_role: target.role,
            })
        })
        .collect()
}

/// The immediate delivery fan-out this commit owes.
///
/// Empty for every path that exists today: email is the only interface a channel speaks, and
/// delivering a message back to the interface it arrived on is an echo. It is written here anyway,
/// in this transaction, because that is the property the whole commit exists for -- a mirror queued
/// separately could be lost by a crash that kept the message, and a mirror written first could be
/// sent for a message that was rolled back.
///
/// Two workers racing one provider redelivery both reach this, and the unique index on
/// `(destination_binding_id, idempotency_key)` absorbs the loser -- but only one of them gets past
/// the advisory lock and the redelivery check above, so in practice this writes once.
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

    Ok(MessageWrite {
        id: CanonicalMessageId::random(),
        thread_id: primary_thread_id,
        author: MessageAuthorWrite::Observed(observation(author)),
        subject: envelope.content.subject().to_string(),
        clean_text_body: envelope.content.body_text().to_string(),
        attachments: envelope.attachments.to_vec(),
        direction: MessageDirection::Inbound,
        // A relayed message is another agent talking, not a person.
        role: if envelope.directives.source_channel_id.is_some() {
            MessageRole::Agent
        } else {
            MessageRole::Human
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
        crate::entities::transport::TransportKind::Email => IdentityProvenance::EmailIngress,
        crate::entities::transport::TransportKind::Slack => IdentityProvenance::SlackEvent,
    };
    IdentityObservation {
        identity,
        display_label: None,
        claim_metadata: IdentityClaimMetadata::observation(),
        provenance,
    }
}
