use super::super::message::{MessageQuery, page};
use super::*;

async fn invite_journey_member(h: &Harness) -> crate::entities::user::User {
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("journey-{suffix}@example.test");
    let user = h
        .persistence
        .create_user(&format!("journey_{suffix}"), &email, "hash")
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role) VALUES ($1, $2, $3, 'admin')",
    )
    .bind(Uuid::new_v4())
    .bind(h.company_id)
    .bind(user.id)
    .execute(h.persistence.pool())
    .await
    .unwrap();
    let principal_id = Uuid::new_v4();
    sqlx::query("INSERT INTO principals (id, company_id, kind, user_id, display_label) VALUES ($1, $2, 'person', $3, 'Journey member')")
        .bind(principal_id)
        .bind(h.company_id)
        .bind(user.id)
        .execute(h.persistence.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE channels SET access_mode = 'allowlist' WHERE id = $1")
        .bind(h.channel_id)
        .execute(h.persistence.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO channel_principal_grants (company_id, channel_id, principal_id, capability, provenance) VALUES ($1, $2, $3, 'view', 'configured_allowlist')")
        .bind(h.company_id)
        .bind(h.channel_id)
        .bind(principal_id)
        .execute(h.persistence.pool())
        .await
        .unwrap();
    user
}

fn journey_viewer(user: &crate::entities::user::User) -> Viewer {
    Viewer {
        user_id: user.id,
        email: EmailAddress::from(user.email.as_str()),
        email_verified_at: user.email_verified_at,
    }
}

async fn invitation_for_journey(
    h: &Harness,
    member: &crate::entities::user::User,
) -> (
    crate::use_cases::company_invite::CompanyInviteUseCases,
    crate::entities::company_invite::CompanyInvite,
) {
    use crate::entities::company_member::CompanyAccessRole;
    let invitations = crate::use_cases::company_invite::CompanyInviteUseCases::new(
        h.persistence.clone(),
        h.persistence.clone(),
    );
    let company = CompanyPersistence::create(
        h.persistence.as_ref(),
        h.owner.user_id,
        CompanyWrite {
            name: "Invitation journey".into(),
            slug: format!("invitation-journey-{}", Uuid::new_v4().simple()),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let invite = invitations
        .create_company_invite(
            h.owner.user_id,
            company.id,
            &member.email,
            CompanyAccessRole::Admin,
        )
        .await
        .unwrap();
    (invitations, invite)
}

async fn install_unverified_journey_address(
    h: &Harness,
    user: &crate::entities::user::User,
) -> crate::entities::user::User {
    use crate::use_cases::user::ProfileUpdate;
    let next = EmailAddress::from(format!("next-{}@example.test", Uuid::new_v4().simple()));
    UserPersistence::update_profile(
        h.persistence.as_ref(),
        user.id,
        ProfileUpdate {
            username: &user.username,
            email: &next,
            avatar_url: user.avatar_url.as_ref(),
        },
    )
    .await
    .unwrap();
    UserPersistence::get_by_id(h.persistence.as_ref(), user.id)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn security_email_identity_one_session_loses_invite_channel_and_operator_after_address_change()
 {
    use crate::adapters::http::session::SessionAuthority;

    let Some(h) = Harness::new().await else {
        return;
    };
    let member = invite_journey_member(&h).await;
    let (invitations, invite) = invitation_for_journey(&h, &member).await;
    let mut operator_config = test_config();
    Arc::get_mut(&mut operator_config)
        .unwrap()
        .operator_emails
        .push(EmailAddress::from(member.email.as_str()));
    let sessions = SessionAuthority::new(&operator_config, h.persistence.clone());
    let same_cookie = sessions.cookie(&member).await.unwrap().value().to_string();

    assert!(invitations.accept_invite(&member, invite.id).await.is_err());
    assert!(
        h.channels
            .get_readable_channel(&journey_viewer(&member), h.company_id, h.channel_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!operator_config.is_operator(&member));

    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(member.id)
        .execute(h.persistence.pool())
        .await
        .unwrap();
    let verified = UserPersistence::get_by_id(h.persistence.as_ref(), member.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        sessions
            .verify(&same_cookie)
            .await
            .unwrap()
            .map(|s| s.user_id),
        Some(member.id)
    );
    assert_eq!(
        invitations
            .accept_invite(&verified, invite.id)
            .await
            .unwrap()
            .status,
        "accepted"
    );
    assert!(
        h.channels
            .get_readable_channel(&journey_viewer(&verified), h.company_id, h.channel_id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(operator_config.is_operator(&verified));

    let changed = install_unverified_journey_address(&h, &verified).await;
    assert_eq!(
        sessions
            .verify(&same_cookie)
            .await
            .unwrap()
            .map(|s| s.user_id),
        None
    );
    assert!(changed.email_verified_at.is_none());
    assert!(matches!(
        invitations.accept_invite(&verified, invite.id).await,
        Err(AppError::NotFound(_))
    ));
    assert!(
        h.channels
            .get_readable_channel(&journey_viewer(&changed), h.company_id, h.channel_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!operator_config.is_operator(&changed));
}

async fn unverified_team_viewers(h: &Harness) -> [Viewer; 2] {
    let suffix = Uuid::new_v4().simple().to_string();
    let member_email = format!("team-{suffix}@example.test");
    let member = h
        .persistence
        .create_user(&format!("team_{suffix}"), &member_email, "hash")
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role) VALUES ($1, $2, $3, 'admin')",
    )
    .bind(Uuid::new_v4())
    .bind(h.company_id)
    .bind(member.id)
    .execute(h.persistence.pool())
    .await
    .unwrap();
    let member_principal = Uuid::new_v4();
    sqlx::query("INSERT INTO principals (id, company_id, kind, user_id, display_label) VALUES ($1, $2, 'person', $3, 'Team member')")
        .bind(member_principal)
        .bind(h.company_id)
        .bind(member.id)
        .execute(h.persistence.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at = NULL WHERE id = ANY($1)")
        .bind([h.owner.user_id, member.id])
        .execute(h.persistence.pool())
        .await
        .unwrap();

    [
        h.owner.clone(),
        Viewer {
            user_id: member.id,
            email: EmailAddress::from(member_email.as_str()),
            email_verified_at: None,
        },
    ]
}

async fn assert_unverified_team_attention_and_note(h: &Harness, viewer: Viewer, thread_id: Uuid) {
    use crate::entities::{
        internal_note::{AddInternalNote, InternalNoteProvenance, TombstoneInternalNote},
        participant::PrincipalGrantAuthority,
    };
    let context = h
        .threads
        .principal_access_for_user(h.company_id, viewer.user_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        context.grant_authority,
        PrincipalGrantAuthority::WithheldUnverifiedEmail
    );
    let principal_id = context.principal_id.expect("account keeps its principal");
    let companies = CompanyUseCases::new(h.persistence.clone());
    let attention = super::super::super::attention::read_context(
        &companies,
        &h.channels,
        &h.threads,
        &viewer,
        h.company_id,
    )
    .await
    .unwrap();
    assert_eq!(attention.principal_id, principal_id);
    assert!(attention.visible_channel_ids.contains(&h.channel_id));

    let note = h
        .threads
        .add_internal_note(
            AddInternalNote {
                company_id: h.company_id,
                channel_id: h.channel_id,
                thread_id,
                text: "Unverified team note".into(),
                command_id: Uuid::new_v4(),
                supersedes_note_id: None,
                provenance: InternalNoteProvenance::HumanUi,
            },
            context,
        )
        .await
        .unwrap();
    let note_id = note.internal_note.expect("created note").id;
    let tombstoned = h
        .threads
        .tombstone_internal_note(
            TombstoneInternalNote {
                company_id: h.company_id,
                channel_id: h.channel_id,
                thread_id,
                note_id,
                command_id: Uuid::new_v4(),
            },
            context,
        )
        .await
        .unwrap();
    assert_eq!(tombstoned.author_principal_id, principal_id);
    assert!(tombstoned.tombstoned_at.is_some());
}

#[tokio::test]
async fn security_email_identity_unverified_team_keeps_attention_and_internal_notes() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let thread_id = h.thread("Team notes").await;
    for viewer in unverified_team_viewers(&h).await {
        assert_unverified_team_attention_and_note(&h, viewer, thread_id).await;
    }
}

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
