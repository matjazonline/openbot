//! The purpose-built message reads.
//!
//! Each query selects the columns of exactly one projection in
//! [`crate::entities::message_view`] and joins only the tables that projection needs. A thread
//! page therefore never reads a raw MIME body, an agent prompt never reads a provider key, and
//! neither joins `email_message_metadata` at all -- which is what makes a message no mail carried
//! render through the same code as one that did.
//!
//! Every list here is bounded at the query. `THREAD_HISTORY_LIMIT` is the newest-N window the
//! product shows and the prompt reads; the cursor-driven reads take their bound from the caller
//! and are clamped to the same ceiling, because a caller is not a bound.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use std::str::FromStr;
use uuid::Uuid;

use super::message::decode_attachments;
use crate::{
    app_error::{AppError, AppResult},
    entities::{
        correlation::CorrelationId,
        cursor::MessageCursor,
        internal_note::{InternalNoteProvenance, InternalNoteView},
        message::{
            CanonicalMessageId, MessageAudience, MessageDirection, MessageRole, ThreadEntryKind,
        },
        message_view::{
            AgentHistoryMessage, AuthorView, EmailReplyContext, ExternalMessageRef,
            MessageAuditView, THREAD_HISTORY_LIMIT, ThreadMessageView,
        },
        transport::{ChannelBindingId, ExternalMessageKey, PrincipalId, TransportKind},
        value_objects::{EmailAddress, MessageId},
    },
};

/// The author columns every projection shares, and the joins that produce them.
///
/// One definition because the three queries below must agree on what "the author" is: the
/// principal's label, the handle they wrote under, and which interface that handle belongs to.
const AUTHOR_COLUMNS: &str = r#"
           message.author_principal_id,
           author.display_label AS author_label,
           author_identity.transport AS author_transport,
           author_identity.subject AS author_subject
"#;

const AUTHOR_JOINS: &str = r#"
    JOIN messages AS message
      ON (message.company_id, message.id) = (association.company_id, association.message_id)
    JOIN principals AS author
      ON (author.company_id, author.id) = (message.company_id, message.author_principal_id)
    LEFT JOIN participant_identities AS author_identity
      ON (author_identity.company_id, author_identity.id) =
         (message.company_id, message.authored_identity_id)
"#;

/// The author fields as every row below carries them.
#[derive(sqlx::FromRow, Debug)]
struct AuthorDb {
    author_principal_id: Uuid,
    author_label: String,
    author_transport: Option<String>,
    author_subject: Option<String>,
}

impl TryFrom<AuthorDb> for AuthorView {
    type Error = AppError;

    fn try_from(db: AuthorDb) -> AppResult<Self> {
        Ok(AuthorView {
            principal_id: PrincipalId::new(db.author_principal_id),
            label: db.author_label,
            handle: db.author_subject,
            transport: db
                .author_transport
                .as_deref()
                .map(transport_kind)
                .transpose()?,
        })
    }
}

/// Refuse a transport this build does not know rather than showing a message with no badge.
fn transport_kind(value: &str) -> AppResult<TransportKind> {
    TransportKind::from_str(value)
        .map_err(|error| AppError::Internal(format!("Stored identity is unusable: {error}")))
}

#[derive(sqlx::FromRow, Debug)]
struct ThreadMessageDb {
    id: Uuid,
    canonical_id: Uuid,
    thread_id: Uuid,
    #[sqlx(flatten)]
    author: AuthorDb,
    subject: String,
    clean_text_body: String,
    attachments: Option<Value>,
    direction: String,
    role: String,
    audience: String,
    entry_kind: String,
    created_at: DateTime<Utc>,
    correlation_id: Uuid,
    note_id: Option<Uuid>,
    note_author_principal_id: Option<Uuid>,
    note_provenance: Option<String>,
    supersedes_note_id: Option<Uuid>,
    superseded_by_note_id: Option<Uuid>,
    tombstoned_at: Option<DateTime<Utc>>,
}

impl ThreadMessageDb {
    fn into_view(self, task_id: Option<Uuid>) -> AppResult<ThreadMessageView> {
        let internal_note = match (
            self.note_id,
            self.note_author_principal_id,
            self.note_provenance,
        ) {
            (Some(id), Some(author_principal_id), Some(provenance)) => Some(InternalNoteView {
                id,
                author_principal_id: PrincipalId::new(author_principal_id),
                provenance: InternalNoteProvenance::from_str(&provenance)
                    .map_err(AppError::Internal)?,
                supersedes_note_id: self.supersedes_note_id,
                superseded_by_note_id: self.superseded_by_note_id,
                tombstoned_at: self.tombstoned_at,
            }),
            (None, None, None) => None,
            _ => {
                return Err(AppError::Internal(format!(
                    "Incomplete internal note metadata for message {}",
                    self.canonical_id
                )));
            }
        };
        Ok(ThreadMessageView {
            id: self.id,
            canonical_id: CanonicalMessageId::new(self.canonical_id),
            thread_id: self.thread_id,
            task_id,
            author: self.author.try_into()?,
            subject: self.subject,
            body: self.clean_text_body,
            attachments: decode_attachments(self.attachments)?.unwrap_or_default(),
            direction: MessageDirection::from_str(&self.direction)
                .map_err(|error| AppError::Internal(error.to_string()))?,
            role: MessageRole::from_str(&self.role)
                .map_err(|error| AppError::Internal(error.to_string()))?,
            audience: MessageAudience::from_str(&self.audience)
                .map_err(|error| AppError::Internal(error.to_string()))?,
            entry_kind: ThreadEntryKind::from_str(&self.entry_kind)
                .map_err(|error| AppError::Internal(error.to_string()))?,
            internal_note,
            created_at: self.created_at,
        })
    }
}

#[derive(sqlx::FromRow, Debug)]
struct ThreadTaskLookupDb {
    id: Uuid,
    source_message_uuid: Option<Uuid>,
    correlation_id: Uuid,
    task_type: String,
    created_at: DateTime<Utc>,
}

async fn fetch_thread_tasks(pool: &PgPool, thread_id: Uuid) -> AppResult<Vec<ThreadTaskLookupDb>> {
    sqlx::query_as::<_, ThreadTaskLookupDb>(
        r#"SELECT id, source_message_uuid, correlation_id, task_type, created_at
             FROM background_tasks
            WHERE thread_id = $1
            ORDER BY created_at ASC"#,
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::from)
}

fn match_task_id(
    canonical_id: Uuid,
    correlation_id: Uuid,
    tasks: &[ThreadTaskLookupDb],
) -> Option<Uuid> {
    if let Some(task) = tasks
        .iter()
        .find(|t| t.source_message_uuid == Some(canonical_id))
    {
        return Some(task.id);
    }

    let mut candidates: Vec<&ThreadTaskLookupDb> = tasks
        .iter()
        .filter(|t| t.correlation_id == correlation_id)
        .collect();

    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by_key(|t| {
        let is_main = t.task_type == "email_agent_dispatch" || t.task_type == "scheduled_agent_run";
        (!is_main, std::cmp::Reverse(t.created_at))
    });

    candidates.first().map(|t| t.id)
}

fn thread_message_select() -> String {
    format!(
        r#"
    SELECT association.id,
           message.id AS canonical_id,
           association.thread_id,
{AUTHOR_COLUMNS},
           message.subject,
           message.clean_text_body,
           message.attachments,
           message.direction,
           message.role,
           message.audience,
           association.entry_kind,
           association.created_at,
           message.correlation_id,
           note.id AS note_id,
           note.author_principal_id AS note_author_principal_id,
           note.provenance AS note_provenance,
           note.supersedes_note_id,
           successor.id AS superseded_by_note_id,
           tombstone.created_at AS tombstoned_at
    FROM thread_messages AS association
{AUTHOR_JOINS}
    LEFT JOIN internal_notes AS note
      ON (note.company_id, note.message_id) = (message.company_id, message.id)
    LEFT JOIN internal_notes AS successor ON successor.supersedes_note_id = note.id
    LEFT JOIN internal_note_tombstones AS tombstone ON tombstone.note_id = note.id
"#
    )
}

/// The newest turns of a thread, oldest first.
///
/// The window is the newest `THREAD_HISTORY_LIMIT`, then reversed: a page shows the end of a
/// conversation, and the alternative -- the oldest 200 -- would freeze a busy thread's view at
/// whatever it looked like when it started.
pub(super) async fn list_thread_messages(
    pool: &PgPool,
    thread_id: Uuid,
) -> AppResult<Vec<ThreadMessageView>> {
    let select = thread_message_select();
    let query = format!(
        r#"SELECT * FROM (
               {select}
               WHERE association.thread_id = $1
               ORDER BY association.created_at DESC, association.id DESC
               LIMIT $2
           ) recent
           ORDER BY recent.created_at ASC, recent.id ASC"#
    );
    let rows = sqlx::query_as::<_, ThreadMessageDb>(&query)
        .bind(thread_id)
        .bind(THREAD_HISTORY_LIMIT as i64)
        .fetch_all(pool)
        .await
        .map_err(AppError::from)?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let tasks = fetch_thread_tasks(pool, thread_id).await?;
    rows.into_iter()
        .map(|row| {
            let task_id = match_task_id(row.canonical_id, row.correlation_id, &tasks);
            row.into_view(task_id)
        })
        .collect()
}

/// The turns a live reader is missing, oldest first.
///
/// Ascending and unwrapped, unlike the read above: this walks *forwards* from a point the reader
/// has already seen, so there is no newest-N window to reverse. `(created_at, id)` as a row
/// comparison is exactly `thread_messages_thread_created_idx`.
pub(super) async fn list_thread_messages_after(
    pool: &PgPool,
    thread_id: Uuid,
    after: Option<MessageCursor>,
    limit: usize,
) -> AppResult<Vec<ThreadMessageView>> {
    let select = thread_message_select();
    let query = format!(
        r#"{select}
           WHERE association.thread_id = $1
             AND ($2::timestamptz IS NULL
                  OR (association.created_at, association.id) > ($2, $3))
           ORDER BY association.created_at ASC, association.id ASC
           LIMIT $4"#
    );
    let rows = sqlx::query_as::<_, ThreadMessageDb>(&query)
        .bind(thread_id)
        // Both sides stay `timestamptz`. Binding a naive value instead would make Postgres promote
        // it through the *session* `TimeZone` to compare it, so the same cursor would mean
        // different instants on a UTC server and a local one.
        .bind(after.map(|cursor| cursor.created_at))
        .bind(after.map(|cursor| cursor.id))
        .bind(limit.min(THREAD_HISTORY_LIMIT) as i64)
        .fetch_all(pool)
        .await
        .map_err(AppError::from)?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let tasks = fetch_thread_tasks(pool, thread_id).await?;
    rows.into_iter()
        .map(|row| {
            let task_id = match_task_id(row.canonical_id, row.correlation_id, &tasks);
            row.into_view(task_id)
        })
        .collect()
}

/// One message as a page renders it, scoped through the thread it is being read from.
pub(super) async fn get_thread_message(
    pool: &PgPool,
    thread_id: Uuid,
    message_id: CanonicalMessageId,
) -> AppResult<Option<ThreadMessageView>> {
    let select = thread_message_select();
    let query =
        format!("{select} WHERE association.thread_id = $1 AND association.message_id = $2");
    let row = sqlx::query_as::<_, ThreadMessageDb>(&query)
        .bind(thread_id)
        .bind(message_id.as_uuid())
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)?;

    let Some(row) = row else {
        return Ok(None);
    };

    let tasks = fetch_thread_tasks(pool, thread_id).await?;
    let task_id = match_task_id(row.canonical_id, row.correlation_id, &tasks);
    Ok(Some(row.into_view(task_id)?))
}

#[derive(sqlx::FromRow, Debug)]
struct AgentHistoryDb {
    role: String,
    audience: String,
    entry_kind: String,
    author_label: String,
    author_subject: Option<String>,
    subject: String,
    clean_text_body: String,
}

/// The thread so far, as an agent prompt reads it.
///
/// Four columns. An agent needs no ids, no addresses and no headers to follow a conversation, and
/// selecting any would put provider strings inside the prompt fence for nothing.
pub(super) async fn list_agent_history(
    pool: &PgPool,
    thread_id: Uuid,
) -> AppResult<Vec<AgentHistoryMessage>> {
    let rows = sqlx::query_as::<_, AgentHistoryDb>(
        r#"SELECT * FROM (
               SELECT message.role,
                      message.audience,
                      association.entry_kind,
                      author.display_label AS author_label,
                      author_identity.subject AS author_subject,
                      message.subject,
                      message.clean_text_body,
                      association.created_at,
                      association.id
                 FROM thread_messages AS association
                 JOIN messages AS message
                   ON (message.company_id, message.id) =
                      (association.company_id, association.message_id)
                 JOIN principals AS author
                   ON (author.company_id, author.id) =
                      (message.company_id, message.author_principal_id)
                LEFT JOIN participant_identities AS author_identity
                   ON (author_identity.company_id, author_identity.id) =
                      (message.company_id, message.authored_identity_id)
                WHERE association.thread_id = $1
                  AND NOT EXISTS (
                      SELECT 1 FROM internal_notes AS private_note
                       WHERE private_note.company_id = association.company_id
                         AND private_note.message_id = association.message_id
                  )
                ORDER BY association.created_at DESC, association.id DESC
                LIMIT $2
           ) recent
           ORDER BY recent.created_at ASC, recent.id ASC"#,
    )
    .bind(thread_id)
    .bind(THREAD_HISTORY_LIMIT as i64)
    .fetch_all(pool)
    .await
    .map_err(AppError::from)?;

    rows.into_iter()
        .map(|row| {
            Ok(AgentHistoryMessage {
                role: MessageRole::from_str(&row.role)
                    .map_err(|error| AppError::Internal(error.to_string()))?,
                audience: MessageAudience::from_str(&row.audience)
                    .map_err(|error| AppError::Internal(error.to_string()))?,
                entry_kind: ThreadEntryKind::from_str(&row.entry_kind)
                    .map_err(|error| AppError::Internal(error.to_string()))?,
                author_display: display_name(row.author_label, row.author_subject),
                subject: row.subject,
                body: row.clean_text_body,
            })
        })
        .collect()
}

/// The same rule [`AuthorView::display`] applies, for the projections that carry only the name.
fn display_name(label: String, handle: Option<String>) -> String {
    match label.trim() {
        "" => handle.unwrap_or_else(|| "Unknown".to_string()),
        _ => label,
    }
}

#[derive(sqlx::FromRow, Debug)]
struct EmailReplyContextDb {
    canonical_id: Uuid,
    author_transport: Option<String>,
    author_subject: Option<String>,
    rfc_message_id: Option<String>,
    references_list: Option<Vec<String>>,
    cc: Vec<String>,
}

/// What the mail renderer needs to answer the newest message in a thread.
///
/// The only projection that joins `email_message_metadata`, and it returns `None` fields rather
/// than fabricating them: a thread whose newest turn arrived over a transport with no headers has
/// no `Message-ID` to reply to, and the caller has to decide what to do about that.
pub(super) async fn latest_email_reply_context(
    pool: &PgPool,
    thread_id: Uuid,
) -> AppResult<Option<EmailReplyContext>> {
    let row = sqlx::query_as::<_, EmailReplyContextDb>(
        r#"SELECT message.id AS canonical_id,
                  author_identity.transport AS author_transport,
                  author_identity.subject AS author_subject,
                  email.rfc_message_id,
                  email.references_list,
                  COALESCE((
                      SELECT array_agg(identity.subject ORDER BY participant.position)
                        FROM message_participants AS participant
                        JOIN participant_identities AS identity
                          ON (identity.company_id, identity.id) =
                             (participant.company_id, participant.participant_identity_id)
                       WHERE participant.company_id = message.company_id
                         AND participant.message_id = message.id
                         AND participant.kind = 'cc'
                         AND identity.transport = 'email'
                  ), ARRAY[]::text[]) AS cc
             FROM thread_messages AS association
             JOIN messages AS message
               ON (message.company_id, message.id) =
                  (association.company_id, association.message_id)
             LEFT JOIN participant_identities AS author_identity
               ON (author_identity.company_id, author_identity.id) =
                  (message.company_id, message.authored_identity_id)
            LEFT JOIN email_message_metadata AS email
              ON (email.company_id, email.message_id) = (message.company_id, message.id)
            WHERE association.thread_id = $1
            ORDER BY association.created_at DESC, association.id DESC
            LIMIT 1"#,
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::from)?;

    Ok(row.map(email_reply_context))
}

/// Find the external inbound turn a human completion is answering. Internal quiet context and
/// agent messages may be newer in the canonical thread, but neither supplies a customer address
/// and neither may redirect the final response.
pub(super) async fn latest_replyable_email_context(
    pool: &PgPool,
    thread_id: Uuid,
) -> AppResult<Option<EmailReplyContext>> {
    let row = sqlx::query_as::<_, EmailReplyContextDb>(
        r#"SELECT message.id AS canonical_id,
                  author_identity.transport AS author_transport,
                  author_identity.subject AS author_subject,
                  email.rfc_message_id,
                  email.references_list,
                  COALESCE((
                      SELECT array_agg(identity.subject ORDER BY participant.position)
                        FROM message_participants AS participant
                        JOIN participant_identities AS identity
                          ON (identity.company_id, identity.id) =
                             (participant.company_id, participant.participant_identity_id)
                       WHERE participant.company_id = message.company_id
                         AND participant.message_id = message.id
                         AND participant.kind = 'cc'
                         AND identity.transport = 'email'
                  ), ARRAY[]::text[]) AS cc
             FROM thread_messages AS association
             JOIN messages AS message
               ON (message.company_id, message.id) =
                  (association.company_id, association.message_id)
             JOIN participant_identities AS author_identity
               ON (author_identity.company_id, author_identity.id) =
                  (message.company_id, message.authored_identity_id)
              AND author_identity.transport = 'email'
             JOIN email_message_metadata AS email
               ON (email.company_id, email.message_id) = (message.company_id, message.id)
            WHERE association.thread_id = $1
              AND message.direction = 'inbound'
              AND message.audience = 'external_conversation'
              AND association.entry_kind = 'conversation'
            ORDER BY association.created_at DESC, association.id DESC
            LIMIT 1"#,
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::from)?;

    Ok(row.map(email_reply_context))
}

fn email_reply_context(row: EmailReplyContextDb) -> EmailReplyContext {
    let author_email = row
        .author_subject
        .filter(|_| row.author_transport.as_deref() == Some(TransportKind::Email.as_str()))
        .map(EmailAddress::from);

    EmailReplyContext {
        canonical_id: CanonicalMessageId::new(row.canonical_id),
        author_email,
        rfc_message_id: row.rfc_message_id.map(MessageId::from),
        references: row
            .references_list
            .unwrap_or_default()
            .into_iter()
            .map(MessageId::from)
            .collect(),
        cc: row.cc.into_iter().map(EmailAddress::from).collect(),
    }
}

/// The newest RFC Message-ID in a thread, looking back past turns with no email headers.
pub(super) async fn latest_thread_rfc_message_id(
    pool: &PgPool,
    thread_id: Uuid,
) -> AppResult<Option<MessageId>> {
    let row: Option<String> = sqlx::query_scalar(
        r#"SELECT email.rfc_message_id
             FROM thread_messages AS association
             JOIN email_message_metadata AS email
               ON (email.company_id, email.message_id) =
                  (association.company_id, association.message_id)
            WHERE association.thread_id = $1
              AND email.rfc_message_id IS NOT NULL
            ORDER BY association.created_at DESC, association.id DESC
            LIMIT 1"#,
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::from)?;

    Ok(row.map(MessageId::from))
}

#[derive(sqlx::FromRow, Debug)]
struct MessageAuditDb {
    id: Uuid,
    canonical_id: Uuid,
    company_id: Uuid,
    thread_id: Uuid,
    channel_id: Uuid,
    #[sqlx(flatten)]
    author: AuthorDb,
    direction: String,
    role: String,
    audience: String,
    entry_kind: String,
    correlation_id: Uuid,
    external_keys: Value,
    created_at: DateTime<Utc>,
}

#[derive(serde::Deserialize)]
struct ExternalKeyRow {
    binding_id: Uuid,
    transport: String,
    key: String,
}

/// One message with the provider keys that reach it, for an authorized diagnostic pane.
///
/// Tenant-scoped by `company_id` in the predicate rather than by whatever the caller believed:
/// this is the read that would otherwise let a guessed association id return another company's
/// correlation trail.
pub(super) async fn get_message_audit(
    pool: &PgPool,
    company_id: Uuid,
    association_id: Uuid,
) -> AppResult<Option<MessageAuditView>> {
    let query = format!(
        r#"
    SELECT association.id,
           message.id AS canonical_id,
           message.company_id,
           association.thread_id,
           association.channel_id,
{AUTHOR_COLUMNS},
           message.direction,
           message.role,
           message.audience,
           association.entry_kind,
           message.correlation_id,
           association.created_at,
           COALESCE((
               SELECT jsonb_agg(jsonb_build_object(
                          'binding_id', external.binding_id,
                          'transport', binding.transport,
                          'key', external.external_message_key)
                      ORDER BY external.created_at, external.id)
                 FROM external_messages AS external
                 JOIN channel_bindings AS binding
                   ON (binding.company_id, binding.id) =
                      (external.company_id, external.binding_id)
                WHERE external.company_id = message.company_id
                  AND external.message_id = message.id
           ), '[]'::jsonb) AS external_keys
    FROM thread_messages AS association
{AUTHOR_JOINS}
    WHERE association.id = $1 AND association.company_id = $2"#
    );

    let Some(db) = sqlx::query_as::<_, MessageAuditDb>(&query)
        .bind(association_id)
        .bind(company_id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)?
    else {
        return Ok(None);
    };

    let rows: Vec<ExternalKeyRow> = serde_json::from_value(db.external_keys).map_err(|error| {
        AppError::Internal(format!("Unreadable external message keys: {error}"))
    })?;
    let mut external_keys = Vec::with_capacity(rows.len());
    for row in rows {
        external_keys.push(ExternalMessageRef {
            binding_id: ChannelBindingId::new(row.binding_id),
            transport: transport_kind(&row.transport)?,
            key: ExternalMessageKey::parse(row.key).map_err(|error| {
                AppError::Internal(format!("Stored provider key is unusable: {error}"))
            })?,
        });
    }

    Ok(Some(MessageAuditView {
        id: db.id,
        canonical_id: CanonicalMessageId::new(db.canonical_id),
        company_id: db.company_id,
        thread_id: db.thread_id,
        channel_id: db.channel_id,
        author: db.author.try_into()?,
        direction: MessageDirection::from_str(&db.direction)
            .map_err(|error| AppError::Internal(error.to_string()))?,
        role: MessageRole::from_str(&db.role)
            .map_err(|error| AppError::Internal(error.to_string()))?,
        audience: MessageAudience::from_str(&db.audience)
            .map_err(|error| AppError::Internal(error.to_string()))?,
        entry_kind: ThreadEntryKind::from_str(&db.entry_kind)
            .map_err(|error| AppError::Internal(error.to_string()))?,
        correlation_id: CorrelationId::from(db.correlation_id),
        external_keys,
        created_at: db.created_at,
    }))
}
