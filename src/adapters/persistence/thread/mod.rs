//! Threads, and the canonical messages associated with them.
//!
//! The module is split along the boundaries the storage model has: [`message`] owns the canonical
//! payload and its thread associations, [`external`] owns the provider keys those rows are reached
//! by, and [`email_metadata`] owns the headers only mail has. This file owns threads themselves and
//! the [`ThreadPersistence`] port that ties the three together.

mod email_metadata;
mod external;
mod inbound;
mod relay;
pub(in crate::adapters::persistence) use inbound::{AgentNoticeAuthority, commit_agent_notice_on};
mod internal_note;
mod message;
mod views;
pub(in crate::adapters::persistence) use views::THREAD_RUN_LOOKUP_SQL;

#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "read_tests.rs"]
mod read_tests;

#[cfg(test)]
#[path = "inbound_tests.rs"]
mod inbound_tests;

#[cfg(test)]
#[path = "view_tests.rs"]
mod view_tests;

pub(crate) use message::{MessageStampSource, associate_message_on, insert_message_on};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};
use uuid::Uuid;

use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::participant::resolve_or_create_external_identity_on;
use crate::adapters::protocols::email::EmailIdentity;
use crate::app_error::AppError;
use crate::app_error::AppResult;
use crate::application::ports::participant::IdentityObservation;
use crate::application::ports::thread::MessageWrite;
use crate::application::ports::thread::ThreadPersistence;
use crate::entities::address::AddressTarget;
use crate::entities::cursor::MessageCursor;
use crate::entities::cursor::ThreadCursor;
use crate::entities::internal_note::AddInternalNote;
use crate::entities::internal_note::InternalNoteView;
use crate::entities::internal_note::TombstoneInternalNote;
use crate::entities::message::CanonicalMessageId;
use crate::entities::message::Message;
use crate::entities::message::MessageRole;
use crate::entities::message_view::AgentHistoryMessage;
use crate::entities::message_view::EmailReplyContext;
use crate::entities::message_view::MessageAuditView;
use crate::entities::message_view::ThreadMessageView;
use crate::entities::participant::IdentityClaimMetadata;
use crate::entities::participant::IdentityProvenance;
use crate::entities::participant::ThreadPrincipalRole;
use crate::entities::thread::Thread;
use crate::entities::thread::ThreadParticipantProjection;
use crate::entities::transport::PrincipalId;
use crate::entities::transport::TransportKind;
use crate::entities::value_objects::EmailAddress;
use crate::entities::value_objects::MessageId;
use crate::entities::value_objects::ThreadIndex;
use crate::transport::{ThreadPrincipalIntent, ThreadPrincipalReference};

use message::{MESSAGE_SELECT, MessageDb};

#[derive(sqlx::FromRow, Debug)]
pub struct ThreadDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub principal_id: Option<Uuid>,
    pub subject: String,
    pub participant_principal_ids: Vec<Uuid>,
    pub participant_identities: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct ThreadIdentityRow {
    transport: String,
    namespace: String,
    subject: String,
}

impl TryFrom<ThreadDb> for Thread {
    type Error = AppError;

    fn try_from(db: ThreadDb) -> AppResult<Self> {
        let rows: Vec<ThreadIdentityRow> = serde_json::from_value(db.participant_identities)
            .map_err(|error| {
                AppError::Internal(format!("Stored thread identities are invalid: {error}"))
            })?;
        let identities = rows
            .into_iter()
            .map(|row| {
                Ok(crate::entities::transport::QualifiedIdentity::new(
                    TransportKind::from_str(&row.transport).map_err(|error| {
                        AppError::Internal(format!("Stored thread identity is invalid: {error}"))
                    })?,
                    crate::entities::transport::IdentityNamespace::parse(row.namespace).map_err(
                        |error| {
                            AppError::Internal(format!(
                                "Stored thread identity is invalid: {error}"
                            ))
                        },
                    )?,
                    crate::entities::transport::IdentitySubject::parse(row.subject).map_err(
                        |error| {
                            AppError::Internal(format!(
                                "Stored thread identity is invalid: {error}"
                            ))
                        },
                    )?,
                ))
            })
            .collect::<AppResult<Vec<_>>>()?;
        Ok(Thread {
            id: db.id,
            company_id: db.company_id,
            target: crate::entities::address::AddressTarget::from_parts(
                db.channel_id,
                db.principal_id.map(PrincipalId::new),
            )
            .ok_or_else(|| AppError::Internal(format!("Invalid target on thread {}", db.id)))?,
            subject: db.subject,
            participant_principal_ids: db
                .participant_principal_ids
                .into_iter()
                .map(PrincipalId::new)
                .collect(),
            participant_projection: ThreadParticipantProjection { identities },
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

const THREAD_SELECT: &str = r#"
    SELECT thread.id, thread.company_id, thread.channel_id, thread.principal_id, thread.subject,
           COALESCE(
               (SELECT array_agg(DISTINCT thread_principal.principal_id)
                  FROM thread_principals AS thread_principal
                 WHERE thread_principal.company_id = thread.company_id
                   AND thread_principal.thread_id = thread.id),
               ARRAY[]::uuid[]
           ) AS participant_principal_ids,
           COALESCE((
               SELECT jsonb_agg(jsonb_build_object(
                          'transport', identity.transport,
                          'namespace', identity.namespace,
                          'subject', identity.subject)
                      ORDER BY identity.transport, identity.namespace, identity.subject)
                 FROM (
                     SELECT DISTINCT handle.transport, handle.namespace, handle.subject
                       FROM thread_principals AS thread_principal
                       JOIN participant_identities AS handle
                         ON (handle.company_id, handle.principal_id) =
                            (thread_principal.company_id, thread_principal.principal_id)
                      WHERE thread_principal.company_id = thread.company_id
                        AND thread_principal.thread_id = thread.id
                        AND handle.status <> 'disabled'
                 ) AS identity
           ), '[]'::jsonb) AS participant_identities,
           thread.created_at, thread.updated_at
      FROM threads AS thread
"#;

async fn load_thread(pool: &PgPool, company_id: Uuid, id: Uuid) -> AppResult<Option<Thread>> {
    let query = format!("{THREAD_SELECT} WHERE thread.company_id = $1 AND thread.id = $2");
    let db = sqlx::query_as::<_, ThreadDb>(&query)
        .bind(company_id)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)?;
    db.map(Thread::try_from).transpose()
}

async fn load_message(pool: &PgPool, association_id: Uuid) -> AppResult<Message> {
    let query = format!("{MESSAGE_SELECT} WHERE association.id = $1");
    sqlx::query_as::<_, MessageDb>(&query)
        .bind(association_id)
        .fetch_one(pool)
        .await
        .map_err(AppError::from)?
        .try_into()
}

fn normalized_participants(participants: &[EmailAddress]) -> Vec<String> {
    let mut seen = HashSet::new();
    participants
        .iter()
        .filter_map(|email| {
            let normalized = email.trim().to_lowercase();
            (!normalized.is_empty() && seen.insert(normalized.clone())).then_some(normalized)
        })
        .collect()
}

async fn insert_thread_email_participants(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    target: AddressTarget,
    thread_id: Uuid,
    participant_emails: &[EmailAddress],
) -> AppResult<()> {
    let participants = normalized_participants(participant_emails);
    let mut intents = Vec::with_capacity(participants.len() + 1);
    if let Some(principal) = target.principal_id() {
        intents.push(ThreadPrincipalIntent::principal(
            principal,
            ThreadPrincipalRole::Participant,
        ));
    }
    if participants.len() + intents.len() > crate::transport::MAX_THREAD_PRINCIPALS {
        return Err(AppError::BadRequest("Too many thread participants".into()));
    }
    for email in participants {
        let identity = EmailIdentity::parse(EmailAddress::from(email))
            .map(EmailIdentity::qualify_default)
            .map_err(|error| {
                AppError::BadRequest(format!("Invalid thread participant: {error}"))
            })?;
        intents.push(ThreadPrincipalIntent::new(
            identity,
            ThreadPrincipalRole::Participant,
        ));
    }
    insert_thread_principals(transaction, company_id, target, thread_id, &intents).await
}

/// Resolve transport-qualified handles and record their explicitly stated thread roles.
pub(super) async fn insert_thread_principals(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    target: AddressTarget,
    thread_id: Uuid,
    intents: &[ThreadPrincipalIntent],
) -> AppResult<()> {
    let mut seen = HashSet::new();
    for intent in intents {
        if !seen.insert((intent.reference.clone(), intent.role)) {
            continue;
        }
        let principal = match &intent.reference {
            ThreadPrincipalReference::Known(principal) => *principal,
            ThreadPrincipalReference::Observed(identity) => {
                resolve_or_create_external_identity_on(
                    transaction,
                    company_id,
                    IdentityObservation {
                        identity: identity.clone(),
                        display_label: None,
                        claim_metadata: IdentityClaimMetadata::observation(),
                        provenance: IdentityProvenance::TransportIngress,
                    },
                )
                .await?
                .principal
                .id
            }
        };
        sqlx::query(
            r#"INSERT INTO thread_principals
                   (company_id, channel_id, thread_id, principal_id, role, target_principal_id)
               VALUES ($1, $2, $3, $4, $5, $6)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(company_id)
        .bind(target.desk_id())
        .bind(thread_id)
        .bind(principal.as_uuid())
        .bind(intent.role.as_str())
        .bind(target.principal_id().map(PrincipalId::as_uuid))
        .execute(&mut **transaction)
        .await
        .map_err(AppError::from)?;
    }
    Ok(())
}

#[async_trait]
impl ThreadPersistence for PostgresPersistence {
    async fn thread_access_levels(
        &self,
        company_id: Uuid,
        thread_ids: &[Uuid],
    ) -> AppResult<std::collections::HashMap<Uuid, crate::entities::access_level::AccessLevel>>
    {
        #[derive(sqlx::FromRow)]
        struct LevelRow {
            thread_id: Uuid,
            access_level: i16,
        }
        let rows = sqlx::query_as::<_, LevelRow>(
            "SELECT thread.id AS thread_id, COALESCE(MAX(message.access_level),0)::smallint AS access_level \
             FROM threads AS thread LEFT JOIN thread_messages AS association \
               ON (association.company_id,association.thread_id)=(thread.company_id,thread.id) \
             LEFT JOIN messages AS message ON (message.company_id,message.id)=(association.company_id,association.message_id) \
             WHERE thread.company_id=$1 AND thread.id=ANY($2) GROUP BY thread.id")
            .bind(company_id).bind(thread_ids).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let level = crate::entities::access_level::AccessLevel::try_from(row.access_level)
                    .map_err(|error| AppError::Internal(error.into()))?;
                Ok((row.thread_id, level))
            })
            .collect()
    }
    async fn create_thread(
        &self,
        company_id: Uuid,
        target: AddressTarget,
        subject: &str,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread> {
        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO threads(id,company_id,channel_id,principal_id,subject) VALUES($1,$2,$3,$4,$5)")
            .bind(id).bind(company_id).bind(target.desk_id())
            .bind(target.principal_id().map(PrincipalId::as_uuid)).bind(subject)
            .execute(&mut *tx).await?;
        insert_thread_email_participants(&mut tx, company_id, target, id, participant_emails)
            .await?;
        tx.commit().await?;
        load_thread(&self.pool, company_id, id)
            .await?
            .ok_or_else(|| AppError::Internal("Created thread was not found".into()))
    }

    async fn get_thread_by_id(&self, company_id: Uuid, id: Uuid) -> AppResult<Option<Thread>> {
        load_thread(&self.pool, company_id, id).await
    }

    async fn list_threads_by_target(
        &self,
        company_id: Uuid,
        target: AddressTarget,
        before: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        let query = format!(
            r#"{THREAD_SELECT}
            WHERE thread.company_id=$1 AND thread.channel_id IS NOT DISTINCT FROM $2
              AND thread.principal_id IS NOT DISTINCT FROM $3
              AND ($4::timestamptz IS NULL OR (thread.updated_at,thread.id)<($4,$5))
            ORDER BY thread.updated_at DESC,thread.id DESC LIMIT $6"#
        );
        let rows = sqlx::query_as::<_, ThreadDb>(&query)
            .bind(company_id)
            .bind(target.desk_id())
            .bind(target.principal_id().map(PrincipalId::as_uuid))
            .bind(before.map(|cursor| cursor.updated_at))
            .bind(before.map(|cursor| cursor.id))
            .bind(
                i64::try_from(limit)
                    .map_err(|_| AppError::BadRequest("Thread page limit is too large".into()))?,
            )
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(Thread::try_from).collect()
    }

    async fn list_thread_last_roles(
        &self,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, MessageRole>> {
        if thread_ids.is_empty() {
            return Ok(HashMap::new());
        }

        // `DISTINCT ON` with the column's own sort key, so this rides
        // `thread_messages_thread_created_idx` instead of reading each thread's history.
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            r#"SELECT DISTINCT ON (association.thread_id) association.thread_id, message.role
               FROM thread_messages AS association
               JOIN messages AS message
                 ON (message.company_id, message.id) =
                    (association.company_id, association.message_id)
               WHERE association.thread_id = ANY($1)
               ORDER BY association.thread_id, association.created_at DESC, association.id DESC"#,
        )
        .bind(thread_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        rows.into_iter()
            .map(|(thread_id, role)| {
                let role = MessageRole::from_str(&role)
                    .map_err(|error| AppError::Internal(error.to_string()))?;
                Ok((thread_id, role))
            })
            .collect()
    }

    async fn list_threads_updated_after(
        &self,
        company_id: Uuid,
        target: AddressTarget,
        after: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        let query = format!(
            r#"{THREAD_SELECT}
            WHERE thread.company_id=$1 AND thread.channel_id IS NOT DISTINCT FROM $2
              AND thread.principal_id IS NOT DISTINCT FROM $3
              AND ($4::timestamptz IS NULL OR (thread.updated_at,thread.id)>($4,$5))
            ORDER BY thread.updated_at ASC,thread.id ASC LIMIT $6"#
        );
        let rows = sqlx::query_as::<_, ThreadDb>(&query)
            .bind(company_id)
            .bind(target.desk_id())
            .bind(target.principal_id().map(PrincipalId::as_uuid))
            .bind(after.map(|cursor| cursor.updated_at))
            .bind(after.map(|cursor| cursor.id))
            .bind(
                i64::try_from(limit)
                    .map_err(|_| AppError::BadRequest("Thread page limit is too large".into()))?,
            )
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(Thread::try_from).collect()
    }

    async fn update_thread_participants(
        &self,
        company_id: Uuid,
        id: Uuid,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread> {
        let mut tx = self.pool.begin().await?;
        #[derive(sqlx::FromRow)]
        struct TargetRow {
            channel_id: Option<Uuid>,
            principal_id: Option<Uuid>,
        }
        let columns: Option<TargetRow> = sqlx::query_as(
            "SELECT channel_id,principal_id FROM threads WHERE company_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(columns) = columns else {
            return Err(AppError::NotFound("Thread not found".into()));
        };
        let target = AddressTarget::from_parts(
            columns.channel_id,
            columns.principal_id.map(PrincipalId::new),
        )
        .ok_or_else(|| AppError::Internal("Invalid stored thread target".into()))?;
        insert_thread_email_participants(&mut tx, company_id, target, id, participant_emails)
            .await?;
        sqlx::query(
            "UPDATE threads SET updated_at=CURRENT_TIMESTAMP WHERE company_id=$1 AND id=$2",
        )
        .bind(company_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        load_thread(&self.pool, company_id, id)
            .await?
            .ok_or_else(|| AppError::Internal("Updated thread was not found".into()))
    }

    async fn find_thread_by_thread_index(
        &self,
        company_id: Uuid,
        target: AddressTarget,
        thread_index: &ThreadIndex,
    ) -> AppResult<Option<Thread>> {
        let candidates = match thread_index.ancestor_chain() {
            Ok(candidates) => candidates,
            Err(_) => return Ok(None),
        };
        let values: Vec<&str> = candidates.iter().map(ThreadIndex::as_str).collect();
        let query = format!(
            r#"{THREAD_SELECT}
            JOIN thread_messages AS association ON association.company_id=thread.company_id
                AND association.thread_id=thread.id
            JOIN email_message_metadata AS email ON email.company_id=association.company_id
                AND email.message_id=association.message_id
            WHERE thread.company_id=$1 AND thread.channel_id IS NOT DISTINCT FROM $2
                AND thread.principal_id IS NOT DISTINCT FROM $3 AND email.thread_index=ANY($4)
            ORDER BY length(email.thread_index) DESC,association.created_at DESC,association.id DESC LIMIT 1"#
        );
        let row = sqlx::query_as::<_, ThreadDb>(&query)
            .bind(company_id)
            .bind(target.desk_id())
            .bind(target.principal_id().map(PrincipalId::as_uuid))
            .bind(&values)
            .fetch_optional(&self.pool)
            .await?;
        row.map(Thread::try_from).transpose()
    }

    async fn count_recent_messages(&self, thread_id: Uuid, duration_secs: i64) -> AppResult<usize> {
        let count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM thread_messages
               WHERE thread_id = $1
                 AND created_at >= CURRENT_TIMESTAMP - make_interval(secs => $2)"#,
        )
        .bind(thread_id)
        .bind(duration_secs as f64)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(count as usize)
    }

    async fn get_thread_message(
        &self,
        thread_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<Message>> {
        let query = format!(
            "{MESSAGE_SELECT} WHERE association.thread_id = $1 AND association.message_id = $2"
        );
        let db = sqlx::query_as::<_, MessageDb>(&query)
            .bind(thread_id)
            .bind(message_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        db.map(Message::try_from).transpose()
    }

    async fn find_thread_for_message(
        &self,
        company_id: Uuid,
        target: AddressTarget,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<Thread>> {
        let query = format!(
            r#"{THREAD_SELECT}
            JOIN thread_messages AS association ON association.company_id=thread.company_id
                AND association.thread_id=thread.id
            WHERE association.company_id=$1 AND association.channel_id IS NOT DISTINCT FROM $2
                AND association.principal_id IS NOT DISTINCT FROM $3 AND association.message_id=$4"#
        );
        let row = sqlx::query_as::<_, ThreadDb>(&query)
            .bind(company_id)
            .bind(target.desk_id())
            .bind(target.principal_id().map(PrincipalId::as_uuid))
            .bind(message_id.as_uuid())
            .fetch_optional(&self.pool)
            .await?;
        row.map(Thread::try_from).transpose()
    }

    async fn get_message_protocol_extension(
        &self,
        company_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<crate::application::ports::thread::ProtocolExtension> {
        Ok(
            email_metadata::load_email_metadata(&self.pool, company_id, message_id.as_uuid())
                .await?
                .map_or_else(
                    crate::application::ports::thread::ProtocolExtension::none,
                    crate::application::ports::thread::ProtocolExtension::email,
                ),
        )
    }

    async fn create_message(&self, write: &MessageWrite) -> AppResult<Message> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let inserted =
            insert_message_on(&mut tx, write, message::MessageStampSource::Public).await?;
        tx.commit().await.map_err(AppError::from)?;
        load_message(&self.pool, inserted.association_id).await
    }

    async fn find_outbound_reply_after(
        &self,
        thread_id: Uuid,
        answering: CanonicalMessageId,
    ) -> AppResult<Option<Message>> {
        let query = format!(
            r#"{MESSAGE_SELECT}
               WHERE association.thread_id = $1
                 AND message.direction = 'outbound'
                 AND association.entry_kind = 'conversation'
                 AND (association.created_at, association.id) >
                     (SELECT answered.created_at, answered.id
                        FROM thread_messages AS answered
                       WHERE answered.thread_id = $1 AND answered.message_id = $2)
               ORDER BY association.created_at DESC, association.id DESC
               LIMIT 1"#
        );
        let db = sqlx::query_as::<_, MessageDb>(&query)
            .bind(thread_id)
            .bind(answering.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        db.map(TryInto::try_into).transpose()
    }

    async fn associate_message(
        &self,
        thread_id: Uuid,
        message: CanonicalMessageId,
        entry_kind: crate::entities::message::ThreadEntryKind,
    ) -> AppResult<Message> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let association_id = associate_message_on(&mut tx, thread_id, message, entry_kind).await?;
        tx.commit().await.map_err(AppError::from)?;
        load_message(&self.pool, association_id).await
    }

    async fn list_thread_message_views(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Vec<ThreadMessageView>> {
        views::list_thread_messages(&self.pool, thread_id).await
    }

    async fn list_thread_message_views_after(
        &self,
        thread_id: Uuid,
        after: Option<MessageCursor>,
        limit: usize,
    ) -> AppResult<Vec<ThreadMessageView>> {
        views::list_thread_messages_after(&self.pool, thread_id, after, limit).await
    }

    async fn get_thread_message_view(
        &self,
        thread_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<ThreadMessageView>> {
        views::get_thread_message(&self.pool, thread_id, message_id).await
    }

    async fn list_agent_history(&self, thread_id: Uuid) -> AppResult<Vec<AgentHistoryMessage>> {
        views::list_agent_history(&self.pool, thread_id).await
    }

    async fn latest_email_reply_context(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Option<EmailReplyContext>> {
        views::latest_email_reply_context(&self.pool, thread_id).await
    }

    async fn latest_replyable_email_context(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Option<EmailReplyContext>> {
        views::latest_replyable_email_context(&self.pool, thread_id).await
    }

    async fn latest_thread_rfc_message_id(&self, thread_id: Uuid) -> AppResult<Option<MessageId>> {
        views::latest_thread_rfc_message_id(&self.pool, thread_id).await
    }

    async fn get_message_audit(
        &self,
        company_id: Uuid,
        association_id: Uuid,
    ) -> AppResult<Option<MessageAuditView>> {
        views::get_message_audit(&self.pool, company_id, association_id).await
    }

    async fn create_internal_note(
        &self,
        command: &AddInternalNote,
        actor: PrincipalId,
    ) -> AppResult<ThreadMessageView> {
        internal_note::create_internal_note(&self.pool, command, actor).await
    }

    async fn tombstone_internal_note(
        &self,
        command: &TombstoneInternalNote,
        actor: PrincipalId,
    ) -> AppResult<InternalNoteView> {
        internal_note::tombstone_internal_note(&self.pool, command, actor).await
    }
}
