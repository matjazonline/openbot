//! The pending-task claim: which tasks one batch takes, and which it never takes.
//!
//! Every test here runs against a database of its own, because the claim sweeps every row of the
//! database it runs against. With nothing else in there, a test queues exactly the tasks it means
//! to reason about, states their ages, and asserts exactly which ones came back.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use super::tests::{seed_channel_agent, seed_company_and_channel};
use super::*;
use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::test_support::own_database;
use crate::entities::task::NewTask;

fn lease() -> DateTime<Utc> {
    Utc::now() + chrono::Duration::minutes(5)
}

/// Queue an agent task that fell due `minutes_ago` minutes back. Stating each task's age states
/// the order the claim must see them in.
async fn queue_due(
    persistence: &PostgresPersistence,
    pool: &PgPool,
    company_id: Uuid,
    channel_id: Uuid,
    minutes_ago: i32,
) -> Uuid {
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company_id,
            channel_id,
            None,
            "claim",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE background_tasks
            SET run_at = CURRENT_TIMESTAMP - make_interval(mins => $2)
          WHERE id = $1",
    )
    .bind(task.id)
    .bind(minutes_ago)
    .execute(pool)
    .await
    .unwrap();
    task.id
}

/// Queue `count` due tasks in one statement, for a test that needs a backlog rather than named
/// tasks. The insert trigger gives each one the channel's agent, exactly as `enqueue_task` does.
async fn queue_backlog(pool: &PgPool, company_id: Uuid, channel_id: Uuid, count: i32) {
    sqlx::query(
        "INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, task_type,
                                       status, run_at)
         SELECT gen_random_uuid(), $1, $2, gen_random_uuid(), 'claim', 'pending',
                CURRENT_TIMESTAMP - make_interval(mins => queued)
           FROM generate_series(1, $3) AS queued",
    )
    .bind(company_id)
    .bind(channel_id)
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

/// Put `tasks` into `processing` under a lease ending at `lease_expires_at`, as a claim would.
async fn mark_running(pool: &PgPool, tasks: &[Uuid], lease_expires_at: DateTime<Utc>) {
    sqlx::query(
        "UPDATE background_tasks
            SET status = 'processing', worker_id = gen_random_uuid(),
                execution_generation = gen_random_uuid(),
                locked_at = $2 - INTERVAL '10 minutes', lock_expires_at = $2,
                transition_reason = 'claimed', transition_actor_kind = 'worker',
                transition_actor_id = gen_random_uuid(), transition_approval_id = NULL,
                transition_outreach_id = NULL, updated_at = CURRENT_TIMESTAMP
          WHERE id = ANY($1)",
    )
    .bind(tasks)
    .bind(lease_expires_at)
    .execute(pool)
    .await
    .unwrap();
}

fn ids(tasks: &[BackgroundTask]) -> HashSet<Uuid> {
    tasks.iter().map(|task| task.id).collect()
}

async fn principal_of_agent(pool: &PgPool, company_id: Uuid, agent_id: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2")
        .bind(company_id)
        .bind(agent_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_batch_takes_every_waiting_company_before_a_second_task_from_a_deep_backlog() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    let (backlog, backlog_channel) = seed_company_and_channel(&persistence).await;
    let (first, first_channel) = seed_company_and_channel(&persistence).await;
    let (second, second_channel) = seed_company_and_channel(&persistence).await;

    // Five backlog tasks, every one older than either waiting company's only task. Taking the
    // oldest tasks in the queue would fill the batch from the backlog alone.
    let mut backlog_tasks = Vec::new();
    for minutes_ago in [60, 59, 58, 57, 56] {
        backlog_tasks.push(
            queue_due(
                &persistence,
                &pool,
                backlog.id,
                backlog_channel.id,
                minutes_ago,
            )
            .await,
        );
    }
    let waiting_first = queue_due(&persistence, &pool, first.id, first_channel.id, 10).await;
    let waiting_second = queue_due(&persistence, &pool, second.id, second_channel.id, 9).await;

    let claimed = persistence
        .claim_pending_tasks(Uuid::new_v4(), lease(), 3)
        .await
        .unwrap();

    assert_eq!(
        ids(&claimed),
        HashSet::from([backlog_tasks[0], waiting_first, waiting_second]),
        "a batch of three takes the backlog's oldest and each waiting company's only task"
    );
}

/// Nothing competes for the batch, so every place in it goes to the one company that has work.
#[tokio::test]
async fn a_company_alone_with_work_fills_the_whole_batch() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let mut queued = Vec::new();
    for minutes_ago in [60, 59, 58, 57] {
        queued.push(queue_due(&persistence, &pool, company.id, channel.id, minutes_ago).await);
    }

    let claimed = persistence
        .claim_pending_tasks(Uuid::new_v4(), lease(), 3)
        .await
        .unwrap();

    assert_eq!(
        ids(&claimed),
        HashSet::from([queued[0], queued[1], queued[2]]),
        "one company's oldest three, rather than one task and two idle slots"
    );
}

/// The index the claim reads holds agent-owned rows only, so a person's task or an unowned one
/// never reaches it. Both are older than the agent's task, and the batch has room for all three.
#[tokio::test]
async fn a_task_owned_by_a_person_or_by_nobody_is_never_claimed() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let person: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company.id)
            .bind(company.user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let parked = queue_due(&persistence, &pool, company.id, channel.id, 60).await;
    let unowned = queue_due(&persistence, &pool, company.id, channel.id, 59).await;
    let agent_task = queue_due(&persistence, &pool, company.id, channel.id, 58).await;
    sqlx::query(
        "UPDATE background_tasks SET owner_principal_id = $2, owner_principal_kind = 'person'
          WHERE id = $1",
    )
    .bind(parked)
    .bind(person)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE background_tasks SET owner_principal_id = NULL, owner_principal_kind = NULL
          WHERE id = $1",
    )
    .bind(unowned)
    .execute(&pool)
    .await
    .unwrap();

    let claimed = persistence
        .claim_pending_tasks(Uuid::new_v4(), lease(), 3)
        .await
        .unwrap();

    assert_eq!(ids(&claimed), HashSet::from([agent_task]));
}

/// An agent may own a task on a channel it is no longer assigned to. It must not run it.
#[tokio::test]
async fn a_task_whose_agent_is_not_assigned_to_its_channel_is_never_claimed() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let outsider = seed_channel_agent(&persistence, company.id, "outsider").await;
    let outsider_principal = principal_of_agent(&pool, company.id, outsider).await;
    let stranded = queue_due(&persistence, &pool, company.id, channel.id, 60).await;
    let assigned = queue_due(&persistence, &pool, company.id, channel.id, 59).await;
    sqlx::query("UPDATE background_tasks SET owner_principal_id = $2 WHERE id = $1")
        .bind(stranded)
        .bind(outsider_principal)
        .execute(&pool)
        .await
        .unwrap();

    let claimed = persistence
        .claim_pending_tasks(Uuid::new_v4(), lease(), 3)
        .await
        .unwrap();

    assert_eq!(ids(&claimed), HashSet::from([assigned]));
}

/// A free slot goes to the company with the fewest tasks running, not to the oldest task.
///
/// Under load the worker claims one slot at a time, and taking the oldest due task then served a
/// busy company's whole backlog before an idle company's newer task. Here the busy company has two
/// tasks running and three waiting, every one older than the idle company's only task.
#[tokio::test]
async fn a_free_slot_goes_to_the_company_with_the_fewest_tasks_running() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    let (busy, busy_channel) = seed_company_and_channel(&persistence).await;
    let (idle, idle_channel) = seed_company_and_channel(&persistence).await;
    let mut busy_tasks = Vec::new();
    for minutes_ago in [60, 59, 58, 57, 56] {
        busy_tasks
            .push(queue_due(&persistence, &pool, busy.id, busy_channel.id, minutes_ago).await);
    }
    let waiting = queue_due(&persistence, &pool, idle.id, idle_channel.id, 10).await;
    mark_running(&pool, &busy_tasks[..2], lease()).await;

    let claimed = persistence
        .claim_pending_tasks(Uuid::new_v4(), lease(), 1)
        .await
        .unwrap();

    assert_eq!(
        ids(&claimed),
        HashSet::from([waiting]),
        "the idle company's newer task goes before a third task for the busy company"
    );
}

/// With the same number running, the oldest waiting task wins.
#[tokio::test]
async fn companies_running_the_same_number_of_tasks_are_served_oldest_first() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    let (early, early_channel) = seed_company_and_channel(&persistence).await;
    let (late, late_channel) = seed_company_and_channel(&persistence).await;
    let early_running = queue_due(&persistence, &pool, early.id, early_channel.id, 90).await;
    let late_running = queue_due(&persistence, &pool, late.id, late_channel.id, 80).await;
    let early_waiting = queue_due(&persistence, &pool, early.id, early_channel.id, 30).await;
    queue_due(&persistence, &pool, late.id, late_channel.id, 20).await;
    mark_running(&pool, &[early_running, late_running], lease()).await;

    let claimed = persistence
        .claim_pending_tasks(Uuid::new_v4(), lease(), 1)
        .await
        .unwrap();

    assert_eq!(ids(&claimed), HashSet::from([early_waiting]));
}

/// An expired lease's worker is gone, so its task is not one the company has running.
///
/// Counted, the two abandoned tasks would put `stalled` two places behind `other`, whose task is
/// newer, and `other` would win.
#[tokio::test]
async fn an_expired_lease_does_not_count_as_a_running_task() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    let (stalled, stalled_channel) = seed_company_and_channel(&persistence).await;
    let (other, other_channel) = seed_company_and_channel(&persistence).await;
    let abandoned = [
        queue_due(&persistence, &pool, stalled.id, stalled_channel.id, 60).await,
        queue_due(&persistence, &pool, stalled.id, stalled_channel.id, 59).await,
    ];
    let next = queue_due(&persistence, &pool, stalled.id, stalled_channel.id, 58).await;
    queue_due(&persistence, &pool, other.id, other_channel.id, 57).await;
    mark_running(&pool, &abandoned, Utc::now() - chrono::Duration::minutes(1)).await;

    let claimed = persistence
        .claim_pending_tasks(Uuid::new_v4(), lease(), 1)
        .await
        .unwrap();

    assert_eq!(ids(&claimed), HashSet::from([next]));
}

/// Workers on several machines, one queue, nothing staged: no task may reach two of them.
///
/// The test below pins the double claim deterministically, by holding one claim open at a known
/// point with a lock and a planner setting. This one keeps the shape production has -- machines
/// polling the same queue, each taking what its free slots allow -- and asserts the invariant that
/// matters. It cannot prove the window is hit on any given run, which is why the deterministic
/// test stays; what it does is fail if real contention ever hands one task to two workers.
///
/// The status ledger cannot answer this. `record_task_status_event` returns early when the status
/// does not change, so a second claim of an already-`processing` row writes no event at all --
/// which is how `concurrent_workers_claim_once_and_a_failed_task_is_not_immediately_reclaimed`
/// stayed green for as long as the race was live. What each claim returned is the evidence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_workers_never_receive_the_same_task_twice() {
    const WORKERS: usize = 8;
    const ROUNDS: usize = 15;
    const SLOTS: i64 = 2;

    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    for _ in 0..3 {
        let (company, channel) = seed_company_and_channel(&persistence).await;
        queue_backlog(&pool, company.id, channel.id, 40).await;
    }

    // Every worker's claim must be in flight at once, which takes a connection each. A test's own
    // database comes with a deliberately small pool, so the fan-out gets a pool of its own.
    let workers_pool = PgPoolOptions::new()
        .max_connections(WORKERS as u32)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();

    let mut handed_out: Vec<Uuid> = Vec::new();
    for _ in 0..ROUNDS {
        let round = (0..WORKERS).map(|_| {
            let worker = PostgresPersistence::new(workers_pool.clone());
            async move {
                worker
                    .claim_pending_tasks(Uuid::new_v4(), lease(), SLOTS)
                    .await
                    .unwrap()
            }
        });
        for claimed in futures::future::join_all(round).await {
            handed_out.extend(claimed.iter().map(|task| task.id));
        }
    }

    workers_pool.close().await;

    let distinct: HashSet<Uuid> = handed_out.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        handed_out.len(),
        "the claim handed one task to two workers"
    );
    assert!(
        handed_out.len() >= ROUNDS,
        "the workers claimed {} tasks in {ROUNDS} rounds, too few to have contended at all",
        handed_out.len()
    );
}

/// A claim whose snapshot still shows a task as pending must not take it once another worker's
/// claim of that task has committed.
///
/// The claim chooses its candidates under one snapshot and locks them afterwards. When a candidate
/// is claimed and committed by another worker in between, Postgres hands the locking step the
/// task's new `processing` version and re-checks only the conditions written against the table
/// being locked. The pending check used to live only in the candidate subquery, so nothing
/// re-checked it: the second worker took the task again and overwrote its execution generation,
/// so two workers ran it and only one could finish.
///
/// The window has to be held open between the second claim's snapshot and its lock on the task.
/// The claim's own trigger does that. `lock_task_agent_harnesses` takes `FOR SHARE` on the agent
/// of every task being claimed, so a transaction holding one agent `FOR UPDATE` stops the claim
/// part-way through its batch: after its snapshot, holding its first task, its second not yet
/// locked.
#[tokio::test]
async fn a_claim_never_takes_a_task_another_worker_claimed_after_its_snapshot() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    // One task per company, so all three are first in their company and sort by age alone.
    let (first, first_channel) = seed_company_and_channel(&persistence).await;
    let (second, second_channel) = seed_company_and_channel(&persistence).await;
    let (third, third_channel) = seed_company_and_channel(&persistence).await;
    let paused_on = queue_due(&persistence, &pool, first.id, first_channel.id, 60).await;
    let contested = queue_due(&persistence, &pool, second.id, second_channel.id, 59).await;
    let spare = queue_due(&persistence, &pool, third.id, third_channel.id, 58).await;
    let paused_agent: Uuid = sqlx::query_scalar(
        "SELECT agent_id FROM channel_agents WHERE company_id = $1 AND channel_id = $2",
    )
    .bind(first.id)
    .bind(first_channel.id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let mut blocker = pool.begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT 1 FROM agents WHERE id = $1 FOR UPDATE")
        .bind(paused_agent)
        .execute(&mut *blocker)
        .await
        .unwrap();

    // The paused worker's `UPDATE` must pull its claimed tasks one at a time, so that it stops on
    // the first before locking the second. A hash join built on the claimed set would lock the
    // whole batch up front and close the window, and on a near-empty database that is the plan the
    // planner picks. So this worker's connection plans without one. The setting orders execution;
    // it does not change which conditions are re-checked.
    let paused_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            (*pool.connect_options())
                .clone()
                .options([("enable_hashjoin", "off"), ("enable_mergejoin", "off")]),
        )
        .await
        .unwrap();
    let paused_worker = Uuid::new_v4();
    let paused = tokio::spawn({
        let persistence = PostgresPersistence::new(paused_pool.clone());
        async move {
            persistence
                .claim_pending_tasks(paused_worker, lease(), 2)
                .await
        }
    });
    wait_until_blocked_by(&pool, blocker_pid).await;

    // Meanwhile another worker passes over the locked `paused_on`, claims `contested` and commits.
    let racing_worker = Uuid::new_v4();
    let raced = persistence
        .claim_pending_tasks(racing_worker, lease(), 1)
        .await
        .unwrap();
    assert_eq!(ids(&raced), HashSet::from([contested]));

    blocker.rollback().await.unwrap();
    let resumed = ids(&paused.await.unwrap().unwrap());
    paused_pool.close().await;

    assert!(
        !resumed.contains(&contested),
        "the paused worker re-claimed a task another worker already held"
    );
    assert_eq!(
        resumed,
        HashSet::from([paused_on, spare]),
        "the paused worker moves past the task it can no longer have"
    );
    let holder: Option<Uuid> =
        sqlx::query_scalar("SELECT worker_id FROM background_tasks WHERE id = $1")
            .bind(contested)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        holder,
        Some(racing_worker),
        "the task stays with the worker that claimed it first"
    );
}

/// A task another worker is already running is claimable by nobody.
///
/// Both claims require the task to be pending, and this holds them to it from the outside. Neither
/// may take a task that carries a live lease, and neither may overwrite the execution generation
/// that fences the worker running it -- the overwrite is what made the double claim harmful, since
/// it left the first worker unable to complete the task it was still running.
#[tokio::test]
async fn a_task_with_a_live_lease_is_claimable_by_nobody() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let running = queue_due(&persistence, &pool, company.id, channel.id, 60).await;
    mark_running(&pool, &[running], lease()).await;
    let fence = generation_of(&pool, running).await;

    let batch = persistence
        .claim_pending_tasks(Uuid::new_v4(), lease(), 3)
        .await
        .unwrap();
    let by_id = persistence
        .claim_task(running, Uuid::new_v4(), lease())
        .await
        .unwrap();

    assert!(
        batch.is_empty(),
        "the batch claim took a task that is already running"
    );
    assert!(
        !by_id,
        "the by-id claim took a task that is already running"
    );
    assert_eq!(
        generation_of(&pool, running).await,
        fence,
        "a claim overwrote the execution generation that fences the running worker"
    );
}

/// Every statement that hands a task a new execution generation must require it to be pending.
///
/// Two do today: the batch claim and the by-id claim. A third is how the double claim comes back,
/// because a task that is already `processing` carries a worker running it right now. The tests
/// above pin the two statements that exist; this one notices a new one, including on a path no
/// test of its own exercises yet.
#[test]
fn every_statement_that_claims_a_task_requires_it_to_be_pending() {
    const ASSIGNMENT: &str = "execution_generation = gen_random_uuid()";
    let sources = production_sources();
    assert!(
        sources.len() > 100,
        "the scan found only {} files; it is not looking where it thinks it is",
        sources.len()
    );

    let mut found = 0;
    let mut unguarded = Vec::new();
    for (file, source) in &sources {
        for (index, _) in source.match_indices(ASSIGNMENT) {
            found += 1;
            if !enclosing_statement(source, index).contains("status = 'pending'") {
                unguarded.push(format!("{file}:{}", source[..index].lines().count()));
            }
        }
    }

    assert!(
        found >= 2,
        "the scan found {found} statements assigning an execution generation; the claims it is \
         meant to watch have moved or been renamed"
    );
    assert!(
        unguarded.is_empty(),
        "these statements give a task a new execution generation without requiring it to be \
         pending, so they can take a task another worker is already running and fence that worker \
         out of its own completion: {unguarded:?}"
    );
}

/// The raw string literal `index` falls inside, which for these statements is the whole SQL.
fn enclosing_statement(source: &str, index: usize) -> &str {
    let start = source[..index].rfind("r#\"").map_or(0, |at| at + 3);
    let end = source[index..]
        .find("\"#")
        .map_or(source.len(), |at| index + at);
    &source[start..end]
}

/// Every non-test Rust source in the crate, keyed by its path relative to `src/`.
///
/// Test-only files and inline `#[cfg(test)]` modules are left out: a fixture writes rows the
/// production paths are forbidden to write, which is what makes it a fixture.
fn production_sources() -> Vec<(String, String)> {
    fn is_test_only(path: &std::path::Path) -> bool {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        name == "tests.rs" || name == "test_support.rs" || name.ends_with("_tests.rs")
    }

    fn before_the_test_module(source: &str) -> String {
        let mut lines = source.lines().peekable();
        let mut kept = Vec::new();
        while let Some(line) = lines.next() {
            if line.trim_start().starts_with("#[cfg(test)]")
                && lines
                    .peek()
                    .is_some_and(|next| next.trim_start().starts_with("mod "))
            {
                break;
            }
            kept.push(line);
        }
        kept.join("\n")
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    let mut pending = vec![root.clone()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("the crate source is readable") {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") && !is_test_only(&path) {
                let relative = path
                    .strip_prefix(&root)
                    .expect("every source is under src/")
                    .to_string_lossy()
                    .replace('\\', "/");
                let source = std::fs::read_to_string(&path).expect("a readable source file");
                sources.push((relative, before_the_test_module(&source)));
            }
        }
    }
    sources
}

async fn generation_of(pool: &PgPool, task: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT execution_generation FROM background_tasks WHERE id = $1")
        .bind(task)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Wait until some backend is waiting on a lock `blocker_pid` holds.
async fn wait_until_blocked_by(pool: &PgPool, blocker_pid: i32) {
    for _ in 0..500 {
        let blocked: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid)))",
        )
        .bind(blocker_pid)
        .fetch_one(pool)
        .await
        .unwrap();
        if blocked {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the paused claim never reached the agent lock");
}
