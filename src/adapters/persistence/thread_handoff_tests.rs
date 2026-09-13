use super::*;
use crate::{
    adapters::persistence::test_support::{
        ThreadHandoffFixtureRequest, test_pool, thread_handoff_fixture,
    },
    application::attention::AttentionPersistence,
    entities::attention::{
        AttentionQuery, AttentionSourceCommand, AttentionSourceKind, AttentionView,
    },
    use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};

/// One company with one channel, owned by a freshly created user.
///
/// Every assertion below is scoped to the ids this returns: the test database is shared and runs
/// these in parallel, so nothing may look at a table as a whole.
async fn fixture(persistence: &PostgresPersistence) -> (Uuid, Uuid) {
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("handoff-{suffix}@example.com");
    persistence
        .create_user(&format!("handoff-{suffix}"), &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        persistence,
        owner.id,
        CompanyWrite {
            name: "Reply Handling Test".into(),
            slug: format!("handoff-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        persistence,
        company.id,
        ChannelWrite {
            name: "Support".into(),
            slug: format!("handoff-support-{suffix}"),
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    (company.id, channel.id)
}

async fn stored_override(persistence: &PostgresPersistence, channel_id: Uuid) -> Option<String> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT external_reply_handling_override FROM channels WHERE id = $1",
    )
    .bind(channel_id)
    .fetch_one(persistence.pool())
    .await
    .unwrap()
}

async fn effective_in_transaction(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    channel_id: Uuid,
) -> AppResult<ExternalReplyHandling> {
    let mut tx = persistence.pool().begin().await.unwrap();
    let effective = effective_reply_handling_on(&mut tx, company_id, channel_id).await;
    tx.commit().await.unwrap();
    effective
}

#[tokio::test]
async fn a_new_company_and_channel_resolve_to_the_migration_default() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id) = fixture(&persistence).await;

    let policy = persistence
        .reply_handling_policy(company_id, channel_id)
        .await
        .unwrap()
        .expect("the channel exists in this company");

    assert_eq!(policy.company_default, ExternalReplyHandling::Automatic);
    assert_eq!(policy.channel_override, None);
    assert_eq!(policy.effective(), ExternalReplyHandling::Automatic);
    assert!(policy.is_inherited());
    assert_eq!(stored_override(&persistence, channel_id).await, None);
}

#[tokio::test]
async fn an_inheriting_channel_follows_the_company_default_without_changing_its_own_row() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id) = fixture(&persistence).await;

    persistence
        .set_company_reply_handling(company_id, ExternalReplyHandling::ManualHandoff)
        .await
        .unwrap();

    let policy = persistence
        .reply_handling_policy(company_id, channel_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(policy.company_default, ExternalReplyHandling::ManualHandoff);
    assert_eq!(policy.effective(), ExternalReplyHandling::ManualHandoff);
    assert!(policy.is_inherited());
    assert_eq!(
        stored_override(&persistence, channel_id).await,
        None,
        "inheritance is resolved at read time, never copied into the channel"
    );
}

#[tokio::test]
async fn a_channel_override_wins_in_both_directions() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id) = fixture(&persistence).await;

    persistence
        .set_company_reply_handling(company_id, ExternalReplyHandling::ManualHandoff)
        .await
        .unwrap();
    persistence
        .set_channel_reply_handling_override(
            company_id,
            channel_id,
            Some(ExternalReplyHandling::Automatic),
        )
        .await
        .unwrap();
    let policy = persistence
        .reply_handling_policy(company_id, channel_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        policy.channel_override,
        Some(ExternalReplyHandling::Automatic)
    );
    assert_eq!(policy.effective(), ExternalReplyHandling::Automatic);
    assert!(!policy.is_inherited());

    persistence
        .set_company_reply_handling(company_id, ExternalReplyHandling::Automatic)
        .await
        .unwrap();
    persistence
        .set_channel_reply_handling_override(
            company_id,
            channel_id,
            Some(ExternalReplyHandling::ManualHandoff),
        )
        .await
        .unwrap();
    let policy = persistence
        .reply_handling_policy(company_id, channel_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        policy.channel_override,
        Some(ExternalReplyHandling::ManualHandoff)
    );
    assert_eq!(policy.effective(), ExternalReplyHandling::ManualHandoff);
    assert!(!policy.is_inherited());
}

/// The regression guard for the `COALESCE` decision in phase 1 §1.5: written as
/// `COALESCE($3, external_reply_handling_override)`, the clearing update below would silently
/// leave the override in place and this test would fail.
#[tokio::test]
async fn clearing_the_override_restores_live_inheritance() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id) = fixture(&persistence).await;

    persistence
        .set_channel_reply_handling_override(
            company_id,
            channel_id,
            Some(ExternalReplyHandling::ManualHandoff),
        )
        .await
        .unwrap();
    assert_eq!(
        stored_override(&persistence, channel_id).await.as_deref(),
        Some("manual_handoff")
    );

    persistence
        .set_channel_reply_handling_override(company_id, channel_id, None)
        .await
        .unwrap();

    assert_eq!(stored_override(&persistence, channel_id).await, None);
    let policy = persistence
        .reply_handling_policy(company_id, channel_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(policy.channel_override, None);
    assert!(policy.is_inherited());
    assert_eq!(policy.effective(), ExternalReplyHandling::Automatic);

    persistence
        .set_company_reply_handling(company_id, ExternalReplyHandling::ManualHandoff)
        .await
        .unwrap();
    assert_eq!(
        persistence
            .reply_handling_policy(company_id, channel_id)
            .await
            .unwrap()
            .unwrap()
            .effective(),
        ExternalReplyHandling::ManualHandoff,
        "a cleared override must follow the company again"
    );
}

#[tokio::test]
async fn the_transaction_scoped_read_agrees_with_the_policy_read_for_every_combination() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id) = fixture(&persistence).await;

    for company_default in [
        ExternalReplyHandling::Automatic,
        ExternalReplyHandling::ManualHandoff,
    ] {
        for channel_override in [
            None,
            Some(ExternalReplyHandling::Automatic),
            Some(ExternalReplyHandling::ManualHandoff),
        ] {
            persistence
                .set_company_reply_handling(company_id, company_default)
                .await
                .unwrap();
            persistence
                .set_channel_reply_handling_override(company_id, channel_id, channel_override)
                .await
                .unwrap();

            let policy = persistence
                .reply_handling_policy(company_id, channel_id)
                .await
                .unwrap()
                .unwrap();
            let effective = effective_in_transaction(&persistence, company_id, channel_id)
                .await
                .unwrap();
            assert_eq!(
                effective,
                policy.effective(),
                "default {company_default:?} with override {channel_override:?}"
            );
        }
    }
}

#[tokio::test]
async fn an_unknown_or_foreign_channel_is_refused_by_both_reads() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, _) = fixture(&persistence).await;
    let (other_company_id, other_channel_id) = fixture(&persistence).await;

    for channel_id in [Uuid::new_v4(), other_channel_id] {
        assert!(
            persistence
                .reply_handling_policy(company_id, channel_id)
                .await
                .unwrap()
                .is_none(),
            "a channel of another company must read exactly like a missing one"
        );
        assert!(matches!(
            effective_in_transaction(&persistence, company_id, channel_id).await,
            Err(AppError::NotFound(_))
        ));
    }

    // The same channel is readable from the company that owns it, so the predicate under test is
    // the tenant scope rather than the id.
    assert!(
        persistence
            .reply_handling_policy(other_company_id, other_channel_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn the_check_constraints_refuse_values_the_enum_does_not_know() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id) = fixture(&persistence).await;

    assert!(
        sqlx::query(
            "UPDATE channels SET external_reply_handling_override = 'nonsense' WHERE id = $1"
        )
        .bind(channel_id)
        .execute(persistence.pool())
        .await
        .is_err()
    );
    assert!(
        sqlx::query("UPDATE companies SET external_reply_handling = NULL WHERE id = $1")
            .bind(company_id)
            .execute(persistence.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE companies SET external_reply_handling = 'review_all_external' WHERE id = $1"
        )
        .bind(company_id)
        .execute(persistence.pool())
        .await
        .is_err(),
        "the sibling policy's vocabulary is not this column's"
    );
}

#[tokio::test]
async fn writing_the_policy_of_something_that_does_not_exist_is_not_found() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, _) = fixture(&persistence).await;

    assert!(matches!(
        persistence
            .set_company_reply_handling(Uuid::new_v4(), ExternalReplyHandling::ManualHandoff)
            .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        persistence
            .set_channel_reply_handling_override(
                company_id,
                Uuid::new_v4(),
                Some(ExternalReplyHandling::ManualHandoff),
            )
            .await,
        Err(AppError::NotFound(_))
    ));
}

// ---------------------------------------------------------------------------------------------
// The `thread_handoffs` / `thread_handoff_events` schema itself: what the database refuses, what
// it cascades, and what it will not let anyone rewrite.
//
// These write rows directly rather than through a commit, because the properties under test are
// PostgreSQL's. The commit-level behaviour lives in `thread/inbound_tests.rs`.
// ---------------------------------------------------------------------------------------------

/// A company with a channel, a thread on it, and one stored message to hang a handoff off.
struct HandoffFixture {
    persistence: PostgresPersistence,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    message_id: Uuid,
    principal_id: Uuid,
}

async fn handoff_fixture(persistence: &PostgresPersistence) -> HandoffFixture {
    let (company_id, channel_id) = fixture(persistence).await;
    let thread_id = crate::use_cases::thread::ThreadPersistence::create_thread(
        persistence,
        channel_id,
        "Invoice 4471",
        &[],
    )
    .await
    .unwrap()
    .id;
    let (message_id, principal_id) = store_message(persistence, company_id).await;
    HandoffFixture {
        persistence: persistence.clone(),
        company_id,
        channel_id,
        thread_id,
        message_id,
        principal_id,
    }
}

/// One canonical message, written directly: the handoff's source-message foreign key needs a real
/// row and the ingest path is tested elsewhere.
async fn store_message(persistence: &PostgresPersistence, company_id: Uuid) -> (Uuid, Uuid) {
    let suffix = Uuid::new_v4().simple().to_string();
    let address = format!("ana-{suffix}@client.test");
    let resolved =
        crate::use_cases::participant::IdentityDirectory::resolve_or_create_external_identity(
            persistence,
            company_id,
            crate::use_cases::participant::IdentityObservation {
                identity: crate::use_cases::thread::qualified_email_identity(address).unwrap(),
                display_label: None,
                claim_metadata: crate::entities::participant::IdentityClaimMetadata::observation(),
                provenance: crate::entities::participant::IdentityProvenance::TransportIngress,
            },
        )
        .await
        .unwrap();
    let message_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO messages (
                id, company_id, author_principal_id, authored_identity_id, subject,
                clean_text_body, direction, role, correlation_id, content_hash, audience
           ) VALUES ($1, $2, $3, $4, 'Invoice 4471', 'Any news?', 'inbound', 'human', $5, $6,
                     'external_conversation')"#,
    )
    .bind(message_id)
    .bind(company_id)
    .bind(resolved.principal.id.as_uuid())
    .bind(resolved.identity.id.as_uuid())
    .bind(Uuid::new_v4())
    .bind(vec![0_u8; 32])
    .execute(persistence.pool())
    .await
    .unwrap();
    (message_id, resolved.principal.id.as_uuid())
}

impl HandoffFixture {
    /// Open a generation the way the inbound commit does, on a transaction of its own.
    async fn open(&self, handoff_id: Uuid, generation: Uuid) -> AppResult<Uuid> {
        let mut tx = self.persistence.pool().begin().await.unwrap();
        let opened = open_handoff_generation_on(
            &mut tx,
            OpenHandoffGeneration {
                company_id: self.company_id,
                channel_id: self.channel_id,
                thread_id: self.thread_id,
                handoff_id,
                generation,
                source_message_id: CanonicalMessageId::new(self.message_id),
            },
        )
        .await;
        match opened {
            Ok(id) => {
                tx.commit().await.unwrap();
                Ok(id)
            }
            Err(error) => {
                tx.rollback().await.unwrap();
                Err(error)
            }
        }
    }

    async fn insert_handoff_row(&self, columns: &str, values: &str) -> Result<(), sqlx::Error> {
        sqlx::query(&format!(
            "INSERT INTO thread_handoffs (id, company_id, channel_id, thread_id, generation, \
             source_message_id{columns}) VALUES ($1, $2, $3, $4, $5, $6{values})"
        ))
        .bind(Uuid::new_v4())
        .bind(self.company_id)
        .bind(self.channel_id)
        .bind(self.thread_id)
        .bind(Uuid::new_v4())
        .bind(self.message_id)
        .execute(self.persistence.pool())
        .await
        .map(|_| ())
    }

    async fn insert_event(
        &self,
        handoff_id: Uuid,
        command_id: Uuid,
        actor_kind: &str,
        actor: Option<Uuid>,
        versions: (i64, i64),
        failure_reason: Option<String>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO thread_handoff_events (
                    company_id, handoff_id, generation, command_id, command_fingerprint,
                    operation, actor_kind, actor_principal_id, to_state, from_version, to_version,
                    failure_reason
               ) VALUES ($1, $2, $3, $4, 'fingerprint', 'opened', $5, $6, 'needs_instruction',
                         $7, $8, $9)"#,
        )
        .bind(self.company_id)
        .bind(handoff_id)
        .bind(Uuid::new_v4())
        .bind(command_id)
        .bind(actor_kind)
        .bind(actor)
        .bind(versions.0)
        .bind(versions.1)
        .bind(failure_reason)
        .execute(self.persistence.pool())
        .await
        .map(|_| ())
    }

    async fn event_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM thread_handoff_events WHERE company_id = $1")
            .bind(self.company_id)
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }

    async fn handoff_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM thread_handoffs WHERE company_id = $1")
            .bind(self.company_id)
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }
}

/// Case 16. The audit trail is the idempotency store and the history of every version, so nothing
/// may rewrite it -- and the message must name *this* table, not `attention_source_events`.
#[tokio::test]
async fn thread_handoff_events_cannot_be_rewritten_or_deleted() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let handoff = handoff_fixture(&persistence).await;
    let handoff_id = handoff.open(Uuid::new_v4(), Uuid::new_v4()).await.unwrap();

    for statement in [
        "UPDATE thread_handoff_events SET operation = 'claimed' WHERE handoff_id = $1",
        "DELETE FROM thread_handoff_events WHERE handoff_id = $1",
    ] {
        let error = sqlx::query(statement)
            .bind(handoff_id)
            .execute(persistence.pool())
            .await
            .expect_err("the immutability trigger must refuse this");
        let message = error.to_string();
        assert!(
            message.contains("thread handoff events are immutable"),
            "{message}"
        );
        assert!(
            !message.contains("attention source events"),
            "the new function must name its own table: {message}"
        );
    }
    assert_eq!(handoff.event_count().await, 1);
}

/// Case 17. Every value the state machine cannot mean is refused by the database rather than only
/// by the enum, so a hand-written statement cannot open a handoff nothing can read.
#[tokio::test]
async fn the_handoff_check_constraints_and_unique_keys_refuse_impossible_rows() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let handoff = handoff_fixture(&persistence).await;

    for (label, columns, values) in [
        ("an unknown state", ", state", ", 'parked'"),
        ("version zero", ", version", ", 0"),
        (
            "a terminal state with no closed_at",
            ", state",
            ", 'resolved'",
        ),
        (
            "an open state carrying a closed_at",
            ", closed_at",
            ", CURRENT_TIMESTAMP",
        ),
        ("an unknown priority", ", business_priority", ", 'blocker'"),
    ] {
        assert!(
            handoff.insert_handoff_row(columns, values).await.is_err(),
            "{label} must be refused"
        );
    }
    assert_eq!(
        handoff.handoff_count().await,
        0,
        "nothing above was written"
    );

    // The row the rest of this test builds on.
    let handoff_id = handoff.open(Uuid::new_v4(), Uuid::new_v4()).await.unwrap();
    assert_eq!(handoff.handoff_count().await, 1);

    // At most one handoff per thread, and one thread per generation.
    assert!(
        handoff.insert_handoff_row("", "").await.is_err(),
        "a second handoff on one thread must be refused"
    );
    let shared_generation: Uuid =
        sqlx::query_scalar("SELECT generation FROM thread_handoffs WHERE company_id = $1")
            .bind(handoff.company_id)
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    let second_thread = crate::use_cases::thread::ThreadPersistence::create_thread(
        &persistence,
        handoff.channel_id,
        "Invoice 4472",
        &[],
    )
    .await
    .unwrap()
    .id;
    assert!(
        sqlx::query(
            r#"INSERT INTO thread_handoffs (
                    id, company_id, channel_id, thread_id, generation, source_message_id
               ) VALUES ($1, $2, $3, $4, $5, $6)"#,
        )
        .bind(Uuid::new_v4())
        .bind(handoff.company_id)
        .bind(handoff.channel_id)
        .bind(second_thread)
        .bind(shared_generation)
        .bind(handoff.message_id)
        .execute(persistence.pool())
        .await
        .is_err(),
        "two handoffs may not share a generation"
    );

    // The event table's own refusals.
    /// One event row the table must refuse, and why a reviewer should care.
    struct RefusedEvent {
        label: &'static str,
        actor_kind: &'static str,
        actor: Option<Uuid>,
        versions: (i64, i64),
        failure_reason: Option<String>,
    }

    let refused = [
        RefusedEvent {
            label: "a human with no principal",
            actor_kind: "human",
            actor: None,
            versions: (1, 2),
            failure_reason: None,
        },
        RefusedEvent {
            label: "the system acting as a person",
            actor_kind: "system",
            actor: Some(handoff.principal_id),
            versions: (1, 2),
            failure_reason: None,
        },
        RefusedEvent {
            label: "a version that skips one",
            actor_kind: "system",
            actor: None,
            versions: (1, 3),
            failure_reason: None,
        },
        RefusedEvent {
            label: "an oversized failure reason",
            actor_kind: "system",
            actor: None,
            versions: (1, 2),
            failure_reason: Some("x".repeat(2049)),
        },
    ];
    for case in refused {
        assert!(
            handoff
                .insert_event(
                    handoff_id,
                    Uuid::new_v4(),
                    case.actor_kind,
                    case.actor,
                    case.versions,
                    case.failure_reason,
                )
                .await
                .is_err(),
            "{} must be refused",
            case.label
        );
    }

    // A 2048-byte reason is accepted, so the cap above is the boundary and not a blanket refusal.
    handoff
        .insert_event(
            handoff_id,
            Uuid::new_v4(),
            "system",
            None,
            (1, 2),
            Some("x".repeat(2048)),
        )
        .await
        .unwrap();

    // And the idempotency key: one command id per handoff, ever.
    let replayed = Uuid::new_v4();
    handoff
        .insert_event(handoff_id, replayed, "system", None, (2, 3), None)
        .await
        .unwrap();
    assert!(
        handoff
            .insert_event(handoff_id, replayed, "system", None, (3, 4), None)
            .await
            .is_err(),
        "a replayed command id must be refused by the unique key"
    );
}

/// Case 18. The handoff is tenant-scoped through every key it has; the audit trail outlives the
/// row it describes and dies only with the tenant.
#[tokio::test]
async fn a_handoff_is_scoped_to_its_tenant_and_its_events_outlive_it() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let handoff = handoff_fixture(&persistence).await;
    let foreign = handoff_fixture(&persistence).await;

    // A thread belonging to another company cannot be handed off under this one.
    assert!(
        sqlx::query(
            r#"INSERT INTO thread_handoffs (
                    id, company_id, channel_id, thread_id, generation, source_message_id
               ) VALUES ($1, $2, $3, $4, $5, $6)"#,
        )
        .bind(Uuid::new_v4())
        .bind(handoff.company_id)
        .bind(handoff.channel_id)
        .bind(foreign.thread_id)
        .bind(Uuid::new_v4())
        .bind(handoff.message_id)
        .execute(persistence.pool())
        .await
        .is_err(),
        "thread_handoffs_thread_fk is what makes this unrepresentable"
    );

    handoff.open(Uuid::new_v4(), Uuid::new_v4()).await.unwrap();
    assert_eq!(handoff.handoff_count().await, 1);
    assert_eq!(handoff.event_count().await, 1);

    // The thread goes, the handoff goes with it -- and the audit trail stays, because it has no
    // foreign key to the row it describes.
    sqlx::query("DELETE FROM threads WHERE id = $1")
        .bind(handoff.thread_id)
        .execute(persistence.pool())
        .await
        .unwrap();
    assert_eq!(handoff.handoff_count().await, 0);
    assert_eq!(
        handoff.event_count().await,
        1,
        "the trail must outlive the row it describes"
    );

    // The tenant is what it does not outlive.
    CompanyPersistence::delete(&persistence, handoff.company_id)
        .await
        .unwrap();
    assert_eq!(handoff.event_count().await, 0);
    assert_eq!(
        foreign.event_count().await,
        0,
        "the other company's fixture is untouched and still empty"
    );
}

// ---------------------------------------------------------------------------------------------
// The four responsibility commands: claim, release, reassign and set-attributes.
//
// Every one of them is tenant-scoped, fenced on the generation *and* the version, idempotent
// under a client `command_id`, and audited by exactly one `thread_handoff_events` row. Each test
// scopes its counts to the company it created, because the test database is shared.
// ---------------------------------------------------------------------------------------------

/// One held reply, and everything a command has to name to reach it.
#[derive(Clone, Copy)]
struct Hold {
    channel_id: Uuid,
    thread_id: Uuid,
    handoff_id: Uuid,
    generation: Uuid,
}

/// One audited transition, as the tests below read it back.
#[derive(sqlx::FromRow, Debug)]
struct HandoffEvent {
    operation: String,
    actor_kind: String,
    actor_principal_id: Option<Uuid>,
    from_state: Option<String>,
    to_state: String,
    from_version: i64,
    to_version: i64,
    previous_priority: Option<String>,
    new_priority: Option<String>,
    previous_due_at: Option<DateTime<Utc>>,
    new_due_at: Option<DateTime<Utc>>,
    previous_responsible_principal_id: Option<Uuid>,
    new_responsible_principal_id: Option<Uuid>,
}

/// A company, a channel its whole team can see, and the three people the authorization rule is
/// about: one manager and two teammates who manage nothing.
///
/// A struct with methods rather than free functions over `(company_id, channel_id, ...)`: both are
/// `Uuid`, so a transposed pair would compile and then read a channel as a company.
struct CommandFixture {
    persistence: PostgresPersistence,
    company_id: Uuid,
    channel_id: Uuid,
    manager: PrincipalId,
    bo: PrincipalId,
    cleo: PrincipalId,
}

impl CommandFixture {
    async fn new(persistence: &PostgresPersistence) -> Self {
        let (company_id, channel_id) = fixture(persistence).await;
        let manager: Uuid = sqlx::query_scalar(
            r#"SELECT principal.id FROM principals AS principal
               JOIN companies AS company
                 ON company.id = principal.company_id AND company.user_id = principal.user_id
               WHERE principal.company_id = $1"#,
        )
        .bind(company_id)
        .fetch_one(persistence.pool())
        .await
        .unwrap();
        Self {
            persistence: persistence.clone(),
            company_id,
            channel_id,
            manager: PrincipalId::new(manager),
            bo: add_person(persistence, company_id, "Bo Okonkwo").await,
            cleo: add_person(persistence, company_id, "Cleo Marsh").await,
        }
    }

    async fn hold(&self, state: ThreadHandoffState, responsible: Option<PrincipalId>) -> Hold {
        self.hold_on(self.channel_id, state, responsible).await
    }

    /// A held reply on a thread of its own: `thread_handoffs_thread_key` allows exactly one
    /// handoff per thread, so a test wanting two holds needs two threads.
    async fn hold_on(
        &self,
        channel_id: Uuid,
        state: ThreadHandoffState,
        responsible: Option<PrincipalId>,
    ) -> Hold {
        let thread_id = crate::use_cases::thread::ThreadPersistence::create_thread(
            &self.persistence,
            channel_id,
            "Invoice 4471",
            &[],
        )
        .await
        .unwrap()
        .id;
        let held = thread_handoff_fixture(
            &self.persistence,
            ThreadHandoffFixtureRequest {
                state,
                responsible_principal_id: responsible,
                ..ThreadHandoffFixtureRequest::new(self.company_id, channel_id, thread_id)
            },
        )
        .await;
        Hold {
            channel_id,
            thread_id,
            handoff_id: held.handoff_id,
            generation: held.generation,
        }
    }

    /// A channel only its named participants and the company owner may see.
    async fn restricted_channel(&self) -> Uuid {
        ChannelPersistence::create(
            &self.persistence,
            self.company_id,
            ChannelWrite {
                name: "Restricted".into(),
                slug: format!("handoff-restricted-{}", Uuid::new_v4().simple()),
                participant_emails: Some(vec!["outsider@example.com".to_string()]),
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    /// A claim by `actor` against version 1 of `hold`'s current generation. Every command below is
    /// this value with one field changed, which is what keeps the fences legible.
    fn command(&self, hold: &Hold, actor: PrincipalId) -> ThreadHandoffCommand {
        ThreadHandoffCommand {
            company_id: self.company_id,
            handoff_id: hold.handoff_id,
            command_id: Uuid::new_v4(),
            expected_version: 1,
            expected_generation: hold.generation,
            operation: ThreadHandoffOperation::Claim,
            priority: BusinessPriority::Normal,
            due_at: None,
            actor_principal_id: actor,
            visible_channel_ids: vec![hold.channel_id],
        }
    }

    async fn change(&self, command: ThreadHandoffCommand) -> AppResult<u64> {
        self.persistence.change_thread_handoff(command).await
    }

    async fn row(&self, hold: &Hold) -> ThreadHandoff {
        self.persistence
            .get_thread_handoff(self.company_id, hold.handoff_id, &[hold.channel_id])
            .await
            .unwrap()
            .expect("the handoff is readable")
    }

    async fn events(&self, hold: &Hold) -> Vec<HandoffEvent> {
        sqlx::query_as(
            r#"SELECT operation, actor_kind, actor_principal_id, from_state, to_state,
                      from_version, to_version, previous_priority, new_priority, previous_due_at,
                      new_due_at, previous_responsible_principal_id, new_responsible_principal_id
               FROM thread_handoff_events
               WHERE company_id = $1 AND handoff_id = $2
               ORDER BY to_version, occurred_at"#,
        )
        .bind(self.company_id)
        .bind(hold.handoff_id)
        .fetch_all(self.persistence.pool())
        .await
        .unwrap()
    }

    /// A second outside reply on the same thread, through the path the inbound commit uses.
    async fn regenerate(&self, hold: &Hold) -> Uuid {
        let (message_id, _) = store_message(&self.persistence, self.company_id).await;
        let generation = Uuid::new_v4();
        let mut tx = self.persistence.pool().begin().await.unwrap();
        open_handoff_generation_on(
            &mut tx,
            OpenHandoffGeneration {
                company_id: self.company_id,
                channel_id: hold.channel_id,
                thread_id: hold.thread_id,
                handoff_id: hold.handoff_id,
                generation,
                source_message_id: CanonicalMessageId::new(message_id),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        generation
    }

    /// Notifications are out of scope for this whole plan, and the boundary is enforced by the
    /// schema: the events live outside `attention_source_events`, so no notification trigger
    /// fires for any of it.
    async fn notification_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM notifications WHERE company_id = $1")
            .bind(self.company_id)
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }

    /// Whether any attention view lists this hold, for the caller's own channel.
    async fn is_queued(&self, hold: &Hold, principal: PrincipalId) -> bool {
        let visible = [hold.channel_id];
        for view in [
            AttentionView::MyWork,
            AttentionView::Unassigned,
            AttentionView::TeamWork,
        ] {
            let listed = self
                .persistence
                .list_attention(AttentionQuery {
                    company_id: self.company_id,
                    principal_id: principal,
                    visible_channel_ids: &visible,
                    view,
                    all_owned: false,
                    cursor: None,
                    limit: 50,
                })
                .await
                .unwrap();
            if listed.items.iter().any(|item| {
                item.source_kind == AttentionSourceKind::ThreadHandoff
                    && item.source_id == hold.handoff_id
            }) {
                return true;
            }
        }
        false
    }
}

/// A teammate with a principal, a company membership, and no authority over anybody else's work.
async fn add_person(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    display_label: &str,
) -> PrincipalId {
    let suffix = Uuid::new_v4().simple().to_string();
    let user = persistence
        .create_user(
            &format!("handoff-person-{suffix}"),
            &format!("handoff-person-{suffix}@example.com"),
            "hash",
        )
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role) VALUES ($1, $2, $3, 'member')",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(user.id)
    .execute(persistence.pool())
    .await
    .unwrap();
    let principal = PrincipalId::random();
    sqlx::query(
        r#"INSERT INTO principals (id, company_id, kind, user_id, display_label)
           VALUES ($1, $2, 'person', $3, $4)"#,
    )
    .bind(principal.as_uuid())
    .bind(company_id)
    .bind(user.id)
    .bind(display_label)
    .execute(persistence.pool())
    .await
    .unwrap();
    principal
}

/// A due time written as a literal rather than `Utc::now()`: PostgreSQL keeps microseconds and
/// `now()` carries nanoseconds, so a round-trip comparison would be flaky by a rounding error.
fn due_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn is_not_found(error: &AppError) -> bool {
    matches!(error, AppError::NotFound(message) if message == "Thread handoff not found.")
}

/// Case 13.
#[tokio::test]
async fn a_teammate_claims_an_unclaimed_hold_and_the_trail_records_who() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx.hold(ThreadHandoffState::NeedsInstruction, None).await;

    // Bo manages nothing and the row is nobody's, which is the one case a non-manager may act on.
    assert_eq!(fx.change(fx.command(&hold, fx.bo)).await.unwrap(), 2);

    let row = fx.row(&hold).await;
    assert_eq!(row.responsible_principal_id, Some(fx.bo));
    assert_eq!(row.version, 2);
    assert_eq!(row.state, ThreadHandoffState::NeedsInstruction);
    assert_eq!(
        row.generation, hold.generation,
        "a claim is not a regeneration"
    );

    let events = fx.events(&hold).await;
    assert_eq!(events.len(), 1, "exactly one event per accepted command");
    let event = &events[0];
    assert_eq!(event.operation, "claimed");
    assert_eq!(event.actor_kind, "human");
    assert_eq!(event.actor_principal_id, Some(fx.bo.as_uuid()));
    assert_eq!(event.from_state.as_deref(), Some("needs_instruction"));
    assert_eq!(event.to_state, "needs_instruction");
    assert_eq!((event.from_version, event.to_version), (1, 2));
    assert_eq!(event.previous_responsible_principal_id, None);
    assert_eq!(event.new_responsible_principal_id, Some(fx.bo.as_uuid()));

    assert_eq!(
        fx.notification_count().await,
        0,
        "notifications are out of scope for this plan, and the schema is what enforces it"
    );
}

/// Case 14.
#[tokio::test]
async fn two_teammates_claiming_at_once_produce_one_claim_and_one_conflict() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx.hold(ThreadHandoffState::NeedsInstruction, None).await;

    let (first, second) = tokio::join!(
        fx.change(fx.command(&hold, fx.bo)),
        fx.change(fx.command(&hold, fx.cleo))
    );
    assert_eq!(
        usize::from(first.is_ok()) + usize::from(second.is_ok()),
        1,
        "exactly one of two concurrent claims may win"
    );
    let refused = first
        .as_ref()
        .err()
        .or(second.as_ref().err())
        .expect("the loser is refused");
    assert!(
        matches!(refused, AppError::Conflict(message) if message.contains("to 2")),
        "the loser is told the version it must refresh to: {refused:?}"
    );

    let events = fx.events(&hold).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].operation, "claimed");
    assert_eq!(fx.row(&hold).await.version, 2);
}

/// Case 15.
#[tokio::test]
async fn a_manager_releases_one_hold_and_reassigns_another() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let released = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.bo))
        .await;
    let reassigned = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.bo))
        .await;

    assert_eq!(
        fx.change(ThreadHandoffCommand {
            operation: ThreadHandoffOperation::Release,
            ..fx.command(&released, fx.manager)
        })
        .await
        .unwrap(),
        2
    );
    assert_eq!(fx.row(&released).await.responsible_principal_id, None);
    let event = fx.events(&released).await.pop().expect("one event");
    assert_eq!(event.operation, "released");
    assert_eq!(
        event.previous_responsible_principal_id,
        Some(fx.bo.as_uuid())
    );
    assert_eq!(event.new_responsible_principal_id, None);

    assert_eq!(
        fx.change(ThreadHandoffCommand {
            operation: ThreadHandoffOperation::Reassign { to: fx.cleo },
            ..fx.command(&reassigned, fx.manager)
        })
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        fx.row(&reassigned).await.responsible_principal_id,
        Some(fx.cleo)
    );
    let event = fx.events(&reassigned).await.pop().expect("one event");
    assert_eq!(event.operation, "reassigned");
    assert_eq!(
        event.previous_responsible_principal_id,
        Some(fx.bo.as_uuid())
    );
    assert_eq!(event.new_responsible_principal_id, Some(fx.cleo.as_uuid()));
}

/// Case 16.
#[tokio::test]
async fn setting_priority_and_due_moves_the_row_and_the_queue_but_not_the_responsibility() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.bo))
        .await;
    let due = due_time();

    assert_eq!(
        fx.change(ThreadHandoffCommand {
            operation: ThreadHandoffOperation::SetAttributes,
            priority: BusinessPriority::Urgent,
            due_at: Some(due),
            ..fx.command(&hold, fx.bo)
        })
        .await
        .unwrap(),
        2
    );

    let row = fx.row(&hold).await;
    assert_eq!(row.priority, BusinessPriority::Urgent);
    assert_eq!(row.due_at, Some(due));
    assert_eq!(
        row.responsible_principal_id,
        Some(fx.bo),
        "attributes are a different axis from responsibility"
    );

    let item = fx
        .persistence
        .list_attention(AttentionQuery {
            company_id: fx.company_id,
            principal_id: fx.bo,
            visible_channel_ids: &[hold.channel_id],
            view: AttentionView::MyWork,
            all_owned: false,
            cursor: None,
            limit: 50,
        })
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|item| item.source_id == hold.handoff_id)
        .expect("the queue carries the same pair");
    assert_eq!(item.priority, BusinessPriority::Urgent);
    assert_eq!(item.due_at, Some(due));

    let event = fx.events(&hold).await.pop().expect("one event");
    assert_eq!(event.operation, "attributes_changed");
    assert_eq!(event.previous_priority.as_deref(), Some("normal"));
    assert_eq!(event.new_priority.as_deref(), Some("urgent"));
    assert_eq!(event.previous_due_at, None);
    assert_eq!(event.new_due_at, Some(due));
    assert_eq!(
        event.previous_responsible_principal_id,
        event.new_responsible_principal_id
    );
}

/// Case 17. A teammate must not learn that a handoff they cannot act on exists, so acting on
/// somebody else's claim and acting on a channel they cannot see answer identically.
#[tokio::test]
async fn a_non_manager_is_refused_as_indistinguishably_as_a_channel_they_cannot_see() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let theirs = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.cleo))
        .await;
    let unseen = fx.hold(ThreadHandoffState::NeedsInstruction, None).await;

    for operation in [
        ThreadHandoffOperation::Release,
        ThreadHandoffOperation::Reassign { to: fx.manager },
        ThreadHandoffOperation::SetAttributes,
    ] {
        let refused = fx
            .change(ThreadHandoffCommand {
                operation,
                ..fx.command(&theirs, fx.bo)
            })
            .await
            .expect_err("a teammate may not act on somebody else's claim");
        assert!(is_not_found(&refused), "{operation:?}: {refused:?}");

        let invisible = fx
            .change(ThreadHandoffCommand {
                operation,
                visible_channel_ids: Vec::new(),
                ..fx.command(&unseen, fx.bo)
            })
            .await
            .expect_err("a channel the caller cannot view is a handoff they cannot see");
        assert!(is_not_found(&invisible), "{operation:?}: {invisible:?}");
        assert_eq!(
            format!("{refused:?}"),
            format!("{invisible:?}"),
            "the two refusals must be indistinguishable"
        );
    }

    assert_eq!(fx.row(&theirs).await.version, 1);
    assert_eq!(fx.row(&unseen).await.version, 1);
    assert!(fx.events(&theirs).await.is_empty());
    assert!(fx.events(&unseen).await.is_empty());
}

/// Case 18.
#[tokio::test]
async fn a_handoff_cannot_be_reassigned_to_somebody_who_could_not_open_the_thread() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let restricted = fx.restricted_channel().await;
    let hold = fx
        .hold_on(restricted, ThreadHandoffState::NeedsInstruction, None)
        .await;

    let refused = fx
        .change(ThreadHandoffCommand {
            operation: ThreadHandoffOperation::Reassign { to: fx.bo },
            ..fx.command(&hold, fx.manager)
        })
        .await
        .expect_err("Bo has no view grant on a restricted channel");
    assert!(matches!(refused, AppError::NotFound(_)), "{refused:?}");
    assert_eq!(fx.row(&hold).await.version, 1);
    assert_eq!(fx.row(&hold).await.responsible_principal_id, None);
    assert!(
        fx.events(&hold).await.is_empty(),
        "a refusal writes nothing"
    );

    // The same reassignment succeeds once the target may view the channel, so what refused it
    // above is the missing grant rather than the channel or the operation.
    sqlx::query(
        r#"INSERT INTO channel_principal_grants
               (company_id, channel_id, principal_id, capability, provenance)
           VALUES ($1, $2, $3, 'view', 'manager')"#,
    )
    .bind(fx.company_id)
    .bind(restricted)
    .bind(fx.bo.as_uuid())
    .execute(persistence.pool())
    .await
    .unwrap();
    assert_eq!(
        fx.change(ThreadHandoffCommand {
            operation: ThreadHandoffOperation::Reassign { to: fx.bo },
            ..fx.command(&hold, fx.manager)
        })
        .await
        .unwrap(),
        2
    );
    assert_eq!(fx.row(&hold).await.responsible_principal_id, Some(fx.bo));
}

/// Case 19. An instruction written for the previous customer message is refused rather than
/// applied to whatever arrived since -- and the hold stays claimable once the command is refreshed.
#[tokio::test]
async fn a_command_naming_a_replaced_generation_is_refused_and_then_succeeds_when_refreshed() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx.hold(ThreadHandoffState::NeedsInstruction, None).await;
    let current = fx.regenerate(&hold).await;
    let after_regeneration = fx.events(&hold).await;
    assert_eq!(after_regeneration.len(), 1);
    assert_eq!(after_regeneration[0].operation, "regenerated");

    let stale = fx
        .change(fx.command(&hold, fx.bo))
        .await
        .expect_err("the generation this claim was written for is gone");
    assert!(
        matches!(&stale, AppError::Conflict(message) if message.contains(&current.to_string())),
        "the conflict names the generation to refresh to: {stale:?}"
    );
    let row = fx.row(&hold).await;
    assert_eq!(row.version, 2, "the refused command wrote nothing");
    assert_eq!(row.responsible_principal_id, None);
    assert_eq!(fx.events(&hold).await.len(), 1);

    assert_eq!(
        fx.change(ThreadHandoffCommand {
            expected_version: 2,
            expected_generation: current,
            ..fx.command(&hold, fx.bo)
        })
        .await
        .unwrap(),
        3,
        "the hold is still claimable; the command just had to be refreshed"
    );
    assert_eq!(fx.row(&hold).await.responsible_principal_id, Some(fx.bo));
}

/// Case 20.
#[tokio::test]
async fn a_stale_version_on_the_current_generation_names_the_version_to_refresh_to() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx.hold(ThreadHandoffState::NeedsInstruction, None).await;
    assert_eq!(fx.change(fx.command(&hold, fx.bo)).await.unwrap(), 2);

    let stale = fx
        .change(ThreadHandoffCommand {
            operation: ThreadHandoffOperation::SetAttributes,
            priority: BusinessPriority::High,
            ..fx.command(&hold, fx.bo)
        })
        .await
        .expect_err("version 1 is gone");
    assert!(
        matches!(&stale, AppError::Conflict(message)
            if message.contains("from version 1 to 2")),
        "{stale:?}"
    );
    let row = fx.row(&hold).await;
    assert_eq!(row.version, 2);
    assert_eq!(row.priority, BusinessPriority::Normal);
    assert_eq!(fx.events(&hold).await.len(), 1);
}

/// Case 21. A replay returns the recorded outcome; the same id with different *semantics* is a
/// conflict; and the authorization context is not semantics.
#[tokio::test]
async fn a_replayed_command_id_returns_its_recorded_outcome_unless_the_command_changed() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx.hold(ThreadHandoffState::NeedsInstruction, None).await;
    let claim = fx.command(&hold, fx.bo);

    assert_eq!(fx.change(claim.clone()).await.unwrap(), 2);
    assert_eq!(
        fx.change(claim.clone()).await.unwrap(),
        2,
        "a replay returns the version the first call wrote"
    );
    assert_eq!(
        fx.events(&hold).await.len(),
        1,
        "and writes no second event"
    );

    let repriced = fx
        .change(ThreadHandoffCommand {
            priority: BusinessPriority::Urgent,
            ..claim.clone()
        })
        .await
        .expect_err("the same id may not mean two different things");
    assert!(
        matches!(&repriced, AppError::Conflict(message)
            if message.contains("different parameters")),
        "{repriced:?}"
    );

    assert_eq!(
        fx.change(ThreadHandoffCommand {
            visible_channel_ids: vec![hold.channel_id, Uuid::new_v4()],
            ..claim
        })
        .await
        .unwrap(),
        2,
        "who was allowed to ask is not part of what was asked"
    );
    assert_eq!(fx.events(&hold).await.len(), 1);
}

/// Case 22. The two terminal states are gone for good, and refused exactly like a handoff in
/// another company -- while a route that has to explain itself can still read the row.
#[tokio::test]
async fn a_terminal_handoff_refuses_every_command_but_is_still_readable() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;

    for state in [ThreadHandoffState::Resolved, ThreadHandoffState::Dismissed] {
        let hold = fx.hold(state, None).await;
        for operation in [
            ThreadHandoffOperation::Claim,
            ThreadHandoffOperation::Release,
            ThreadHandoffOperation::Reassign { to: fx.cleo },
            ThreadHandoffOperation::SetAttributes,
        ] {
            let refused = fx
                .change(ThreadHandoffCommand {
                    operation,
                    ..fx.command(&hold, fx.manager)
                })
                .await
                .expect_err("a closed handoff accepts nothing");
            assert!(is_not_found(&refused), "{state:?} / {operation:?}");
        }
        let row = fx.row(&hold).await;
        assert_eq!(row.state, state, "it is still readable, and still closed");
        assert_eq!(row.version, 1);
        assert!(fx.events(&hold).await.is_empty());
    }
}

/// Case 23. Out of the queue, still on the thread: a manager must be able to hand over work whose
/// drafting run is in flight.
#[tokio::test]
async fn a_drafting_handoff_accepts_responsibility_commands_while_absent_from_every_view() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx.hold(ThreadHandoffState::Drafting, None).await;

    assert_eq!(
        fx.change(ThreadHandoffCommand {
            operation: ThreadHandoffOperation::Reassign { to: fx.bo },
            ..fx.command(&hold, fx.manager)
        })
        .await
        .unwrap(),
        2
    );
    let row = fx.row(&hold).await;
    assert_eq!(row.responsible_principal_id, Some(fx.bo));
    assert_eq!(row.state, ThreadHandoffState::Drafting);
    assert_eq!(
        fx.events(&hold).await.pop().unwrap().operation,
        "reassigned"
    );

    for principal in [fx.manager, fx.bo, fx.cleo] {
        assert!(
            !fx.is_queued(&hold, principal).await,
            "drafting is visible progress, not work anybody must pick up"
        );
    }
}

/// Case 24.
#[tokio::test]
async fn the_shared_attention_command_refuses_a_thread_handoff_without_touching_it() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx.hold(ThreadHandoffState::NeedsInstruction, None).await;

    let refused = persistence
        .change_source_attributes(AttentionSourceCommand {
            company_id: fx.company_id,
            source_kind: AttentionSourceKind::ThreadHandoff,
            source_id: hold.handoff_id,
            command_id: Uuid::new_v4(),
            expected_version: 1,
            actor_principal_id: fx.manager,
            visible_channel_ids: vec![hold.channel_id],
            priority: BusinessPriority::Urgent,
            due_at: Some(due_time()),
            responsible_principal_id: Some(fx.bo),
        })
        .await
        .expect_err("a thread handoff moves through its own command");
    assert!(
        matches!(&refused, AppError::BadRequest(message)
            if message == "Thread handoff responsibility must be changed through its own command."),
        "{refused:?}"
    );

    let row = fx.row(&hold).await;
    assert_eq!(row.version, 1);
    assert_eq!(row.priority, BusinessPriority::Normal);
    assert_eq!(row.responsible_principal_id, None);
    assert!(fx.events(&hold).await.is_empty());
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM attention_source_events WHERE company_id = $1 AND source_id = $2",
    )
    .bind(fx.company_id)
    .bind(hold.handoff_id)
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    assert_eq!(
        audited, 0,
        "`attention_source_events_source_kind_check` never learns this kind"
    );
}

/// Case 25. The `ON DELETE SET NULL` path: a command written against the claim that no longer
/// exists must be refused rather than applied to whoever the row belongs to now.
#[tokio::test]
async fn deleting_the_responsible_principal_releases_the_hold_and_invalidates_its_version() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.bo))
        .await;

    sqlx::query("DELETE FROM principals WHERE company_id = $1 AND id = $2")
        .bind(fx.company_id)
        .bind(fx.bo.as_uuid())
        .execute(persistence.pool())
        .await
        .unwrap();

    let row = fx.row(&hold).await;
    assert_eq!(row.responsible_principal_id, None);
    assert_eq!(row.version, 2, "the cleanup trigger bumps the version");

    let stale = fx
        .change(ThreadHandoffCommand {
            operation: ThreadHandoffOperation::Release,
            ..fx.command(&hold, fx.manager)
        })
        .await
        .expect_err("version 1 described a claim that no longer exists");
    assert!(matches!(&stale, AppError::Conflict(_)), "{stale:?}");
    assert!(fx.events(&hold).await.is_empty());
}

// -- Phase 4: Generate draft, and dismiss ------------------------------------------------------

/// One `thread_handoff_runs` row, as the assertions below read it.
#[derive(sqlx::FromRow, Debug)]
struct RunRow {
    task_id: Uuid,
    handoff_id: Uuid,
    generation: Uuid,
    requested_by_principal_id: Uuid,
    command_id: Uuid,
    draft_id: Option<Uuid>,
    draft_version: Option<i32>,
    state: String,
}

impl CommandFixture {
    /// One active internal note on this hold's thread, which is what both run commands select.
    async fn note(&self, hold: &Hold, author: PrincipalId) -> Uuid {
        let view = crate::use_cases::thread::ThreadPersistence::create_internal_note(
            &self.persistence,
            &crate::entities::internal_note::AddInternalNote {
                company_id: self.company_id,
                channel_id: hold.channel_id,
                thread_id: hold.thread_id,
                text: "Quote her the 30-day terms; do not offer the discount.".into(),
                command_id: Uuid::new_v4(),
                supersedes_note_id: None,
                provenance: crate::entities::internal_note::InternalNoteProvenance::HumanUi,
            },
            author,
        )
        .await
        .unwrap();
        // `ThreadMessageView::internal_note` carries the note's own lifecycle row, which is what
        // the two run commands select on -- `view.id` is the message's position in the thread.
        view.internal_note
            .expect("a note message carries its note view")
            .id
    }

    /// **Generate draft** on a thread with no active task, at the hold's current generation.
    async fn generate_draft(
        &self,
        hold: &Hold,
        actor: PrincipalId,
        note_id: Uuid,
    ) -> AppResult<crate::entities::task::BackgroundTask> {
        self.generate_draft_with(
            hold,
            actor,
            note_id,
            Uuid::new_v4(),
            HandoffRunRequest {
                handoff_id: hold.handoff_id,
                generation: hold.generation,
                expected_version: 1,
            },
        )
        .await
    }

    async fn generate_draft_with(
        &self,
        hold: &Hold,
        actor: PrincipalId,
        note_id: Uuid,
        command_id: Uuid,
        request: HandoffRunRequest,
    ) -> AppResult<crate::entities::task::BackgroundTask> {
        crate::task_queue::TaskPersistence::start_agent_task(
            &self.persistence,
            &crate::entities::internal_note::StartAgentTask {
                company_id: self.company_id,
                channel_id: hold.channel_id,
                thread_id: hold.thread_id,
                note_ids: vec![note_id],
                command_id,
                handoff: Some(request),
            },
            actor,
        )
        .await
    }

    async fn runs(&self, hold: &Hold) -> Vec<RunRow> {
        sqlx::query_as::<_, RunRow>(
            r#"SELECT task_id, handoff_id, generation, requested_by_principal_id, command_id,
                      draft_id, draft_version, state
               FROM thread_handoff_runs
               WHERE company_id = $1 AND handoff_id = $2
               ORDER BY created_at"#,
        )
        .bind(self.company_id)
        .bind(hold.handoff_id)
        .fetch_all(self.persistence.pool())
        .await
        .unwrap()
    }

    /// Scoped to the company this test created: the shared database makes a whole-table count racy.
    async fn task_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM background_tasks WHERE company_id = $1")
            .bind(self.company_id)
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }

    /// Leave no claimable task behind: the test database is shared, and an unscoped claim in
    /// another test must not find this one.
    async fn complete_task(&self, task_id: Uuid) {
        sqlx::query(
            r#"UPDATE background_tasks
               SET status = 'completed', transition_reason = 'completed',
                   transition_actor_kind = 'system', transition_actor_id = NULL,
                   worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                   lock_expires_at = NULL
               WHERE company_id = $1 AND id = $2"#,
        )
        .bind(self.company_id)
        .bind(task_id)
        .execute(self.persistence.pool())
        .await
        .unwrap();
    }

    async fn dismiss(&self, hold: &Hold, actor: PrincipalId) -> AppResult<u64> {
        self.persistence
            .dismiss_thread_handoff(ThreadHandoffDismiss {
                company_id: self.company_id,
                handoff_id: hold.handoff_id,
                command_id: Uuid::new_v4(),
                expected_version: self.row(hold).await.version,
                expected_generation: hold.generation,
                actor_principal_id: actor,
                visible_channel_ids: vec![hold.channel_id],
            })
            .await
    }
}

/// Case 4. The happy path on a thread with no active task.
#[tokio::test]
async fn generate_draft_starts_one_fenced_run_and_moves_the_hold_to_drafting() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.bo))
        .await;
    let note = fx.note(&hold, fx.bo).await;

    let task = fx.generate_draft(&hold, fx.bo, note).await.unwrap();

    let runs = fx.runs(&hold).await;
    assert_eq!(runs.len(), 1, "one run per generation");
    assert_eq!(runs[0].task_id, task.id);
    assert_eq!(runs[0].handoff_id, hold.handoff_id);
    assert_eq!(runs[0].generation, hold.generation);
    assert_eq!(runs[0].state, "running");
    assert_eq!(runs[0].requested_by_principal_id, fx.bo.as_uuid());
    assert_eq!(
        (runs[0].draft_id, runs[0].draft_version),
        (None, None),
        "a running run has produced nothing yet"
    );

    let row = fx.row(&hold).await;
    assert_eq!(row.state, ThreadHandoffState::Drafting);
    assert_eq!(row.version, 2);
    assert_eq!(row.responsible_principal_id, Some(fx.bo));

    let events = fx.events(&hold).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].operation, "draft_requested");
    assert_eq!(events[0].actor_kind, "human");
    assert_eq!(events[0].actor_principal_id, Some(fx.bo.as_uuid()));
    assert_eq!(events[0].from_state.as_deref(), Some("needs_instruction"));
    assert_eq!(events[0].to_state, "drafting");
    assert_eq!((events[0].from_version, events[0].to_version), (1, 2));
    let task_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT task_id FROM thread_handoff_events WHERE company_id = $1 AND handoff_id = $2",
    )
    .bind(fx.company_id)
    .bind(hold.handoff_id)
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    assert_eq!(task_id, Some(task.id), "the trail names the run's task");

    fx.complete_task(task.id).await;
}

/// Case 12. `drafting` is visible progress rather than work, so the item leaves the queue -- and
/// the thread's own handoff row is still readable, because the queue and the thread are different
/// surfaces.
#[tokio::test]
async fn a_drafting_hold_is_absent_from_every_attention_view_but_still_readable() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.bo))
        .await;
    assert!(fx.is_queued(&hold, fx.bo).await);
    let note = fx.note(&hold, fx.bo).await;

    let task = fx.generate_draft(&hold, fx.bo, note).await.unwrap();

    assert!(!fx.is_queued(&hold, fx.bo).await);
    assert!(!fx.is_queued(&hold, fx.manager).await);
    assert_eq!(fx.row(&hold).await.state, ThreadHandoffState::Drafting);
    fx.complete_task(task.id).await;
}

/// Case 6. An unclaimed hold is refused outright: the reviewer of the draft this run would write
/// is the responsible principal, so there has to be one before the agent starts writing.
#[tokio::test]
async fn generate_draft_refuses_an_unclaimed_hold_and_writes_nothing() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx.hold(ThreadHandoffState::NeedsInstruction, None).await;
    let note = fx.note(&hold, fx.bo).await;

    let refused = fx
        .generate_draft(&hold, fx.bo, note)
        .await
        .expect_err("an unclaimed reply has no assigned reviewer");
    assert!(matches!(&refused, AppError::Conflict(_)), "{refused:?}");

    assert!(fx.runs(&hold).await.is_empty());
    assert_eq!(
        fx.task_count().await,
        0,
        "the whole transaction rolled back"
    );
    let row = fx.row(&hold).await;
    assert_eq!(row.state, ThreadHandoffState::NeedsInstruction);
    assert_eq!(row.version, 1);
    assert!(fx.events(&hold).await.is_empty());
}

/// Case 7. Somebody else's claim, then the same command from a manager.
#[tokio::test]
async fn generate_draft_is_refused_for_another_persons_hold_and_allowed_for_a_manager() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.bo))
        .await;
    let note = fx.note(&hold, fx.cleo).await;

    let refused = fx
        .generate_draft(&hold, fx.cleo, note)
        .await
        .expect_err("Cleo is not responsible for this reply and manages nothing");
    assert!(is_not_found(&refused), "{refused:?}");
    assert!(fx.runs(&hold).await.is_empty());
    assert_eq!(fx.task_count().await, 0);

    let task = fx.generate_draft(&hold, fx.manager, note).await.unwrap();
    assert_eq!(fx.runs(&hold).await.len(), 1);
    assert_eq!(
        fx.row(&hold).await.responsible_principal_id,
        Some(fx.bo),
        "a manager acting on somebody's reply does not take it from them"
    );
    fx.complete_task(task.id).await;
}

/// Case 8. Two concurrent presses on one generation: one run, one task, one conflict.
#[tokio::test]
async fn two_generate_drafts_on_one_generation_leave_one_run_and_one_task() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.bo))
        .await;
    let note = fx.note(&hold, fx.bo).await;

    let first = fx.generate_draft(&hold, fx.bo, note).await.unwrap();
    // The second press still believes the row is at version 1, which is what a second browser tab
    // holding the page from before the first press actually has.
    let second = fx
        .generate_draft(&hold, fx.bo, note)
        .await
        .expect_err("a generation gets exactly one drafting run");
    assert!(matches!(&second, AppError::Conflict(_)), "{second:?}");

    assert_eq!(fx.runs(&hold).await.len(), 1);
    assert_eq!(
        fx.task_count().await,
        1,
        "the loser's task rolled back with its transaction"
    );
    assert_eq!(fx.row(&hold).await.version, 2);
    fx.complete_task(first.id).await;
}

/// Case 9. Both fences, each naming the value to refresh to.
#[tokio::test]
async fn generate_draft_refuses_a_stale_generation_and_a_stale_version() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.bo))
        .await;
    let note = fx.note(&hold, fx.bo).await;

    let stale_generation = fx
        .generate_draft_with(
            &hold,
            fx.bo,
            note,
            Uuid::new_v4(),
            HandoffRunRequest {
                handoff_id: hold.handoff_id,
                generation: Uuid::new_v4(),
                expected_version: 1,
            },
        )
        .await
        .expect_err("that generation is not this thread's");
    let AppError::Conflict(message) = &stale_generation else {
        panic!("{stale_generation:?}")
    };
    assert!(
        message.contains(&hold.generation.to_string()),
        "the message names the generation to refresh to: {message}"
    );

    let stale_version = fx
        .generate_draft_with(
            &hold,
            fx.bo,
            note,
            Uuid::new_v4(),
            HandoffRunRequest {
                handoff_id: hold.handoff_id,
                generation: hold.generation,
                expected_version: 7,
            },
        )
        .await
        .expect_err("version 7 was never this row's");
    let AppError::Conflict(message) = &stale_version else {
        panic!("{stale_version:?}")
    };
    assert!(message.contains("version 7 to 1"), "{message}");

    assert!(fx.runs(&hold).await.is_empty());
    assert_eq!(fx.task_count().await, 0);
    assert_eq!(fx.row(&hold).await.version, 1);
}

/// Case 10. Replay through the existing `start_agent_task_commands` record, and the same id with a
/// different generation.
#[tokio::test]
async fn a_replayed_generate_draft_writes_nothing_further_and_a_changed_one_conflicts() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.bo))
        .await;
    let note = fx.note(&hold, fx.bo).await;
    let command_id = Uuid::new_v4();
    let request = HandoffRunRequest {
        handoff_id: hold.handoff_id,
        generation: hold.generation,
        expected_version: 1,
    };

    let first = fx
        .generate_draft_with(&hold, fx.bo, note, command_id, request)
        .await
        .unwrap();
    let replay = fx
        .generate_draft_with(&hold, fx.bo, note, command_id, request)
        .await
        .unwrap();
    assert_eq!(
        replay.id, first.id,
        "the recorded outcome, not a second run"
    );
    let runs = fx.runs(&hold).await;
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0].command_id, command_id,
        "the run records the command that asked for it"
    );
    assert_eq!(fx.task_count().await, 1);
    assert_eq!(fx.row(&hold).await.version, 2);
    assert_eq!(fx.events(&hold).await.len(), 1);

    let changed = fx
        .generate_draft_with(
            &hold,
            fx.bo,
            note,
            command_id,
            HandoffRunRequest {
                generation: Uuid::new_v4(),
                ..request
            },
        )
        .await
        .expect_err("the same id with a different generation is a different command");
    assert!(matches!(&changed, AppError::Conflict(_)), "{changed:?}");
    fx.complete_task(first.id).await;
}

/// Case 11. Only `needs_instruction` accepts a run.
#[tokio::test]
async fn generate_draft_refuses_a_drafting_or_terminal_hold() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    for state in [
        ThreadHandoffState::Drafting,
        ThreadHandoffState::DraftReady,
        ThreadHandoffState::Resolved,
        ThreadHandoffState::Dismissed,
    ] {
        let hold = fx.hold(state, Some(fx.bo)).await;
        let note = fx.note(&hold, fx.bo).await;
        let refused = fx
            .generate_draft(&hold, fx.bo, note)
            .await
            .expect_err("{state:?} is not waiting for an instruction");
        assert!(
            matches!(&refused, AppError::Conflict(_)),
            "{state:?}: {refused:?}"
        );
        assert!(fx.runs(&hold).await.is_empty(), "{state:?}");
        assert_eq!(fx.row(&hold).await.state, state);
    }
    assert_eq!(fx.task_count().await, 0);
}

/// Case 27. Dismissal writes the handoff and its event, and nothing else.
#[tokio::test]
async fn dismissing_a_hold_closes_it_without_touching_the_customers_message() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx
        .hold(ThreadHandoffState::NeedsInstruction, Some(fx.bo))
        .await;
    let source_message_id = fx.row(&hold).await.source_message_id;

    assert_eq!(fx.dismiss(&hold, fx.bo).await.unwrap(), 2);

    let row = fx.row(&hold).await;
    assert_eq!(row.state, ThreadHandoffState::Dismissed);
    assert_eq!(row.version, 2);
    let closed_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT closed_at FROM thread_handoffs WHERE company_id = $1 AND id = $2",
    )
    .bind(fx.company_id)
    .bind(hold.handoff_id)
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    assert!(closed_at.is_some());

    let events = fx.events(&hold).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].operation, "dismissed");
    assert_eq!(events[0].actor_kind, "human");
    assert_eq!(events[0].to_state, "dismissed");

    // The customer's message is still the customer's message.
    let audience: (String, Uuid) =
        sqlx::query_as("SELECT audience, id FROM messages WHERE company_id = $1 AND id = $2")
            .bind(fx.company_id)
            .bind(source_message_id.as_uuid())
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    assert_eq!(audience.0, "external_conversation");
    assert_eq!(fx.task_count().await, 0, "dismissal starts no work");
    assert!(!fx.is_queued(&hold, fx.bo).await);
    assert!(
        fx.persistence
            .get_thread_handoff(fx.company_id, hold.handoff_id, &[hold.channel_id])
            .await
            .unwrap()
            .is_some(),
        "a dismissed hold is still readable"
    );
}

/// Case 27, second half: a run is in flight, so the answer to "stop it" is to let it end.
#[tokio::test]
async fn a_drafting_hold_cannot_be_dismissed() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fx = CommandFixture::new(&persistence).await;
    let hold = fx.hold(ThreadHandoffState::Drafting, Some(fx.bo)).await;

    let refused = fx
        .dismiss(&hold, fx.bo)
        .await
        .expect_err("a drafting reply has a run in flight");
    assert!(matches!(&refused, AppError::Conflict(_)), "{refused:?}");
    assert_eq!(fx.row(&hold).await.state, ThreadHandoffState::Drafting);
    assert!(fx.events(&hold).await.is_empty());
}
