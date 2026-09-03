//! Serving what arrived in the mail, to whoever the channel it arrived on allows.
//!
//! Attachments are kept in a bucket nothing public can read, so a download is a request to *this*
//! app, authorized the way opening the thread is authorized — through the very same guards
//! ([`super::ui::load_viewable_channel`], [`super::ui::load_channel_thread`]). There is no link to
//! leak and nothing to expire: someone who loses access to a channel loses its files in the same
//! instant.
//!
//! The bytes are streamed back with headers that say "save this": a file somebody emailed us is
//! never rendered as a page on our own origin.

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::{Path, Query, State},
    http::{
        StatusCode,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, HeaderMap, HeaderValue},
    },
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::{
        http::app_state::AppState,
        storage::{BucketKind, FileStorage},
    },
    app_error::{AppError, AppResult},
    entities::{message::AttachmentMetadata, user::Viewer},
    use_cases::{channel::ChannelUseCases, company::CompanyUseCases, thread::ThreadUseCases},
};

use super::ui::{load_channel_thread, load_viewable_channel};

/// Nothing an attachment is served with may be guessed at by the browser, and nothing may be kept.
const NO_SNIFF: (&str, &str) = ("x-content-type-options", "nosniff");

/// A downloaded attachment is one person's copy of one company's mail; no shared cache may hold it.
const PRIVATE_NO_STORE: &str = "private, no-store";

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/ui/threads/{thread_id}/attachments/{sha256}",
        get(download_attachment),
    )
}

/// Which thread's attachment, scoped the way every mailbox fragment is.
#[derive(Debug, Clone, Deserialize)]
pub struct AttachmentQuery {
    pub company_id: Uuid,
    pub channel_id: Uuid,
}

/// GET /ui/threads/{thread_id}/attachments/{sha256} - One attachment of one thread (Protected).
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    thread_use_cases,
    storage,
    viewer
))]
async fn download_attachment(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(storage): State<Option<Arc<dyn FileStorage>>>,
    viewer: Viewer,
    Path((thread_id, sha256)): Path<(Uuid, String)>,
    Query(query): Query<AttachmentQuery>,
) -> AppResult<Response> {
    // The same two guards the mailbox opens a thread with. Nothing here decides access on its own.
    let (_, channel) = load_viewable_channel(
        &company_use_cases,
        &channel_use_cases,
        &viewer,
        query.company_id,
        query.channel_id,
    )
    .await?;

    let thread = load_channel_thread(&thread_use_cases, channel.id, thread_id)
        .await?
        .ok_or_else(missing)?;

    // The attachment has to hang on a message of *this* thread: that, rather than the key, is what
    // stops a caller naming a file that belongs to somebody else's mail.
    let messages = thread_use_cases.get_thread_history(thread.id).await?;
    let attachment = find_attachment(&messages, &sha256).ok_or_else(missing)?;

    let key = attachment.storage_key.clone().ok_or_else(|| {
        AppError::NotFound(
            "This attachment was never stored, so there is nothing to download".into(),
        )
    })?;

    let storage = storage
        .ok_or_else(|| AppError::Internal("No file storage is configured on this server".into()))?;

    let object = storage.read_object(BucketKind::Private, &key).await?;

    Ok(attachment_response(
        &attachment,
        object.content_type,
        object.bytes,
    ))
}

/// One attachment of these messages, by the hash of its contents.
fn find_attachment(
    messages: &[crate::entities::message::Message],
    sha256: &str,
) -> Option<AttachmentMetadata> {
    messages
        .iter()
        .filter_map(|message| message.attachments.as_ref())
        .flatten()
        .find(|attachment| attachment.sha256_hash.eq_ignore_ascii_case(sha256))
        .cloned()
}

/// What a caller is told about an attachment that is not theirs, does not exist, or is not on the
/// thread they named: the same thing in all three cases.
fn missing() -> AppError {
    AppError::NotFound("Attachment not found".into())
}

/// The bytes, with the headers that make a browser save them rather than run them.
fn attachment_response(
    attachment: &AttachmentMetadata,
    content_type: String,
    bytes: Vec<u8>,
) -> Response {
    let mut headers = HeaderMap::new();

    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(&attachment.filename))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(PRIVATE_NO_STORE));
    headers.insert(
        axum::http::HeaderName::from_static(NO_SNIFF.0),
        HeaderValue::from_static(NO_SNIFF.1),
    );

    (StatusCode::OK, headers, Body::from(bytes)).into_response()
}

/// `Content-Disposition` for a file name that came from a stranger.
///
/// Always `attachment`: an HTML or SVG file served `inline` would run as a page on this app's own
/// origin, with this app's own cookies. The name is offered twice — a stripped ASCII form for old
/// clients, and the RFC 5987 encoding that carries the real one — so a name with a quote, a
/// newline or a non-Latin script cannot break out of the header.
fn content_disposition(filename: &str) -> String {
    let fallback: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let fallback = fallback.trim();
    let fallback = if fallback.is_empty() {
        "attachment"
    } else {
        fallback
    };

    format!(
        "attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}",
        encoded = percent_encode(filename),
    )
}

/// RFC 5987's `ext-value` encoding: everything but the unreserved set is a percent-escape.
fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        message::{Message, MessageDirection, MessageRole},
        value_objects::ObjectKey,
    };
    use crate::use_cases::thread::test_support::{EmailMessageDraft, stored_email};

    fn attachment(sha: &str, filename: &str, key: Option<&str>) -> AttachmentMetadata {
        AttachmentMetadata {
            filename: filename.to_string(),
            content_type: "application/pdf".to_string(),
            sha256_hash: sha.to_string(),
            size_bytes: 12,
            storage_key: key.map(ObjectKey::new),
        }
    }

    fn message(attachments: Vec<AttachmentMetadata>) -> Message {
        stored_email(EmailMessageDraft {
            id: Uuid::new_v4(),
            thread_id: Uuid::new_v4(),
            message_id: "<a@example.test>".into(),
            in_reply_to: None,
            references_list: Vec::new(),
            sender: "someone@example.test".into(),
            recipients_to: Vec::new(),
            recipients_cc: Vec::new(),
            subject: "Invoice".to_string(),
            clean_text_body: "See attached.".to_string(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: Some(attachments),
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            thread_index: None,
            created_at: chrono::Utc::now(),
        })
    }

    #[test]
    fn an_attachment_is_found_by_its_hash_only_within_the_thread_it_hangs_on() {
        let messages = vec![
            message(vec![attachment(
                "aaa",
                "first.pdf",
                Some("attachments/aaa.pdf"),
            )]),
            message(vec![attachment("bbb", "second.pdf", None)]),
        ];

        assert_eq!(
            find_attachment(&messages, "aaa").map(|a| a.filename),
            Some("first.pdf".to_string())
        );
        // Case-insensitive, because a hash is written both ways.
        assert!(find_attachment(&messages, "AAA").is_some());
        // A hash from somebody else's thread is simply not here.
        assert!(find_attachment(&messages, "ccc").is_none());
    }

    #[test]
    fn a_name_from_a_stranger_cannot_break_out_of_the_header() {
        let header = content_disposition("in\"voice\r\n.pdf");

        // Always a download, never something rendered on our own origin.
        assert!(header.starts_with("attachment; "));
        // Nothing that would end the quoted string or start a new header line.
        assert!(
            !header.contains('"') || header.matches('"').count() == 2,
            "{header}"
        );
        assert!(!header.contains('\r') && !header.contains('\n'), "{header}");
        assert!(
            header.contains("filename*=UTF-8''in%22voice%0D%0A.pdf"),
            "{header}"
        );
    }

    #[test]
    fn a_name_in_another_script_survives_in_the_encoded_form() {
        let header = content_disposition("отчёт.pdf");

        assert!(header.contains("filename=\"_____.pdf\""), "{header}");
        assert!(header.contains("filename*=UTF-8''%D0%BE%D1%82"), "{header}");
    }

    #[test]
    fn a_nameless_attachment_still_downloads() {
        assert!(content_disposition("").contains("filename=\"attachment\""));
        assert!(content_disposition("???").contains("filename=\"___\""));
    }
}
