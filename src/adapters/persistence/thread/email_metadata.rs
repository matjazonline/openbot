//! The email protocol extension of a canonical message.
//!
//! One row per canonical message that mail carried, holding the RFC threading headers and the raw
//! bodies. `UNIQUE (company_id, rfc_message_id)` is the dedup key mail has always had, and it is
//! kept here rather than on `messages` so that a message no mail carried needs none of it.

use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        email_message::EmailMessageMetadata,
        value_objects::{MessageId, ThreadIndex},
    },
};

/// Reassemble the email extension from a read row.
///
/// Every column is nullable in the projection because the join is a `LEFT JOIN`: a message no mail
/// carried has no row here at all. The RFC Message-ID is what says which of the two it is.
pub(super) fn email_metadata_from_row(
    rfc_message_id: Option<String>,
    in_reply_to: Option<String>,
    references_list: Option<Vec<String>>,
    thread_index: Option<String>,
    raw_text_body: Option<String>,
    raw_html_body: Option<String>,
) -> Option<EmailMessageMetadata> {
    let rfc_message_id = rfc_message_id?;
    Some(
        EmailMessageMetadata::new(MessageId::from(rfc_message_id))
            .in_reply_to(in_reply_to.map(MessageId::from))
            .references(
                references_list
                    .unwrap_or_default()
                    .into_iter()
                    .map(MessageId::from)
                    .collect(),
            )
            .thread_index(thread_index.map(ThreadIndex::from))
            .raw_bodies(raw_text_body, raw_html_body),
    )
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
