//! Canonical message writes and reads.
//!
//! One `messages` row is the payload; one `thread_messages` row per conversation it landed in is
//! the association. Everything protocol-shaped -- the RFC headers, the sender/to/cc projection, the
//! provider keys -- hangs off that payload in its own table, so storing a Slack post, a schedule's
//! prompt or an agent's answer needs none of it.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{email_metadata, external};
use crate::{
    adapters::persistence::participant::{
        create_agent_principal_on, ensure_system_principal_on,
        resolve_or_create_external_identity_on,
    },
    app_error::{AppError, AppResult},
    entities::{
        correlation::CorrelationId,
        email_message::EmailMessageMetadata,
        message::{
            AttachmentMetadata, CanonicalMessageId, Message, MessageAttachments, MessageAuthor,
            MessageDirection, MessageParticipant, MessageParticipantKind, MessageRole,
        },
        transport::{
            ExternalMessageKey, ExternalThreadKey, IdentityNamespace, IdentitySubject,
            ParticipantIdentityId, PrincipalId, QualifiedIdentity, TransportKind,
        },
    },
    use_cases::thread::{MessageAuthorWrite, MessageCorrelation, MessageWrite},
};

/// One canonical message as it appears in one thread, plus the projections a reader needs.
///
/// Wide on purpose: assembling a message from four tables in one query is what keeps the mailbox's
/// per-message cost at one round trip rather than four.
#[derive(sqlx::FromRow, Debug)]
pub(super) struct MessageDb {
    pub id: Uuid,
    pub canonical_id: Uuid,
    pub company_id: Uuid,
    pub thread_id: Uuid,
    pub author_principal_id: Uuid,
    pub authored_identity_id: Option<Uuid>,
    pub author_label: String,
    pub author_transport: Option<String>,
    pub author_namespace: Option<String>,
    pub author_subject: Option<String>,
    pub subject: String,
    pub clean_text_body: String,
    pub attachments: Option<Value>,
    pub direction: String,
    pub role: String,
    pub correlation_id: Uuid,
    pub participants: Value,
    pub created_at: DateTime<Utc>,
}

/// The aggregated participant projection, as the query builds it.
///
/// Decoded fallibly rather than trusted: the JSON is assembled by the database from columns whose
/// vocabulary Rust also has to agree with, and a value neither side recognizes must be an error.
#[derive(Debug, Deserialize)]
struct ParticipantRow {
    kind: String,
    position: i32,
    identity_id: Uuid,
    transport: String,
    namespace: String,
    subject: String,
}

pub(super) const MESSAGE_SELECT: &str = r#"
    SELECT association.id,
           message.id AS canonical_id,
           message.company_id,
           association.thread_id,
           message.author_principal_id,
           message.authored_identity_id,
           author.display_label AS author_label,
           author_identity.transport AS author_transport,
           author_identity.namespace AS author_namespace,
           author_identity.subject AS author_subject,
           message.subject,
           message.clean_text_body,
           message.attachments,
           message.direction,
           message.role,
           message.correlation_id,
           COALESCE((
               SELECT jsonb_agg(jsonb_build_object(
                          'kind', participant.kind,
                          'position', participant.position,
                          'identity_id', participant.participant_identity_id,
                          'transport', identity.transport,
                          'namespace', identity.namespace,
                          'subject', identity.subject)
                      ORDER BY participant.kind, participant.position)
                 FROM message_participants AS participant
                 JOIN participant_identities AS identity
                   ON (identity.company_id, identity.id) =
                      (participant.company_id, participant.participant_identity_id)
                WHERE participant.company_id = message.company_id
                  AND participant.message_id = message.id
           ), '[]'::jsonb) AS participants,
           association.created_at
    FROM thread_messages AS association
    JOIN messages AS message
      ON (message.company_id, message.id) = (association.company_id, association.message_id)
    JOIN principals AS author
      ON (author.company_id, author.id) = (message.company_id, message.author_principal_id)
    LEFT JOIN participant_identities AS author_identity
      ON (author_identity.company_id, author_identity.id) =
         (message.company_id, message.authored_identity_id)
"#;

impl TryFrom<MessageDb> for Message {
    type Error = AppError;

    fn try_from(db: MessageDb) -> AppResult<Self> {
        let direction = MessageDirection::from_str(&db.direction)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let role = MessageRole::from_str(&db.role)
            .map_err(|error| AppError::Internal(error.to_string()))?;

        Ok(Message {
            id: db.id,
            canonical_id: CanonicalMessageId::new(db.canonical_id),
            company_id: db.company_id,
            thread_id: db.thread_id,
            author: MessageAuthor {
                principal_id: PrincipalId::new(db.author_principal_id),
                identity_id: db.authored_identity_id.map(ParticipantIdentityId::new),
                label: db.author_label,
                identity: qualified_identity(
                    db.author_transport,
                    db.author_namespace,
                    db.author_subject,
                )?,
            },
            subject: db.subject,
            clean_text_body: db.clean_text_body,
            attachments: decode_attachments(db.attachments)?,
            direction,
            role,
            correlation_id: CorrelationId::from(db.correlation_id),
            participants: decode_participants(db.participants)?,
            created_at: db.created_at,
        })
    }
}

/// Rebuild a stored handle, refusing a transport or a bound this build does not recognize.
fn qualified_identity(
    transport: Option<String>,
    namespace: Option<String>,
    subject: Option<String>,
) -> AppResult<Option<QualifiedIdentity>> {
    let (Some(transport), Some(namespace), Some(subject)) = (transport, namespace, subject) else {
        return Ok(None);
    };
    let invalid =
        |error: String| AppError::Internal(format!("Stored identity is unusable: {error}"));
    Ok(Some(QualifiedIdentity::new(
        TransportKind::from_str(&transport).map_err(|error| invalid(error.to_string()))?,
        IdentityNamespace::parse(namespace).map_err(|error| invalid(error.to_string()))?,
        IdentitySubject::parse(subject).map_err(|error| invalid(error.to_string()))?,
    )))
}

fn decode_participants(value: Value) -> AppResult<Vec<MessageParticipant>> {
    let rows: Vec<ParticipantRow> = serde_json::from_value(value)
        .map_err(|error| AppError::Internal(format!("Unreadable message participants: {error}")))?;
    rows.into_iter()
        .map(|row| {
            let position = u16::try_from(row.position).map_err(|_| {
                AppError::Internal(format!(
                    "Message participant position {} is out of range",
                    row.position
                ))
            })?;
            Ok(MessageParticipant {
                kind: MessageParticipantKind::from_str(&row.kind).map_err(AppError::Internal)?,
                position,
                identity_id: ParticipantIdentityId::new(row.identity_id),
                identity: qualified_identity(
                    Some(row.transport),
                    Some(row.namespace),
                    Some(row.subject),
                )?
                .ok_or_else(|| {
                    AppError::Internal("Message participant has no stored handle".into())
                })?,
            })
        })
        .collect()
}

/// Read attachment metadata back out of its versioned envelope.
///
/// Fallible on purpose: this is untrusted, long-lived JSON, and a shape a rolling deploy has not
/// learned yet must surface as an application error rather than as a panic in a request handler.
pub(super) fn decode_attachments(
    value: Option<Value>,
) -> AppResult<Option<Vec<AttachmentMetadata>>> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let stored: MessageAttachments = serde_json::from_value(value)
        .map_err(|error| AppError::Internal(format!("Unreadable message attachments: {error}")))?;
    Ok(Some(stored.into_items()))
}

pub(super) fn encode_attachments(attachments: &[AttachmentMetadata]) -> AppResult<Option<Value>> {
    if attachments.is_empty() {
        return Ok(None);
    }
    serde_json::to_value(MessageAttachments::new(attachments.to_vec()))
        .map(Some)
        .map_err(|error| AppError::Internal(format!("Failed to serialize attachments: {error}")))
}

/// The author of a message, once the handle a producer stated has been resolved to an actor.
pub(super) struct ResolvedAuthor {
    principal_id: PrincipalId,
    identity_id: Option<ParticipantIdentityId>,
    identity: Option<QualifiedIdentity>,
}

/// One participant, once its handle has been resolved and given its position.
pub(super) struct ResolvedParticipant {
    kind: MessageParticipantKind,
    position: u16,
    identity_id: ParticipantIdentityId,
    identity: QualifiedIdentity,
}

/// The company and channel a thread belongs to.
#[derive(Clone, Copy)]
pub(super) struct ThreadScope {
    pub company_id: Uuid,
    pub channel_id: Uuid,
}

pub(super) async fn thread_scope(
    connection: &mut sqlx::PgConnection,
    thread_id: Uuid,
) -> AppResult<ThreadScope> {
    let scope: Option<(Uuid, Uuid)> =
        sqlx::query_as("SELECT company_id, channel_id FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(AppError::from)?;
    scope
        .map(|(company_id, channel_id)| ThreadScope {
            company_id,
            channel_id,
        })
        .ok_or_else(|| AppError::NotFound(format!("Thread {thread_id} was not found")))
}

pub(super) async fn resolve_author(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    author: &MessageAuthorWrite,
) -> AppResult<ResolvedAuthor> {
    match author {
        MessageAuthorWrite::Observed(observation) => {
            let resolved =
                resolve_or_create_external_identity_on(connection, company_id, observation.clone())
                    .await?;
            Ok(ResolvedAuthor {
                principal_id: resolved.principal.id,
                identity_id: Some(resolved.identity.id),
                identity: Some(observation.identity.clone()),
            })
        }
        MessageAuthorWrite::Principal(principal_id) => Ok(ResolvedAuthor {
            principal_id: *principal_id,
            identity_id: None,
            identity: None,
        }),
        // Upserted rather than looked up: the principal is created with the agent, so this is the
        // same row every time, and writing the current name here is what keeps a renamed agent
        // from signing its next message with its old one.
        MessageAuthorWrite::Agent(agent) => Ok(ResolvedAuthor {
            principal_id: create_agent_principal_on(
                connection,
                company_id,
                agent.agent_id,
                &agent.display_label,
            )
            .await?,
            identity_id: None,
            identity: None,
        }),
        MessageAuthorWrite::Platform => Ok(ResolvedAuthor {
            principal_id: ensure_system_principal_on(connection, company_id).await?,
            identity_id: None,
            identity: None,
        }),
    }
}

/// Resolve each stated handle and give it its position within its role.
///
/// A handle repeated in the same role keeps its first position and is not written twice: the
/// `To:` header of a message that named someone twice still renders them once.
pub(super) async fn resolve_participants(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    write: &MessageWrite,
) -> AppResult<Vec<ResolvedParticipant>> {
    let mut resolved: Vec<ResolvedParticipant> = Vec::with_capacity(write.participants.len());
    for participant in &write.participants {
        if resolved
            .iter()
            .any(|seen| seen.kind == participant.kind && seen.identity == participant.identity)
        {
            continue;
        }
        let position = u16::try_from(
            resolved
                .iter()
                .filter(|seen| seen.kind == participant.kind)
                .count(),
        )
        .map_err(|_| AppError::BadRequest("Too many recipients in one role.".into()))?;
        let identity = resolve_or_create_external_identity_on(
            connection,
            company_id,
            write.participant_observation(participant.identity.clone()),
        )
        .await?;
        resolved.push(ResolvedParticipant {
            kind: participant.kind,
            position,
            identity_id: identity.identity.id,
            identity: participant.identity.clone(),
        });
    }
    Ok(resolved)
}

/// The fingerprint of everything a provider actually delivered.
///
/// Email hashes its raw body representations: `clean_text_body` is a rendering produced by quote
/// stripping against the receiving thread, so it is not stable across fan-out. A non-email event
/// has no raw representation elsewhere and hashes its canonical body instead. Identities include
/// transport and namespace as well as subject, so equal provider subjects in different workspaces
/// cannot collide.
pub(super) fn canonical_message_hash(
    write: &MessageWrite,
    author: &ResolvedAuthor,
    participants: &[ResolvedParticipant],
    attachments: Option<&Value>,
) -> Vec<u8> {
    let email = write.email_metadata();
    let canonical = serde_json::json!({
        "author_principal": author.principal_id.as_uuid(),
        "author_identity": author
            .identity
            .as_ref()
            .map(|identity| serde_json::json!({
                "transport": identity.transport().as_str(),
                "namespace": identity.namespace().as_str(),
                "subject": identity.subject().as_str(),
            })),
        "participants": participants
            .iter()
            .map(|participant| {
                serde_json::json!({
                    "kind": participant.kind.as_str(),
                    "position": participant.position,
                    "transport": participant.identity.transport().as_str(),
                    "namespace": participant.identity.namespace().as_str(),
                    "subject": participant.identity.subject().as_str(),
                })
            })
            .collect::<Vec<_>>(),
        "subject": write.subject,
        "canonical_body": email.is_none().then_some(write.clean_text_body.as_str()),
        "direction": write.direction.as_str(),
        "role": write.role.as_str(),
        "attachments": attachments,
        "rfc_message_id": email.map(|email| email.rfc_message_id.as_str()),
        "in_reply_to": email.and_then(|email| email.in_reply_to.as_deref()),
        "references": email.map(|email| {
            email
                .references
                .iter()
                .map(|reference| reference.as_str())
                .collect::<Vec<_>>()
        }),
        "thread_index": email.and_then(|email| email.thread_index.as_deref()),
        "raw_text": email.and_then(|email| email.raw_text_body.as_deref()),
        "raw_html": email.and_then(|email| email.raw_html_body.as_deref()),
    });
    Sha256::digest(canonical.to_string().as_bytes()).to_vec()
}

/// Store one message and attach it to its thread, on a caller-supplied connection.
///
/// Extracted so that a caller which must land this write together with others -- the agent
/// dispatch commits its reply, its delivery and its task payload as one transaction -- can reuse
/// exactly this path rather than keep a second copy of it in step with this one.
///
/// Returns both identities the write produced: the canonical payload and its association with
/// this thread. Callers need different ones -- a reader loads by association, a caller attaching
/// the same message to further threads needs the canonical id -- and deriving either from the
/// other would be a second query.
pub(crate) async fn insert_message_on(
    connection: &mut sqlx::PgConnection,
    write: &MessageWrite,
) -> AppResult<InsertedMessage> {
    let scope = thread_scope(connection, write.thread_id).await?;
    let author = resolve_author(connection, scope.company_id, &write.author).await?;
    let participants = resolve_participants(connection, scope.company_id, write).await?;
    let attachments = encode_attachments(&write.attachments)?;
    let content_hash = canonical_message_hash(write, &author, &participants, attachments.as_ref());

    let canonical_id = match &write.correlation {
        MessageCorrelation::Email(metadata) => {
            store_email_correlated(
                connection,
                scope,
                StoredPayload {
                    write,
                    author: &author,
                    participants: &participants,
                    attachments: attachments.as_ref(),
                    content_hash: &content_hash,
                },
                metadata,
            )
            .await?
        }
        MessageCorrelation::Internal => {
            let canonical_id = insert_canonical_message(
                connection,
                scope.company_id,
                write,
                &author,
                attachments.as_ref(),
                &content_hash,
            )
            .await?;
            insert_participants(connection, scope.company_id, canonical_id, &participants).await?;
            canonical_id
        }
    };

    let association_id = insert_thread_association(
        connection,
        scope,
        AssociationWrite {
            thread_id: write.thread_id,
            created_at: write.created_at,
        },
        canonical_id,
    )
    .await?;

    sqlx::query("UPDATE threads SET updated_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(write.thread_id)
        .execute(&mut *connection)
        .await
        .map_err(AppError::from)?;
    Ok(InsertedMessage {
        canonical_id,
        association_id,
    })
}

/// What one message write produced.
///
/// `canonical_id` is not always `MessageWrite::id`: a provider redelivery resolves to the message
/// already stored under that key, and this is the id it actually has.
pub(crate) struct InsertedMessage {
    pub canonical_id: CanonicalMessageId,
    pub association_id: Uuid,
}

/// Attach a canonical message that already exists to another thread.
///
/// The composite foreign key on `thread_messages` is what enforces tenancy: naming a thread in
/// another company fails the constraint rather than quietly widening who can read the message.
pub(crate) async fn associate_message_on(
    connection: &mut sqlx::PgConnection,
    thread_id: Uuid,
    message_id: CanonicalMessageId,
) -> AppResult<Uuid> {
    let scope = thread_scope(connection, thread_id).await?;
    let write = AssociationWrite {
        thread_id,
        created_at: Utc::now(),
    };
    let association_id = insert_thread_association(connection, scope, write, message_id).await?;
    sqlx::query("UPDATE threads SET updated_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(thread_id)
        .execute(&mut *connection)
        .await
        .map_err(AppError::from)?;
    Ok(association_id)
}

/// Everything about one message that is already resolved by the time it is stored, so the
/// correlation branches take one value rather than five positional arguments of which three are
/// slices.
pub(super) struct StoredPayload<'a> {
    pub write: &'a MessageWrite,
    pub author: &'a ResolvedAuthor,
    pub participants: &'a [ResolvedParticipant],
    pub attachments: Option<&'a Value>,
    pub content_hash: &'a [u8],
}

/// Store a message mail carried, and bind the conversation it belongs to.
///
/// Dedup is by the provider key qualified by the interface that carried it. That is the whole
/// rule: a Message-ID is not a company-wide identity -- see `email_message_metadata` in the
/// migration for the inter-channel case that settles it.
async fn store_email_correlated(
    connection: &mut sqlx::PgConnection,
    scope: ThreadScope,
    payload: StoredPayload<'_>,
    metadata: &EmailMessageMetadata,
) -> AppResult<CanonicalMessageId> {
    let binding_id =
        external::canonical_email_binding(connection, scope.company_id, scope.channel_id).await?;
    let message_key = bounded_message_key(metadata.rfc_message_id.as_str())?;
    let thread_key = bounded_thread_key(metadata.conversation_root_key().as_str())?;

    let canonical_id =
        match external::find_external_message(connection, binding_id, &message_key).await? {
            Some(existing) => {
                external::reuse_or_reject(existing, binding_id, &message_key, payload.content_hash)?
            }
            None => {
                let canonical_id = insert_canonical_message(
                    connection,
                    scope.company_id,
                    payload.write,
                    payload.author,
                    payload.attachments,
                    payload.content_hash,
                )
                .await?;
                insert_participants(
                    connection,
                    scope.company_id,
                    canonical_id,
                    payload.participants,
                )
                .await?;
                email_metadata::insert_email_metadata_on(
                    connection,
                    scope.company_id,
                    canonical_id.as_uuid(),
                    metadata,
                )
                .await?;
                external::insert_external_message(
                    connection,
                    scope.company_id,
                    binding_id,
                    &message_key,
                    canonical_id,
                )
                .await?;
                canonical_id
            }
        };

    // Bound the conversation whether the message is new or a redelivery: a reply that arrives
    // before its root is what creates the binding the root later joins.
    external::upsert_external_thread(
        connection,
        scope.company_id,
        binding_id,
        &thread_key,
        payload.write.thread_id,
    )
    .await?;
    Ok(canonical_id)
}

pub(super) fn bounded_message_key(value: &str) -> AppResult<ExternalMessageKey> {
    ExternalMessageKey::parse(value)
        .map_err(|error| AppError::BadRequest(format!("Unusable provider message key: {error}")))
}

pub(super) fn bounded_thread_key(value: &str) -> AppResult<ExternalThreadKey> {
    ExternalThreadKey::parse(value)
        .map_err(|error| AppError::BadRequest(format!("Unusable provider thread key: {error}")))
}

pub(super) async fn insert_canonical_message(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    write: &MessageWrite,
    author: &ResolvedAuthor,
    attachments: Option<&Value>,
    content_hash: &[u8],
) -> AppResult<CanonicalMessageId> {
    let id = write.id;
    sqlx::query(
        r#"INSERT INTO messages (
                id, company_id, author_principal_id, authored_identity_id, subject,
                clean_text_body, attachments, direction, role, correlation_id, content_hash,
                created_at
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"#,
    )
    .bind(id.as_uuid())
    .bind(company_id)
    .bind(author.principal_id.as_uuid())
    .bind(author.identity_id.map(ParticipantIdentityId::as_uuid))
    .bind(&write.subject)
    .bind(&write.clean_text_body)
    .bind(attachments)
    .bind(write.direction.as_str())
    .bind(write.role.as_str())
    .bind(write.correlation_id.as_uuid())
    .bind(content_hash)
    .bind(write.created_at)
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;
    Ok(id)
}

pub(super) async fn insert_participants(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    message_id: CanonicalMessageId,
    participants: &[ResolvedParticipant],
) -> AppResult<()> {
    for participant in participants {
        sqlx::query(
            r#"INSERT INTO message_participants (
                    company_id, message_id, participant_identity_id, kind, position
               ) VALUES ($1, $2, $3, $4, $5)"#,
        )
        .bind(company_id)
        .bind(message_id.as_uuid())
        .bind(participant.identity_id.as_uuid())
        .bind(participant.kind.as_str())
        .bind(i32::from(participant.position))
        .execute(&mut *connection)
        .await
        .map_err(AppError::from)?;
    }
    Ok(())
}

/// Attach the canonical message to its thread, returning the association that already exists if
/// this is a redelivery of a message the thread already holds.
/// Just the two facts an association row needs of its own, so both callers -- a fresh message and
/// a message being attached to a further thread -- reach the same statement.
#[derive(Clone, Copy)]
pub(super) struct AssociationWrite {
    pub thread_id: Uuid,
    pub created_at: DateTime<Utc>,
}

pub(super) async fn insert_thread_association(
    connection: &mut sqlx::PgConnection,
    scope: ThreadScope,
    write: AssociationWrite,
    canonical_id: CanonicalMessageId,
) -> AppResult<Uuid> {
    let association_id: Option<Uuid> = sqlx::query_scalar(
        r#"INSERT INTO thread_messages (
                id, company_id, channel_id, thread_id, message_id, created_at
           ) VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (channel_id, message_id) DO NOTHING
           RETURNING id"#,
    )
    .bind(Uuid::new_v4())
    .bind(scope.company_id)
    .bind(scope.channel_id)
    .bind(write.thread_id)
    .bind(canonical_id.as_uuid())
    .bind(write.created_at)
    .fetch_optional(&mut *connection)
    .await
    .map_err(AppError::from)?;

    if let Some(association_id) = association_id {
        return Ok(association_id);
    }

    // The channel already holds this message. That is the redelivery case; it is only an error if
    // the existing association names a *different* thread, which would mean one conversation had
    // been split in two for the same audience.
    let existing: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, thread_id FROM thread_messages WHERE channel_id = $1 AND message_id = $2",
    )
    .bind(scope.channel_id)
    .bind(canonical_id.as_uuid())
    .fetch_optional(&mut *connection)
    .await
    .map_err(AppError::from)?;

    match existing {
        Some((association_id, thread_id)) if thread_id == write.thread_id => Ok(association_id),
        Some((_, thread_id)) => Err(AppError::Conflict(format!(
            "Message {canonical_id} is already part of thread {thread_id} in this channel"
        ))),
        None => Err(AppError::Internal(
            "Message association vanished during its own insert".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::value_objects::{MessageId, ObjectKey};

    fn slack_identity(namespace: &str, subject: &str) -> QualifiedIdentity {
        QualifiedIdentity::new(
            TransportKind::Slack,
            IdentityNamespace::parse(namespace).unwrap(),
            IdentitySubject::parse(subject).unwrap(),
        )
    }

    fn write_with(correlation: MessageCorrelation) -> MessageWrite {
        MessageWrite::internal(
            Uuid::new_v4(),
            MessageAuthorWrite::Platform,
            "Subject",
            "Body",
            MessageDirection::Inbound,
            MessageRole::Human,
            CorrelationId::new(),
        )
        .with_correlation(correlation)
    }

    fn author() -> ResolvedAuthor {
        ResolvedAuthor {
            principal_id: PrincipalId::new(Uuid::nil()),
            identity_id: None,
            identity: None,
        }
    }

    /// The reason `clean_text_body` is not hashed: one email fanned out to two channels is stripped
    /// against two different histories, and that must stay one canonical message rather than
    /// becoming a redelivery collision.
    #[test]
    fn a_differently_stripped_body_is_the_same_provider_payload() {
        let email = MessageCorrelation::Email(EmailMessageMetadata::new(MessageId::from(
            "<m@example.com>",
        )));
        let first = write_with(email.clone());
        let mut second = write_with(email);
        second.clean_text_body = "Body\n\n> quoted history".into();

        assert_eq!(
            canonical_message_hash(&first, &author(), &[], None),
            canonical_message_hash(&second, &author(), &[], None),
        );
    }

    #[test]
    fn changed_provider_content_changes_the_hash() {
        let base = EmailMessageMetadata::new(MessageId::from("<m@example.com>"));
        let first = write_with(MessageCorrelation::Email(base.clone()));
        let edited = write_with(MessageCorrelation::Email(
            base.raw_bodies(Some("edited".into()), None),
        ));

        assert_ne!(
            canonical_message_hash(&first, &author(), &[], None),
            canonical_message_hash(&edited, &author(), &[], None),
        );

        let mut resubjected = write_with(MessageCorrelation::Email(EmailMessageMetadata::new(
            MessageId::from("<m@example.com>"),
        )));
        resubjected.subject = "Different".into();
        assert_ne!(
            canonical_message_hash(&first, &author(), &[], None),
            canonical_message_hash(&resubjected, &author(), &[], None),
        );
    }

    #[test]
    fn equal_provider_subjects_in_different_namespaces_hash_differently() {
        let write = write_with(MessageCorrelation::Internal);
        let identity_id = ParticipantIdentityId::random();
        let first = ResolvedParticipant {
            kind: MessageParticipantKind::Sender,
            position: 0,
            identity_id,
            identity: slack_identity("workspace-a", "U123"),
        };
        let second = ResolvedParticipant {
            kind: MessageParticipantKind::Sender,
            position: 0,
            identity_id,
            identity: slack_identity("workspace-b", "U123"),
        };

        assert_ne!(
            canonical_message_hash(&write, &author(), &[first], None),
            canonical_message_hash(&write, &author(), &[second], None),
        );
    }

    #[test]
    fn attachments_round_trip_through_their_stored_envelope() {
        let attachments = vec![AttachmentMetadata {
            filename: "invoice.pdf".into(),
            content_type: "application/pdf".into(),
            sha256_hash: "deadbeef".into(),
            size_bytes: 2048,
            storage_key: Some(ObjectKey::from("attachments/invoice.pdf")),
        }];
        let encoded = encode_attachments(&attachments).unwrap().unwrap();

        assert_eq!(
            decode_attachments(Some(encoded)).unwrap(),
            Some(attachments)
        );
        assert_eq!(encode_attachments(&[]).unwrap(), None);
        assert_eq!(decode_attachments(None).unwrap(), None);
        assert_eq!(decode_attachments(Some(Value::Null)).unwrap(), None);
    }

    /// Stored JSON is untrusted input read back long after it was written. A shape this build does
    /// not know must surface as an application error, never as a panic in a request handler.
    #[test]
    fn unreadable_stored_json_is_an_application_error() {
        let future_version = serde_json::json!({ "version": "9", "items": [] });
        assert!(matches!(
            decode_attachments(Some(future_version)),
            Err(AppError::Internal(_))
        ));

        let unknown_kind = serde_json::json!([{
            "kind": "bcc", "position": 0, "identity_id": Uuid::nil(),
            "transport": "email", "namespace": "deployment", "subject": "a@example.com"
        }]);
        assert!(matches!(
            decode_participants(unknown_kind),
            Err(AppError::Internal(_))
        ));

        let unknown_transport = serde_json::json!([{
            "kind": "to", "position": 0, "identity_id": Uuid::nil(),
            "transport": "carrier-pigeon", "namespace": "deployment", "subject": "a@example.com"
        }]);
        assert!(matches!(
            decode_participants(unknown_transport),
            Err(AppError::Internal(_))
        ));
    }
}
