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
    test_support::{DeliveryFixtureRequest, delivery_fixture, test_machine, test_pool},
};
use crate::entities::task::NewTask;
use crate::entities::task::{
    TaskAttemptOutcome, TaskAttemptRef, TaskAttemptStatus, TaskStopReason, TokenUsage,
};
use crate::services::dashboard_snapshot::DashboardSnapshotService;
use crate::task_queue::TaskPersistence;
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
///
/// The channel has no agent assigned. A task queued on it therefore has no owner, and
/// `claim_pending_tasks` passes over it even while it is `pending` and due, so a neighbouring
/// test's claim cannot move it.
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

async fn queue_task(persistence: &PostgresPersistence, fixture: &CompanyFixture) -> Uuid {
    persistence
        .enqueue_task(NewTask::starting_new_chain(
            fixture.company,
            fixture.channel,
            None,
            "dashboard-probe",
            serde_json::json!({}),
        ))
        .await
        .expect("the fixture task is queued")
        .id
}

/// A company and a task of this test's own, for the attempt-ledger tests below.
///
/// `task_attempts.task_id` is a foreign key, so those tests need a task that exists. They used
/// to take whichever task happened to already be in the database and return early when there
/// was none — which meant they reported success while asserting nothing the moment the table
/// was empty, and the table is empty at the start of every run now.
async fn task_fixture(persistence: &PostgresPersistence, label: &str) -> (Uuid, Uuid) {
    let fixture = company_fixture(persistence, label).await;
    (queue_task(persistence, &fixture).await, fixture.company)
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
async fn seed_company(
    persistence: &PostgresPersistence,
    label: &str,
    units: i64,
    attempts_minutes_ago: i32,
) -> CompanyFixture {
    let fixture = company_fixture(persistence, label).await;
    let thread = ThreadPersistence::create_thread(persistence, fixture.channel, "Dashboard", &[])
        .await
        .expect("the fixture thread is created");

    let mut open = Vec::new();
    let mut finished = Vec::new();
    for unit in 0..units {
        let task = queue_task(persistence, &fixture).await;
        let attempt = TaskAttemptRef {
            task_id: task,
            attempt_number: 1,
            execution_generation: Uuid::new_v4(),
            worker_id: Uuid::new_v4(),
        };
        persistence
            .begin_task_attempt(attempt, &test_machine())
            .await
            .expect("the ledger row opens");
        persistence
            .finish_task_attempt(&TaskAttemptOutcome {
                attempt,
                status: TaskAttemptStatus::Completed,
                stop_reason: TaskStopReason::Completed,
                error: None,
                tokens: None,
            })
            .await
            .expect("the ledger row closes");
        open.push(task);
        finished.push(queue_task(persistence, &fixture).await);
        queue_blocked_delivery(persistence, &fixture, thread.id, unit).await;
    }

    sqlx::query(
        "UPDATE background_tasks
            SET created_at = CURRENT_TIMESTAMP - interval '30 minutes'
          WHERE id = ANY($1)",
    )
    .bind(&open)
    .execute(&persistence.pool)
    .await
    .expect("the open tasks are backdated");
    sqlx::query(
        "UPDATE background_tasks
            SET status = 'failed',
                created_at = CURRENT_TIMESTAMP - interval '50 minutes',
                updated_at = CURRENT_TIMESTAMP - interval '40 minutes'
          WHERE id = ANY($1)",
    )
    .bind(&finished)
    .execute(&persistence.pool)
    .await
    .expect("the finished tasks are backdated");
    // One statement, so every attempt in this company starts at the same instant and falls in one
    // latency bucket.
    sqlx::query(
        "UPDATE task_attempts
            SET started_at = CURRENT_TIMESTAMP - make_interval(mins => $2),
                finished_at = CURRENT_TIMESTAMP - make_interval(mins => $2) + interval '1 second'
          WHERE task_id = ANY($1)",
    )
    .bind(&open)
    .bind(attempts_minutes_ago)
    .execute(&persistence.pool)
    .await
    .expect("the attempts are backdated");

    fixture
}

/// One aggregate, read out of a snapshot as a count of [`seed_company`]'s rows.
struct Aggregate {
    name: &'static str,
    count: fn(&DashboardSnapshot) -> i64,
    /// What `count` reads for a company seeded with this many units.
    expected: fn(i64) -> i64,
}

/// Every aggregate in the snapshot, each measured by the rows its own statement reads.
///
/// Latency counts buckets that have a percentile. Each company's attempts share one bucket, and
/// the two companies' buckets are twenty minutes apart. A company form that let the other
/// company in would therefore have two such buckets.
const AGGREGATES: [Aggregate; 10] = [
    Aggregate {
        name: "task queue",
        count: |snapshot| snapshot.tasks.count_of(TaskStatus::Pending),
        expected: |units| units,
    },
    Aggregate {
        name: "task pressure",
        count: |snapshot| snapshot.tasks.due_now,
        expected: |units| units,
    },
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
    Aggregate {
        name: "throughput",
        count: DashboardSnapshot::throughput_total,
        expected: |units| units,
    },
    Aggregate {
        name: "latency",
        count: |snapshot| {
            snapshot
                .latency
                .iter()
                .filter(|bucket| bucket.p50_ms.is_some())
                .count() as i64
        },
        expected: |_| 1,
    },
    Aggregate {
        name: "queue depth",
        count: |snapshot| snapshot.queue_depth.last().map_or(0, |bucket| bucket.open),
        expected: |units| units,
    },
    Aggregate {
        name: "attempt stats",
        count: |snapshot| snapshot.attempts.attempts,
        expected: |units| units,
    },
    Aggregate {
        name: "retry rate",
        count: |snapshot| {
            snapshot
                .retry_rate
                .iter()
                .map(|bucket| bucket.attempts)
                .sum()
        },
        expected: |units| units,
    },
    Aggregate {
        name: "outstanding",
        count: |snapshot| snapshot.outstanding.len() as i64,
        expected: |units| units,
    },
];

/// Every statement's two forms, for the checks that need no database.
fn scoped_statements() -> [(&'static str, &'static ScopedSql); 10] {
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
    let a = seed_company(&persistence, "scope-a", 1, 20).await;
    let b = seed_company(&persistence, "scope-b", 2, 40).await;
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
        if let Some(stray) = snapshot
            .outstanding
            .iter()
            .find(|task| task.company_id != fixture.company)
        {
            leaks.push(format!(
                "outstanding: listed {stray:?} under another company"
            ));
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
    let a = seed_company(&persistence, "global-a", 1, 20).await;
    let b = seed_company(&persistence, "global-b", 2, 40).await;

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
    let a = seed_company(&persistence, "cache-a", 1, 20).await;
    let b = seed_company(&persistence, "cache-b", 2, 40).await;
    let service = DashboardSnapshotService::with_long_ttl(Arc::new(persistence.clone()));
    let window = DashboardWindow::last_hour();
    let pending = |snapshot: &DashboardSnapshot| snapshot.tasks.count_of(TaskStatus::Pending);
    let read = |company, window| service.snapshot(company, window);

    let first_a = pending(&read(Some(a.company), window).await.unwrap());
    let first_b = pending(&read(Some(b.company), window).await.unwrap());
    let first_global = pending(&read(None, window).await.unwrap());
    assert_eq!((first_a, first_b), (1, 2));
    assert!(first_global >= 3, "the global rollup holds both companies");

    queue_task(&persistence, &a).await;

    assert_eq!(pending(&read(Some(a.company), window).await.unwrap()), 1);
    assert_eq!(pending(&read(Some(b.company), window).await.unwrap()), 2);
    assert_eq!(pending(&read(None, window).await.unwrap()), first_global);
    assert_eq!(
        pending(
            &persistence
                .dashboard_snapshot(Some(a.company), window)
                .await
                .unwrap()
        ),
        2,
        "the uncached path reads through"
    );
    assert_eq!(
        pending(
            &read(Some(a.company), DashboardWindow::last_day())
                .await
                .unwrap()
        ),
        2,
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

/// Reads the percentiles with a *finished* attempt in the window.
///
/// [`the_global_snapshot_runs`] cannot catch a wrong percentile column type on its own: with
/// nothing finished the value is `NULL`, and a `NULL` is never decoded. `extract(epoch ...)`
/// returns `numeric`, so without the cast in [`ATTEMPT_STATS_SQL`] this is where it shows.
#[tokio::test]
async fn finished_attempts_decode_their_latency_percentiles() {
    let Some(persistence) = test_persistence().await else {
        return;
    };

    let (task_id, company) = task_fixture(&persistence, "latency").await;

    let attempt = TaskAttemptRef {
        task_id,
        attempt_number: 9_998,
        execution_generation: Uuid::new_v4(),
        worker_id: Uuid::new_v4(),
    };
    persistence
        .begin_task_attempt(attempt, &test_machine())
        .await
        .expect("the ledger row opens");
    persistence
        .finish_task_attempt(&TaskAttemptOutcome {
            attempt,
            status: TaskAttemptStatus::Completed,
            stop_reason: TaskStopReason::Completed,
            error: None,
            tokens: Some(TokenUsage::new(3, 5)),
        })
        .await
        .expect("the ledger row closes");

    // Scoped to this task's company so a neighbouring test cannot empty the window from under
    // it — the assertion needs at least one finished attempt to be there.
    let snapshot = persistence
        .dashboard_snapshot(Some(company), DashboardWindow::last_hour())
        .await
        .expect("the percentile columns decode once something has finished");

    assert!(
        snapshot.attempts.p50_ms.is_some(),
        "a finished attempt must produce a latency: {:?}",
        snapshot.attempts
    );

    CompanyPersistence::delete(&persistence, company)
        .await
        .expect("the fixture company is removed");
}

#[tokio::test]
async fn retry_rate_counts_only_attempt_numbers_above_one_and_stays_company_scoped() {
    let Some(persistence) = test_persistence().await else {
        return;
    };
    let (task_id, company) = task_fixture(&persistence, "retry-rate").await;

    for attempt_number in [1, 2] {
        persistence
            .begin_task_attempt(
                TaskAttemptRef {
                    task_id,
                    attempt_number,
                    execution_generation: Uuid::new_v4(),
                    worker_id: Uuid::new_v4(),
                },
                &test_machine(),
            )
            .await
            .expect("the attempt starts");
    }

    for window in DashboardWindow::PRESETS {
        let snapshot = persistence
            .dashboard_snapshot(Some(company), window)
            .await
            .expect("the scoped attempt aggregates run");
        assert_eq!(snapshot.attempts.attempts, 2);
        assert_eq!(snapshot.attempts.retries, 1);
        assert_eq!(snapshot.attempts.retry_rate_percent(), Some(50.0));
        assert_eq!(snapshot.retry_rate.len() as i64, window.bucket_count());
        assert!(snapshot.retry_rate.iter().any(|bucket| {
            bucket.attempts == 2 && bucket.retries == 1 && bucket.rate_percent() == Some(50.0)
        }));
    }

    let unrelated = persistence
        .dashboard_snapshot(Some(Uuid::new_v4()), DashboardWindow::last_hour())
        .await
        .expect("an unrelated company scope runs");
    assert_eq!(unrelated.attempts, AttemptStats::default());
    assert!(
        unrelated
            .retry_rate
            .iter()
            .all(|bucket| bucket.attempts == 0)
    );

    CompanyPersistence::delete(&persistence, company)
        .await
        .expect("the fixture company is removed");
}

#[tokio::test]
async fn an_attempt_is_ledgered_and_can_be_reopened_by_a_re_run() {
    let Some(persistence) = test_persistence().await else {
        return;
    };

    let (task_id, company) = task_fixture(&persistence, "ledger").await;

    // A number far above any real retry count, so this test cannot collide with live rows.
    let attempt = TaskAttemptRef {
        task_id,
        attempt_number: 9_999,
        execution_generation: Uuid::new_v4(),
        worker_id: Uuid::new_v4(),
    };

    persistence
        .begin_task_attempt(attempt, &test_machine())
        .await
        .expect("the ledger row opens");

    let outcome = TaskAttemptOutcome {
        attempt,
        status: TaskAttemptStatus::Completed,
        stop_reason: TaskStopReason::Completed,
        error: None,
        tokens: Some(TokenUsage::new(11, 7)),
    };
    assert!(
        persistence
            .finish_task_attempt(&outcome)
            .await
            .expect("the ledger row closes"),
        "closing a row that was just opened must report that it wrote"
    );

    // Closing twice must not write twice: the second call finds no open row, which is the same
    // guard that stops a superseded run from overwriting the run that took its task over.
    assert!(
        !persistence
            .finish_task_attempt(&outcome)
            .await
            .expect("the second close runs"),
        "an already-closed attempt must not be closed again"
    );

    // A task re-claimed after its lease lapsed comes back with the same attempt number. The
    // conflict must reopen the row rather than fail the insert.
    let replacement = TaskAttemptRef {
        execution_generation: Uuid::new_v4(),
        worker_id: Uuid::new_v4(),
        ..attempt
    };
    persistence
        .begin_task_attempt(replacement, &test_machine())
        .await
        .expect("a re-run reopens the same attempt rather than colliding with it");

    assert!(
        !persistence
            .finish_task_attempt(&outcome)
            .await
            .expect("the stale execution can report without writing"),
        "the execution replaced during reclaim must not finish the new ledger row"
    );
    assert!(
        persistence
            .finish_task_attempt(&TaskAttemptOutcome {
                attempt: replacement,
                ..outcome.clone()
            })
            .await
            .expect("the replacement execution closes"),
        "the current execution generation must still be able to finish"
    );

    let reopened: (String, Option<i32>, Option<String>, Uuid) = sqlx::query_as(
        "SELECT status, prompt_tokens, stop_reason, worker_id FROM task_attempts WHERE task_id = $1 AND attempt_number = $2",
    )
    .bind(task_id)
    .bind(9_999_i32)
    .fetch_one(persistence.pool())
    .await
    .expect("the reopened row is readable");

    assert_eq!(
        reopened.0,
        TaskAttemptStatus::Completed.as_str(),
        "the replacement run finished"
    );
    assert_eq!(reopened.1, Some(11));
    assert_eq!(
        reopened.2.as_deref(),
        Some(TaskStopReason::Completed.as_str())
    );
    assert_eq!(
        reopened.3, replacement.worker_id,
        "reopening the row hands it to the run that reclaimed the task, not the one that vanished"
    );

    CompanyPersistence::delete(&persistence, company)
        .await
        .expect("the fixture company is removed");
}
