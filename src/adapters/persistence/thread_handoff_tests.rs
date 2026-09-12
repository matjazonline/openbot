use super::*;
use crate::{
    adapters::persistence::test_support::test_pool,
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
