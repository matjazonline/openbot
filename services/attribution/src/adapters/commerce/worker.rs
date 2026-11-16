//! Bounded leased settlement; acknowledged envelopes survive crashes and key rotation.
use super::{http::CommerceHttp, postgres::*, settle};
use crate::{
    adapters::{
        identity::tenants::lock_tenant,
        postgres::failure as db,
        setup::postgres::{SetupPostgres, enabled_store},
    },
    application::{
        commerce::*,
        identity::{CompanyId, Failure, Result},
        setup::StoreId,
    },
};
use chrono::{DateTime, Utc};
use sqlx::Row;
use std::time::Duration;
use tokio::{task::JoinSet, time};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone, Copy)]
pub struct Claim {
    pub event: EventId,
    pub company: CompanyId,
    pub store: StoreId,
    pub execution: Uuid,
}
pub async fn claim(store: &SetupPostgres) -> Result<Vec<Claim>> {
    let mut tx = store.pool.begin().await.map_err(db)?;
    let rows = sqlx::query(
        r#"WITH first_due AS (
        SELECT DISTINCT ON(company_id) id FROM attribution_merchant_events
        WHERE state='pending' AND next_attempt_at<=clock_timestamp()
            AND (lease_until IS NULL OR lease_until<=clock_timestamp())
        ORDER BY company_id,next_attempt_at,id
    ) SELECT e.id,e.company_id,e.store_id,e.attempts,e.deadline_at
    FROM attribution_merchant_events e JOIN first_due d ON d.id=e.id
    WHERE e.state='pending' AND e.next_attempt_at<=clock_timestamp()
        AND (e.lease_until IS NULL OR e.lease_until<=clock_timestamp())
    ORDER BY e.next_attempt_at,e.company_id,e.id LIMIT 8 FOR UPDATE OF e SKIP LOCKED"#,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(db)?;
    let mut claims = Vec::new();
    for row in rows {
        let event = EventId(row.try_get("id").map_err(db)?);
        let attempts: i32 = row.try_get("attempts").map_err(db)?;
        let deadline: DateTime<Utc> = row.try_get("deadline_at").map_err(db)?;
        let time = now(&mut tx).await?;
        if attempts >= MAX_ATTEMPTS || deadline <= time {
            let reason = if attempts >= MAX_ATTEMPTS {
                EventReason::RetryExhausted
            } else {
                EventReason::DeadlineExpired
            };
            sqlx::query("UPDATE attribution_merchant_events SET state='unresolved',reason=$2,next_attempt_at=NULL,execution=NULL,lease_until=NULL WHERE id=$1")
                .bind(event.0).bind(reason_value(Some(reason))?).execute(&mut *tx).await.map_err(db)?;
            continue;
        }
        let execution = Uuid::new_v4();
        sqlx::query("UPDATE attribution_merchant_events SET attempts=attempts+1,execution=$2,lease_until=clock_timestamp()+interval '30 seconds' WHERE id=$1")
            .bind(event.0).bind(execution).execute(&mut *tx).await.map_err(db)?;
        claims.push(Claim {
            event,
            company: CompanyId(row.try_get("company_id").map_err(db)?),
            store: StoreId(row.try_get("store_id").map_err(db)?),
            execution,
        });
    }
    tx.commit().await.map_err(db)?;
    Ok(claims)
}
fn backoff(attempts: i32) -> i64 {
    (5i64 * 2i64.pow(attempts.clamp(0, 10) as u32)).min(3600)
}
pub async fn process(state: &CommerceHttp, claim: Claim) -> Result<EventState> {
    let ticket = state.setup.identity.service.tenant(claim.company).await?;
    let mut tx = state.setup.store.pool.begin().await.map_err(db)?;
    lock_tenant(&mut tx, claim.company, &ticket).await?;
    enabled_store(&mut tx, claim.company, claim.store).await?;
    let row=sqlx::query("SELECT order_id,envelope,attempts,execution,lease_until,state FROM attribution_merchant_events WHERE company_id=$1 AND store_id=$2 AND id=$3 FOR UPDATE")
        .bind(claim.company.0).bind(claim.store.0).bind(claim.event.0).fetch_optional(&mut *tx).await.map_err(db)?.ok_or(Failure::Conflict)?;
    if row.try_get::<Option<Uuid>, _>("execution").map_err(db)? != Some(claim.execution)
        || row.try_get::<String, _>("state").map_err(db)? != "pending"
    {
        return Err(Failure::Conflict);
    }
    // Database time after all lock waits, including a final fence before commit.
    let live:bool=sqlx::query_scalar("SELECT execution=$2 AND lease_until>clock_timestamp() FROM attribution_merchant_events WHERE id=$1")
        .bind(claim.event.0).bind(claim.execution).fetch_one(&mut *tx).await.map_err(db)?;
    if !live {
        return Err(Failure::Conflict);
    }
    let order = OrderId(row.try_get("order_id").map_err(db)?);
    let event: Conversion = decode(row.try_get("envelope").map_err(db)?)?;
    let event = event.normalize()?;
    let outcome = settle::apply(
        &mut tx,
        Scope {
            company: claim.company,
            store: claim.store,
        },
        order,
        &event,
        state.policy,
    )
    .await?;
    let delay = backoff(row.try_get("attempts").map_err(db)?);
    let changed=sqlx::query("UPDATE attribution_merchant_events SET state=$3,reason=$4,next_attempt_at=CASE WHEN $3='pending' THEN clock_timestamp()+make_interval(secs=>$5::double precision) ELSE NULL END,execution=NULL,lease_until=NULL WHERE id=$1 AND execution=$2 AND lease_until>clock_timestamp()")
        .bind(claim.event.0).bind(claim.execution).bind(outcome.state.as_str()).bind(reason_value(outcome.reason)?).bind(delay as f64).execute(&mut *tx).await.map_err(db)?;
    if changed.rows_affected() != 1 {
        return Err(Failure::Conflict);
    }
    tx.commit().await.map_err(|_| Failure::Unavailable)?;
    Ok(outcome.state)
}
pub async fn failed(store: &SetupPostgres, claim: Claim) -> Result<()> {
    sqlx::query("UPDATE attribution_merchant_events SET reason='temporarily_unavailable',next_attempt_at=clock_timestamp()+make_interval(secs=>LEAST(3600,5*power(2,LEAST(attempts,10)))::double precision),execution=NULL,lease_until=NULL WHERE id=$1 AND company_id=$2 AND store_id=$3 AND state='pending' AND execution=$4 AND lease_until>clock_timestamp()")
        .bind(claim.event.0).bind(claim.company.0).bind(claim.store.0).bind(claim.execution).execute(&store.pool).await.map_err(db)?;
    Ok(())
}
pub async fn probe(store: &SetupPostgres) -> Result<()> {
    let row=sqlx::query("SELECT count(*) FILTER(WHERE state='pending') AS pending,count(*) FILTER(WHERE state='unresolved') AS unresolved,coalesce(date_part('epoch',clock_timestamp()-min(received_at))::bigint,0) AS oldest_seconds FROM attribution_merchant_events WHERE state IN ('pending','unresolved')")
        .fetch_one(&store.pool).await.map_err(db)?;
    tracing::info!(
        operation = "merchant_backlog",
        pending = row.try_get::<i64, _>("pending").map_err(db)?,
        unresolved = row.try_get::<i64, _>("unresolved").map_err(db)?,
        oldest_seconds = row.try_get::<i64, _>("oldest_seconds").map_err(db)?
    );
    Ok(())
}
pub async fn iteration(state: CommerceHttp, shutdown: CancellationToken) {
    tokio::select! { biased; ()=shutdown.cancelled()=>return,
        r=time::timeout(Duration::from_secs(2),probe(&state.setup.store))=>{
            if !matches!(r,Ok(Ok(()))) {tracing::warn!(operation="merchant_backlog",outcome="unavailable");}
        }
    }
    let claims = tokio::select! {
        biased;
        ()=shutdown.cancelled()=>return,
        result=time::timeout(Duration::from_secs(10),claim(&state.setup.store))=>match result {
            Ok(Ok(claims))=>claims,
            _=>{tracing::warn!(component="commerce_reconciliation","claim unavailable");return;}
        }
    };
    let mut tasks = JoinSet::new();
    for claim in claims {
        let state = state.clone();
        let shutdown = shutdown.clone();
        tasks.spawn(async move {
            let work=async {
                match process(&state,claim).await {
                    Ok(result)=>tracing::info!(event_id=%claim.event.0,state=result.as_str(),"merchant event reconciled"),
                    Err(Failure::Conflict)=>tracing::warn!(event_id=%claim.event.0,"merchant event lease no longer owned"),
                    Err(_)=>{
                        if failed(&state.setup.store,claim).await.is_err() {tracing::warn!(event_id=%claim.event.0,"merchant retry state unavailable");}
                    }
                }
            };
            tokio::select! {biased; ()=shutdown.cancelled()=>(), _=time::timeout(Duration::from_secs(10),work)=>()}
        });
    }
    while let Some(result) = tasks.join_next().await {
        if result.is_err() {
            tracing::warn!(
                component = "commerce_reconciliation",
                "settlement task failed; lease is recoverable"
            );
        }
    }
}
pub async fn run(state: CommerceHttp, shutdown: CancellationToken) {
    let mut interval = time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {biased; ()=shutdown.cancelled()=>break, _=interval.tick()=>iteration(state.clone(),shutdown.clone()).await}
    }
}
