//! Deleting an agent retires it: what it wrote and took part in stays, under an outsider principal.
//!
//! The guarantee is the database's -- the foreign keys from an agent to its principal and on to
//! messages and their participants no longer cascade -- so these run against real databases.

use super::*;
use crate::adapters::persistence::test_support::{
    own_database, test_pool, wait_until_a_backend_is_blocked,
};
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::channel::ChannelWrite;
use crate::application::ports::participant::IdentityObservation;
use crate::entities::participant::{IdentityClaimMetadata, IdentityProvenance};
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
use crate::use_cases::participant::IdentityDirectory;
use crate::use_cases::user::UserPersistence;

/// A company with one agent that has spoken in a thread to a customer.
struct SpokenAgent {
    company_id: Uuid,
    agent_id: Uuid,
    principal_id: Uuid,
    thread_id: Uuid,
    message_id: Uuid,
}

async fn spoken_agent(persistence: &PostgresPersistence) -> SpokenAgent {
    let suffix = Uuid::new_v4().simple().to_string();
    let owner = persistence
        .create_user(
            &format!("retire_{suffix}"),
            &format!("retire_{suffix}@example.test"),
            "hash",
        )
        .await
        .unwrap();
    let company_id = CompanyPersistence::create(
        persistence,
        owner.id,
        CompanyWrite {
            name: "Retire".into(),
            slug: format!("retire-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap()
    .id;
    let channel = ChannelPersistence::create(
        persistence,
        company_id,
        ChannelWrite {
            name: "Support".into(),
            slug: "support".into(),
            enabled: true,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let agent_id = AgentPersistence::create(
        persistence,
        company_id,
        AgentWrite {
            name: "Helper".into(),
            slug: "helper".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let principal_id: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2")
            .bind(company_id)
            .bind(agent_id)
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    let customer = persistence
        .resolve_or_create_external_identity(
            company_id,
            IdentityObservation {
                identity: crate::adapters::protocols::email::EmailIdentity::parse(
                    crate::entities::value_objects::EmailAddress::from("customer@partner.test"),
                )
                .unwrap()
                .qualify_default(),
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance: IdentityProvenance::TransportIngress,
            },
        )
        .await
        .unwrap();

    let thread_id = Uuid::new_v4();
    let message_id = Uuid::new_v4();
    let pool = persistence.pool();
    sqlx::query(
        "INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1, $2, $3, 'Help')",
    )
    .bind(thread_id)
    .bind(company_id)
    .bind(channel.id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO messages
               (id, company_id, author_principal_id, subject, clean_text_body, direction, role,
                correlation_id, content_hash, audience, content_exact)
           VALUES ($1, $2, $3, 'Help', 'On it', 'outbound', 'agent', $4,
                   decode(repeat('cd', 32), 'hex'), 'external_conversation', false)"#,
    )
    .bind(message_id)
    .bind(company_id)
    .bind(principal_id)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO message_participants
               (company_id, message_id, participant_identity_id, kind, position)
           VALUES ($1, $2, $3, 'to', 0)"#,
    )
    .bind(company_id)
    .bind(message_id)
    .bind(customer.identity.id.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO thread_messages
               (id, company_id, channel_id, thread_id, message_id, entry_kind)
           VALUES ($1, $2, $3, $4, $5, 'conversation')"#,
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(channel.id)
    .bind(thread_id)
    .bind(message_id)
    .execute(pool)
    .await
    .unwrap();
    for (principal, role) in [
        (principal_id, "author"),
        (customer.principal.id.as_uuid(), "participant"),
    ] {
        sqlx::query(
            r#"INSERT INTO thread_principals (company_id, channel_id, thread_id, principal_id, role)
               VALUES ($1, $2, $3, $4, $5)"#,
        )
        .bind(company_id)
        .bind(channel.id)
        .bind(thread_id)
        .bind(principal)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    }
    SpokenAgent {
        company_id,
        agent_id,
        principal_id,
        thread_id,
        message_id,
    }
}

#[tokio::test]
async fn agent_retire_keeps_messages_participants_and_threads() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let spoken = spoken_agent(&persistence).await;

    AgentPersistence::delete(&persistence, spoken.agent_id)
        .await
        .unwrap();

    assert!(
        AgentPersistence::get_by_id(&persistence, spoken.agent_id)
            .await
            .unwrap()
            .is_none()
    );
    let (kind, agent_id): (String, Option<Uuid>) =
        sqlx::query_as("SELECT kind, agent_id FROM principals WHERE id = $1")
            .bind(spoken.principal_id)
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    assert_eq!((kind.as_str(), agent_id), ("external", None));
    let author: Uuid = sqlx::query_scalar("SELECT author_principal_id FROM messages WHERE id = $1")
        .bind(spoken.message_id)
        .fetch_one(persistence.pool())
        .await
        .unwrap();
    assert_eq!(author, spoken.principal_id, "the message keeps its author");
    let survivors: (i64, i64, i64) = sqlx::query_as(
        r#"SELECT (SELECT COUNT(*) FROM message_participants WHERE message_id = $1),
                  (SELECT COUNT(*) FROM thread_messages WHERE message_id = $1),
                  (SELECT COUNT(*) FROM thread_principals WHERE thread_id = $2)"#,
    )
    .bind(spoken.message_id)
    .bind(spoken.thread_id)
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    assert_eq!(survivors, (1, 1, 2), "participants and thread rows survive");

    // The database itself refuses the delete that used to cascade, should anything skip the
    // retirement: the principal still names an agent.
    let fresh = AgentPersistence::create(
        &persistence,
        spoken.company_id,
        AgentWrite {
            name: "Another".into(),
            slug: "another".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let raw = sqlx::query("DELETE FROM agents WHERE id = $1")
        .bind(fresh.id)
        .execute(persistence.pool())
        .await
        .expect_err("a plain delete would orphan the agent principal");
    assert_eq!(
        raw.as_database_error().and_then(|error| error.constraint()),
        Some("principals_company_agent_fk")
    );

    let _ = CompanyPersistence::delete(&persistence, spoken.company_id).await;
}

/// Deleting the company still removes everything it owns in one statement: the history foreign
/// keys no longer cascade, but they are NO ACTION, checked once the whole delete has run.
#[tokio::test]
async fn agent_retire_company_delete_still_removes_everything() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let spoken = spoken_agent(&persistence).await;
    let retired = spoken_agent(&persistence).await;
    AgentPersistence::delete(&persistence, retired.agent_id)
        .await
        .unwrap();

    for company_id in [spoken.company_id, retired.company_id] {
        assert!(
            CompanyPersistence::delete(&persistence, company_id)
                .await
                .is_ok()
        );
    }
    for table in [
        "companies",
        "company_members",
        "agents",
        "principals",
        "participant_identities",
        "messages",
        "message_participants",
        "threads",
        "thread_messages",
        "thread_principals",
        "channel_principal_grants",
    ] {
        let rows: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(persistence.pool())
            .await
            .unwrap();
        assert_eq!(rows, 0, "{table} still has rows");
    }
}

/// Competing claimants: a writer creating the agent's principal (as an agent message does) holds
/// its share lock on the agent row while the delete starts. The delete waits for it, then demotes
/// the principal it created; it never fails on that principal, and never leaves one pointing at a
/// deleted agent. In the other order the late writer is the one refused.
#[tokio::test]
async fn agent_retire_races_agent_principal_upsert() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let spoken = spoken_agent(&persistence).await;
    // Start from an agent with no principal yet, so the writer inserts one and its foreign key
    // check is what holds the agent row.
    let agent = AgentPersistence::create(
        &persistence,
        spoken.company_id,
        AgentWrite {
            name: "Racer".into(),
            slug: "racer".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sqlx::query("DELETE FROM principals WHERE agent_id = $1")
        .bind(agent.id)
        .execute(persistence.pool())
        .await
        .unwrap();

    let mut writer = persistence.pool().begin().await.unwrap();
    let written = crate::adapters::persistence::participant::create_agent_principal_on(
        &mut writer,
        spoken.company_id,
        agent.id,
        "Racer",
    )
    .await
    .unwrap();
    let delete = {
        let persistence = persistence.clone();
        tokio::spawn(async move { AgentPersistence::delete(&persistence, agent.id).await })
    };
    wait_until_a_backend_is_blocked(persistence.pool()).await;
    writer.commit().await.unwrap();
    delete
        .await
        .unwrap()
        .expect("the delete waits, then retires");

    let (kind, agent_id): (String, Option<Uuid>) =
        sqlx::query_as("SELECT kind, agent_id FROM principals WHERE id = $1")
            .bind(written.as_uuid())
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    assert_eq!((kind.as_str(), agent_id), ("external", None));

    // The other order: the agent is gone, so a late writer cannot attach a principal to it.
    let mut late = persistence.pool().begin().await.unwrap();
    let refused = crate::adapters::persistence::participant::create_agent_principal_on(
        &mut late,
        spoken.company_id,
        agent.id,
        "Racer",
    )
    .await;
    assert!(
        refused.is_err(),
        "a principal for a deleted agent is refused"
    );
    late.rollback().await.unwrap();
    let dangling: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM principals WHERE agent_id = $1")
        .bind(agent.id)
        .fetch_one(persistence.pool())
        .await
        .unwrap();
    assert_eq!(dangling, 0);
}
