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
//! * A transaction-scoped advisory lock on `(binding_id, message_key)` serializes two deliveries of
//!   the same provider message. Without it the "is this already stored?" read and the insert that
//!   follows are a classic check-then-act, and two simultaneous SMTP sessions would both pass the
//!   check.
//! * Threads are bound to their provider conversation key with `ON CONFLICT DO NOTHING`, so a reply
//!   that arrives before its root creates the binding and the root joins the thread the reply
//!   started, rather than the two opening two conversations.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{email_metadata, external, message};
use crate::{
    adapters::persistence::{PostgresPersistence, task::insert_task},
    app_error::{AppError, AppResult},
    entities::{
        email_message::EmailMessageMetadata,
        message::{CanonicalMessageId, MessageDirection, MessageParticipantKind, MessageRole},
        participant::{IdentityClaimMetadata, IdentityProvenance},
        task::{NewTask, TaskSource},
        transport::{ChannelBindingId, DeliveryId, ExternalMessageKey, QualifiedIdentity},
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
    let envelope = &request.envelope;
    let binding_id = envelope.source.binding_id;
    let message_key = &envelope.source.message_key;
    lock_provider_message(tx, binding_id, message_key).await?;

    // Asked first, and under the lock. A redelivery must open no thread: the pre-canonical path
    // created the thread before it knew whether the message was new, so a provider retry left an
    // empty conversation behind and then failed associating the message it already held.
    if let Some(existing) = external::find_external_message(tx, binding_id, message_key).await? {
        return recognise_redelivery(tx, request, existing).await;
    }

    let threads = resolve_threads(tx, request).await?;
    bind_conversations(tx, request, &threads).await?;

    let write = message_write(request, threads[0].thread_id)?;
    let message_id = store_canonical(tx, request, &write).await?;

    associate_threads(tx, request, &threads, message_id).await?;
    map_provider_message(tx, request, &threads, message_id).await?;

    let task_id = create_task(tx, request, &threads, message_id).await?;
    let delivery_ids = create_deliveries(request)?;
    complete_claimed_event(request)?;

    Ok(InboundCommitOutcome {
        disposition: CommitDisposition::Created,
        message_id,
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
    existing: external::ExistingExternalMessage,
) -> AppResult<InboundCommitOutcome> {
    let message_id = existing.message_id;
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
    external::reuse_or_reject(
        existing,
        request.envelope.source.binding_id,
        &request.envelope.source.message_key,
        &content_hash,
    )?;

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

/// Serialize concurrent deliveries of one provider message on one interface.
///
/// Transaction-scoped, so it is released by commit or rollback with nothing to clean up, and keyed
/// on the pair that identifies the message rather than on a table: the row it protects does not
/// exist yet, which is exactly why a row lock cannot do this job.
async fn lock_provider_message(
    tx: &mut Transaction<'_, Postgres>,
    binding_id: ChannelBindingId,
    message_key: &ExternalMessageKey,
) -> AppResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("{binding_id}:{}", message_key.as_str()))
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    Ok(())
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
    for association in &request.associations {
        let thread_id = match &association.target {
            ThreadTarget::Existing(thread_id) => {
                verify_thread_scope(tx, request.company_id, association, *thread_id).await?;
                add_thread_principals(tx, request, association, *thread_id).await?;
                *thread_id
            }
            ThreadTarget::Create { subject } => {
                let thread_id =
                    open_thread(tx, request.company_id, association.channel_id, subject).await?;
                add_thread_principals(tx, request, association, thread_id).await?;
                thread_id
            }
        };
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
    let opens_thread = matches!(association.target, ThreadTarget::Create { .. });
    super::insert_thread_email_principals(
        tx,
        request.company_id,
        association.channel_id,
        thread_id,
        &association.participants,
        opens_thread,
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
/// Before the message is materialized, so that a delivery which fails afterwards still leaves the
/// conversation bound: the retry then lands in the thread the first attempt chose instead of
/// opening a second one.
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
) -> AppResult<CanonicalMessageId> {
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
    Ok(message_id)
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
/// an interface. `ON CONFLICT DO NOTHING` covers the case where the same channel already carried
/// this key on another association.
async fn map_provider_message(
    tx: &mut Transaction<'_, Postgres>,
    request: &InboundCommitRequest,
    threads: &[ResolvedThread],
    message_id: CanonicalMessageId,
) -> AppResult<()> {
    let key = request.envelope.source.message_key.as_str();
    let mut written = Vec::with_capacity(threads.len());
    for thread in threads {
        if written.contains(&thread.binding_id) {
            continue;
        }
        written.push(thread.binding_id);
        sqlx::query(
            r#"INSERT INTO external_messages (
                    id, company_id, binding_id, external_message_key, message_id
               ) VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (binding_id, external_message_key) DO NOTHING"#,
        )
        .bind(Uuid::new_v4())
        .bind(request.company_id)
        .bind(thread.binding_id.as_uuid())
        .bind(key)
        .bind(message_id.as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
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
/// delivering a message back to the interface it arrived on is an echo. The durable delivery queue
/// arrives in step 9, so a non-empty list here is a caller that has run ahead of its storage and is
/// refused rather than silently dropped -- an intent that vanishes is a message nobody ever sends.
fn create_deliveries(request: &InboundCommitRequest) -> AppResult<Vec<DeliveryId>> {
    if request.deliveries.is_empty() {
        return Ok(Vec::new());
    }
    Err(AppError::Internal(
        "Inbound delivery fan-out has no durable queue until the generic outbox exists".into(),
    ))
}

/// Mark the durable inbound event this commit consumed as complete.
///
/// Email has none: the SMTP transaction is the claim and it is held open until this returns, so
/// there is nothing to fence. A transport with a durable inbox supplies one in step 10, and it is
/// refused here rather than ignored, because an event that stays claimed is work that never runs
/// again.
fn complete_claimed_event(request: &InboundCommitRequest) -> AppResult<()> {
    match request.claimed_event {
        None => Ok(()),
        Some(_) => Err(AppError::Internal(
            "Inbound event completion has no durable inbox until the generic queue exists".into(),
        )),
    }
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
    IdentityObservation {
        identity,
        display_label: None,
        claim_metadata: IdentityClaimMetadata::observation(),
        provenance: IdentityProvenance::EmailIngress,
    }
}
