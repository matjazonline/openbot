//! `memory_storage_` indexing protocol over PostgreSQL (D7, D13): competing claimants on separate
//! connections, fences against stale commits, lease expiry, heartbeat loss, the local deadline
//! and tenant state at commit. Require MEMORY_TEST_DATABASE_URL.
use super::storage_tests::{Fixture, context, correct, exact, fixture_with, save};
use crate::application::{
    indexer::{Indexer, IndexerSettings},
    ports::{IndexCommit, IndexJob},
    test_embedder::{FAKE_MODEL, FakeEmbedder, fake_pool_with},
};
use serde_json::json;
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use uuid::Uuid;

const LEASE: Duration = Duration::from_secs(60);

async fn fixture_delay(delay: Duration) -> Fixture {
    let mut fake = FakeEmbedder::new(FAKE_MODEL);
    fake.delay = delay;
    fixture_with(fake_pool_with(Arc::new(fake)), None).await
}

async fn seed(f: &Fixture, count: usize) -> (crate::application::service::Principal, Vec<Uuid>) {
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    let saved = f
        .run(
            &client,
            "observations.save",
            save("seed", "a", context(json!(3), exact(3)), count),
        )
        .await
        .unwrap();
    let ids = saved["observations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| serde_json::from_value(o["observation_id"].clone()).unwrap())
        .collect();
    (client, ids)
}

fn indexer(f: &Fixture, settings: IndexerSettings) -> Indexer {
    Indexer::new(f.app.store().clone(), f.app.embeddings().clone(), settings)
}

async fn parts_of(f: &Fixture, id: Uuid) -> Vec<(i64, i32)> {
    sqlx::query_as("SELECT revision, ordinal FROM observation_parts WHERE observation_id = $1 ORDER BY ordinal")
        .bind(id)
        .fetch_all(f.db.pool())
        .await
        .unwrap()
}

/// Competing claimants on separate connections never share a row, and competing indexers
/// index every row exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_storage_competing_indexers_claim_disjoint_rows_and_index_each_once() {
    let f = fixture_delay(Duration::ZERO).await;
    let (_, ids) = seed(&f, 40).await;
    let claims: Vec<Vec<IndexJob>> = futures_join(
        (0..4)
            .map(|_| {
                let store = f.app.store().clone();
                tokio::spawn(async move { store.claim_index_jobs(8, LEASE).await.unwrap() })
            })
            .collect(),
    )
    .await;
    let claimed: Vec<Uuid> = claims
        .iter()
        .flatten()
        .map(|job| job.observation_id)
        .collect();
    assert_eq!(claimed.len(), 32, "each claimant took a full batch");
    assert_eq!(
        BTreeSet::from_iter(&claimed).len(),
        32,
        "no row was claimed twice"
    );
    // Release those leases, then let four indexers race over everything.
    sqlx::query("UPDATE observations SET index_lease_until = now() - interval '1 second'")
        .execute(f.db.pool())
        .await
        .unwrap();
    let passes = futures_join(
        (0..4)
            .map(|_| {
                let worker = indexer(&f, IndexerSettings::default());
                tokio::spawn(async move {
                    let mut indexed = 0;
                    loop {
                        let pass = worker.run_once().await.unwrap();
                        if pass.claimed == 0 {
                            return indexed;
                        }
                        indexed += pass.indexed;
                    }
                })
            })
            .collect(),
    )
    .await;
    assert_eq!(passes.iter().sum::<usize>(), 40, "{passes:?}");
    for id in ids {
        assert_eq!(parts_of(&f, id).await, vec![(1, 0)]);
    }
    assert_eq!(
        f.scalar("SELECT count(*) FROM observations WHERE index_state <> 'indexed'")
            .await,
        0
    );
}

async fn futures_join<T: Send + 'static>(handles: Vec<tokio::task::JoinHandle<T>>) -> Vec<T> {
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }
    results
}

/// A correction moves the fence: the earlier claim commits nothing and loses its heartbeat; an
/// expired lease is reclaimed under a new fence and the old holder's commit is stale.
#[tokio::test]
async fn memory_storage_index_fences_reject_stale_commits_and_expired_leases() {
    let f = fixture_delay(Duration::ZERO).await;
    let (client, ids) = seed(&f, 1).await;
    let id = ids[0];
    let store = f.app.store().clone();
    let first = store.claim_index_jobs(8, LEASE).await.unwrap().remove(0);
    let parts = f
        .app
        .embeddings()
        .embed_parts(first.title.clone(), first.body.clone(), Default::default())
        .await
        .unwrap()
        .unwrap();
    f.run(
        &client,
        "observations.correct",
        correct("c", "a", id, 1, 9, context(json!(3), exact(3))),
    )
    .await
    .unwrap();
    assert_eq!(
        store
            .commit_index(&first, FAKE_MODEL, &parts)
            .await
            .unwrap(),
        IndexCommit::Stale
    );
    assert!(
        store
            .renew_index_leases(std::slice::from_ref(&first), LEASE)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(parts_of(&f, id).await.is_empty());
    // The corrected text is claimed under a new fence; let its lease expire and reclaim it.
    let second = store.claim_index_jobs(8, LEASE).await.unwrap().remove(0);
    assert_eq!((second.revision, second.body.as_str()), (2, "new text"));
    assert!(second.fence > first.fence);
    sqlx::query(
        "UPDATE observations SET index_lease_until = now() - interval '1 second' WHERE id = $1",
    )
    .bind(id)
    .execute(f.db.pool())
    .await
    .unwrap();
    let third = store.claim_index_jobs(8, LEASE).await.unwrap().remove(0);
    assert!(third.fence > second.fence);
    let parts = f
        .app
        .embeddings()
        .embed_parts(third.title.clone(), third.body.clone(), Default::default())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        store
            .commit_index(&second, FAKE_MODEL, &parts)
            .await
            .unwrap(),
        IndexCommit::Stale
    );
    assert_eq!(
        store
            .commit_index(&third, FAKE_MODEL, &parts)
            .await
            .unwrap(),
        IndexCommit::Indexed
    );
    assert_eq!(parts_of(&f, id).await, vec![(2, 0)]);
    // Deleting the observation removes its parts with it.
    f.run(
        &client,
        "observations.delete",
        super::storage_tests::delete("d", "a", id, 9),
    )
    .await
    .unwrap();
    assert_eq!(f.scalar("SELECT count(*) FROM observation_parts").await, 0);
}

/// The worker stops at its local deadline without renewals, keeps going while its heartbeat
/// renews, and abandons work when a heartbeat finds the fence moved.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_storage_indexer_deadline_heartbeat_and_heartbeat_loss() {
    let f = fixture_delay(Duration::from_millis(1500)).await;
    let (client, ids) = seed(&f, 1).await;
    let id = ids[0];
    // No renewal before the local deadline (lease 2 s − margin 1 s): the work is dropped.
    let stalled = IndexerSettings {
        lease: Duration::from_secs(2),
        heartbeat: Duration::from_secs(30),
        margin: Duration::from_secs(1),
        ..IndexerSettings::default()
    };
    let pass = indexer(&f, stalled).run_once().await.unwrap();
    assert_eq!(
        (pass.claimed, pass.abandoned, pass.indexed),
        (1, 1, 0),
        "{pass:?}"
    );
    assert!(parts_of(&f, id).await.is_empty());
    // With renewals every 200 ms the same work outlives the original deadline and commits.
    sqlx::query("UPDATE observations SET index_lease_until = now() - interval '1 second'")
        .execute(f.db.pool())
        .await
        .unwrap();
    let renewing = IndexerSettings {
        lease: Duration::from_secs(2),
        heartbeat: Duration::from_millis(200),
        margin: Duration::from_millis(1500),
        ..IndexerSettings::default()
    };
    let pass = indexer(&f, renewing.clone()).run_once().await.unwrap();
    assert_eq!(pass.indexed, 1, "{pass:?}");
    // A correction during the work: the next heartbeat fails its fence and the work is dropped.
    let worker = indexer(
        &f,
        IndexerSettings {
            lease: LEASE,
            ..renewing
        },
    );
    f.run(
        &client,
        "observations.correct",
        correct("c1", "a", id, 1, 9, context(json!(3), exact(3))),
    )
    .await
    .unwrap();
    let running = tokio::spawn(async move { worker.run_once().await.unwrap() });
    tokio::time::sleep(Duration::from_millis(300)).await;
    f.run(
        &client,
        "observations.correct",
        correct("c2", "a", id, 2, 9, context(json!(3), exact(3))),
    )
    .await
    .unwrap();
    let pass = running.await.unwrap();
    assert_eq!(
        (pass.claimed, pass.abandoned, pass.indexed),
        (1, 1, 0),
        "{pass:?}"
    );
    assert!(
        parts_of(&f, id).await.is_empty(),
        "nothing stale was committed"
    );
}

/// A disabled tenant's rows are not claimed; a commit after a disable is deferred, not lost.
#[tokio::test]
async fn memory_storage_index_commits_recheck_the_tenant() {
    let f = fixture_delay(Duration::ZERO).await;
    let (_, ids) = seed(&f, 2).await;
    let store = f.app.store().clone();
    let job = store.claim_index_jobs(1, LEASE).await.unwrap().remove(0);
    f.run(&f.operator, "admin.tenants.disable", json!({"tenant": "t"}))
        .await
        .unwrap();
    assert!(store.claim_index_jobs(8, LEASE).await.unwrap().is_empty());
    let parts = f
        .app
        .embeddings()
        .embed_parts(job.title.clone(), job.body.clone(), Default::default())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        store.commit_index(&job, FAKE_MODEL, &parts).await.unwrap(),
        IndexCommit::Deferred
    );
    let attempts: i32 = sqlx::query_scalar("SELECT index_attempts FROM observations WHERE id = $1")
        .bind(job.observation_id)
        .fetch_one(f.db.pool())
        .await
        .unwrap();
    assert_eq!(attempts, 0, "a deferred attempt is refunded");
    f.run(&f.operator, "admin.tenants.enable", json!({"tenant": "t"}))
        .await
        .unwrap();
    sqlx::query("UPDATE observations SET next_index_at = now()")
        .execute(f.db.pool())
        .await
        .unwrap();
    let pass = indexer(&f, IndexerSettings::default())
        .run_once()
        .await
        .unwrap();
    assert_eq!(pass.indexed, 2);
    assert_eq!(ids.len(), 2);
}

/// F3: `run` stops the indexer within the shutdown grace while a slow embedding is in flight,
/// and nothing of the interrupted work is committed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_startup_run_stops_the_indexer_within_the_shutdown_grace() {
    let f = fixture_delay(Duration::from_secs(3)).await;
    let (_, ids) = seed(&f, 1).await;
    let config = crate::infra::config::Config::from_lookup(|key| match key {
        "MEMORY_DATABASE_URL" => Some("postgres://u@localhost/m".into()),
        "MEMORY_SHUTDOWN_GRACE_MS" => Some("1000".into()),
        _ => None,
    })
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let shutdown = tokio_util::sync::CancellationToken::new();
    let running = tokio::spawn(crate::infra::startup::run(
        config,
        f.db.pool().clone(),
        f.app.embeddings().clone(),
        listener,
        shutdown.clone(),
    ));
    // Let the indexer claim the row and start its 3 s embedding.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let started = std::time::Instant::now();
    shutdown.cancel();
    tokio::time::timeout(Duration::from_millis(2_500), running)
        .await
        .expect("run returns within the shutdown grace")
        .unwrap()
        .unwrap();
    // Server drain and indexer join share one grace (1 s), plus scheduling slack.
    assert!(
        started.elapsed() < Duration::from_millis(1_500),
        "{:?}",
        started.elapsed()
    );
    use sqlx::Connection;
    let mut db = sqlx::PgConnection::connect_with(&f.db.owner_options)
        .await
        .unwrap();
    let (state, parts): (String, i64) = sqlx::query_as(
        "SELECT index_state, (SELECT count(*) FROM observation_parts) FROM observations WHERE id = $1",
    )
    .bind(ids[0])
    .fetch_one(&mut db)
    .await
    .unwrap();
    assert_eq!((state.as_str(), parts), ("pending", 0));
    let attempts: i32 = sqlx::query_scalar("SELECT index_attempts FROM observations WHERE id = $1")
        .bind(ids[0])
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(attempts, 1, "the claim happened before shutdown");
}
