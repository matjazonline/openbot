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
//!
//! # A test that cannot share at all gets a database of its own
//!
//! The queue claims sweep every row of the database they run against, so a test asserting *which*
//! rows a claim took cannot tolerate a neighbour's rows, and a test that leaves a claimable row
//! behind breaks whichever test claims it next. [`own_database`] hands one test a database of its
//! own, migrated, and drops it when the handle goes out of scope — on a panic too.
//! `task/claim_tests.rs` runs on it, which is why those tests assert exact claim results instead of
//! working around whatever else is in the queue.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::{Connection, PgConnection, PgPool};
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    entities::{
        attention::BusinessPriority,
        correlation::CorrelationId,
        message::{CanonicalMessageId, MessageDirection, MessageRole},
        runtime_metrics::{MachineId, MachineIdentity, MachineRegion},
        thread_handoff::ThreadHandoffState,
        transport::{
            ChannelBindingId, DeliveryId, DeliveryPurpose, ExternalDestination, PrincipalId,
            TransportKind,
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

/// A fixed machine for the attempt ledger, so a test asserting on what was recorded has something
/// to compare against. The real one is [`MachineIdentity::process`], which is boot-local off Fly
/// and would differ from run to run.
pub fn test_machine() -> MachineIdentity {
    MachineIdentity {
        id: MachineId::new("test-machine"),
        region: Some(MachineRegion::new("tst")),
    }
}

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
    let base = url.split_once('?').map_or(url, |(base, _)| base);
    let name = base.rsplit_once('/').map_or("", |(_, name)| name);

    // Nothing to rename: no database name at all, or already a test database — a
    // `TEST_DATABASE_URL` that went through `DATABASE_URL`, or a second call. Suffixing again
    // would invent `mail_agents_test_test`. Hand the URL back and let the connection report why.
    if name.is_empty() || name.ends_with(TEST_DB_SUFFIX) {
        return url.to_string();
    }
    with_database_name(url, &format!("{name}{TEST_DB_SUFFIX}"))
}

/// Point `url` at database `name`, leaving everything else about it alone.
fn with_database_name(url: &str, name: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };

    let renamed = match base.rsplit_once('/') {
        Some((prefix, _)) => format!("{prefix}/{name}"),
        None => base.to_string(),
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
            sqlx::migrate!("./migrations")
                .run(&pool)
                .await
                .expect("the test database accepts this checkout's migrations");
            purge_fixtures(&pool).await;
        })
        .await;

    Some(pool)
}

/// A database created for one test, dropped when this handle goes out of scope.
///
/// Hold it for the length of the test and take `database.pool.clone()` for a persistence handle:
///
/// ```ignore
/// let Some(database) = own_database().await else { return };
/// let pool = database.pool.clone();
/// ```
pub struct OwnDatabase {
    /// A pool against this test's own database.
    pub pool: PgPool,
    name: String,
    admin_url: String,
}

impl Drop for OwnDatabase {
    fn drop(&mut self) {
        // There is no async `Drop` to hang this on, and the test's runtime may be mid-unwind and
        // about to shut down, so the statement runs on a thread and a runtime of its own.
        let name = std::mem::take(&mut self.name);
        let admin_url = std::mem::take(&mut self.admin_url);
        let dropped = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime for one statement")
                .block_on(admin_statement(
                    &admin_url,
                    // `FORCE`: this test's pool still holds connections, and a panicking test may
                    // have left others open.
                    &format!(r#"DROP DATABASE "{name}" WITH (FORCE)"#),
                ));
        })
        .join();
        if dropped.is_err() {
            // Reported, not raised: a test that is already failing keeps its own panic, and a
            // `mail_agents_own_%` database left behind shows up in `psql -l`.
            eprintln!("could not drop this test's database; it is left behind");
        }
    }
}

/// A database of this test's own: created, migrated, and dropped when the handle drops.
///
/// `None` when no database is configured and skipping has been permitted, exactly as [`test_pool`]
/// skips. Migrations run once per database rather than once per binary, which is what the isolation
/// costs: the database starts empty, so the test states every row that exists in it.
pub async fn own_database() -> Option<OwnDatabase> {
    let url = test_database_url()?;
    // Generated, so it needs no quoting beyond the identifier quotes: `CREATE DATABASE` takes no
    // parameters.
    let name = format!("mail_agents_own_{}", Uuid::new_v4().simple());
    let admin_url = with_database_name(&url, "postgres");
    admin_statement(&admin_url, &format!(r#"CREATE DATABASE "{name}""#)).await;

    // Every isolated test holds a pool of its own and `cargo test` runs them in parallel, so these
    // are kept small: the default of ten would put a busy machine's worth of tests within reach of
    // the server's `max_connections`. A test that needs more fan-out than this opens its own pool.
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&with_database_name(&url, &name))
        .await
        .expect("a database this test just created accepts connections");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("a fresh database accepts this checkout's migrations");

    Some(OwnDatabase {
        pool,
        name,
        admin_url,
    })
}

/// Run one statement against the maintenance database. `CREATE`/`DROP DATABASE` cannot run inside
/// a transaction or through a pool holding other connections to the target.
async fn admin_statement(admin_url: &str, statement: &str) {
    let mut connection = PgConnection::connect(admin_url)
        .await
        .expect("the maintenance database is reachable");
    sqlx::query(statement)
        .execute(&mut connection)
        .await
        .unwrap_or_else(|error| panic!("{statement} failed: {error}"));
    let _ = connection.close().await;
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
    )
    .external_conversation();
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
            message_audience: crate::entities::message::MessageAudience::ExternalConversation,
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

/// One `thread_handoffs` row and the canonical message it names.
///
/// Shared because three test modules need the same two rows -- the projection tests, the command
/// tests and the route tests -- and because the handoff's foreign keys make "just insert a row"
/// impossible: its source message has to be a real, same-company message.
pub struct ThreadHandoffFixture {
    pub handoff_id: Uuid,
    pub generation: Uuid,
    pub message_id: CanonicalMessageId,
}

/// What a fixture handoff should look like. Defaults to a freshly held reply nobody has claimed.
pub struct ThreadHandoffFixtureRequest {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub state: ThreadHandoffState,
    pub responsible_principal_id: Option<PrincipalId>,
    pub priority: BusinessPriority,
    pub due_at: Option<DateTime<Utc>>,
}

impl ThreadHandoffFixtureRequest {
    pub const fn new(company_id: Uuid, channel_id: Uuid, thread_id: Uuid) -> Self {
        Self {
            company_id,
            channel_id,
            thread_id,
            state: ThreadHandoffState::NeedsInstruction,
            responsible_principal_id: None,
            priority: BusinessPriority::Normal,
            due_at: None,
        }
    }
}

/// Write the message and the handoff row that names it, at version 1.
///
/// Written directly rather than through `open_handoff_generation_on`, because a projection or
/// command test needs to state a state, a responsibility and a priority that the opening path
/// deliberately cannot produce. `closed_at` follows the state, so `thread_handoffs_closure_check`
/// accepts a terminal fixture as readily as an open one.
pub async fn thread_handoff_fixture(
    persistence: &PostgresPersistence,
    request: ThreadHandoffFixtureRequest,
) -> ThreadHandoffFixture {
    let write = MessageWrite::internal(
        request.thread_id,
        MessageAuthorWrite::Platform,
        "Invoice 4471".to_string(),
        "Any news on this?".to_string(),
        MessageDirection::Inbound,
        MessageRole::Human,
        CorrelationId::new(),
    )
    .external_conversation();
    let message_id = write.id;
    ThreadPersistence::create_message(persistence, &write)
        .await
        .expect("the fixture message is stored");

    let fixture = ThreadHandoffFixture {
        handoff_id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
        message_id,
    };
    sqlx::query(
        r#"INSERT INTO thread_handoffs (
               id, company_id, channel_id, thread_id, generation, state, source_message_id,
               responsible_principal_id, business_priority, business_due_at, closed_at
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                     CASE WHEN $6 IN ('resolved', 'dismissed') THEN CURRENT_TIMESTAMP END)"#,
    )
    .bind(fixture.handoff_id)
    .bind(request.company_id)
    .bind(request.channel_id)
    .bind(request.thread_id)
    .bind(fixture.generation)
    .bind(request.state.as_str())
    .bind(message_id.as_uuid())
    .bind(request.responsible_principal_id.map(PrincipalId::as_uuid))
    .bind(request.priority.as_str())
    .bind(request.due_at)
    .execute(persistence.pool())
    .await
    .expect("the fixture handoff is stored");
    fixture
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
