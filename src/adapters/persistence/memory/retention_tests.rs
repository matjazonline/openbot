//! Retention of settled memory intents and lifecycle rows, and the maintenance gauges (p06 gate
//! A3-1–A3-3). Each test owns its database: the sweeps cover whole tables.
use super::tests::{Fixture, PROVISIONER};
use crate::adapters::monitoring::in_memory_monitor::InMemoryMonitor;
use crate::application::memory::*;
use crate::domain::monitoring::MonitoringService;
use std::sync::Arc;
use uuid::Uuid;

/// `count` terminal save intents of the company namespace whose runs never existed (retired),
/// created `days` ago, in `state`.
async fn settled(f: &Fixture, namespace: Uuid, count: i64, days: i32, state: &str, kind: &str) {
    sqlx::query(
        "INSERT INTO memory_deliveries (company_id, id, namespace_id, kind, run_id, execution_id, \
           exchange_id, idempotency_key, expires_at, request, disable_epoch, state, available_at, \
           result, outcome_code, created_at) \
         SELECT $1, gen_random_uuid(), $2, $5, gen_random_uuid(), \
                CASE WHEN $5 = 'save' THEN gen_random_uuid() END, \
                CASE WHEN $5 = 'generation' THEN gen_random_uuid() END, \
                'ob.t.' || replace(gen_random_uuid()::text, '-', ''), \
                now() - make_interval(days => $3) + interval '6 days', \
                CASE WHEN $4 IN ('pending', 'claimed') \
                     THEN '{\"version\": \"1\", \"body\": {}}'::jsonb END, \
                0, $4, now(), \
                CASE WHEN $4 = 'delivered' THEN '{}'::jsonb END, \
                CASE WHEN $4 IN ('failed', 'expired', 'cancelled') THEN 'refused' END, \
                now() - make_interval(days => $3) \
           FROM generate_series(1, $6)",
    )
    .bind(f.company)
    .bind(namespace)
    .bind(days)
    .bind(state)
    .bind(kind)
    .bind(count)
    .execute(f.pool())
    .await
    .unwrap();
}

async fn count(f: &Fixture, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(f.pool()).await.unwrap()
}

/// A settled absent namespace lifecycle row for `namespace`, `days` old.
async fn absent_row(f: &Fixture, namespace: Uuid, days: i32, state: &str) {
    sqlx::query(
        "INSERT INTO memory_lifecycles (id, company_id, resource, namespace_id, owner_kind, \
           owner_id, provisioner_credential_id, desired, observed, generation, fence, state, \
           attempts, available_at, outcome_code, updated_at) \
         VALUES (gen_random_uuid(), $1, 'namespace', $2, 'company', $1, $3, 'absent', \
                 CASE WHEN $5 = 'settled' THEN 'absent' ELSE 'present' END, 1, 0, $5, 0, now(), \
                 CASE WHEN $5 = 'failed' THEN 'namespace_conflict' END, \
                 now() - make_interval(days => $4))",
    )
    .bind(f.company)
    .bind(namespace)
    .bind(PROVISIONER)
    .bind(days)
    .bind(state)
    .execute(f.pool())
    .await
    .unwrap();
}

async fn company_namespace(f: &Fixture) -> Uuid {
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::Company).await;
    f.namespaces("company").await[0].0
}

/// A3-1 and A3-2 boundaries: settled intents go after 7 days (a capture, or a save whose run is
/// retired), open and younger ones stay; settled absent lifecycle rows go 30 days after they
/// settled once their owner row is gone, and failed rows, younger rows and rows whose owner
/// remains stay.
#[tokio::test]
async fn memory_operations_settled_intents_and_lifecycle_rows_age_out_at_their_bounds() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let namespace = company_namespace(&f).await;
    settled(&f, namespace, 2, 8, "delivered", "save").await;
    settled(&f, namespace, 1, 8, "cancelled", "generation").await;
    settled(&f, namespace, 1, 6, "delivered", "save").await;
    settled(&f, namespace, 1, 8, "pending", "save").await;
    let (gone, kept_young, kept_failed) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    absent_row(&f, gone, 31, "settled").await;
    absent_row(&f, kept_young, 29, "settled").await;
    absent_row(&f, kept_failed, 31, "failed").await;
    // A row whose owner namespace still exists stays.
    sqlx::query(
        "UPDATE memory_lifecycles SET desired = 'absent', observed = 'absent', state = 'settled', \
           updated_at = now() - interval '31 days' WHERE company_id = $1 AND namespace_id = $2",
    )
    .bind(f.company)
    .bind(namespace)
    .execute(f.pool())
    .await
    .unwrap();
    let persistence = Arc::new(f.persistence.clone());
    assert_eq!(persistence.sweep_settled_deliveries(500).await.unwrap(), 3);
    assert_eq!(persistence.sweep_settled_lifecycles(500).await.unwrap(), 1);
    assert_eq!(
        count(&f, "SELECT count(*) FROM memory_deliveries").await,
        2,
        "the young settled intent and the open one stay"
    );
    let left: Vec<Uuid> = sqlx::query_scalar(
        "SELECT namespace_id FROM memory_lifecycles WHERE resource = 'namespace' ORDER BY 1",
    )
    .fetch_all(f.pool())
    .await
    .unwrap();
    assert!(!left.contains(&gone));
    for kept in [kept_young, kept_failed, namespace] {
        assert!(left.contains(&kept), "{kept} kept");
    }
}

/// Competing sweepers on separate connections remove each settled intent exactly once.
#[tokio::test]
async fn memory_operations_competing_retention_sweepers_remove_each_row_once() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let namespace = company_namespace(&f).await;
    settled(&f, namespace, 1_200, 8, "expired", "save").await;
    let sweepers: Vec<_> = (0..3)
        .map(|_| {
            let persistence = f.persistence.clone();
            tokio::spawn(async move {
                let mut removed = 0;
                loop {
                    let batch = persistence.sweep_settled_deliveries(100).await.unwrap();
                    removed += batch;
                    if batch == 0 {
                        return removed;
                    }
                }
            })
        })
        .collect();
    let mut total = 0;
    for sweeper in sweepers {
        total += sweeper.await.unwrap();
    }
    assert_eq!(total, 1_200);
    assert_eq!(count(&f, "SELECT count(*) FROM memory_deliveries").await, 0);
}

/// A3-3: one run repeats full batches (bounded), counts what it removed, and records the open-row
/// gauges and a backlog that is zero once it kept up.
#[tokio::test]
async fn memory_operations_maintenance_records_gauges_and_the_backlog() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let namespace = company_namespace(&f).await;
    settled(&f, namespace, 1_100, 8, "failed", "save").await;
    settled(&f, namespace, 2, 1, "pending", "save").await;
    let monitor = Arc::new(InMemoryMonitor::new());
    let maintenance = MemoryMaintenance::new(Arc::new(f.persistence.clone()), monitor.clone());
    let run = maintenance.pass().await.unwrap();
    assert_eq!(run.deliveries_removed, 1_100, "three batches in one run");
    assert_eq!(run.counts.deliveries_pending, 2);
    assert_eq!(run.counts.delivery_backlog, 0);
    let stats = monitor.get_stats_json();
    assert_eq!(
        stats["gauges"]["memory_deliveries_rows{state=pending}"],
        serde_json::json!(2.0)
    );
    assert_eq!(
        stats["gauges"]["memory_retention_backlog_rows{table=memory_deliveries}"],
        serde_json::json!(0.0)
    );
    assert_eq!(
        stats["custom_counters"]["memory_retention_deleted_total{table=memory_deliveries}"],
        serde_json::json!(1_100)
    );
    // The batch bound holds: a larger backlog than one run removes stays visible as backlog.
    settled(
        &f,
        namespace,
        i64::from(RETENTION_BATCH) * MAX_BATCHES_PER_RUN as i64 + 7,
        8,
        "failed",
        "save",
    )
    .await;
    let run = maintenance.pass().await.unwrap();
    assert_eq!(
        run.deliveries_removed,
        u64::from(RETENTION_BATCH) * MAX_BATCHES_PER_RUN as u64
    );
    assert_eq!(run.counts.delivery_backlog, 7);
}

/// The sweeps and the lifecycle claim's expired-lease arm are served by their partial indexes
/// (checked with sequential scans disabled, so a missing index shows instead of hiding behind a
/// small table).
#[tokio::test]
async fn memory_operations_retention_and_lease_queries_use_their_indexes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let mut tx = f.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL enable_seqscan = off")
        .execute(&mut *tx)
        .await
        .unwrap();
    let plan = |rows: Vec<(String,)>| {
        rows.into_iter()
            .map(|(line,)| line)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let deliveries: Vec<(String,)> =
        sqlx::query_as(&format!("EXPLAIN {}", super::retention::delivery_sweep()))
            .bind(SETTLED_INTENT_RETENTION_DAYS)
            .bind(500_i64)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    let deliveries = plan(deliveries);
    assert!(
        deliveries.contains("memory_deliveries_settled"),
        "{deliveries}"
    );
    let lifecycles: Vec<(String,)> =
        sqlx::query_as(&format!("EXPLAIN {}", super::retention::lifecycle_sweep()))
            .bind(SETTLED_LIFECYCLE_RETENTION_DAYS)
            .bind(500_i64)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    let lifecycles = plan(lifecycles);
    assert!(
        lifecycles.contains("memory_lifecycles_settled"),
        "{lifecycles}"
    );
    let expiry: Vec<(String,)> =
        sqlx::query_as(&format!("EXPLAIN {}", super::lifecycle::EXPIRE_LEASES))
            .bind(8_i32)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    let expiry = plan(expiry);
    assert!(expiry.contains("memory_lifecycles_leased"), "{expiry}");
    let claim: Vec<(String,)> = sqlx::query_as(&format!("EXPLAIN {}", super::lifecycle::CLAIM))
        .bind(Uuid::new_v4())
        .bind(60_000_i64)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    let claim = plan(claim);
    assert!(claim.contains("memory_lifecycles_leased"), "{claim}");
    assert!(claim.contains("memory_lifecycles_due"), "{claim}");
}
