use super::postgres::*;
use crate::{
    adapters::{
        management::{Actor, Receipt},
        postgres::{Tx, failure as db},
        setup::postgres::SetupPostgres,
    },
    application::{
        catalog::{AdId, ClickId},
        commerce::*,
        identity::{CompanyId, Failure, Permission, Result, TenantPrincipal},
        setup::{Retry, StoreId},
        website::parse_time,
    },
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Resource {
    Orders,
    Refunds,
    Events,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    pub limit: Option<u16>,
    pub cursor: Option<String>,
    pub store_id: Option<StoreId>,
    pub state: Option<EventState>,
}
#[derive(Serialize, Deserialize)]
struct Cursor {
    company: CompanyId,
    resource: Resource,
    store: Option<StoreId>,
    state: Option<EventState>,
    parent: Option<OrderId>,
    after: Uuid,
}
async fn verify_store(tx: &mut Tx, company: CompanyId, store: StoreId) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM attribution_stores WHERE company_id=$1 AND id=$2)",
    )
    .bind(company.0)
    .bind(store.0)
    .fetch_one(&mut **tx)
    .await
    .map_err(db)?;
    if exists {
        Ok(())
    } else {
        Err(Failure::NotFound)
    }
}
async fn order_value(tx: &mut Tx, company: CompanyId, id: OrderId) -> Result<Value> {
    let row = sqlx::query(
        "SELECT * FROM attribution_orders WHERE company_id=$1 AND id=$2 AND retired_at IS NULL AND paid_at IS NOT NULL",
    )
    .bind(company.0)
    .bind(id.0)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db)?
    .ok_or(Failure::NotFound)?;
    let mut lines = Vec::new();
    for line in sqlx::query("SELECT line_id,product_id,quantity,merchandise_amount FROM attribution_order_lines WHERE company_id=$1 AND order_id=$2 ORDER BY line_id")
        .bind(company.0).bind(id.0).fetch_all(&mut **tx).await.map_err(db)? {
        lines.push(OrderLine {line_id:LineId(line.try_get("line_id").map_err(db)?),product_id:ProductId(line.try_get("product_id").map_err(db)?),
            quantity:u32::try_from(line.try_get::<i64,_>("quantity").map_err(db)?).map_err(|_|Failure::Unavailable)?,merchandise_amount:line.try_get("merchandise_amount").map_err(db)?});
    }
    let order = Order {
        id,
        store_id: StoreId(row.try_get("store_id").map_err(db)?),
        external_order_id: OrderKey(row.try_get("external_order_id").map_err(db)?),
        order_created_at: parse_time(&row.try_get::<String, _>("order_created_at").map_err(db)?)?,
        paid_at: parse_time(&row.try_get::<String, _>("paid_original").map_err(db)?)?,
        currency: decode(Value::String(row.try_get("currency").map_err(db)?))?,
        lines,
        merchandise_amount: row.try_get("merchandise_amount").map_err(db)?,
        tax_amount: row.try_get("tax_amount").map_err(db)?,
        shipping_amount: row.try_get("shipping_amount").map_err(db)?,
        refunded_merchandise_amount: row.try_get("refunded_merchandise_amount").map_err(db)?,
        refunded_tax_amount: row.try_get("refunded_tax_amount").map_err(db)?,
        refunded_shipping_amount: row.try_get("refunded_shipping_amount").map_err(db)?,
        returned_units: row.try_get("returned_units").map_err(db)?,
        click_id: row
            .try_get::<Option<Uuid>, _>("click_id")
            .map_err(db)?
            .map(ClickId),
        ad_id: row
            .try_get::<Option<Uuid>, _>("ad_id")
            .map_err(db)?
            .map(AdId),
        attribution_reason: decode(Value::String(
            row.try_get("attribution_reason").map_err(db)?,
        ))?,
        policy: row.try_get("policy").map_err(db)?,
    };
    serde_json::to_value(order).map_err(|_| Failure::Unavailable)
}
pub(crate) async fn read_order(
    store: &SetupPostgres,
    p: &TenantPrincipal,
    id: OrderId,
) -> Result<Value> {
    let mut tx = store.user_transaction(p).await?;
    let value = order_value(&mut tx, p.access.company_id, id).await?;
    tx.commit().await.map_err(db)?;
    Ok(value)
}
pub(crate) async fn list(
    store: &SetupPostgres,
    p: &TenantPrincipal,
    resource: Resource,
    parent: Option<OrderId>,
    q: ListQuery,
) -> Result<Value> {
    let limit = q.limit.unwrap_or(25);
    if !(1..=100).contains(&limit)
        || q.cursor
            .as_ref()
            .is_some_and(|c| c.is_empty() || c.len() > 512)
        || (resource != Resource::Events && q.state.is_some())
    {
        return Err(Failure::Invalid);
    }
    let company = p.access.company_id;
    let after = match &q.cursor {
        None => Uuid::nil(),
        Some(raw) => {
            let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| Failure::Invalid)?;
            let clear = store
                .cipher
                .open("commerce-cursor", Uuid::nil(), &bytes)
                .map_err(|_| Failure::Invalid)?;
            let cursor: Cursor = serde_json::from_slice(&clear).map_err(|_| Failure::Invalid)?;
            if cursor.company != company
                || cursor.resource != resource
                || cursor.store != q.store_id
                || cursor.state != q.state
                || cursor.parent != parent
            {
                return Err(Failure::Invalid);
            }
            cursor.after
        }
    };
    let mut tx = store.user_transaction(p).await?;
    if let Some(id) = q.store_id {
        verify_store(&mut tx, company, id).await?;
    }
    if let Some(id) = parent {
        let parent_store:Uuid=sqlx::query_scalar("SELECT store_id FROM attribution_orders WHERE company_id=$1 AND id=$2 AND retired_at IS NULL AND paid_at IS NOT NULL")
            .bind(company.0).bind(id.0).fetch_optional(&mut *tx).await.map_err(db)?.ok_or(Failure::NotFound)?;
        if q.store_id.is_some_and(|s| s.0 != parent_store) {
            return Err(Failure::NotFound);
        }
    }
    let sql = match resource {
        Resource::Orders => {
            "SELECT id FROM attribution_orders WHERE company_id=$1 AND id>$2 AND ($3::uuid IS NULL OR store_id=$3) AND retired_at IS NULL AND paid_at IS NOT NULL AND $4::uuid IS NULL AND $5::text IS NULL ORDER BY id LIMIT $6"
        }
        Resource::Refunds => {
            "SELECT id FROM attribution_refunds WHERE company_id=$1 AND id>$2 AND ($3::uuid IS NULL OR store_id=$3) AND order_id=$4 AND applied_at IS NOT NULL AND $5::text IS NULL ORDER BY id LIMIT $6"
        }
        Resource::Events => {
            "SELECT id FROM attribution_merchant_events WHERE company_id=$1 AND id>$2 AND ($3::uuid IS NULL OR store_id=$3) AND $4::uuid IS NULL AND ($5::text IS NULL OR state=$5) ORDER BY id LIMIT $6"
        }
    };
    let mut ids: Vec<Uuid> = sqlx::query_scalar(sql)
        .bind(company.0)
        .bind(after)
        .bind(q.store_id.map(|s| s.0))
        .bind(parent.map(|o| o.0))
        .bind(q.state.map(EventState::as_str))
        .bind(i64::from(limit) + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
    let next = if ids.len() > usize::from(limit) {
        ids.pop();
        let cursor = Cursor {
            company,
            resource,
            store: q.store_id,
            state: q.state,
            parent,
            after: *ids.last().ok_or(Failure::Unavailable)?,
        };
        Some(URL_SAFE_NO_PAD.encode(store.cipher.seal(
            "commerce-cursor",
            Uuid::nil(),
            &serde_json::to_vec(&cursor).map_err(|_| Failure::Unavailable)?,
        )?))
    } else {
        None
    };
    let mut items = Vec::new();
    for id in ids {
        items.push(match resource {
            Resource::Orders=>order_value(&mut tx,company,OrderId(id)).await?,
            Resource::Events=>serde_json::to_value(event_result(&mut tx,company,EventId(id)).await?).map_err(|_|Failure::Unavailable)?,
            Resource::Refunds=>{
                let envelope:Value=sqlx::query_scalar("SELECT envelope FROM attribution_refunds WHERE company_id=$1 AND id=$2 AND applied_at IS NOT NULL")
                    .bind(company.0).bind(id).fetch_one(&mut *tx).await.map_err(db)?;
                let Conversion::Refund(e)=decode(envelope)? else {return Err(Failure::Unavailable);};
                serde_json::to_value(Refund {refund_id:e.refund_id,occurred_at:e.occurred_at,currency:e.currency,lines:e.lines,tax_amount:e.tax_amount,shipping_amount:e.shipping_amount}).map_err(|_|Failure::Unavailable)?
            }
        });
    }
    tx.commit().await.map_err(db)?;
    Ok(json!({"items":items,"next_cursor":next}))
}
pub(super) async fn retry_event(
    store: &SetupPostgres,
    p: &TenantPrincipal,
    id: EventId,
    retry: Retry,
) -> Result<Value> {
    if !p
        .access
        .permissions
        .contains(&Permission::ManageIntegrations)
    {
        return Err(Failure::Forbidden);
    }
    let receipt = Receipt::new(
        Actor::user(p),
        format!("POST /v1/merchant-events/{}/retry", id.0),
        retry,
        &id,
    )?;
    let mut tx = store.user_transaction(p).await?;
    if let Some(body) = receipt.replay(&mut tx).await? {
        tx.commit().await.map_err(db)?;
        return Ok(body);
    }
    let row=sqlx::query("SELECT state,store_id FROM attribution_merchant_events WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(p.access.company_id.0).bind(id.0).fetch_optional(&mut *tx).await.map_err(db)?.ok_or(Failure::NotFound)?;
    crate::adapters::setup::postgres::enabled_store(
        &mut tx,
        p.access.company_id,
        StoreId(row.try_get("store_id").map_err(db)?),
    )
    .await?;
    if row.try_get::<String, _>("state").map_err(db)? == "unresolved" {
        sqlx::query("UPDATE attribution_merchant_events SET state='pending',reason='payment_missing',attempts=0,next_attempt_at=clock_timestamp(),deadline_at=clock_timestamp()+interval '7 days',execution=NULL,lease_until=NULL WHERE id=$1")
            .bind(id.0).execute(&mut *tx).await.map_err(db)?;
    }
    let body = serde_json::to_value(event_result(&mut tx, p.access.company_id, id).await?)
        .map_err(|_| Failure::Unavailable)?;
    receipt
        .record(&mut tx, body.clone(), "merchant_event.retry", id.0)
        .await?;
    tx.commit().await.map_err(|_| Failure::Unavailable)?;
    Ok(body)
}
