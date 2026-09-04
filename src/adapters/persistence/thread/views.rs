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
        message::{CanonicalMessageId, MessageDirection, MessageRole},
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
    created_at: DateTime<Utc>,
}

impl TryFrom<ThreadMessageDb> for ThreadMessageView {
    type Error = AppError;

    fn try_from(db: ThreadMessageDb) -> AppResult<Self> {
        Ok(ThreadMessageView {
            id: db.id,
            canonical_id: CanonicalMessageId::new(db.canonical_id),
            thread_id: db.thread_id,
            author: db.author.try_into()?,
            subject: db.subject,
            body: db.clean_text_body,
            attachments: decode_attachments(db.attachments)?.unwrap_or_default(),
            direction: MessageDirection::from_str(&db.direction)
                .map_err(|error| AppError::Internal(error.to_string()))?,
            role: MessageRole::from_str(&db.role)
                .map_err(|error| AppError::Internal(error.to_string()))?,
            created_at: db.created_at,
        })
    }
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
           association.created_at
    FROM thread_messages AS association
{AUTHOR_JOINS}
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
    sqlx::query_as::<_, ThreadMessageDb>(&query)
        .bind(thread_id)
        .bind(THREAD_HISTORY_LIMIT as i64)
        .fetch_all(pool)
        .await
        .map_err(AppError::from)?
        .into_iter()
        .map(TryInto::try_into)
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
    sqlx::query_as::<_, ThreadMessageDb>(&query)
        .bind(thread_id)
        // Both sides stay `timestamptz`. Binding a naive value instead would make Postgres promote
        // it through the *session* `TimeZone` to compare it, so the same cursor would mean
        // different instants on a UTC server and a local one.
        .bind(after.map(|cursor| cursor.created_at))
        .bind(after.map(|cursor| cursor.id))
        .bind(limit.min(THREAD_HISTORY_LIMIT) as i64)
        .fetch_all(pool)
        .await
        .map_err(AppError::from)?
        .into_iter()
        .map(TryInto::try_into)
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
    sqlx::query_as::<_, ThreadMessageDb>(&query)
        .bind(thread_id)
        .bind(message_id.as_uuid())
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)?
        .map(TryInto::try_into)
        .transpose()
}

#[derive(sqlx::FromRow, Debug)]
struct AgentHistoryDb {
    role: String,
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

    let Some(row) = row else {
        return Ok(None);
    };
    let author_email = row
        .author_subject
        .filter(|_| row.author_transport.as_deref() == Some(TransportKind::Email.as_str()))
        .map(EmailAddress::from);

    Ok(Some(EmailReplyContext {
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
    }))
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
        correlation_id: CorrelationId::from(db.correlation_id),
        external_keys,
        created_at: db.created_at,
    }))
}
