use super::super::message::{MessageQuery, page};
use super::*;

async fn source(h: &Harness, viewer: Viewer, query: MessageQuery) -> AppResult<Html<String>> {
    page(
        State(Arc::new(CompanyUseCases::new(h.persistence.clone()))),
        State(h.channels.clone()),
        State(h.threads.clone()),
        State(h.reader()),
        viewer,
        Query(query),
    )
    .await
}

async fn seed(h: &Harness, thread: Uuid) -> Vec<Uuid> {
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(h.company_id)
            .bind(h.owner.user_id)
            .fetch_one(h.persistence.pool())
            .await
            .unwrap();
    let ids:Vec<Uuid>=sqlx::query_scalar(
        "INSERT INTO messages(id,company_id,author_principal_id,subject,clean_text_body,direction,role,correlation_id,content_hash,created_at,audience) \
        SELECT gen_random_uuid(),$1,$2,'Source','Source message '||ordinal||$3,'inbound','human',gen_random_uuid(),decode(repeat('00',32),'hex'),now()-ordinal*interval '1 second','external_conversation' \
        FROM generate_series(0,209) AS fixture(ordinal) RETURNING id")
        .bind(h.company_id).bind(principal).bind(" <script>").fetch_all(h.persistence.pool()).await.unwrap();
    sqlx::query("INSERT INTO thread_messages(id,company_id,channel_id,thread_id,message_id,created_at,entry_kind) SELECT gen_random_uuid(),company_id,$2,$3,id,created_at,'conversation' FROM messages WHERE company_id=$1 AND id=ANY($4)")
        .bind(h.company_id).bind(h.channel_id).bind(thread).bind(&ids).execute(h.persistence.pool()).await.unwrap();
    ids
}

#[tokio::test]
async fn source_message_outside_window_is_exact_and_authorized() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let Some(other) = Harness::new().await else {
        return;
    };
    let thread = h.thread("Context").await;
    let ids = seed(&h, thread).await;
    let history = h.threads.get_thread_history(thread).await.unwrap();
    assert_eq!(history.len(), 200);
    let old = ids[209];
    assert!(
        !history
            .iter()
            .any(|message| message.canonical_id.as_uuid() == old)
    );
    let query = MessageQuery {
        company_id: h.company_id,
        channel_id: h.channel_id,
        thread_id: thread,
        message_id: old,
    };
    for (name, id, body) in [
        ("old", old, "Source message 209 &lt;script&gt;"),
        ("retained", ids[5], "Source message 5 &lt;script&gt;"),
    ] {
        let page = source(
            &h,
            h.owner.clone(),
            MessageQuery {
                message_id: id,
                ..query.clone()
            },
        )
        .await
        .unwrap()
        .0;
        assert!(page.contains(body), "{page}");
        assert!(page.contains(&format!("Message {id}")));
        assert!(!page.contains("id=\"message-scroll\""));
        assert!(!page.contains("sse-connect="));
        assert!(!page.contains("hx-target="));
        assert!(!page.contains("Source message 0 &lt;script&gt;"));
        if let Some(dir) = std::env::var_os("WORKFLOW_DECISION_BROWSER_FIXTURES") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("source-{name}.html")), page).unwrap();
        }
    }
    note_has_no_mailbox_controls(&h, query.clone()).await;
    refuses_other_scopes(&h, &other, query).await;
}

async fn refuses_other_scopes(h: &Harness, other: &Harness, query: MessageQuery) {
    let unrelated = h.thread("Other thread").await;
    for refused in [
        MessageQuery {
            company_id: other.company_id,
            ..query.clone()
        },
        MessageQuery {
            channel_id: other.channel_id,
            ..query.clone()
        },
        MessageQuery {
            thread_id: unrelated,
            ..query.clone()
        },
        MessageQuery {
            message_id: Uuid::new_v4(),
            ..query.clone()
        },
    ] {
        assert!(matches!(
            source(h, h.owner.clone(), refused).await,
            Err(AppError::NotFound(_))
        ));
    }
    sqlx::query("UPDATE channels SET access_mode='allowlist' WHERE id=$1")
        .bind(h.channel_id)
        .execute(h.persistence.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role) VALUES($1,$2,$3,'admin')")
        .bind(Uuid::new_v4())
        .bind(h.company_id)
        .bind(other.owner.user_id)
        .execute(h.persistence.pool())
        .await
        .unwrap();
    assert!(matches!(
        source(h, other.owner.clone(), query).await,
        Err(AppError::NotFound(_))
    ));
}

async fn note_has_no_mailbox_controls(h: &Harness, query: MessageQuery) {
    use crate::entities::internal_note::{AddInternalNote, InternalNoteProvenance};
    let actor = h
        .threads
        .principal_access_for_user(h.company_id, h.owner.user_id)
        .await
        .unwrap()
        .unwrap();
    let note = h
        .threads
        .add_internal_note(
            AddInternalNote {
                company_id: h.company_id,
                channel_id: h.channel_id,
                thread_id: query.thread_id,
                text: "Source internal note".into(),
                command_id: Uuid::new_v4(),
                supersedes_note_id: None,
                provenance: InternalNoteProvenance::HumanUi,
            },
            actor,
        )
        .await
        .unwrap();
    let ordinary = pages::message_bubble_chat(
        &note,
        None,
        pages::MessageScope {
            company_id: h.company_id,
            channel_id: h.channel_id,
        },
        pages::RunLinkAccess::Label,
    );
    assert!(ordinary.contains("hx-target=\"#detail-pane\""));
    assert!(ordinary.contains("message-diagnostics-body"));
    let page = source(
        h,
        h.owner.clone(),
        MessageQuery {
            message_id: note.canonical_id.as_uuid(),
            ..query
        },
    )
    .await
    .unwrap()
    .0;
    assert!(page.contains("Source internal note"));
    assert!(!page.contains("hx-target="));
    let bubble = page
        .split("id=\"source-message\"")
        .nth(1)
        .unwrap()
        .split("</article>")
        .next()
        .unwrap();
    assert!(!bubble.contains("<form"));
    if let Some(dir) = std::env::var_os("WORKFLOW_DECISION_BROWSER_FIXTURES") {
        std::fs::write(std::path::PathBuf::from(dir).join("source-note.html"), page).unwrap();
    }
}
