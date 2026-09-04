//! The email protocol extension of a canonical message.
//!
//! One row per canonical message that mail carried, holding the RFC threading headers and the raw
//! bodies. RFC Message-ID is deliberately non-unique here; provider deduplication belongs to the
//! binding-scoped `(binding_id, external_message_key)` map. The extension stays separate from
//! `messages` so that a message no mail carried needs none of it.

use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        email_message::EmailMessageMetadata,
        value_objects::{MessageId, ThreadIndex},
    },
};

/// The email extension columns as they are stored, named so a five-`Option<String>` projection
/// cannot be reassembled in the wrong order.
#[derive(sqlx::FromRow)]
struct EmailMetadataDb {
    rfc_message_id: String,
    in_reply_to: Option<String>,
    references_list: Vec<String>,
    thread_index: Option<String>,
    raw_text_body: Option<String>,
    raw_html_body: Option<String>,
}

impl From<EmailMetadataDb> for EmailMessageMetadata {
    fn from(row: EmailMetadataDb) -> Self {
        EmailMessageMetadata::new(MessageId::from(row.rfc_message_id))
            .in_reply_to(row.in_reply_to.map(MessageId::from))
            .references(
                row.references_list
                    .into_iter()
                    .map(MessageId::from)
                    .collect(),
            )
            .thread_index(row.thread_index.map(ThreadIndex::from))
            .raw_bodies(row.raw_text_body, row.raw_html_body)
    }
}

/// The email extension of one canonical message, or `None` when no mail carried it.
///
/// Absence is the answer, not an error: a message that arrived over a transport with no RFC
/// headers has no row here at all, and the caller decides what a reply without a `Message-ID`
/// means.
pub(super) async fn load_email_metadata(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    message_id: Uuid,
) -> AppResult<Option<EmailMessageMetadata>> {
    let row = sqlx::query_as::<_, EmailMetadataDb>(
        r#"SELECT rfc_message_id, in_reply_to, references_list, thread_index,
                  raw_text_body, raw_html_body
             FROM email_message_metadata
            WHERE company_id = $1 AND message_id = $2"#,
    )
    .bind(company_id)
    .bind(message_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::from)?;

    Ok(row.map(EmailMessageMetadata::from))
}

/// Record the email headers of a message being stored for the first time.
///
/// The insert is unconditional: it runs only on the branch that just created the canonical
/// message, so a conflict here means two writers raced past the external-message dedup and the
/// unique key is the thing that must reject the second one.
pub(super) async fn insert_email_metadata_on(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    message_id: Uuid,
    metadata: &EmailMessageMetadata,
) -> AppResult<()> {
    let references: Vec<&str> = metadata.references.iter().map(MessageId::as_str).collect();
    sqlx::query(
        r#"INSERT INTO email_message_metadata (
                company_id, message_id, rfc_message_id, in_reply_to, references_list,
                thread_index, raw_text_body, raw_html_body
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
    )
    .bind(company_id)
    .bind(message_id)
    .bind(metadata.rfc_message_id.as_str())
    .bind(metadata.in_reply_to.as_deref())
    .bind(&references)
    .bind(metadata.thread_index.as_deref())
    .bind(metadata.raw_text_body.as_deref())
    .bind(metadata.raw_html_body.as_deref())
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;
    Ok(())
}
