use crate::{
    adapters::{
        identity::crypto,
        postgres::{Tx, failure as db},
        setup::{merchant::Merchant, postgres::SetupPostgres},
    },
    application::{
        commerce::*,
        identity::{CompanyId, Failure, Result},
        setup::{KeyScope, StoreId},
    },
};
use chrono::{DateTime, Utc};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub(super) struct Scope {
    pub company: CompanyId,
    pub store: StoreId,
}
pub(super) fn decode<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| Failure::Unavailable)
}
pub(super) fn fingerprint(value: &impl Serialize) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value).map_err(|_| Failure::Unavailable)?;
    Ok(crypto::verifier(&value.to_string()))
}
pub(super) async fn now(tx: &mut Tx) -> Result<DateTime<Utc>> {
    sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut **tx)
        .await
        .map_err(db)
}
pub(super) struct Outcome {
    pub state: EventState,
    pub reason: Option<EventReason>,
}
impl Outcome {
    pub fn accepted() -> Self {
        Self {
            state: EventState::Accepted,
            reason: None,
        }
    }
    pub fn conflict(reason: EventReason) -> Self {
        Self {
            state: EventState::Conflict,
            reason: Some(reason),
        }
    }
    pub fn pending() -> Self {
        Self {
            state: EventState::Pending,
            reason: Some(EventReason::PaymentMissing),
        }
    }
}
pub(super) fn reason_value(reason: Option<EventReason>) -> Result<Option<String>> {
    reason
        .map(|r| {
            serde_json::to_value(r)
                .map_err(|_| Failure::Unavailable)?
                .as_str()
                .map(str::to_owned)
                .ok_or(Failure::Unavailable)
        })
        .transpose()
}
pub(super) async fn order_lock(tx: &mut Tx, scope: Scope, order: OrderId) -> Result<PgRow> {
    sqlx::query(
        "SELECT * FROM attribution_orders WHERE company_id=$1 AND store_id=$2 AND id=$3 AND retired_at IS NULL FOR UPDATE",
    )
    .bind(scope.company.0)
    .bind(scope.store.0)
    .bind(order.0)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db)?
    .ok_or(Failure::NotFound)
}
pub(super) fn result_row(row: &PgRow) -> Result<EventResult> {
    let event: Conversion = decode(row.try_get("envelope").map_err(db)?)?;
    let reason: Option<String> = row.try_get("reason").map_err(db)?;
    Ok(EventResult {
        id: EventId(row.try_get("id").map_err(db)?),
        event_id: event.event_key().clone(),
        state: decode(Value::String(row.try_get("state").map_err(db)?))?,
        reason: reason.map(|s| decode(Value::String(s))).transpose()?,
        order_id: if row.try_get::<bool, _>("paid").map_err(db)? {
            Some(OrderId(row.try_get("order_id").map_err(db)?))
        } else {
            None
        },
        kind: event.kind().to_owned(),
        external_order_id: event.order_key().clone(),
        refund_id: event.refund_key().cloned(),
        occurred_at: event.occurred_at(),
        received_at: row.try_get("received_at").map_err(db)?,
        attempts: row.try_get("attempts").map_err(db)?,
        next_attempt_at: row.try_get("next_attempt_at").map_err(db)?,
        deadline_at: row.try_get("deadline_at").map_err(db)?,
    })
}
pub(super) async fn event_result(
    tx: &mut Tx,
    company: CompanyId,
    id: EventId,
) -> Result<EventResult> {
    let row=sqlx::query("SELECT e.*, o.paid_at IS NOT NULL AND o.retired_at IS NULL AS paid FROM attribution_merchant_events e JOIN attribution_orders o ON (o.company_id,o.store_id,o.id)=(e.company_id,e.store_id,e.order_id) WHERE e.company_id=$1 AND e.id=$2")
        .bind(company.0).bind(id.0).fetch_optional(&mut **tx).await.map_err(db)?.ok_or(Failure::NotFound)?;
    result_row(&row)
}
pub async fn ingest(
    store: &SetupPostgres,
    principal: &Merchant,
    policy: CommercePolicy,
    event: Conversion,
) -> Result<EventResult> {
    principal.require(KeyScope::ConversionsWrite)?;
    let event = event.normalize()?;
    let digest = fingerprint(&event)?;
    let scope = Scope {
        company: principal.company,
        store: principal.store,
    };
    let mut tx = store.begin_merchant(principal).await?;
    event.validate_delivery(policy, now(&mut tx).await?)?;
    if let Some(row)=sqlx::query("SELECT id,fingerprint FROM attribution_merchant_events WHERE company_id=$1 AND store_id=$2 AND external_event_id=$3")
        .bind(scope.company.0).bind(scope.store.0).bind(&event.event_key().0).fetch_optional(&mut *tx).await.map_err(db)? {
        if row.try_get::<Vec<u8>,_>("fingerprint").map_err(db)? != digest { tracing::warn!(operation="merchant_delivery",outcome="conflicting_identity"); return Err(Failure::Conflict); }
        let result=event_result(&mut tx, scope.company, EventId(row.try_get("id").map_err(db)?)).await?;
        tx.commit().await.map_err(|_| Failure::Unavailable)?;
        tracing::info!(operation="merchant_delivery", event_id=%result.id.0, outcome="duplicate", state=result.state.as_str());
        return Ok(result);
    }
    sqlx::query("INSERT INTO attribution_orders(id,company_id,store_id,external_order_id) VALUES($1,$2,$3,$4) ON CONFLICT(company_id,store_id,external_order_id) DO NOTHING")
        .bind(Uuid::new_v4()).bind(scope.company.0).bind(scope.store.0).bind(&event.order_key().0).execute(&mut *tx).await.map_err(db)?;
    let order=OrderId(sqlx::query_scalar("SELECT id FROM attribution_orders WHERE company_id=$1 AND store_id=$2 AND external_order_id=$3 FOR UPDATE")
        .bind(scope.company.0).bind(scope.store.0).bind(&event.order_key().0).fetch_one(&mut *tx).await.map_err(db)?);
    let id = EventId(Uuid::new_v4());
    sqlx::query("INSERT INTO attribution_merchant_events(id,company_id,store_id,order_id,external_event_id,kind,fingerprint,envelope,occurred_at,state,next_attempt_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'pending',clock_timestamp())")
        .bind(id.0).bind(scope.company.0).bind(scope.store.0).bind(order.0).bind(&event.event_key().0).bind(event.kind()).bind(digest)
        .bind(serde_json::to_value(&event).map_err(|_|Failure::Unavailable)?).bind(event.occurred_at()).execute(&mut *tx).await.map_err(db)?;
    let outcome = super::settle::apply(&mut tx, scope, order, &event, policy).await?;
    sqlx::query("UPDATE attribution_merchant_events SET state=$2,reason=$3,next_attempt_at=CASE WHEN $2='pending' THEN clock_timestamp()+interval '5 seconds' ELSE NULL END WHERE id=$1")
        .bind(id.0).bind(outcome.state.as_str()).bind(reason_value(outcome.reason)?).execute(&mut *tx).await.map_err(db)?;
    let result = event_result(&mut tx, scope.company, id).await?;
    let attribution: Option<String> = sqlx::query_scalar(
        "SELECT attribution_reason FROM attribution_orders WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.0)
    .bind(order.0)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    tx.commit().await.map_err(|_| Failure::Unavailable)?;
    tracing::info!(operation="merchant_delivery", event_id=%id.0, outcome="committed", state=result.state.as_str(),
        lag_seconds=(result.received_at-result.occurred_at).num_seconds(), attribution_reason=attribution.as_deref().unwrap_or("payment_pending"));
    Ok(result)
}
