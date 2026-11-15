//! Real shared ingress and viewer projections, using the same transaction as desk mail.
use super::*;
use crate::{
    adapters::protocols::email::{EmailIngressAdapter, EmailIngressTrust},
    entities::address::AddressTarget,
    transport::ReplyDelivery,
    use_cases::thread::{
        CanonicalMessageIngress, InboundMessage, InboundPreflight, InboxReadScope, IngressOrigin,
        ThreadReadScope, ThreadUseCases,
    },
};
use std::sync::Arc;

#[path = "status.rs"]
mod status;
#[path = "validation.rs"]
mod validation;

#[path = "identity_races.rs"]
mod identity_races;
#[path = "notifications.rs"]
mod notifications;

const DOMAIN: &str = "direct.test";

// Exercise p02 ingress policies against a production-created principal, binding and workflow.
async fn production_agent(f: &Fixture, public: bool) -> Uuid {
    use crate::adapters::persistence::{
        company_agent::CompanyAgentService, test_support::provisioning_templates,
    };
    use crate::application::use_cases::{
        agent::CompanyAgents, company::CompanyModelConnectionWrite,
    };
    use crate::application::workflow::{CompanyId, WorkflowActor};
    CompanyPersistence::replace_model_connections_for_user(
        &f.persistence,
        f.owner,
        f.company.id,
        vec![CompanyModelConnectionWrite {
            provider: "openai".into(),
            api_key: Some("fixture-key".into()),
            models: vec!["gpt-4o-mini".into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let service = CompanyAgentService::new(f.persistence.clone(), provisioning_templates());
    // Publication and template compilation are a substantive future; keep fixture stack bounded.
    let created = Box::pin(service.create(
        WorkflowActor::authenticated(f.owner).unwrap(),
        CompanyId::new(f.company.id),
        AgentWrite {
            name: "Direct agent".into(),
            slug: "agent-direct-inbox".into(),
            open_to_public: public,
            confirm_spam_disabled: public,
            ..Default::default()
        },
        None,
    ))
    .await
    .unwrap();
    created.principal_id.as_uuid()
}

fn threads(f: &Fixture) -> ThreadUseCases {
    crate::adapters::persistence::test_support::thread_use_cases(&f.persistence, DOMAIN)
}

async fn canonical(f: &Fixture, target: Uuid, sender: Uuid) -> CanonicalMessageIngress {
    CanonicalMessageIngress::new(
        f.company.id,
        AddressTarget::Principal(PrincipalId::new(target)),
        crate::application::ports::thread::qualified_email_identity(
            email_of(f.pool(), sender).await,
        )
        .unwrap(),
        "Private correspondence",
        IngressOrigin::TrustedApplication,
    )
    .with_subject("Direct subject")
}

async fn email(f: &Fixture, target: Uuid, sender: &str, key: &str, root: &str) -> InboundMessage {
    let address = f
        .persistence
        .address_for_target(
            f.company.id,
            AddressTarget::Principal(PrincipalId::new(target)),
        )
        .await
        .unwrap()
        .unwrap();
    let to = format!("{}@{}.{DOMAIN}", address.slug, f.company.slug);
    EmailIngressAdapter::new(DOMAIN).accept(
        crate::adapters::protocols::email::parser::RawInboundPayload {
            from: sender.into(), to, subject: Some("Same subject".into()), text: Some(format!("Body {key}")),
            headers: Some(format!("Message-ID: <{key}@sender.test>\nIn-Reply-To: <{root}@sender.test>\nReferences: <{root}@sender.test>\n")),
            ..Default::default()
        }, EmailIngressTrust::Application,
    ).unwrap().into_inbound(IngressOrigin::TrustedApplication, ReplyDelivery::Send)
}

fn viewer(user: Uuid) -> crate::entities::user::Viewer {
    crate::entities::user::Viewer {
        user_id: user,
        email: "viewer@example.test".into(),
        email_verified_at: Some(chrono::Utc::now()),
    }
}

fn scope(f: &Fixture, principal: Uuid, thread_id: Uuid) -> ThreadReadScope {
    ThreadReadScope {
        inbox: InboxReadScope {
            company_id: f.company.id,
            target: AddressTarget::Principal(PrincipalId::new(principal)),
        },
        thread_id,
    }
}

#[tokio::test]
async fn direct_thread_real_ingress_and_all_viewer_projections_enforce_person_target() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (member, name) = account(&f.persistence, "direct_reader").await;
    let principal = f.join(member, &name).await;
    let (teammate, name) = account(&f.persistence, "direct_other").await;
    f.join(teammate, &name).await;
    let service = threads(&f);
    let accepted = service
        .ingest_canonical(canonical(&f, principal, member).await)
        .await
        .unwrap();
    assert!(accepted.accepted, "{:?}", accepted.rejection);
    let thread = accepted.thread.unwrap();
    assert!(thread.belongs_to(
        f.company.id,
        AddressTarget::Principal(PrincipalId::new(principal))
    ));
    assert!(thread.contains_principal(PrincipalId::new(principal)));
    assert!(accepted.channel.is_none());
    let read = scope(&f, principal, thread.id);
    for user in [member, f.owner] {
        let messages = service
            .get_thread_history(&viewer(user), read)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "Private correspondence");
        let audit = service
            .get_message_audit(&viewer(user), read, messages[0].id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(audit.target, read.inbox.target);
    }
    assert!(
        service
            .get_thread_history(&viewer(teammate), read)
            .await
            .is_err()
    );
    assert!(
        service
            .list_inbox_threads(&viewer(teammate), read.inbox, None, 50)
            .await
            .is_err()
    );
    let refused = service
        .ingest_canonical(canonical(&f, principal, teammate).await)
        .await
        .unwrap();
    assert!(!refused.accepted);
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM threads WHERE company_id=$1 AND principal_id=$2")
            .bind(f.company.id)
            .bind(principal)
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(stored, 1);
}

#[tokio::test]
async fn direct_thread_unrelated_roots_stay_separate_and_replies_reuse_the_target() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let principal = f.principal_of(f.owner).await;
    let service = threads(&f);
    let root = service
        .ingest_canonical(canonical(&f, principal, f.owner).await)
        .await
        .unwrap();
    let other = service
        .ingest_canonical(canonical(&f, principal, f.owner).await)
        .await
        .unwrap();
    assert_ne!(root.thread.as_ref().unwrap().id, other.thread.unwrap().id);
    let reply = canonical(&f, principal, f.owner)
        .await
        .with_reply_to_message(root.inbound_message.unwrap().canonical_id);
    let continued = service.ingest_canonical(reply).await.unwrap();
    assert_eq!(root.thread.unwrap().id, continued.thread.unwrap().id);
}

#[tokio::test]
async fn direct_competing_same_conversation_and_redelivery_keep_one_target_thread() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let principal = f.principal_of(f.owner).await;
    let sender = email_of(f.pool(), f.owner).await;
    let service = threads(&f);
    let first = email(&f, principal, &sender, "first", "shared-root").await;
    let second = email(&f, principal, &sender, "second", "shared-root").await;
    let (first, second) = tokio::join!(service.ingest(first), service.ingest(second));
    let first = first.unwrap();
    let second = second.unwrap();
    assert!(first.accepted && second.accepted);
    assert_eq!(first.thread.unwrap().id, second.thread.unwrap().id);
    let repeated = email(&f, principal, &sender, "first", "shared-root").await;
    assert!(service.ingest(repeated).await.unwrap().accepted);
    let counts: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM threads WHERE company_id=$1), \
        (SELECT count(*) FROM thread_messages WHERE company_id=$1), (SELECT count(*) FROM workflow_message_intents WHERE company_id=$1)")
        .bind(f.company.id).fetch_one(f.pool()).await.unwrap();
    assert_eq!(counts, (1, 2, 2));
}

#[tokio::test]
async fn direct_thread_stale_slug_rolls_back_every_ingress_row() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let principal = f.principal_of(f.owner).await;
    let sender = email_of(f.pool(), f.owner).await;
    let service = threads(&f);
    let message = email(&f, principal, &sender, "stale", "root").await;
    let InboundPreflight::Accepted(prepared) = service.preflight_inbound(message).await.unwrap()
    else {
        panic!("preflight refused");
    };
    f.persistence
        .rename_person_address(f.owner, f.company.id, &ChannelSlug::from("new-person-slug"))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        service.commit_prepared_inbound(*prepared).await,
        Err(AppError::Superseded(_))
    ));
    for table in [
        "threads",
        "messages",
        "external_messages",
        "workflow_message_intents",
    ] {
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE company_id=$1"))
                .bind(f.company.id)
                .fetch_one(f.pool())
                .await
                .unwrap();
        assert_eq!(count, 0, "{table}");
    }
    assert!(
        service
            .ingest_canonical(canonical(&f, principal, f.owner).await)
            .await
            .unwrap()
            .accepted
    );
}

#[tokio::test]
async fn direct_thread_agent_policy_and_revocation_do_not_borrow_participant_authority() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let agent = production_agent(&f, false).await;
    let service = threads(&f);
    let denied = service
        .ingest(email(&f, agent, "visitor@example.test", "private", "root").await)
        .await
        .unwrap();
    assert!(!denied.accepted);
    assert!(denied.bounce_info().unwrap().principals.is_empty());
    sqlx::query("UPDATE principals SET open_to_public=true WHERE company_id=$1 AND id=$2")
        .bind(f.company.id)
        .bind(agent)
        .execute(f.pool())
        .await
        .unwrap();
    let accepted = service
        .ingest(email(&f, agent, "visitor@example.test", "public", "root").await)
        .await
        .unwrap();
    assert!(accepted.accepted, "{:?}", accepted.rejection);
    let thread = accepted.thread.unwrap();
    let (teammate, name) = account(&f.persistence, "agent_inbox_reader").await;
    f.join(teammate, &name).await;
    assert_eq!(
        service
            .get_thread_history(&viewer(teammate), scope(&f, agent, thread.id))
            .await
            .unwrap()
            .len(),
        1
    );
    sqlx::query("UPDATE principals SET open_to_public=false WHERE company_id=$1 AND id=$2")
        .bind(f.company.id)
        .bind(agent)
        .execute(f.pool())
        .await
        .unwrap();
    let refused = service
        .ingest(email(&f, agent, "visitor@example.test", "after-revoke", "root").await)
        .await
        .unwrap();
    assert!(
        !refused.accepted,
        "a prior thread participant cannot bypass revocation"
    );
    let copies: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM thread_messages WHERE company_id=$1 AND principal_id=$2",
    )
    .bind(f.company.id)
    .bind(agent)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(copies, 1);
}

#[tokio::test]
async fn direct_thread_identity_adoption_preserves_the_direct_target_and_membership_pin() {
    use crate::application::ports::thread::ThreadPersistence;
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let agent = production_agent(&f, true).await;
    let (user, username) = account(&f.persistence, "direct_existing_owner").await;
    let person = f.join(user, &username).await;
    let name = format!("direct_adopt_{}", Uuid::new_v4().simple());
    let mailbox = format!("{name}@example.test");
    let accepted = threads(&f)
        .ingest(email(&f, agent, &mailbox, "adopt", "root").await)
        .await
        .unwrap();
    assert!(accepted.accepted);
    let thread = accepted.thread.unwrap();
    let outsider = accepted.inbound_message.unwrap().author.principal_id;
    assert_ne!(outsider, PrincipalId::new(person));
    super::rejoin::change_email(&f, user, &EmailAddress::from(mailbox)).await;
    let current = f
        .persistence
        .get_thread_by_id(f.company.id, thread.id)
        .await
        .unwrap()
        .unwrap();
    assert!(current.belongs_to(
        f.company.id,
        AddressTarget::Principal(PrincipalId::new(agent))
    ));
    assert!(current.contains_principal(PrincipalId::new(person)));
    assert!(!current.contains_principal(outsider));
    assert!(current.contains_principal(PrincipalId::new(agent)));
}

#[tokio::test]
async fn direct_trace_native_shape_and_bound_match_the_typed_wire_contract() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let id = Uuid::new_v4();
    let valid = serde_json::json!([
        AddressTarget::Desk(id),
        AddressTarget::Principal(PrincipalId::new(id))
    ]);
    let check =
        |value| sqlx::query_scalar::<_, bool>("SELECT address_target_trace_valid($1)").bind(value);
    assert!(check(valid).fetch_one(f.pool()).await.unwrap());
    for invalid in [
        serde_json::json!([id]),
        serde_json::json!({}),
        serde_json::json!([{"kind":"other","id":id}]),
        serde_json::json!([{"kind":"desk","id":"broken"}]),
        serde_json::json!([{"kind":"desk","id":id,"extra":true}]),
        serde_json::to_value(vec![AddressTarget::Desk(id); 6]).unwrap(),
    ] {
        assert!(!check(invalid).fetch_one(f.pool()).await.unwrap());
    }
}

#[tokio::test]
async fn direct_thread_native_message_and_note_wakes_carry_the_principal_target() {
    use crate::entities::internal_note::{
        AddInternalNote, InternalNoteProvenance, TombstoneInternalNote,
    };
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let principal = f.principal_of(f.owner).await;
    let service = threads(&f);
    let mut listener = sqlx::postgres::PgListener::connect_with(f.pool())
        .await
        .unwrap();
    listener.listen("thread_message").await.unwrap();
    let result = service
        .ingest_canonical(canonical(&f, principal, f.owner).await)
        .await
        .unwrap();
    assert!(result.accepted);
    let thread = result.thread.unwrap();
    async fn wake(
        listener: &mut sqlx::postgres::PgListener,
        thread: &crate::entities::thread::Thread,
    ) {
        let event = tokio::time::timeout(std::time::Duration::from_secs(10), listener.recv())
            .await
            .unwrap()
            .unwrap();
        let scope: crate::infra::events::ThreadScope =
            serde_json::from_str(event.payload()).unwrap();
        assert_eq!(scope.thread_id, thread.id);
        assert_eq!(scope.company_id, thread.company_id);
        assert_eq!(scope.target, thread.target);
    }
    wake(&mut listener, &thread).await;
    let actor = service
        .principal_access_for_user(f.company.id, f.owner)
        .await
        .unwrap()
        .unwrap();
    let note = service
        .add_internal_note(
            AddInternalNote {
                company_id: f.company.id,
                target: thread.target,
                thread_id: thread.id,
                text: "Native direct note".into(),
                command_id: Uuid::new_v4(),
                supersedes_note_id: None,
                provenance: InternalNoteProvenance::HumanUi,
            },
            actor,
        )
        .await
        .unwrap();
    wake(&mut listener, &thread).await;
    service
        .tombstone_internal_note(
            TombstoneInternalNote {
                company_id: f.company.id,
                target: thread.target,
                thread_id: thread.id,
                note_id: note.internal_note.unwrap().id,
                command_id: Uuid::new_v4(),
            },
            actor,
        )
        .await
        .unwrap();
    wake(&mut listener, &thread).await;
}
