//! Real viewer entry points, native PostgreSQL, production HTML and live SSE bodies.
use super::*;
use crate::entities::access_level::AccessLevel;
use crate::entities::message::{AttachmentMetadata, CanonicalMessageId, ThreadEntryKind};
use crate::entities::value_objects::ObjectKey;
use crate::use_cases::thread::{ChannelReadScope, ThreadReadScope};
use axum::extract::Path;

const SUBJECT: &str = "private-subject-canary";
const BODY: &str = "private-body-canary";
const FILENAME: &str = "private-attachment-canary.pdf";

fn scope(h: &Harness, thread_id: Uuid) -> ThreadReadScope {
    ThreadReadScope {
        channel: ChannelReadScope {
            company_id: h.company_id,
            channel_id: h.channel_id,
        },
        thread_id,
    }
}

async fn member(h: &Harness, level: AccessLevel) -> Viewer {
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("knowledge-{suffix}@example.test");
    let user = h
        .persistence
        .create_user(&format!("knowledge_{suffix}"), &email, "hash")
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(user.id)
        .execute(h.persistence.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES($1,$2,$3,'member',$4)")
        .bind(Uuid::new_v4()).bind(h.company_id).bind(user.id).bind(level.as_i16()).execute(h.persistence.pool()).await.unwrap();
    let principal = Uuid::new_v4();
    sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,display_label) VALUES($1,$2,'person',$3,'Reader')")
        .bind(principal).bind(h.company_id).bind(user.id).execute(h.persistence.pool()).await.unwrap();
    sqlx::query("INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) VALUES($1,$2,$3,'view','configured_allowlist')")
        .bind(h.company_id).bind(h.channel_id).bind(principal).execute(h.persistence.pool()).await.unwrap();
    Viewer {
        user_id: user.id,
        email: email.into(),
        email_verified_at: Some(chrono::Utc::now()),
    }
}

async fn classified(h: &Harness, thread: Uuid) -> CanonicalMessageId {
    let id = Uuid::new_v4();
    let attachment = AttachmentMetadata {
        filename: FILENAME.into(),
        content_type: "application/pdf".into(),
        sha256_hash: "a".repeat(64),
        size_bytes: 12,
        storage_key: Some(ObjectKey::new("attachments/private.pdf")),
        source: None,
    };
    // A valid immutable stamped row fixture. Actual producer/run stamping is covered by p03.
    sqlx::query("INSERT INTO messages(id,company_id,author_principal_id,subject,clean_text_body,attachments,direction,role,correlation_id,content_hash,audience,access_level,content_exact) \
        SELECT $1,$2,principal.id,$3,$4,$5,'outbound','agent',$6,decode(repeat('ab',32),'hex'),'external_conversation',5,true \
        FROM principals AS principal WHERE principal.company_id=$2 AND principal.user_id=$7")
        .bind(id).bind(h.company_id).bind(SUBJECT).bind(BODY)
        .bind(serde_json::json!({"version":"1","items":[attachment]})).bind(Uuid::new_v4()).bind(h.owner.user_id)
        .execute(h.persistence.pool()).await.unwrap();
    h.persistence
        .associate_message(
            thread,
            CanonicalMessageId::new(id),
            ThreadEntryKind::Conversation,
        )
        .await
        .unwrap();
    CanonicalMessageId::new(id)
}

async fn history(
    h: &Harness,
    viewer: &Viewer,
    thread: Uuid,
) -> Vec<crate::entities::message_view::ThreadMessageView> {
    h.threads
        .get_thread_history(viewer, scope(h, thread))
        .await
        .unwrap()
}

fn assert_private_absent(html: &str) {
    for secret in [SUBJECT, BODY, FILENAME] {
        assert!(!html.contains(secret), "leaked {secret}: {html}");
    }
}

#[tokio::test]
async fn knowledge_view_public_reader_has_placeholder_and_restricted_thread_title() {
    let h = Harness::new().await.expect("native knowledge fixture");
    let viewer = member(&h, AccessLevel::PUBLIC).await;
    let thread = h.thread(SUBJECT).await;
    let id = classified(&h, thread).await;
    let messages = history(&h, &viewer, thread).await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].canonical_id, id);
    assert!(messages[0].restricted && messages[0].attachments.is_empty());
    let html = pages::message_bubble_chat(
        &messages[0],
        Some(&viewer.email),
        pages::MessageScope {
            company_id: h.company_id,
            channel_id: h.channel_id,
        },
        pages::RunLinkAccess::Label,
    );
    assert!(html.contains("Restricted message"));
    assert_private_absent(&html);
    let threads = h
        .threads
        .list_channel_threads(&viewer, scope(&h, thread).channel, None, 10)
        .await
        .unwrap();
    assert_eq!(
        threads.iter().find(|row| row.id == thread).unwrap().subject,
        "Restricted message"
    );
    assert_private_absent(&serde_json::to_string(&threads).unwrap());
    let exact = h
        .threads
        .get_thread_message_view(&viewer, scope(&h, thread), id)
        .await
        .unwrap()
        .unwrap();
    assert!(exact.restricted && exact.body.is_empty());
}

#[tokio::test]
async fn knowledge_view_equal_level_and_owner_get_full_message_and_fresh_demotion_applies() {
    let h = Harness::new().await.expect("native knowledge fixture");
    let viewer = member(&h, AccessLevel::L5).await;
    let thread = h.thread(SUBJECT).await;
    classified(&h, thread).await;
    for actor in [&viewer, &h.owner] {
        let messages = history(&h, actor, thread).await;
        assert!(!messages[0].restricted);
        assert_eq!(messages[0].body, BODY);
        assert_eq!(messages[0].attachments[0].filename, FILENAME);
    }
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(h.company_id)
        .bind(viewer.user_id)
        .execute(h.persistence.pool())
        .await
        .unwrap();
    assert!(history(&h, &viewer, thread).await[0].restricted);
    let foreign = Harness::new().await.expect("foreign fixture");
    assert!(matches!(
        foreign
            .threads
            .get_thread_history(&viewer, scope(&foreign, thread))
            .await,
        Err(AppError::NotFound(_))
    ));
}

#[tokio::test]
async fn knowledge_view_attachment_denial_precedes_storage_and_filename_access() {
    use super::super::super::ui_attachments::{AttachmentQuery, download_attachment};
    let h = Harness::new().await.expect("native knowledge fixture");
    let viewer = member(&h, AccessLevel::PUBLIC).await;
    let thread = h.thread(SUBJECT).await;
    classified(&h, thread).await;
    let download = |actor| {
        download_attachment(
            State(Arc::new(CompanyUseCases::new(h.persistence.clone()))),
            State(h.channels.clone()),
            State(h.threads.clone()),
            State(None),
            actor,
            Path((thread, "a".repeat(64))),
            Query(AttachmentQuery {
                company_id: h.company_id,
                channel_id: h.channel_id,
            }),
        )
    };
    assert!(matches!(download(viewer).await, Err(AppError::NotFound(_))));
    // No storage is configured: the owner gets as far as that boundary; the lower viewer cannot.
    assert!(matches!(
        download(h.owner.clone()).await,
        Err(AppError::Internal(_))
    ));
}

#[tokio::test]
async fn knowledge_view_live_stream_rechecks_level_after_demotion() {
    let h = Harness::new().await.expect("native knowledge fixture");
    let viewer = member(&h, AccessLevel::L5).await;
    let thread = h.thread(SUBJECT).await;
    classified(&h, thread).await;
    let response = thread_message_stream(
        State(h.channels.clone()),
        State(h.threads.clone()),
        State(h.reader()),
        State(h.events.clone()),
        viewer.clone(),
        HeaderMap::new(),
        Query(ThreadStreamQuery {
            company_id: h.company_id,
            channel_id: h.channel_id,
            thread_id: thread,
            after: None,
        }),
    )
    .await
    .unwrap();
    let mut frames = Frames::of(response.into_response());
    assert!(frames.next_named("message").await.data.contains(BODY));
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(h.company_id)
        .bind(viewer.user_id)
        .execute(h.persistence.pool())
        .await
        .unwrap();
    let next = classified(&h, thread).await;
    h.events
        .publish(MailboxEvent::MessageCommitted(ThreadScope {
            company_id: h.company_id,
            channel_id: h.channel_id,
            thread_id: thread,
        }));
    let frame = frames.next_named("message").await;
    assert!(frame.data.contains(&next.to_string()));
    assert!(frame.data.contains("Restricted message"));
    assert_private_absent(&frame.data);
}

struct RevokeDuringStorageRead {
    pool: sqlx::PgPool,
    company_id: Uuid,
    user_id: Uuid,
    reads: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl crate::adapters::storage::FileStorage for RevokeDuringStorageRead {
    async fn upload_image(
        &self,
        _: &ObjectKey,
        _: &crate::entities::upload::ImageUpload,
    ) -> AppResult<crate::entities::value_objects::ObjectUrl> {
        Err(AppError::Internal("unexpected image upload".into()))
    }
    async fn store_object(
        &self,
        _: crate::adapters::storage::BucketKind,
        _: &ObjectKey,
        _: &str,
        _: Vec<u8>,
    ) -> AppResult<()> {
        Err(AppError::Internal("unexpected object write".into()))
    }
    async fn read_object_bounded(
        &self,
        _: crate::adapters::storage::BucketKind,
        _: &ObjectKey,
        _: usize,
    ) -> AppResult<crate::adapters::storage::StoredObject> {
        Err(AppError::Internal("unexpected bounded object read".into()))
    }
    async fn read_object(
        &self,
        _: crate::adapters::storage::BucketKind,
        _: &ObjectKey,
    ) -> AppResult<crate::adapters::storage::StoredObject> {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // The initial permission check passed, but authority changes while external I/O runs.
        sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
            .bind(self.company_id)
            .bind(self.user_id)
            .execute(&self.pool)
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(crate::adapters::storage::StoredObject {
            content_type: "application/pdf".into(),
            bytes: b"private-file-canary".to_vec(),
        })
    }
}

#[tokio::test]
async fn knowledge_view_attachment_rechecks_demotion_after_storage_before_returning_bytes() {
    use super::super::super::ui_attachments::{AttachmentQuery, download_attachment};
    let h = Harness::new().await.expect("native knowledge fixture");
    let viewer = member(&h, AccessLevel::L5).await;
    let thread = h.thread(SUBJECT).await;
    classified(&h, thread).await;
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let storage: Arc<dyn crate::adapters::storage::FileStorage> =
        Arc::new(RevokeDuringStorageRead {
            pool: h.persistence.pool().clone(),
            company_id: h.company_id,
            user_id: viewer.user_id,
            reads: reads.clone(),
        });
    let result = download_attachment(
        State(Arc::new(CompanyUseCases::new(h.persistence.clone()))),
        State(h.channels.clone()),
        State(h.threads.clone()),
        State(Some(storage.clone())),
        viewer,
        Path((thread, "a".repeat(64))),
        Query(AttachmentQuery {
            company_id: h.company_id,
            channel_id: h.channel_id,
        }),
    )
    .await;
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(matches!(result, Err(AppError::NotFound(_))));
    let response = download_attachment(
        State(Arc::new(CompanyUseCases::new(h.persistence.clone()))),
        State(h.channels.clone()),
        State(h.threads.clone()),
        State(Some(storage)),
        h.owner.clone(),
        Path((thread, "a".repeat(64))),
        Query(AttachmentQuery {
            company_id: h.company_id,
            channel_id: h.channel_id,
        }),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert!(
        response
            .headers()
            .get("content-disposition")
            .unwrap()
            .to_str()
            .unwrap()
            .contains(FILENAME)
    );
    let bytes = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(bytes.as_ref(), b"private-file-canary");
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 2);
}
