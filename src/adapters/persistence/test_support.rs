//! One way for the DB-backed tests in this directory to reach a database.
//!
//! # Why these tests do not use the development database
//!
//! The queue operations under test are deliberately unscoped: `claim_pending_tasks` and
//! `claim_outbox_emails` sweep every row in the database, because that is what a real worker does.
//! Pointed at the development database, a test run therefore competes with — and claims rows out
//! from under — a `cargo run` server polling the same queues twice a second, and can mark real
//! deliveries failed through `reap_expired_outbox_leases`.
//!
//! So the tests get their own database, derived from `DATABASE_URL` by suffixing the database name.
//! The command `src/adapters/persistence/AGENTS.md` tells you to run is unchanged; it just lands
//! somewhere it cannot do damage.
//!
//! # Absent configuration skips, broken configuration shouts
//!
//! With no `DATABASE_URL` at all these tests skip, which is what lets the suite run in CI without a
//! database. That silence has a cost the same AGENTS.md records: three tests in `thread.rs` named
//! columns no migration creates and went unnoticed because CI never set the variable.
//!
//! So the two cases are separated. *No* configuration is a skip, as before. Configuration that is
//! present but unusable — the test database was never created, the server is down — is a panic
//! naming the fix, because that is a broken setup silently reporting success.

use sqlx::PgPool;
use tokio::sync::{Mutex, OnceCell};

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

/// Where the tests should connect, or `None` when nothing is configured.
///
/// `TEST_DATABASE_URL` wins outright, for a database that is not named after the development one.
/// Otherwise `DATABASE_URL` is redirected onto its `_test` sibling.
fn test_database_url() -> Option<String> {
    let explicit = std::env::var("TEST_DATABASE_URL")
        .ok()
        .filter(|url| !url.trim().is_empty());
    if let Some(explicit) = explicit {
        return Some(explicit);
    }

    let configured = std::env::var("DATABASE_URL").ok()?;
    Some(with_test_database_name(&configured))
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

#[cfg(test)]
mod tests {
    use super::*;

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
