use super::*;

#[tokio::test]
async fn direct_thread_private_address_and_unknown_address_have_identical_suggestions() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let target = f.principal_of(f.owner).await;
    f.persistence
        .rename_person_address(f.owner, f.company.id, &ChannelSlug::from("alice"))
        .await
        .unwrap()
        .unwrap();
    ChannelPersistence::create(
        &f.persistence,
        f.company.id,
        ChannelWrite {
            name: "Nearby shared desk".into(),
            slug: "alica".into(),
            enabled: true,
            participant_emails: Some(vec!["@public".into()]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let message = email(&f, target, "stranger@example.test", "privacy", "root").await;
    let service = threads(&f);
    let private = service.ingest(message.clone()).await.unwrap();
    assert!(!private.accepted);
    f.persistence
        .rename_person_address(f.owner, f.company.id, &ChannelSlug::from("elsewhere"))
        .await
        .unwrap()
        .unwrap();
    let absent = service.ingest(message).await.unwrap();
    assert!(!absent.accepted);
    assert!(
        absent
            .bounce_info()
            .unwrap()
            .suggestions
            .iter()
            .any(|suggestion| !suggestion.suggestions.is_empty()),
        "a nearby public desk must exercise the suggestion branch"
    );
    assert_eq!(private.rejection, absent.rejection);
    assert_eq!(private.bounce_info(), absent.bounce_info());
}

#[tokio::test]
async fn direct_thread_commit_requires_one_intent_for_every_gated_target_before_writing() {
    use crate::transport::{BoundedVec, InboundMessageCommitter};
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let target = f.principal_of(f.owner).await;
    let service = threads(&f);
    let input = email(
        &f,
        target,
        &email_of(f.pool(), f.owner).await,
        "alignment",
        "root",
    )
    .await;
    let InboundPreflight::Accepted(prepared) = service.preflight_inbound(input).await.unwrap()
    else {
        panic!("preflight refused");
    };
    let request = prepared.into_commit_request();
    let mut missing = request.clone();
    missing.intents = BoundedVec::empty();
    let mut wrong = request.clone();
    let mut facts = wrong.intents.into_inner();
    facts[0].target = AddressTarget::Principal(PrincipalId::random());
    wrong.intents = BoundedVec::parse("wrong target", facts).unwrap();
    let mut duplicate = request.clone();
    duplicate.intents = BoundedVec::parse(
        "duplicate targets",
        vec![request.intents[0], request.intents[0]],
    )
    .unwrap();
    for invalid in [missing, wrong, duplicate] {
        assert!(matches!(
            f.persistence.commit_inbound(invalid).await,
            Err(AppError::BadRequest(_))
        ));
    }
    let writes: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM threads WHERE company_id=$1), \
        (SELECT count(*) FROM messages WHERE company_id=$1), (SELECT count(*) FROM workflow_message_intents WHERE company_id=$1)")
        .bind(f.company.id).fetch_one(f.pool()).await.unwrap();
    assert_eq!(writes, (0, 0, 0));
    assert!(f.persistence.commit_inbound(request).await.is_ok());
}

#[tokio::test]
async fn address_resolution_directory_lists_only_currently_writable_people_and_agents_after_desks()
{
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (user, name) = account(&f.persistence, "directory_sender").await;
    let person = f.join(user, &name).await;
    let (other, name) = account(&f.persistence, "directory_private").await;
    let private = f.join(other, &name).await;
    let agent = production_agent(&f, false).await;
    ChannelPersistence::create(
        &f.persistence,
        f.company.id,
        ChannelWrite {
            name: "Team desk".into(),
            slug: "shared".into(),
            enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let message = |sender: String| {
        EmailIngressAdapter::new(DOMAIN)
            .accept(
                crate::adapters::protocols::email::parser::RawInboundPayload {
                    from: sender,
                    to: format!("missing@{}.{DOMAIN}", f.company.slug),
                    text: Some("Directory request".into()),
                    ..Default::default()
                },
                EmailIngressTrust::Application,
            )
            .unwrap()
            .into_inbound(IngressOrigin::TrustedApplication, ReplyDelivery::Send)
    };
    let result = threads(&f)
        .ingest(message(email_of(f.pool(), user).await))
        .await
        .unwrap();
    let entries = &result.bounce_info().unwrap().available_channels;
    let targets: Vec<_> = entries.iter().map(|entry| entry.target).collect();
    assert!(targets.contains(&AddressTarget::Principal(PrincipalId::new(person))));
    assert!(targets.contains(&AddressTarget::Principal(PrincipalId::new(agent))));
    assert!(!targets.contains(&AddressTarget::Principal(PrincipalId::new(private))));
    assert!(
        !targets.contains(&AddressTarget::Principal(PrincipalId::new(
            f.principal_of(f.owner).await
        )))
    );
    let help = crate::use_cases::thread::format_help_email_body(entries, &f.company.slug, DOMAIN);
    assert!(
        help.find("Desks you can write to:").unwrap() < help.find("People and agents:").unwrap()
    );
    assert!(!help.contains(&email_of(f.pool(), other).await));
    let outsider = threads(&f)
        .ingest(message("stranger@example.test".into()))
        .await
        .unwrap();
    assert!(
        outsider
            .bounce_info()
            .unwrap()
            .available_channels
            .is_empty()
    );
    f.persistence
        .remove_member(crate::use_cases::company_invite::RemoveCompanyMember {
            actor_id: f.company.user_id,
            company_id: f.company.id,
            member_user_id: other,
        })
        .await
        .unwrap();
    let owner = threads(&f)
        .ingest(message(email_of(f.pool(), f.owner).await))
        .await
        .unwrap();
    assert!(
        !owner
            .bounce_info()
            .unwrap()
            .available_channels
            .iter()
            .any(|entry| entry.target == AddressTarget::Principal(PrincipalId::new(private))),
        "archived targets never become deliverable suggestions"
    );
}

#[tokio::test]
async fn direct_thread_native_foreign_target_and_wrong_membership_pin_are_rejected() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let target = f.principal_of(f.owner).await;
    let (other, name) = account(&f.persistence, "pin_other").await;
    let other_target = f.join(other, &name).await;
    let (foreign_owner, _) = account(&f.persistence, "pin_foreign").await;
    let foreign = company(&f.persistence, foreign_owner).await;
    let accepted = threads(&f)
        .ingest_canonical(canonical(&f, target, f.owner).await)
        .await
        .unwrap();
    let thread = accepted.thread.unwrap();
    let original_message = accepted.inbound_message.unwrap().canonical_id.as_uuid();
    // A different real payload avoids the existing UNIQUE(thread_id,message_id), so the
    // negative association must reach its actual wrong-principal/thread membership FK.
    let separate = threads(&f)
        .ingest_canonical(
            canonical(&f, target, f.owner)
                .await
                .with_subject("Separate payload"),
        )
        .await
        .unwrap();
    assert_ne!(separate.thread.unwrap().id, thread.id);
    let message = separate.inbound_message.unwrap().canonical_id.as_uuid();
    assert_ne!(message, original_message);
    let error = sqlx::query(
        "INSERT INTO threads(id,company_id,principal_id,subject) VALUES($1,$2,$3,'Wrong tenant')",
    )
    .bind(Uuid::new_v4())
    .bind(foreign.id)
    .bind(target)
    .execute(f.pool())
    .await
    .unwrap_err();
    assert_eq!(
        refused_by(error),
        ("23503".into(), "threads_target_principal_fk".into())
    );
    let error = sqlx::query("INSERT INTO thread_messages(id,company_id,principal_id,thread_id,message_id,entry_kind) VALUES($1,$2,$3,$4,$5,'conversation')")
        .bind(Uuid::new_v4()).bind(f.company.id).bind(other_target).bind(thread.id).bind(message)
        .execute(f.pool()).await.unwrap_err();
    assert_eq!(
        refused_by(error),
        (
            "23503".into(),
            "thread_messages_thread_fk_principal_fk".into()
        )
    );
    let error = sqlx::query("INSERT INTO thread_principals(company_id,target_principal_id,thread_id,principal_id,role) VALUES($1,$2,$3,$4,'participant')")
        .bind(f.company.id).bind(other_target).bind(thread.id).bind(other_target)
        .execute(f.pool()).await.unwrap_err();
    assert_eq!(
        refused_by(error),
        (
            "23503".into(),
            "thread_principals_thread_fk_principal_fk".into()
        )
    );
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM thread_messages WHERE company_id=$1 AND thread_id=$2 AND principal_id=$3 AND message_id=$4")
        .bind(f.company.id).bind(thread.id).bind(target).bind(original_message).fetch_one(f.pool()).await.unwrap();
    assert_eq!(
        stored, 1,
        "rejected mutations preserve the canonical exact membership"
    );
}
