//! Tenant-serialized, bounded maintenance. The committed schedule owns retry;
//! every data change belongs to one cancellable transaction, without external effects.
use crate::{
    adapters::postgres::{Tx, failure as db},
    application::{
        identity::{CompanyId, IdentityService, Result},
        retention::RetentionPolicy,
    },
};
use sqlx::{PgPool, Row};
use std::{sync::Arc, time::Duration};
use tokio::{task::JoinSet, time};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
mod prune;

pub const BATCH_ROWS: i64 = 100;
pub async fn claim(pool: &PgPool) -> Result<Vec<Uuid>> {
    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query("SET LOCAL statement_timeout='2s'")
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let ids = sqlx::query_scalar(
        r#"WITH due AS (
        SELECT company_id FROM attribution_tenants
        WHERE purged_at IS NULL AND maintenance_due_at<=clock_timestamp()
        ORDER BY maintenance_due_at,company_id LIMIT 8 FOR UPDATE SKIP LOCKED
    ) UPDATE attribution_tenants t SET maintenance_due_at=clock_timestamp()+interval '60 seconds'
    FROM due WHERE t.company_id=due.company_id RETURNING t.company_id"#,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(ids)
}

/// Only constant, adapter-owned table/predicate strings reach this helper.
async fn delete(
    tx: &mut Tx,
    company: Uuid,
    table: &str,
    predicate: &str,
    remaining: &mut i64,
) -> Result<()> {
    if *remaining == 0 {
        return Ok(());
    }
    let sql = format!(
        "DELETE FROM {table} WHERE ctid IN (SELECT t.ctid FROM {table} t WHERE t.company_id=$1 AND ({predicate}) LIMIT $2)"
    );
    let rows = sqlx::query(&sql)
        .bind(company)
        .bind(*remaining)
        .execute(&mut **tx)
        .await
        .map_err(db)?
        .rows_affected();
    *remaining -= rows as i64;
    Ok(())
}

pub struct Progress {
    pub rows: i64,
    pub purged: bool,
}
pub async fn batch(pool: &PgPool, company: Uuid, policy: RetentionPolicy) -> Result<Progress> {
    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query("SET LOCAL statement_timeout='2s'")
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let row = sqlx::query("SELECT state,purged_at IS NOT NULL AS purged FROM attribution_tenants WHERE company_id=$1 FOR UPDATE")
        .bind(company).fetch_one(&mut *tx).await.map_err(db)?;
    if row.try_get::<bool, _>("purged").map_err(db)? {
        return Ok(Progress {
            rows: 0,
            purged: true,
        });
    }
    let mut remaining = BATCH_ROWS;
    let purged = if row.try_get::<String, _>("state").map_err(db)? == "deleted" {
        prune::purge(&mut tx, company, &mut remaining).await?
    } else {
        prune::retained(&mut tx, company, policy, &mut remaining).await?;
        false
    };
    sqlx::query("UPDATE attribution_tenants SET maintenance_failure_at=NULL,maintenance_due_at=clock_timestamp()+CASE WHEN $2 THEN interval '1 second' ELSE interval '60 seconds' END WHERE company_id=$1")
        .bind(company).bind(remaining<BATCH_ROWS).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Progress {
        rows: BATCH_ROWS - remaining,
        purged,
    })
}

pub async fn iteration(
    pool: PgPool,
    policy: RetentionPolicy,
    identity: Arc<IdentityService>,
    shutdown: CancellationToken,
) {
    let claims = tokio::select! {biased; ()=shutdown.cancelled()=>return,
        r=time::timeout(Duration::from_secs(5),claim(&pool))=>match r {
            Ok(Ok(ids))=>ids,_=>{tracing::warn!(operation="retention_claim",outcome="unavailable");return;}
        }
    };
    let mut tasks = JoinSet::new();
    for company in claims {
        let pool = pool.clone();
        let shutdown = shutdown.clone();
        let identity = identity.clone();
        tasks.spawn(async move {
            let start=std::time::Instant::now();
            let result=tokio::select! {biased; ()=shutdown.cancelled()=>return,
                r=time::timeout(Duration::from_secs(5),batch(&pool,company,policy))=>r};
            match result {
                Ok(Ok(progress))=>{
                    if progress.purged { identity.freshness.invalidate(CompanyId(company)); }
                    tracing::info!(operation="retention",outcome="committed",rows=progress.rows,purged=progress.purged,duration_ms=start.elapsed().as_millis() as u64);
                }
                _=>{
                    tracing::warn!(operation="retention",outcome="unavailable",duration_ms=start.elapsed().as_millis() as u64);
                    // Failure recording has its own deadline. A failure here does not erase
                    // the already committed retry schedule or hold up another tenant.
                    let _=tokio::select! {biased; ()=shutdown.cancelled()=>return,
                        r=time::timeout(Duration::from_secs(2),sqlx::query("UPDATE attribution_tenants SET maintenance_failure_at=clock_timestamp() WHERE company_id=$1").bind(company).execute(&pool))=>r};
                }
            }
        });
    }
    while let Some(result) = tasks.join_next().await {
        if result.is_err() {
            tracing::warn!(operation = "retention", outcome = "task_failed");
        }
    }
}
pub async fn run(
    pool: PgPool,
    policy: RetentionPolicy,
    identity: Arc<IdentityService>,
    shutdown: CancellationToken,
) {
    let mut interval = time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {biased; ()=shutdown.cancelled()=>break,
        _=interval.tick()=>iteration(pool.clone(),policy,identity.clone(),shutdown.clone()).await}
    }
}

#[cfg(test)]
mod cutoff_tests;
