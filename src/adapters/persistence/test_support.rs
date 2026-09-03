//! One way for the DB-backed tests in this directory to reach a database.
//!
//! # Why these tests do not use the development database
//!
//! The queue operations under test are deliberately unscoped: `claim_pending_tasks` and
//! `claim_deliveries` sweep every row in the database, because that is what a real worker does.
//! Pointed at the development database, a test run therefore competes with — and claims rows out
//! from under — a `cargo run` server polling the same queues twice a second, and can charge real
//! deliveries an attempt through `reap_expired_deliveries`.
//!
//! So the tests get their own database, derived from `DATABASE_URL` by suffixing the database name.
//! The command `src/adapters/persistence/AGENTS.md` tells you to run is unchanged; it just lands
//! somewhere it cannot do damage.
//!
//! # Missing configuration shouts too, unless silence is asked for
//!
//! These tests used to skip when `DATABASE_URL` was unset, so the suite could run without a
//! database. The cost is recorded in `src/adapters/persistence/AGENTS.md`: three tests in
//! `thread.rs` named columns no migration creates and went unnoticed, because nothing ever set the
//! variable. Two more slipped through the same gap later — a content-hash mismatch that broke every
//! internal delegation hop, and a `WHERE id = $9` placeholder collision — each hidden behind a
//! green run of a suite that had silently skipped every Postgres test.
//!
//! The failure mode is what makes it dangerous: a skipped test is *counted as passing*, the suite
//! reports the same total either way, and whole-suite wall time is too noisy to notice. So the
//! default is now loud. Unset is a panic naming the fix, exactly like a database that cannot be
//! reached.
//!
//! Silence is still available, but it has to be asked for: set `ALLOW_MISSING_DATABASE_URL=1` and
//! the tests skip as before. That is the switch for a CI job that deliberately runs without
//! Postgres — this repository has no CI at all today, which is why the old default was protecting
//! nothing.

use sqlx::PgPool;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    entities::{
        correlation::CorrelationId,
        message::{CanonicalMessageId, MessageDirection, MessageRole},
        transport::{
            ChannelBindingId, DeliveryId, DeliveryPurpose, ExternalDestination, TransportKind,
        },
        value_objects::EmailAddress,
    },
    transport::{
        ContentDigest, DeliveryKey, MAX_DELIVERY_ATTEMPTS, NewDelivery, PartIndex, PartKey,
        RenderedPart, TransportPayload,
    },
    use_cases::{
        integration::ChannelBindingPersistence,
        thread::{MessageAuthorWrite, MessageWrite, ThreadPersistence},
    },
};

/// The suffix that separates the tests' database from the one you develop against.
const TEST_DB_SUFFIX: &str = "_test";

/// Serialises tests that exercise an *unscoped* queue claim.
///
/// A separate database keeps the tests away from a running server, but it does nothing about the
/// tests themselves: they share one database and `cargo test` runs them in parallel. A claim like
/// `claim_and_advance_due_schedules` sweeps every due row up to its batch size *and advances what
/// it takes*, so two tests that each queue one row and assert their own comes back cannot overlap
/// — whichever claims first carries off both rows, and the other fails on a row that was claimed,
/// just not by it.
///
/// Nothing about the ordering or the batch size fixes that, which is what separates this from the
/// crowding a test can fix on its own (`an_expired_delivery_lease_costs_an_attempt_and_reaches_the_cap`
/// sorts its row to the front and claims with a limit of 1). Here the row is *consumed* by the
/// other claim, so the two simply have to not run at the same time.
///
/// Hold it from before the row is queued until after the claim under test. It is a
/// [`tokio::sync::Mutex`], so a panicking test releases it without poisoning it for the rest.
pub static UNSCOPED_CLAIM: Mutex<()> = Mutex::const_new(());

/// Migrations run once per test binary, not once per test.
static MIGRATED: OnceCell<()> = OnceCell::const_new();

/// The environment variable that buys back the old skip-when-unset behaviour.
const ALLOW_MISSING_URL_VAR: &str = "ALLOW_MISSING_DATABASE_URL";

/// Whether the caller has explicitly asked for DB-backed tests to be skipped.
///
/// Any non-empty value counts. The point is that skipping is a deliberate act with a name on it,
/// not the accident of an unexported variable.
fn skipping_is_permitted() -> bool {
    permits_skipping(std::env::var(ALLOW_MISSING_URL_VAR).ok().as_deref())
}

/// The decision itself, separated from the environment so it can be tested without `set_var` —
/// which is `unsafe` in edition 2024 and process-global besides, so a test using it would race
/// every other test in the binary.
fn permits_skipping(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

/// Where the tests should connect, or `None` when skipping has been explicitly permitted.
///
/// `TEST_DATABASE_URL` wins outright, for a database that is not named after the development one.
/// Otherwise `DATABASE_URL` is redirected onto its `_test` sibling.
///
/// Panics when neither is set and [`ALLOW_MISSING_URL_VAR`] is absent — see the module docs for
/// why an unset variable is a failure rather than a skip.
fn test_database_url() -> Option<String> {
    let explicit = std::env::var("TEST_DATABASE_URL")
        .ok()
        .filter(|url| !url.trim().is_empty());
    if let Some(explicit) = explicit {
        return Some(explicit);
    }

    let configured = std::env::var("DATABASE_URL")
        .ok()
        .filter(|url| !url.trim().is_empty());
    match configured {
        Some(configured) => Some(with_test_database_name(&configured)),
        None if skipping_is_permitted() => None,
        None => panic!(
            "DATABASE_URL is not set, so this test would silently skip and be counted as passing.\n\n\
             Run the suite with a database:\n\n    \
             DATABASE_URL=\"postgres://$(whoami)@localhost:5432/mail_agents\" cargo test\n\n\
             (Tests never use that database directly — they are redirected onto its `_test` \
             sibling. Create it once with `createdb mail_agents_test`.)\n\n\
             To skip them on purpose, set {ALLOW_MISSING_URL_VAR}=1."
        ),
    }
}

/// Redirect a connection URL onto its `_test` sibling, leaving everything else about it alone.
///
/// Split before the query string first: `?sslmode=require` contains no `/`, but a naive
/// `rsplit_once('/')` over the whole URL would still find the last path separator and rebuild the
/// parameters into the database name.
fn with_test_database_name(url: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };

    let renamed = match base.rsplit_once('/') {
        // Already a test database — a `TEST_DATABASE_URL` that went through `DATABASE_URL`, or a
        // second call. Suffixing again would invent `mail_agents_test_test`.
        Some((_, name)) if name.ends_with(TEST_DB_SUFFIX) => base.to_string(),
        Some((prefix, name)) if !name.is_empty() => format!("{prefix}/{name}{TEST_DB_SUFFIX}"),
        // No database name to rename; hand it back and let the connection attempt report why.
        _ => base.to_string(),
    };

    match query {
        Some(query) => format!("{renamed}?{query}"),
        None => renamed,
    }
}

/// Clear what earlier runs left behind, once per test binary.
///
/// Tests build their fixtures — a user, a company, a channel, some queue rows — and delete them on
/// the way out, but a test that panics never reaches its cleanup. Companies at least cascade when
/// a later run deletes them; `users` never does, because *nothing in this codebase deletes a user
/// row at all*. So every panicking run leaked a handful of users permanently, and the table had
/// grown to some 4,600 rows across roughly 200 runs before this existed.
///
/// That debris is not inert. These queue claims are unscoped, so leftover rows are exactly the
/// neighbours that make a claim-and-assert test flaky — the failure this was written alongside.
///
/// `TRUNCATE ... CASCADE` over every table but the migration ledger, rather than a curated list of
/// `DELETE`s in dependency order: it cannot go stale as tables are added, and it cannot silently
/// half-clean because someone forgot that `companies.user_id` is `ON DELETE RESTRICT`.
///
/// It runs *before* the suite rather than after it, which is deliberate: the wreckage of a failed
/// run stays on the database for you to inspect until the next run starts.
async fn purge_fixtures(pool: &PgPool) {
    sqlx::query(
        r#"DO $$
           DECLARE tables text;
           BEGIN
               SELECT string_agg(format('%I', tablename), ', ')
                 INTO tables
                 FROM pg_tables
                WHERE schemaname = 'public' AND tablename <> '_sqlx_migrations';
               IF tables IS NOT NULL THEN
                   EXECUTE 'TRUNCATE TABLE ' || tables || ' CASCADE';
               END IF;
           END $$"#,
    )
    .execute(pool)
    .await
    .expect("the test database can be cleared between runs");
}

/// A pool against the test database, with migrations applied, or `None` when unconfigured.
///
/// Panics if a database was configured but cannot be reached — see the module docs.
pub async fn test_pool() -> Option<PgPool> {
    let url = test_database_url()?;

    let pool = match PgPool::connect(&url).await {
        Ok(pool) => pool,
        Err(error) => panic!(
            "DATABASE_URL is set, so these tests are meant to run, but the test database could not \
             be reached: {error}\n\nCreate it once with:\n\n    createdb mail_agents_test\n\n\
             (Tests never use the development database: they claim from the same queues a running \
             server polls. Set TEST_DATABASE_URL to override the derived name.)"
        ),
    };

    MIGRATED
        .get_or_init(|| async {
            sqlx::migrate!()
                .run(&pool)
                .await
                .expect("the test database accepts this checkout's migrations");
            purge_fixtures(&pool).await;
        })
        .await;

    Some(pool)
}

/// One canonical message and one delivery carrying it, on the channel's own email interface.
///
/// Shared because three test modules need the same four rows -- a message, its thread association,
/// the interface, and the queue row -- and because a delivery's foreign keys make "just insert a
/// row" impossible: every one of them has to be a real, same-company row.
pub struct DeliveryFixture {
    pub delivery: NewDelivery,
    pub message_id: CanonicalMessageId,
    pub binding_id: ChannelBindingId,
}

/// What a fixture delivery should look like. Defaults to a plain email reply to one recipient.
pub struct DeliveryFixtureRequest<'a> {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub task_id: Option<Uuid>,
    /// Distinguishes one fixture delivery from another within the same interface, which is what
    /// the unique index is over.
    pub source_key: &'a str,
    pub recipient: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
    pub purpose: DeliveryPurpose,
    /// How many parts to freeze. Email renders one; more than one exercises the aggregation rule
    /// that a parent is delivered only when every part is.
    pub parts: u16,
    pub depends_on: Option<DeliveryId>,
}

impl<'a> DeliveryFixtureRequest<'a> {
    pub fn new(company_id: Uuid, channel_id: Uuid, thread_id: Uuid, source_key: &'a str) -> Self {
        Self {
            company_id,
            channel_id,
            thread_id,
            task_id: None,
            source_key,
            recipient: "customer@example.com",
            subject: "Re: order",
            body: "On its way.",
            purpose: DeliveryPurpose::Reply,
            parts: 1,
            depends_on: None,
        }
    }
}

/// Write the message and return the delivery that carries it, ready to insert.
///
/// The delivery is *not* written: callers insert it through the path they are testing -- the
/// dispatch commit, the outreach transaction, or `enqueue_delivery` -- which is the whole point of
/// the exercise.
pub async fn delivery_fixture(
    persistence: &PostgresPersistence,
    request: DeliveryFixtureRequest<'_>,
) -> DeliveryFixture {
    let binding = ChannelBindingPersistence::active_bindings_for_channel(
        persistence,
        request.company_id,
        request.channel_id,
    )
    .await
    .expect("the channel's interfaces are readable")
    .into_iter()
    .find(|binding| binding.transport == TransportKind::Email)
    .expect("a channel is created with its canonical email interface");

    let write = MessageWrite::internal(
        request.thread_id,
        MessageAuthorWrite::Platform,
        request.subject.to_string(),
        request.body.to_string(),
        MessageDirection::Outbound,
        MessageRole::Agent,
        CorrelationId::new(),
    );
    let message_id = write.id;
    ThreadPersistence::create_message(persistence, &write)
        .await
        .expect("the fixture message is stored");

    let destination = ExternalDestination::Email(EmailAddress::from(request.recipient));
    let key = crate::transport::delivery_key(
        request.purpose,
        request.source_key,
        &crate::transport::DeliveryDestination::External(destination.clone()),
    );

    DeliveryFixture {
        message_id,
        binding_id: binding.id,
        delivery: NewDelivery {
            id: DeliveryId::random(),
            company_id: request.company_id,
            channel_id: request.channel_id,
            message_id,
            source_binding_id: binding.id,
            destination_binding_id: binding.id,
            external_destination: Some(destination),
            task_id: request.task_id,
            depends_on_delivery_id: request.depends_on,
            correlation_id: CorrelationId::new(),
            transport: TransportKind::Email,
            purpose: request.purpose,
            idempotency_key: key.clone(),
            max_attempts: MAX_DELIVERY_ATTEMPTS,
            parts: NewDelivery::frozen_parts(
                (0..request.parts)
                    .map(|index| fixture_part(&key, index, request.body))
                    .collect(),
            )
            .expect("a fixture freezes at least one part and fewer than the bound"),
        },
    }
}

/// One frozen part, keyed the way the email renderer keys its own: from the delivery's stable
/// idempotency key, so a re-render addresses the part that already exists.
fn fixture_part(key: &DeliveryKey, index: u16, body: &str) -> RenderedPart {
    RenderedPart {
        index: PartIndex::new(index),
        key: PartKey::parse(format!("email:{}:{index}", key.as_str()))
            .expect("a fixture part key is within its bound"),
        payload: TransportPayload::encode(
            TransportKind::Email,
            crate::adapters::protocols::email::OUTBOUND_EMAIL_VERSION,
            &serde_json::json!({ "fixture": index }),
        )
        .expect("a small object encodes"),
        digest: ContentDigest::sha256_of(body.as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipping_needs_a_real_value_not_just_a_present_variable() {
        assert!(permits_skipping(Some("1")));
        assert!(permits_skipping(Some("true")));
        // An exported-but-empty variable is the classic `export FOO=` accident. It must not buy
        // silence, or the failure this whole mechanism exists to prevent comes back.
        assert!(!permits_skipping(Some("")));
        assert!(!permits_skipping(Some("   ")));
        assert!(!permits_skipping(None));
    }

    #[test]
    fn a_development_url_is_redirected_to_its_test_sibling() {
        assert_eq!(
            with_test_database_name("postgres://mac03@localhost:5432/mail_agents"),
            "postgres://mac03@localhost:5432/mail_agents_test"
        );
    }

    #[test]
    fn query_parameters_survive_the_rename() {
        // The rename must land on the database name, not on the last path-looking thing in the URL.
        assert_eq!(
            with_test_database_name("postgres://user:pw@host:5432/mail_agents?sslmode=require"),
            "postgres://user:pw@host:5432/mail_agents_test?sslmode=require"
        );
    }

    #[test]
    fn a_test_database_is_not_suffixed_twice() {
        assert_eq!(
            with_test_database_name("postgres://localhost/mail_agents_test"),
            "postgres://localhost/mail_agents_test"
        );
    }

    #[test]
    fn a_url_with_no_database_name_is_left_for_the_connection_to_reject() {
        assert_eq!(
            with_test_database_name("postgres://localhost:5432/"),
            "postgres://localhost:5432/"
        );
    }
}
