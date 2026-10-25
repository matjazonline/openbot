//! Exercises every dashboard query against a live database.
//!
//! These queries use the runtime sqlx API, so nothing checks their SQL or their parameter types
//! at build time — a `make_interval(mins => $2)` handed a `float8` fails at request time, on the
//! page, in production. Running each one for real is the only thing that catches it. Every
//! aggregate has a company form and a global form, and both are run here.

use std::sync::Arc;

use regex::Regex;

use super::*;
use crate::adapters::persistence::{
    delivery::enqueue::insert_delivery_on,
    test_support::{DeliveryFixtureRequest, delivery_fixture, test_pool},
};
use crate::services::dashboard_snapshot::DashboardSnapshotService;
use crate::use_cases::{
    channel::{ChannelPersistence, ChannelWrite},
    company::{CompanyPersistence, CompanyWrite},
    thread::ThreadPersistence,
    user::UserPersistence,
};

async fn test_persistence() -> Option<PostgresPersistence> {
    Some(PostgresPersistence::new(test_pool().await?))
}

/// A company of this test's own, and the one channel its fixtures are queued on.
struct CompanyFixture {
    company: Uuid,
    channel: Uuid,
}

/// Owning the company scopes each test to rows nothing else touches.
async fn company_fixture(persistence: &PostgresPersistence, label: &str) -> CompanyFixture {
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("{label}_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .expect("the fixture user is created");
    let owner = UserPersistence::get_by_email(persistence, &email)
        .await
        .expect("the fixture user is readable")
        .expect("the fixture user was just created");
    let company = CompanyPersistence::create(
        persistence,
        owner.id,
        CompanyWrite {
            name: "Dashboard Test".to_string(),
            slug: format!("{label}-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .expect("the fixture company is created");
    let channel = ChannelPersistence::create(
        persistence,
        company.id,
        ChannelWrite {
            name: "Dashboard".into(),
            slug: "dashboard".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .expect("the fixture channel is created");

    CompanyFixture {
        company: company.id,
        channel: channel.id,
    }
}

/// A root delivery parked ten years out, and a due delivery that depends on it.
///
/// The leaf is `pending` and due, which is what `DELIVERY_PRESSURE_SQL` counts, but it is not
/// claimable while its root is undelivered. The root is parked inside the transaction that
/// inserts it, so no claim ever sees it due. And a root that is still `pending` gives
/// `reap_expired_deliveries` nothing to orphan the leaf from.
async fn queue_blocked_delivery(
    persistence: &PostgresPersistence,
    fixture: &CompanyFixture,
    thread_id: Uuid,
    unit: i64,
) {
    let root_key = format!("dashboard-root-{unit}");
    let leaf_key = format!("dashboard-leaf-{unit}");
    let root = delivery_fixture(
        persistence,
        DeliveryFixtureRequest::new(fixture.company, fixture.channel, thread_id, &root_key),
    )
    .await;
    let leaf = delivery_fixture(
        persistence,
        DeliveryFixtureRequest {
            depends_on: Some(root.delivery.id),
            ..DeliveryFixtureRequest::new(fixture.company, fixture.channel, thread_id, &leaf_key)
        },
    )
    .await;

    let mut tx = persistence.pool.begin().await.expect("a transaction opens");
    insert_delivery_on(&mut tx, &root.delivery)
        .await
        .expect("the root delivery is queued");
    sqlx::query(
        "UPDATE message_deliveries
            SET available_at = CURRENT_TIMESTAMP + interval '10 years'
          WHERE id = $1",
    )
    .bind(root.delivery.id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("the root delivery is parked");
    insert_delivery_on(&mut tx, &leaf.delivery)
        .await
        .expect("the dependent delivery is queued");
    tx.commit().await.expect("the deliveries commit");
}

/// Seed `units` of every row the dashboard counts into a company of its own.
///
/// One unit is:
///
/// - an open task: `pending`, due, created half an hour ago, so it is open at every recent
///   queue-depth boundary;
/// - a finished task, `failed` forty minutes ago, so it is in the throughput window and closed by
///   the newest boundary;
/// - one finished attempt on the open task, started `attempts_minutes_ago`;
/// - one parked root delivery and one due dependent of it ([`queue_blocked_delivery`]).
///
/// Every row stays out of reach of the unscoped sweeps while the test reads, per
/// `src/adapters/persistence/AGENTS.md` on shared-database fixtures. The finished task is `failed`
/// rather than `completed` for the same reason. Moving a task to `completed` enqueues an
/// assignment notification, and a notification test's global claim could turn that into a
/// delivery on this company.
/// `units` blocked deliveries (two rows each) on a company of this test's own. The job aggregates
/// read the workflow queue, whose fixtures live with the workflow tests (`job_projection_tests`).
async fn seed_company(
    persistence: &PostgresPersistence,
    label: &str,
    units: i64,
) -> CompanyFixture {
    let fixture = company_fixture(persistence, label).await;
    let thread = ThreadPersistence::create_thread(persistence, fixture.channel, "Dashboard", &[])
        .await
        .expect("the fixture thread is created");
    for unit in 0..units {
        queue_blocked_delivery(persistence, &fixture, thread.id, unit).await;
    }
    fixture
}

/// One aggregate, read out of a snapshot as a count of [`seed_company`]'s rows.
struct Aggregate {
    name: &'static str,
    count: fn(&DashboardSnapshot) -> i64,
    /// What `count` reads for a company seeded with this many units.
    expected: fn(i64) -> i64,
}

/// The delivery aggregates, each measured by the rows its own statement reads. The workflow job
/// aggregates are held to their company in `job_projection_tests`.
const AGGREGATES: [Aggregate; 2] = [
    Aggregate {
        name: "delivery queue",
        count: |snapshot| snapshot.deliveries.count_of(DeliveryStatus::Pending),
        expected: |units| 2 * units,
    },
    Aggregate {
        name: "delivery pressure",
        count: |snapshot| snapshot.deliveries.due_now,
        expected: |units| units,
    },
];

/// Every statement's two forms, for the checks that need no database.
fn scoped_statements() -> [(&'static str, &'static ScopedSql); 11] {
    [
        ("TASK_QUEUE", &*TASK_QUEUE),
        ("TASK_PRESSURE", &*TASK_PRESSURE),
        ("DELIVERY_QUEUE", &*DELIVERY_QUEUE),
        ("DELIVERY_PRESSURE", &*DELIVERY_PRESSURE),
        ("THROUGHPUT", &*THROUGHPUT),
        ("LATENCY", &*LATENCY),
        ("QUEUE_DEPTH", &*QUEUE_DEPTH),
        ("RETRY_RATE", &*RETRY_RATE),
        ("ATTEMPT_STATS", &*ATTEMPT_STATS),
        ("TOKENS", &*TOKENS),
        ("OUTSTANDING", &*OUTSTANDING),
    ]
}

/// No statement chooses its scope by testing a bound parameter for `NULL`.
///
/// That is the shape that widens a company's dashboard to every company under a generic plan. See
/// the module docs in `dashboard.rs`.
#[test]
fn no_statement_tests_a_parameter_for_null() {
    let null_test = Regex::new(r"(?i)\$\d+(::\w+)?\s+IS\s+(NOT\s+)?NULL").unwrap();
    for (name, statement) in scoped_statements() {
        for sql in [&statement.company, &statement.global] {
            assert!(
                !null_test.is_match(sql),
                "{name} tests a parameter for NULL:\n{sql}"
            );
        }
    }
}

/// The company is each statement's last parameter, and only the company form has it.
///
/// That is what lets [`ScopedSql::query_with`] bind it last or not at all. A predicate numbered
/// anywhere else would shift the window parameters in one form and not the other.
#[test]
fn the_company_is_the_last_parameter_and_only_the_company_form_binds_it() {
    let parameter = Regex::new(r"\$(\d+)").unwrap();
    let highest = |sql: &str| {
        parameter
            .captures_iter(sql)
            .map(|found| found[1].parse::<u32>().unwrap())
            .max()
            .unwrap_or(0)
    };
    for (name, statement) in scoped_statements() {
        assert_eq!(
            highest(&statement.company),
            highest(&statement.global) + 1,
            "{name}'s company form must add exactly one parameter, numbered last"
        );
    }
}

/// Every aggregate's company form reads its own company's rows and no one else's.
///
/// Company B holds twice what company A holds. A company form that let the other company's rows
/// in would read the sum rather than its own share.
#[tokio::test]
async fn every_company_form_reads_only_its_own_company() {
    let Some(persistence) = test_persistence().await else {
        return;
    };
    let a = seed_company(&persistence, "scope-a", 1).await;
    let b = seed_company(&persistence, "scope-b", 2).await;
    let window = DashboardWindow::last_hour();

    let mut leaks = Vec::new();
    for (fixture, units) in [(&a, 1), (&b, 2)] {
        let snapshot = persistence
            .dashboard_snapshot(Some(fixture.company), window)
            .await
            .expect("the company forms run");
        for aggregate in &AGGREGATES {
            let (read, expected) = ((aggregate.count)(&snapshot), (aggregate.expected)(units));
            if read != expected {
                leaks.push(format!(
                    "{}: a company seeded with {units} read {read}, expected {expected}",
                    aggregate.name
                ));
            }
        }
    }

    for fixture in [a, b] {
        CompanyPersistence::delete(&persistence, fixture.company)
            .await
            .expect("the fixture company is removed");
    }
    assert!(leaks.is_empty(), "{leaks:#?}");
}

/// Every aggregate's global form reads both companies.
///
/// A lower bound rather than an equality. Other tests' rows share these tables and can only add to
/// what the global form reads, so the global total is never asserted.
#[tokio::test]
async fn every_global_form_reads_both_companies() {
    let Some(persistence) = test_persistence().await else {
        return;
    };
    let a = seed_company(&persistence, "global-a", 1).await;
    let b = seed_company(&persistence, "global-b", 2).await;

    let global = persistence
        .dashboard_snapshot(None, DashboardWindow::last_hour())
        .await
        .expect("the global forms run");
    let mut missing = Vec::new();
    for aggregate in &AGGREGATES {
        let floor = (aggregate.expected)(1) + (aggregate.expected)(2);
        let read = (aggregate.count)(&global);
        if read < floor {
            missing.push(format!(
                "{}: read {read}, but the two companies alone hold {floor}",
                aggregate.name
            ));
        }
    }

    for fixture in [a, b] {
        CompanyPersistence::delete(&persistence, fixture.company)
            .await
            .expect("the fixture company is removed");
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

/// A cached snapshot never serves one company's rollup to another, and never crosses between a
/// company and the global rollup.
///
/// Three views read three different answers. Then company A's data changes under the cache, and
/// all three are read again inside the TTL. Each must come back as its own cached reading: A
/// unchanged, B still B, the global rollup unchanged. The uncached path, which the cache stays out
/// of, sees the change, and so does a view the cache has not read yet.
#[tokio::test]
async fn the_snapshot_cache_never_serves_one_view_to_another() {
    let Some(persistence) = test_persistence().await else {
        return;
    };
    let a = seed_company(&persistence, "cache-a", 1).await;
    let b = seed_company(&persistence, "cache-b", 2).await;
    let service = DashboardSnapshotService::with_long_ttl(Arc::new(persistence.clone()));
    let window = DashboardWindow::last_hour();
    let pending =
        |snapshot: &DashboardSnapshot| snapshot.deliveries.count_of(DeliveryStatus::Pending);
    let read = |company, window| service.snapshot(company, window);

    let first_a = pending(&read(Some(a.company), window).await.unwrap());
    let first_b = pending(&read(Some(b.company), window).await.unwrap());
    let first_global = pending(&read(None, window).await.unwrap());
    assert_eq!((first_a, first_b), (2, 4));
    assert!(first_global >= 6, "the global rollup holds both companies");

    let thread = ThreadPersistence::create_thread(&persistence, a.channel, "Later", &[])
        .await
        .unwrap();
    queue_blocked_delivery(&persistence, &a, thread.id, 9).await;

    assert_eq!(pending(&read(Some(a.company), window).await.unwrap()), 2);
    assert_eq!(pending(&read(Some(b.company), window).await.unwrap()), 4);
    assert_eq!(pending(&read(None, window).await.unwrap()), first_global);
    assert_eq!(
        pending(
            &persistence
                .dashboard_snapshot(Some(a.company), window)
                .await
                .unwrap()
        ),
        4,
        "the uncached path reads through"
    );
    assert_eq!(
        pending(
            &read(Some(a.company), DashboardWindow::last_day())
                .await
                .unwrap()
        ),
        4,
        "the window is part of the key"
    );

    for fixture in [a, b] {
        CompanyPersistence::delete(&persistence, fixture.company)
            .await
            .expect("the fixture company is removed");
    }
}

#[tokio::test]
async fn the_global_snapshot_runs() {
    let Some(persistence) = test_persistence().await else {
        return;
    };

    persistence
        .dashboard_snapshot(None, DashboardWindow::last_hour())
        .await
        .expect("every dashboard query is valid SQL with the parameter types it is bound with");
}

#[tokio::test]
async fn a_company_scoped_snapshot_runs_and_stays_inside_its_company() {
    let Some(persistence) = test_persistence().await else {
        return;
    };

    // A company that owns nothing: every count must be zero. If a company form ever lost its
    // predicate, this would catch it. The global snapshot above cannot, because it looks the
    // same either way.
    let nobody = Uuid::new_v4();
    let snapshot = persistence
        .dashboard_snapshot(Some(nobody), DashboardWindow::last_hour())
        .await
        .expect("the scoped queries run");

    assert_eq!(snapshot.tasks.total(), 0, "{:?}", snapshot.tasks);
    assert_eq!(snapshot.tasks.stalled, 0);
    assert_eq!(snapshot.deliveries.total(), 0, "{:?}", snapshot.deliveries);
    // Gap-filled, so "owns nothing" reads as a full series of zeroes rather than no series at
    // all: `SLOTS_CTE` generates one bucket per slice of the window from `CURRENT_TIMESTAMP`
    // alone, and never sees the company. An emptiness check here would assert the chart has no
    // x-axis.
    assert_eq!(snapshot.throughput_total(), 0, "{:?}", snapshot.throughput);
    assert_eq!(snapshot.attempts, AttemptStats::default());
}
