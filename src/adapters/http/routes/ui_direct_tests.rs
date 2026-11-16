//! Real mailbox handlers and shared ingress, with production invite/retirement paths.
use super::*;
#[path = "ui_direct_revocation_tests.rs"]
mod revocation;
use crate::{
    application::ports::participant::PrincipalAccessPersistence,
    entities::{address::AddressTarget, transport::PrincipalId},
    use_cases::company_invite::{
        CompanyInvitePersistence, CompanyInviteUseCases, CreateCompanyInvite,
    },
};

async fn member(h: &Harness, label: &str) -> Viewer {
    let name = format!("{label}_{}", Uuid::new_v4().simple());
    let email = format!("{name}@example.test");
    let user = h
        .persistence
        .create_user(&name, &email, "hash")
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at=CURRENT_TIMESTAMP WHERE id=$1")
        .bind(user.id)
        .execute(h.persistence.pool())
        .await
        .unwrap();
    let user = UserPersistence::get_by_id(h.persistence.as_ref(), user.id)
        .await
        .unwrap()
        .unwrap();
    let invites = CompanyInviteUseCases::new(h.persistence.clone(), h.persistence.clone());
    let invite = invites
        .create_company_invite(CreateCompanyInvite {
            actor_id: h.owner.user_id,
            company_id: h.company_id,
            email: email.into(),
            role: crate::entities::company_member::CompanyAccessRole::Member,
            access_level: crate::entities::access_level::AccessLevel::PUBLIC,
        })
        .await
        .unwrap();
    invites.accept_invite(&user, invite.id).await.unwrap();
    Viewer {
        user_id: user.id,
        email: user.email.into(),
        email_verified_at: user.email_verified_at,
    }
}

async fn selection(h: &Harness, viewer: &Viewer) -> InboxSelection {
    let context = h
        .persistence
        .access_context_for_user(h.company_id, viewer.user_id)
        .await
        .unwrap()
        .unwrap();
    InboxSelection {
        channel_id: None,
        principal_id: context.principal_id,
    }
}

async fn direct_thread(h: &Harness, actor: &Viewer, inbox: InboxSelection) -> Thread {
    let result = h
        .threads
        .ingest_canonical(
            CanonicalMessageIngress::new(
                h.company_id,
                inbox.target().unwrap().unwrap(),
                qualified_email_identity(actor.email.as_str()).unwrap(),
                "Private direct body",
                IngressOrigin::TrustedApplication,
            )
            .with_subject("Private direct subject"),
        )
        .await
        .unwrap();
    assert!(result.accepted, "{:?}", result.rejection);
    result.thread.unwrap()
}

async fn pane(
    h: &Harness,
    viewer: Viewer,
    inbox: InboxSelection,
    thread_id: Uuid,
) -> AppResult<String> {
    message_pane_fragment(
        State(h.channels.clone()),
        State(h.threads.clone()),
        State(h.reader()),
        viewer,
        Query(ThreadQuery {
            company_id: h.company_id,
            inbox,
            thread_id,
        }),
    )
    .await
    .map(|html| html.0)
}

fn users(h: &Harness) -> Arc<UserUseCases> {
    Arc::new(UserUseCases::new(
        Arc::new(crate::adapters::crypto::argon2::ArgonPasswordHasher::default()),
        h.persistence.clone(),
        "direct-ui-fixture-attempt-secret".into(),
    ))
}

#[tokio::test]
async fn direct_thread_ui_compose_reply_and_sidebar_keep_the_exact_person_target() {
    let Some(h) = Harness::isolated().await else {
        return;
    };
    let actor = member(&h, "personal").await;
    let inbox = selection(&h, &actor).await;
    let companies = Arc::new(CompanyUseCases::new(h.persistence.clone()));
    let response = create_thread(
        State(companies.clone()),
        State(h.channels.clone()),
        State(h.threads.clone()),
        State(h.reader()),
        State(users(&h)),
        State(h.config.clone()),
        actor.clone(),
        Form(ComposeForm {
            company_id: h.company_id,
            inbox,
            subject: Some("Composed direct".into()),
            text_body: Some("Composed private body".into()),
            deliver: None,
        }),
    )
    .await
    .unwrap();
    let push = response.headers()["HX-Push-Url"].to_str().unwrap();
    assert!(push.contains(&pages::inbox_target_query(inbox.target().unwrap().unwrap())));
    let read = inbox.scope(h.company_id).unwrap();
    let threads = h
        .threads
        .list_inbox_threads(&actor, read, None, 50)
        .await
        .unwrap();
    assert_eq!(threads.len(), 1);
    let thread = &threads[0];
    assert_eq!(thread.target, read.target);
    send_reply(
        State(companies),
        State(h.channels.clone()),
        State(h.threads.clone()),
        State(h.reader()),
        State(users(&h)),
        State(h.config.clone()),
        actor.clone(),
        Form(ReplyForm {
            company_id: h.company_id,
            inbox,
            thread_id: thread.id,
            reply_to_message_id: None,
            text_body: Some("Continued private body".into()),
            deliver: None,
        }),
    )
    .await
    .unwrap();
    let messages = h
        .threads
        .get_thread_history(
            &actor,
            crate::use_cases::thread::ThreadReadScope {
                inbox: read,
                thread_id: thread.id,
            },
        )
        .await
        .unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].body, "Continued private body");
    let intent_count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_message_intents WHERE company_id=$1 AND principal_id=$2 AND thread_id=$3")
        .bind(h.company_id).bind(inbox.principal_id.unwrap().as_uuid()).bind(thread.id)
        .fetch_one(h.persistence.pool()).await.unwrap();
    assert_eq!(intent_count, 2);
    let inboxes = h
        .threads
        .direct_inboxes(&actor, h.company_id)
        .await
        .unwrap();
    assert_eq!(
        inboxes.len(),
        1,
        "a member sees only their own personal inbox"
    );
    assert_eq!(inboxes[0].target(), read.target);
    let shell = mailbox_page(
        State(Arc::new(CompanyUseCases::new(h.persistence.clone()))),
        State(h.channels.clone()),
        State(h.threads.clone()),
        State(h.reader()),
        State(users(&h)),
        State(h.config.clone()),
        AuthenticatedUser { id: actor.user_id },
        actor.clone(),
        Query(MailboxQuery {
            company_id: Some(h.company_id),
            inbox,
            thread_id: Some(thread.id),
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(shell.contains("menu-title\">Direct</li>"));
    assert!(shell.contains(&format!("principal_id={}", inbox.principal_id.unwrap())));
    assert!(shell.contains("Composed private body") && shell.contains("Continued private body"));
    let html = pane(&h, actor.clone(), inbox, thread.id).await.unwrap();
    assert!(html.contains("Composed private body") && html.contains("Continued private body"));
    assert!(html.contains(&format!(
        "name=\"principal_id\" value=\"{}\"",
        inbox.principal_id.unwrap()
    )));
    assert!(!html.contains("Open in Simulator") && !html.contains("Run locally"));
}

#[tokio::test]
async fn direct_thread_ui_private_reads_and_retirement_do_not_expose_a_target_to_teammates() {
    let Some(h) = Harness::isolated().await else {
        return;
    };
    let actor = member(&h, "private_person").await;
    let other = member(&h, "other_person").await;
    let inbox = selection(&h, &actor).await;
    let thread = direct_thread(&h, &actor, inbox).await;
    for viewer in [actor.clone(), h.owner.clone()] {
        assert!(
            pane(&h, viewer, inbox, thread.id)
                .await
                .unwrap()
                .contains("Private direct body")
        );
    }
    assert!(matches!(
        pane(&h, other.clone(), inbox, thread.id).await,
        Err(AppError::NotFound(_))
    ));
    assert!(
        !h.threads
            .direct_inboxes(&other, h.company_id)
            .await
            .unwrap()
            .iter()
            .any(|entry| entry.target() == thread.target)
    );
    let foreign = Harness::isolated().await.unwrap();
    let invalid = message_pane_fragment(
        State(h.channels.clone()),
        State(h.threads.clone()),
        State(h.reader()),
        h.owner.clone(),
        Query(ThreadQuery {
            company_id: foreign.company_id,
            inbox,
            thread_id: thread.id,
        }),
    )
    .await;
    assert!(matches!(invalid, Err(AppError::NotFound(_))));
    let wrong = InboxSelection {
        channel_id: Some(h.channel_id),
        principal_id: None,
    };
    assert!(matches!(
        pane(&h, h.owner.clone(), wrong, thread.id).await,
        Err(AppError::NotFound(_))
    ));
    h.persistence
        .remove_member(h.company_id, actor.user_id)
        .await
        .unwrap();
    assert!(matches!(
        pane(&h, actor.clone(), inbox, thread.id).await,
        Err(AppError::NotFound(_))
    ));
    let archive = pane(&h, h.owner.clone(), inbox, thread.id).await.unwrap();
    assert!(archive.contains("Private direct body"));
    assert!(!archive.contains("hx-post=\"/ui/reply\"") && !archive.contains("New Message"));
    assert!(!archive.contains("hx-post=\"/ui/internal-notes\""));
    let archived = h
        .threads
        .direct_inboxes(&h.owner, h.company_id)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.target() == thread.target)
        .unwrap();
    assert!(archived.slug().is_none());
}

#[tokio::test]
async fn direct_thread_ui_live_streams_reconcile_then_close_before_revoked_activity() {
    let Some(h) = Harness::isolated().await else {
        return;
    };
    let actor = member(&h, "live_person").await;
    let inbox = selection(&h, &actor).await;
    let thread = direct_thread(&h, &actor, inbox).await;
    let column = thread_column_stream(
        State(h.channels.clone()),
        State(h.threads.clone()),
        State(h.reader()),
        State(h.events.clone()),
        State(h.config.clone()),
        actor.clone(),
        HeaderMap::new(),
        Query(ChannelStreamQuery {
            company_id: h.company_id,
            inbox,
            after: None,
        }),
    )
    .await
    .unwrap();
    let messages = thread_message_stream(
        State(h.channels.clone()),
        State(h.threads.clone()),
        State(h.reader()),
        State(h.events.clone()),
        actor.clone(),
        HeaderMap::new(),
        Query(ThreadStreamQuery {
            company_id: h.company_id,
            inbox,
            thread_id: thread.id,
            after: None,
        }),
    )
    .await
    .unwrap();
    let mut column = Frames::of(column.into_response());
    let mut messages = Frames::of(messages.into_response());
    assert!(
        column
            .next_named("thread")
            .await
            .data
            .contains("Private direct subject")
    );
    assert!(
        messages
            .next_named("message")
            .await
            .data
            .contains("Private direct body")
    );
    h.persistence
        .remove_member(h.company_id, actor.user_id)
        .await
        .unwrap();
    h.set_activity(thread.id, Some(ThreadActivity::AwaitingReview));
    h.events.publish(MailboxEvent::ActivityChanged(ThreadScope {
        company_id: h.company_id,
        target: thread.target,
        thread_id: thread.id,
    }));
    messages.expect_closed().await;
    column.expect_closed().await;
    assert!(
        thread_message_stream(
            State(h.channels.clone()),
            State(h.threads.clone()),
            State(h.reader()),
            State(h.events.clone()),
            actor,
            HeaderMap::new(),
            Query(ThreadStreamQuery {
                company_id: h.company_id,
                inbox,
                thread_id: thread.id,
                after: None,
            })
        )
        .await
        .is_err()
    );
}

#[test]
fn direct_thread_ui_selection_requires_exactly_one_typed_target() {
    let id = Uuid::new_v4();
    let principal = PrincipalId::new(id);
    let query = axum::extract::Query::<ChannelQuery>::try_from_uri(
        &format!("/ui/threads?company_id={id}&principal_id={id}")
            .parse()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        query.inbox.target().unwrap(),
        Some(AddressTarget::Principal(principal))
    );
    assert!(
        InboxSelection {
            channel_id: Some(id),
            principal_id: Some(principal)
        }
        .target()
        .is_err()
    );
    assert_ne!(AddressTarget::Desk(id), AddressTarget::Principal(principal));
}

#[tokio::test]
async fn direct_thread_ui_notes_preserve_target_pin_and_recheck_lifecycle_even_on_replay() {
    use crate::entities::internal_note::{
        AddInternalNote, InternalNoteProvenance, TombstoneInternalNote,
    };
    let Some(h) = Harness::isolated().await else {
        return;
    };
    let actor = member(&h, "notes_person").await;
    let other = member(&h, "notes_other").await;
    let inbox = selection(&h, &actor).await;
    let thread = direct_thread(&h, &actor, inbox).await;
    let context = h
        .threads
        .principal_access_for_user(h.company_id, actor.user_id)
        .await
        .unwrap()
        .unwrap();
    let command = AddInternalNote {
        company_id: h.company_id,
        target: thread.target,
        thread_id: thread.id,
        text: "Private note".into(),
        command_id: Uuid::new_v4(),
        supersedes_note_id: None,
        provenance: InternalNoteProvenance::HumanUi,
    };
    let first = h
        .threads
        .add_internal_note(command.clone(), context)
        .await
        .unwrap();
    let replay = h
        .threads
        .add_internal_note(command.clone(), context)
        .await
        .unwrap();
    assert_eq!(first.canonical_id, replay.canonical_id);
    let pin: (Option<Uuid>, Option<Uuid>, Uuid) = sqlx::query_as(
        "SELECT channel_id,principal_id,thread_id FROM internal_notes WHERE company_id=$1 AND message_id=$2")
        .bind(h.company_id).bind(first.canonical_id.as_uuid()).fetch_one(h.persistence.pool()).await.unwrap();
    assert_eq!(
        pin,
        (None, inbox.principal_id.map(|id| id.as_uuid()), thread.id)
    );
    let stranger = h
        .threads
        .principal_access_for_user(h.company_id, other.user_id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        h.threads.add_internal_note(command.clone(), stranger).await,
        Err(AppError::NotFound(_))
    ));
    let correction = h
        .threads
        .add_internal_note(
            AddInternalNote {
                command_id: Uuid::new_v4(),
                supersedes_note_id: Some(first.internal_note.unwrap().id),
                text: "Corrected private note".into(),
                ..command.clone()
            },
            context,
        )
        .await
        .unwrap();
    let removed = h
        .threads
        .tombstone_internal_note(
            TombstoneInternalNote {
                company_id: h.company_id,
                target: thread.target,
                thread_id: thread.id,
                note_id: correction.internal_note.unwrap().id,
                command_id: Uuid::new_v4(),
            },
            context,
        )
        .await
        .unwrap();
    assert!(removed.tombstoned_at.is_some());
    h.persistence
        .remove_member(h.company_id, actor.user_id)
        .await
        .unwrap();
    assert!(matches!(
        h.threads.add_internal_note(command.clone(), context).await,
        Err(AppError::NotFound(_))
    ));
    // The storage entry point repeats current authority, even when application facts are stale.
    assert!(matches!(
        h.persistence
            .create_internal_note(&command, context.principal_id.unwrap())
            .await,
        Err(AppError::NotFound(_))
    ));
}

#[path = "ui_direct_asset_tests.rs"]
mod assets;
