use super::*;

#[tokio::test]
async fn direct_thread_ui_exact_message_diagnostics_and_attachment_scope_refuse_other_targets() {
    use crate::adapters::http::routes::{
        ui_attachments::{AttachmentQuery, download_attachment},
        ui_message_diagnostics::message_diagnostics,
    };
    use crate::entities::{message::AttachmentMetadata, value_objects::ObjectKey};
    use axum::extract::Path;
    let Some(h) = Harness::isolated().await else {
        return;
    };
    let actor = member(&h, "asset_person").await;
    let other = member(&h, "asset_other").await;
    let inbox = selection(&h, &actor).await;
    let sha = "b".repeat(64);
    let result = h
        .threads
        .ingest_canonical(
            CanonicalMessageIngress::new(
                h.company_id,
                inbox.target().unwrap().unwrap(),
                qualified_email_identity(actor.email.as_str()).unwrap(),
                "Private attachment body",
                IngressOrigin::TrustedApplication,
            )
            .with_subject("Private attachment subject")
            .with_attachments(vec![AttachmentMetadata {
                filename: "private-file.pdf".into(),
                content_type: "application/pdf".into(),
                sha256_hash: sha.clone(),
                size_bytes: 5,
                storage_key: Some(ObjectKey::new("attachments/direct.pdf")),
                source: None,
            }]),
        )
        .await
        .unwrap();
    assert!(result.accepted);
    let thread = result.thread.unwrap();
    let messages = h
        .threads
        .get_thread_history(
            &actor,
            crate::use_cases::thread::ThreadReadScope {
                inbox: inbox.scope(h.company_id).unwrap(),
                thread_id: thread.id,
            },
        )
        .await
        .unwrap();
    let message = &messages[0];
    let companies = Arc::new(CompanyUseCases::new(h.persistence.clone()));
    let exact = |viewer, company_id, selected| {
        crate::adapters::http::routes::ui::message::page(
            State(companies.clone()),
            State(h.channels.clone()),
            State(h.threads.clone()),
            State(h.reader()),
            viewer,
            Query(crate::adapters::http::routes::ui::message::MessageQuery {
                company_id,
                inbox: selected,
                thread_id: thread.id,
                message_id: message.canonical_id.as_uuid(),
            }),
        )
    };
    let diagnostics = |viewer, company_id, selected| {
        message_diagnostics(
            State(companies.clone()),
            State(h.channels.clone()),
            State(h.threads.clone()),
            viewer,
            Path((thread.id, message.id)),
            Query(AttachmentQuery {
                company_id,
                inbox: selected,
            }),
        )
    };
    let attachment = |viewer, company_id, selected| {
        download_attachment(
            State(companies.clone()),
            State(h.channels.clone()),
            State(h.threads.clone()),
            State(None),
            viewer,
            Path((thread.id, sha.clone())),
            Query(AttachmentQuery {
                company_id,
                inbox: selected,
            }),
        )
    };
    let html = exact(actor.clone(), h.company_id, inbox).await.unwrap().0;
    assert!(html.contains("Private attachment body") && html.contains("private-file.pdf"));
    assert!(html.contains(&format!("principal_id={}", inbox.principal_id.unwrap())));
    assert!(
        diagnostics(actor.clone(), h.company_id, inbox)
            .await
            .unwrap()
            .0
            .contains(&message.id.to_string())
    );
    assert!(
        matches!(
            attachment(actor.clone(), h.company_id, inbox).await,
            Err(AppError::Internal(_))
        ),
        "authorized attachment reaches the deliberately unconfigured storage boundary"
    );
    for (viewer, company_id, selected) in [
        (other.clone(), h.company_id, inbox),
        (
            h.owner.clone(),
            h.company_id,
            InboxSelection {
                channel_id: Some(h.channel_id),
                principal_id: None,
            },
        ),
        (actor.clone(), Uuid::new_v4(), inbox),
    ] {
        assert!(matches!(
            exact(viewer.clone(), company_id, selected).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            diagnostics(viewer.clone(), company_id, selected).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            attachment(viewer, company_id, selected).await,
            Err(AppError::NotFound(_))
        ));
    }
}
