//! Keeping what arrived in the mail.
//!
//! Attachments are stored the moment they are parsed, which is the only point in the pipeline that
//! still has the bytes: everything downstream carries [`AttachmentMetadata`], and the durable task
//! payload the ingest is queued through is JSONB in Postgres — no place for a file.
//!
//! That timing decides the object key. The company and the thread are not resolved yet, so the key
//! cannot name them; it is the content hash the parser computes anyway. Two copies of the same
//! attachment are therefore one object, and knowing a key grants nothing on its own — a download
//! is authorized against the thread it hangs on, never against the key.

use sha2::{Digest, Sha256};
use tracing::warn;

use crate::{
    adapters::{
        protocols::email::parser::RawAttachmentData,
        storage::{BucketKind, FileStorage},
    },
    entities::{upload::ImageFormat, value_objects::ObjectKey},
};

/// What an attachment with no recognizable type is stored and served as.
const DEFAULT_CONTENT_TYPE: &str = "application/octet-stream";

/// Put each attachment in the private bucket, recording where it landed.
///
/// A failure to store is logged and left as `None` rather than propagated: an unreachable bucket
/// must not cost us the message. The mail still arrives, and the attachment shows as one we do not
/// have.
pub async fn store_inbound_attachments(
    storage: &dyn FileStorage,
    folder: &str,
    attachments: &mut [RawAttachmentData],
) {
    for attachment in attachments {
        let key = attachment_key(folder, &attachment.content, &attachment.filename);
        let content_type = stored_content_type(attachment);

        match storage
            .store_object(
                BucketKind::Private,
                &key,
                &content_type,
                attachment.content.clone(),
            )
            .await
        {
            Ok(()) => attachment.stored_key = Some(key),
            Err(error) => warn!(
                %key,
                filename = %attachment.filename,
                %error,
                "Could not store an inbound attachment; the message keeps its metadata only"
            ),
        }
    }
}

/// Where an attachment's bytes go: the content hash, under the configured folder.
fn attachment_key(folder: &str, content: &[u8], filename: &str) -> ObjectKey {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let digest = format!("{:x}", hasher.finalize());

    match extension_of(filename) {
        Some(extension) => ObjectKey::new(format!(
            "{folder}/{digest}.{extension}",
            folder = folder.trim_matches('/')
        )),
        None => ObjectKey::new(format!(
            "{folder}/{digest}",
            folder = folder.trim_matches('/')
        )),
    }
}

/// The sender's file extension, if it is short and plain enough to be one.
///
/// Only used to make a stored object recognizable; it is never what decides how the file is served,
/// because the name came from whoever sent the mail.
fn extension_of(filename: &str) -> Option<String> {
    let extension = filename.rsplit_once('.')?.1;

    let plausible = !extension.is_empty()
        && extension.len() <= 8
        && extension.chars().all(|c| c.is_ascii_alphanumeric());

    plausible.then(|| extension.to_ascii_lowercase())
}

/// What the object is stored as.
///
/// The sender's `Content-Type` is a claim, so it is only kept when the bytes agree with it: a
/// picture is stored as the picture it is, and everything else as bytes to download. That is what
/// keeps a `text/html` "attachment" from ever being served as a page.
fn stored_content_type(attachment: &RawAttachmentData) -> String {
    match ImageFormat::detect(&attachment.content) {
        Some(format) => format.mime().to_string(),
        None => DEFAULT_CONTENT_TYPE.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::storage::test_support::FakeStorage;

    fn attachment(filename: &str, content_type: &str, content: &[u8]) -> RawAttachmentData {
        RawAttachmentData {
            filename: filename.to_string(),
            content_type: content_type.to_string(),
            content: content.to_vec(),
            stored_key: None,
        }
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n....";

    #[tokio::test]
    async fn an_attachment_goes_to_the_private_bucket_named_by_its_content() {
        let storage = FakeStorage::new();
        let mut attachments = vec![attachment(
            "Quarterly Report.PDF",
            "application/pdf",
            b"%PDF-1.7",
        )];

        store_inbound_attachments(&storage, "attachments", &mut attachments).await;

        let stored = storage.objects();
        let stored = stored.first().expect("one object");
        assert_eq!(stored.bucket, BucketKind::Private);
        assert_eq!(attachments[0].stored_key.as_ref(), Some(&stored.key));
        assert!(stored.key.starts_with("attachments/"), "{}", stored.key);
        // The sender's extension, lowercased; the name itself never reaches the key.
        assert!(stored.key.ends_with(".pdf"), "{}", stored.key);
        assert!(!stored.key.contains("Quarterly"), "{}", stored.key);
    }

    #[tokio::test]
    async fn the_same_file_twice_is_the_same_object() {
        let storage = FakeStorage::new();
        let mut attachments = vec![
            attachment("a.png", "image/png", PNG),
            attachment("copy.png", "image/png", PNG),
        ];

        store_inbound_attachments(&storage, "attachments", &mut attachments).await;

        assert_eq!(attachments[0].stored_key, attachments[1].stored_key);
    }

    #[tokio::test]
    async fn what_the_sender_calls_it_does_not_decide_how_it_is_stored() {
        let storage = FakeStorage::new();
        let mut attachments = vec![
            // A page dressed as a picture: stored as bytes to download, never as HTML.
            attachment(
                "invoice.png",
                "image/png",
                b"<html><script>alert(1)</script>",
            ),
            // A picture whose bytes agree with the claim.
            attachment("logo.png", "image/png", PNG),
        ];

        store_inbound_attachments(&storage, "attachments", &mut attachments).await;

        let stored = storage.objects();
        assert_eq!(stored[0].content_type, DEFAULT_CONTENT_TYPE);
        assert_eq!(stored[1].content_type, "image/png");
    }

    #[tokio::test]
    async fn a_bucket_that_is_down_costs_the_attachment_not_the_message() {
        let mut attachments = vec![attachment("a.pdf", "application/pdf", b"%PDF-1.7")];

        store_inbound_attachments(&FakeStorage::failing(), "attachments", &mut attachments).await;

        assert_eq!(attachments[0].stored_key, None);
    }

    #[test]
    fn only_a_plausible_extension_is_kept() {
        assert_eq!(extension_of("report.pdf").as_deref(), Some("pdf"));
        assert_eq!(extension_of("IMAGE.JPEG").as_deref(), Some("jpeg"));
        assert_eq!(extension_of("archive.tar.gz").as_deref(), Some("gz"));

        // Nothing that could shape the key into something other than one name.
        assert_eq!(extension_of("no-extension"), None);
        assert_eq!(extension_of("odd.name with spaces"), None);
        assert_eq!(extension_of("trailing."), None);
        assert_eq!(extension_of("x.averylongextension"), None);
        assert_eq!(extension_of("x.a/b"), None);
    }
}
