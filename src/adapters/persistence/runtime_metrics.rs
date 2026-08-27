use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, QueryBuilder, Row};

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        dashboard::DashboardWindow,
        runtime_metrics::{
            MachineId, MachineIdentity, MachineRegion, RuntimeMetricBucket,
            RuntimeMetricObservation, RuntimeMetricSample, RuntimeMetricSnapshot,
            TASK_WORKER_RECOMMENDATION_POLICY, TaskWorkerConcurrencyRecommendation,
        },
    },
    services::runtime_metrics::{RuntimeMetricPersistence, RuntimeMetricProbeError},
};

const CURRENT_SQL: &str = r#"
    SELECT machine_id,
           machine_region,
           sampled_at,
           process_rss_bytes,
           memory_limit_bytes,
           cpu_utilization_percent,
           cpu_steal_percent,
           cpu_throttle_percent,
           active_task_executions,
           task_worker_concurrency_limit,
           database_acquire_duration_ms,
           database_acquire_succeeded,
           pool_size,
           pool_idle,
           pool_active
      FROM runtime_metric_samples
     WHERE machine_id = $1
     ORDER BY sampled_at DESC
     LIMIT 1"#;

const PEAK_RSS_SQL: &str = r#"
    SELECT MAX(process_rss_bytes)::bigint AS peak_rss_bytes
      FROM runtime_metric_samples
     WHERE machine_id = $1
       AND sampled_at >= CURRENT_TIMESTAMP - make_interval(mins => $2)"#;

/// Highest concurrency this machine has actually demonstrated safely over the last day.
///
/// A level needs at least 100 complete samples, and the machine's history must span essentially
/// the full day (allowing one ten-minute gap around deploys). Missing platform counters exclude a
/// sample rather than silently passing it. Pool headroom reserves two connections for HTTP and
/// process infrastructure beyond the observed active count.
const RECOMMENDATION_SQL: &str = r#"
    WITH history AS (
        SELECT sampled_at,
               active_task_executions,
               cpu_utilization_percent,
               cpu_steal_percent + cpu_throttle_percent AS cpu_pressure_percent,
               process_rss_bytes::double precision * 100.0
                   / NULLIF(memory_limit_bytes, 0)::double precision AS rss_percent,
               database_acquire_duration_ms,
               pool_active,
               pool_size
          FROM runtime_metric_samples
         WHERE machine_id = $1
           AND sampled_at >= CURRENT_TIMESTAMP - make_interval(hours => $2)
    ),
    coverage AS (
        SELECT MAX(sampled_at) - MIN(sampled_at) AS span
          FROM history
    ),
    measured AS (
        SELECT active_task_executions AS concurrency,
               COUNT(*)::bigint AS supporting_samples,
               percentile_disc(0.95) WITHIN GROUP (
                   ORDER BY cpu_utilization_percent
               ) AS cpu_p95,
               percentile_disc(0.95) WITHIN GROUP (
                   ORDER BY cpu_pressure_percent
               ) AS pressure_p95,
               percentile_disc(0.95) WITHIN GROUP (
                   ORDER BY rss_percent
               ) AS rss_p95,
               percentile_disc(0.95) WITHIN GROUP (
                   ORDER BY database_acquire_duration_ms
               ) AS database_acquire_p95,
               percentile_disc(0.95) WITHIN GROUP (ORDER BY pool_active) AS pool_active_p95,
               MIN(pool_size)::integer AS minimum_pool_size
          FROM history
         WHERE active_task_executions > 0
           AND cpu_utilization_percent IS NOT NULL
           AND cpu_pressure_percent IS NOT NULL
           AND rss_percent IS NOT NULL
         GROUP BY active_task_executions
    )
    SELECT measured.concurrency,
           measured.supporting_samples
      FROM measured
      CROSS JOIN coverage
     WHERE coverage.span >= make_interval(mins => $3)
       AND measured.supporting_samples >= $4
       AND measured.cpu_p95 < $5
       AND measured.pressure_p95 < $6
       AND measured.rss_p95 < $7
       AND measured.database_acquire_p95 < $8
       AND measured.pool_active_p95 <= measured.minimum_pool_size - $9
     ORDER BY measured.concurrency DESC
     LIMIT 1"#;

/// Gap-filled to the same dashboard grid as task charts, so a database outage remains visible as
/// an absent segment instead of making neighbouring observations look adjacent.
const BUCKETS_SQL: &str = r#"
    WITH slots AS (
        SELECT generate_series(
                   to_timestamp(floor(extract(epoch FROM CURRENT_TIMESTAMP) / $2) * $2)
                       - make_interval(secs => $3 * 60 - $2),
                   to_timestamp(floor(extract(epoch FROM CURRENT_TIMESTAMP) / $2) * $2),
                   make_interval(secs => $2)
               ) AS bucket
    ),
    measured AS (
        SELECT to_timestamp(
                   floor(extract(epoch FROM sampled_at) / $2) * $2
               ) AS bucket,
               AVG(cpu_utilization_percent)::double precision AS cpu_utilization_percent,
               AVG(cpu_steal_percent)::double precision AS cpu_steal_percent,
               AVG(cpu_throttle_percent)::double precision AS cpu_throttle_percent,
               percentile_disc(0.5) WITHIN GROUP (
                   ORDER BY database_acquire_duration_ms
               ) AS database_acquire_p50_ms,
               percentile_disc(0.95) WITHIN GROUP (
                   ORDER BY database_acquire_duration_ms
               ) AS database_acquire_p95_ms
          FROM runtime_metric_samples
         WHERE machine_id = $1
           AND sampled_at >= CURRENT_TIMESTAMP - make_interval(mins => $3)
         GROUP BY bucket
    )
    SELECT slots.bucket,
           measured.cpu_utilization_percent,
           measured.cpu_steal_percent,
           measured.cpu_throttle_percent,
           measured.database_acquire_p50_ms,
           measured.database_acquire_p95_ms
      FROM slots
      LEFT JOIN measured ON measured.bucket = slots.bucket
     ORDER BY slots.bucket"#;

fn pool_counts(pool: &sqlx::PgPool) -> (i32, i32, i32) {
    let size = i32::try_from(pool.size()).unwrap_or(i32::MAX);
    let idle = i32::try_from(pool.num_idle()).unwrap_or(i32::MAX).min(size);
    (size, idle, size.saturating_sub(idle))
}

fn completed_sample(
    observation: RuntimeMetricObservation,
    database_acquire_duration_ms: f64,
    database_acquire_succeeded: bool,
    pool: &sqlx::PgPool,
) -> RuntimeMetricSample {
    let (pool_size, pool_idle, pool_active) = pool_counts(pool);
    RuntimeMetricSample {
        identity: observation.identity,
        sampled_at: observation.sampled_at,
        process_rss_bytes: observation.process_rss_bytes,
        memory_limit_bytes: observation.memory_limit_bytes,
        cpu_utilization_percent: observation.cpu_utilization_percent,
        cpu_steal_percent: observation.cpu_steal_percent,
        cpu_throttle_percent: observation.cpu_throttle_percent,
        active_task_executions: observation.active_task_executions,
        task_worker_concurrency_limit: observation.task_worker_concurrency_limit,
        database_acquire_duration_ms,
        database_acquire_succeeded,
        pool_size,
        pool_idle,
        pool_active,
    }
}

async fn insert_samples(
    connection: &mut sqlx::PgConnection,
    samples: impl IntoIterator<Item = &RuntimeMetricSample>,
) -> Result<(), sqlx::Error> {
    let mut query = QueryBuilder::<Postgres>::new(
        "INSERT INTO runtime_metric_samples (machine_id, machine_region, sampled_at, \
         process_rss_bytes, memory_limit_bytes, cpu_utilization_percent, cpu_steal_percent, \
         cpu_throttle_percent, active_task_executions, task_worker_concurrency_limit, \
         database_acquire_duration_ms, database_acquire_succeeded, pool_size, pool_idle, \
         pool_active) ",
    );
    query.push_values(samples, |mut row, sample| {
        row.push_bind(sample.identity.id.as_str())
            .push_bind(sample.identity.region.as_ref().map(MachineRegion::as_str))
            .push_bind(sample.sampled_at)
            .push_bind(sample.process_rss_bytes)
            .push_bind(sample.memory_limit_bytes)
            .push_bind(sample.cpu_utilization_percent)
            .push_bind(sample.cpu_steal_percent)
            .push_bind(sample.cpu_throttle_percent)
            .push_bind(sample.active_task_executions)
            .push_bind(sample.task_worker_concurrency_limit)
            .push_bind(sample.database_acquire_duration_ms)
            .push_bind(sample.database_acquire_succeeded)
            .push_bind(sample.pool_size)
            .push_bind(sample.pool_idle)
            .push_bind(sample.pool_active);
    });
    query.push(
        " ON CONFLICT (machine_id, sampled_at) DO UPDATE SET \
         machine_region = EXCLUDED.machine_region, \
         process_rss_bytes = EXCLUDED.process_rss_bytes, \
         memory_limit_bytes = EXCLUDED.memory_limit_bytes, \
         cpu_utilization_percent = EXCLUDED.cpu_utilization_percent, \
         cpu_steal_percent = EXCLUDED.cpu_steal_percent, \
         cpu_throttle_percent = EXCLUDED.cpu_throttle_percent, \
         active_task_executions = EXCLUDED.active_task_executions, \
         task_worker_concurrency_limit = EXCLUDED.task_worker_concurrency_limit, \
         database_acquire_duration_ms = EXCLUDED.database_acquire_duration_ms, \
         database_acquire_succeeded = EXCLUDED.database_acquire_succeeded, \
         pool_size = EXCLUDED.pool_size, pool_idle = EXCLUDED.pool_idle, \
         pool_active = EXCLUDED.pool_active",
    );
    query.build().execute(connection).await?;
    Ok(())
}

fn sample_from_row(row: &sqlx::postgres::PgRow) -> AppResult<RuntimeMetricSample> {
    Ok(RuntimeMetricSample {
        identity: MachineIdentity {
            id: MachineId::new(
                row.try_get::<String, _>("machine_id")
                    .map_err(AppError::from)?,
            ),
            region: row
                .try_get::<Option<String>, _>("machine_region")
                .map_err(AppError::from)?
                .map(MachineRegion::new),
        },
        sampled_at: row.try_get("sampled_at").map_err(AppError::from)?,
        process_rss_bytes: row.try_get("process_rss_bytes").map_err(AppError::from)?,
        memory_limit_bytes: row.try_get("memory_limit_bytes").map_err(AppError::from)?,
        cpu_utilization_percent: row
            .try_get("cpu_utilization_percent")
            .map_err(AppError::from)?,
        cpu_steal_percent: row.try_get("cpu_steal_percent").map_err(AppError::from)?,
        cpu_throttle_percent: row
            .try_get("cpu_throttle_percent")
            .map_err(AppError::from)?,
        active_task_executions: row
            .try_get("active_task_executions")
            .map_err(AppError::from)?,
        task_worker_concurrency_limit: row
            .try_get("task_worker_concurrency_limit")
            .map_err(AppError::from)?,
        database_acquire_duration_ms: row
            .try_get("database_acquire_duration_ms")
            .map_err(AppError::from)?,
        database_acquire_succeeded: row
            .try_get("database_acquire_succeeded")
            .map_err(AppError::from)?,
        pool_size: row.try_get("pool_size").map_err(AppError::from)?,
        pool_idle: row.try_get("pool_idle").map_err(AppError::from)?,
        pool_active: row.try_get("pool_active").map_err(AppError::from)?,
    })
}

#[async_trait]
impl RuntimeMetricPersistence for PostgresPersistence {
    async fn probe_and_record(
        &self,
        observation: RuntimeMetricObservation,
        failed: &[RuntimeMetricSample],
    ) -> Result<RuntimeMetricSample, RuntimeMetricProbeError> {
        let started = Instant::now();
        let acquired = self.pool.acquire().await;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut connection = match acquired {
            Ok(connection) => connection,
            Err(error) => {
                return Err(RuntimeMetricProbeError {
                    sample: completed_sample(observation, elapsed_ms, false, &self.pool),
                    message: format!("database acquisition probe failed: {error}"),
                });
            }
        };

        let sample = completed_sample(observation, elapsed_ms, true, &self.pool);
        if let Err(error) = insert_samples(&mut connection, failed.iter().chain([&sample])).await {
            return Err(RuntimeMetricProbeError {
                sample,
                message: format!("runtime samples could not be written: {error}"),
            });
        }
        Ok(sample)
    }

    async fn snapshot(
        &self,
        machine_id: &MachineId,
        window: DashboardWindow,
    ) -> AppResult<RuntimeMetricSnapshot> {
        let current = sqlx::query(CURRENT_SQL)
            .bind(machine_id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?
            .as_ref()
            .map(sample_from_row)
            .transpose()?;
        let peak_row = sqlx::query(PEAK_RSS_SQL)
            .bind(machine_id.as_str())
            .bind(window.minutes() as i32)
            .fetch_one(&self.pool)
            .await
            .map_err(AppError::from)?;
        let rows = sqlx::query(BUCKETS_SQL)
            .bind(machine_id.as_str())
            .bind(window.bucket_seconds())
            .bind(window.minutes() as i32)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;
        let recommendation = sqlx::query(RECOMMENDATION_SQL)
            .bind(machine_id.as_str())
            .bind(TASK_WORKER_RECOMMENDATION_POLICY.window_hours)
            .bind(TASK_WORKER_RECOMMENDATION_POLICY.minimum_coverage_minutes)
            .bind(TASK_WORKER_RECOMMENDATION_POLICY.minimum_samples_per_level)
            .bind(TASK_WORKER_RECOMMENDATION_POLICY.cpu_p95_percent)
            .bind(TASK_WORKER_RECOMMENDATION_POLICY.cpu_pressure_p95_percent)
            .bind(TASK_WORKER_RECOMMENDATION_POLICY.rss_p95_percent)
            .bind(TASK_WORKER_RECOMMENDATION_POLICY.database_acquire_p95_ms)
            .bind(TASK_WORKER_RECOMMENDATION_POLICY.reserved_pool_connections)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?
            .map(|row| -> AppResult<TaskWorkerConcurrencyRecommendation> {
                Ok(TaskWorkerConcurrencyRecommendation {
                    maximum: row.try_get("concurrency").map_err(AppError::from)?,
                    supporting_samples: row
                        .try_get("supporting_samples")
                        .map_err(AppError::from)?,
                })
            })
            .transpose()?;
        let buckets = rows
            .into_iter()
            .map(|row| {
                Ok(RuntimeMetricBucket {
                    bucket: row.try_get("bucket").map_err(AppError::from)?,
                    cpu_utilization_percent: row
                        .try_get("cpu_utilization_percent")
                        .map_err(AppError::from)?,
                    cpu_steal_percent: row.try_get("cpu_steal_percent").map_err(AppError::from)?,
                    cpu_throttle_percent: row
                        .try_get("cpu_throttle_percent")
                        .map_err(AppError::from)?,
                    database_acquire_p50_ms: row
                        .try_get("database_acquire_p50_ms")
                        .map_err(AppError::from)?,
                    database_acquire_p95_ms: row
                        .try_get("database_acquire_p95_ms")
                        .map_err(AppError::from)?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;

        Ok(RuntimeMetricSnapshot {
            current,
            peak_rss_bytes: peak_row.try_get("peak_rss_bytes").map_err(AppError::from)?,
            buckets,
            suggested_task_worker_concurrency: recommendation,
        })
    }

    async fn prune_before(&self, cutoff: DateTime<Utc>) -> AppResult<u64> {
        Ok(
            sqlx::query("DELETE FROM runtime_metric_samples WHERE sampled_at < $1")
                .bind(cutoff)
                .execute(&self.pool)
                .await
                .map_err(AppError::from)?
                .rows_affected(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::test_pool;
    use uuid::Uuid;

    fn observation(machine_id: &MachineId, sampled_at: DateTime<Utc>) -> RuntimeMetricObservation {
        RuntimeMetricObservation {
            identity: MachineIdentity {
                id: machine_id.clone(),
                region: Some(MachineRegion::new("lhr")),
            },
            sampled_at,
            process_rss_bytes: Some(64 * 1024 * 1024),
            memory_limit_bytes: Some(512 * 1024 * 1024),
            cpu_utilization_percent: Some(37.5),
            cpu_steal_percent: Some(2.5),
            cpu_throttle_percent: Some(1.25),
            active_task_executions: 2,
            task_worker_concurrency_limit: 4,
        }
    }

    #[tokio::test]
    async fn runtime_samples_round_trip_and_fill_dashboard_buckets() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let machine_id = MachineId::new(format!("test-{}", Uuid::new_v4()));
        let sampled_at = Utc::now() - chrono::Duration::minutes(7);

        persistence
            .probe_and_record(observation(&machine_id, sampled_at), &[])
            .await
            .expect("the runtime sample is written");
        let other_machine = MachineId::new(format!("test-{}", Uuid::new_v4()));
        persistence
            .probe_and_record(observation(&other_machine, sampled_at), &[])
            .await
            .expect("a neighbouring machine sample is written");
        let snapshot = persistence
            .snapshot(&machine_id, DashboardWindow::last_hour())
            .await
            .expect("the runtime sample is readable");

        let current = snapshot.current.expect("a current sample exists");
        assert_eq!(current.identity.id, machine_id);
        assert_eq!(current.identity.region, Some(MachineRegion::new("lhr")));
        assert_eq!(current.process_rss_bytes, Some(64 * 1024 * 1024));
        assert_eq!(current.memory_limit_bytes, Some(512 * 1024 * 1024));
        assert!(current.database_acquire_succeeded);
        assert_eq!(current.active_task_executions, 2);
        assert_eq!(current.task_worker_concurrency_limit, 4);
        assert_eq!(snapshot.peak_rss_bytes, Some(64 * 1024 * 1024));
        assert_eq!(snapshot.suggested_task_worker_concurrency, None);
        assert_eq!(
            snapshot.buckets.len() as i64,
            DashboardWindow::last_hour().bucket_count()
        );
        assert!(snapshot.buckets.iter().any(|bucket| {
            bucket.cpu_utilization_percent == Some(37.5) && bucket.database_acquire_p50_ms.is_some()
        }));
        for window in [
            DashboardWindow::last_six_hours(),
            DashboardWindow::last_day(),
        ] {
            let ranged = persistence
                .snapshot(&machine_id, window)
                .await
                .expect("every runtime range buckets successfully");
            assert_eq!(ranged.buckets.len() as i64, window.bucket_count());
            assert!(
                ranged
                    .buckets
                    .iter()
                    .any(|bucket| bucket.cpu_utilization_percent == Some(37.5))
            );
        }

        sqlx::query("DELETE FROM runtime_metric_samples WHERE machine_id = $1")
            .bind(machine_id.as_str())
            .execute(persistence.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM runtime_metric_samples WHERE machine_id = $1")
            .bind(other_machine.as_str())
            .execute(persistence.pool())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn recommendation_returns_the_highest_observed_concurrency_meeting_every_limit() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let machine_id = MachineId::new(format!("recommendation-{}", Uuid::new_v4()));

        // Level two has a full day of complete, healthy observations. Level three has the same
        // evidence but breaches the CPU criterion, so it must not raise the recommendation.
        sqlx::query(
            r#"INSERT INTO runtime_metric_samples
                   (machine_id, machine_region, sampled_at, process_rss_bytes, memory_limit_bytes,
                    cpu_utilization_percent, cpu_steal_percent, cpu_throttle_percent,
                    active_task_executions, task_worker_concurrency_limit,
                    database_acquire_duration_ms, database_acquire_succeeded,
                    pool_size, pool_idle, pool_active)
               SELECT $1, 'test', CURRENT_TIMESTAMP - INTERVAL '23 hours 59 minutes'
                              + sample * INTERVAL '863.4 seconds',
                      536870912, 1073741824, 50.0, 2.0, 1.0, 2, 4,
                      20.0, TRUE, 10, 7, 3
                 FROM generate_series(0, 100) AS sample"#,
        )
        .bind(machine_id.as_str())
        .execute(persistence.pool())
        .await
        .expect("healthy level-two samples are inserted");
        sqlx::query(
            r#"INSERT INTO runtime_metric_samples
                   (machine_id, machine_region, sampled_at, process_rss_bytes, memory_limit_bytes,
                    cpu_utilization_percent, cpu_steal_percent, cpu_throttle_percent,
                    active_task_executions, task_worker_concurrency_limit,
                    database_acquire_duration_ms, database_acquire_succeeded,
                    pool_size, pool_idle, pool_active)
               SELECT $1, 'test', CURRENT_TIMESTAMP - INTERVAL '23 hours 58 minutes'
                              + sample * INTERVAL '862.8 seconds',
                      536870912, 1073741824, 90.0, 2.0, 1.0, 3, 4,
                      20.0, TRUE, 10, 7, 3
                 FROM generate_series(0, 100) AS sample"#,
        )
        .bind(machine_id.as_str())
        .execute(persistence.pool())
        .await
        .expect("overloaded level-three samples are inserted");

        let recommendation = persistence
            .snapshot(&machine_id, DashboardWindow::last_day())
            .await
            .expect("the recommendation query runs")
            .suggested_task_worker_concurrency
            .expect("level two has enough safe evidence");
        assert_eq!(recommendation.maximum, 2);
        assert_eq!(recommendation.supporting_samples, 101);

        sqlx::query("DELETE FROM runtime_metric_samples WHERE machine_id = $1")
            .bind(machine_id.as_str())
            .execute(persistence.pool())
            .await
            .unwrap();
    }
}
