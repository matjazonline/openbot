use super::postgres::*;
use crate::{
    adapters::postgres::{Tx, failure as db},
    application::{
        commerce::*,
        identity::{Failure, Result},
        website::{AttributionReason, Candidate, POLICY, parse_time},
    },
};
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

pub(super) async fn apply(
    tx: &mut Tx,
    scope: Scope,
    order: OrderId,
    event: &Conversion,
    policy: CommercePolicy,
) -> Result<Outcome> {
    let row = order_lock(tx, scope, order).await?;
    match event {
        Conversion::Paid(paid) => pay(tx, scope, order, &row, paid, event).await,
        Conversion::Refund(refund) => {
            refund_order(tx, scope, order, &row, refund, event, policy).await
        }
        Conversion::Cancelled(cancelled) => {
            sqlx::query(
                "UPDATE attribution_orders SET cancelled_at=LEAST(cancelled_at,$2) WHERE id=$1",
            )
            .bind(order.0)
            .bind(cancelled.occurred_at)
            .execute(&mut **tx)
            .await
            .map_err(db)?;
            Ok(Outcome::accepted())
        }
    }
}
struct Attribution {
    reason: AttributionReason,
    selected: Option<Candidate>,
}
async fn attribution(
    tx: &mut Tx,
    scope: Scope,
    paid: &PaidEvent,
) -> Result<std::result::Result<Attribution, EventReason>> {
    let unassigned = |reason| {
        Ok(Ok(Attribution {
            reason,
            selected: None,
        }))
    };
    let Some(checkout) = &paid.checkout_id else {
        return unassigned(AttributionReason::Missing);
    };
    let row=sqlx::query("SELECT order_created_at,click_id,reason,policy,evidence FROM attribution_checkouts WHERE company_id=$1 AND store_id=$2 AND checkout_id=$3")
        .bind(scope.company.0).bind(scope.store.0).bind(&checkout.0).fetch_optional(&mut **tx).await.map_err(db)?;
    let Some(row) = row else {
        return unassigned(AttributionReason::EvidenceUnavailable);
    };
    if parse_time(&row.try_get::<String, _>("order_created_at").map_err(db)?)?
        != paid.order_created_at
    {
        return Ok(Err(EventReason::CheckoutTimeMismatch));
    }
    let reason: AttributionReason = decode(Value::String(row.try_get("reason").map_err(db)?))?;
    if reason != AttributionReason::Attributed {
        return unassigned(reason);
    }
    if row.try_get::<String, _>("policy").map_err(db)? != POLICY {
        return unassigned(AttributionReason::EvidenceUnavailable);
    }
    let value: Option<Value> = row.try_get("evidence").map_err(db)?;
    let selected = match value.and_then(|v| serde_json::from_value::<Candidate>(v).ok()) {
        Some(c) => c,
        None => return unassigned(AttributionReason::EvidenceUnavailable),
    };
    if Some(selected.id.0) != row.try_get::<Option<Uuid>, _>("click_id").map_err(db)? {
        return unassigned(AttributionReason::EvidenceUnavailable);
    }
    let raw=sqlx::query("SELECT clicked_at,is_bot,is_prefetch,classifier_version,dimensions FROM attribution_clicks WHERE company_id=$1 AND store_id=$2 AND id=$3")
        .bind(scope.company.0).bind(scope.store.0).bind(selected.id.0).fetch_optional(&mut **tx).await.map_err(db)?;
    let Some(raw) = raw else {
        return unassigned(AttributionReason::EvidenceUnavailable);
    };
    if raw
        .try_get::<chrono::DateTime<chrono::Utc>, _>("clicked_at")
        .map_err(db)?
        != selected.clicked_at
        || raw.try_get::<bool, _>("is_bot").map_err(db)? != selected.is_bot
        || raw.try_get::<bool, _>("is_prefetch").map_err(db)? != selected.is_prefetch
        || raw.try_get::<i16, _>("classifier_version").map_err(db)? != selected.classifier_version
        || raw.try_get::<Value, _>("dimensions").map_err(db)? != selected.dimensions
    {
        return unassigned(AttributionReason::EvidenceUnavailable);
    }
    if selected.clicked_at > paid.order_created_at {
        return unassigned(AttributionReason::Future);
    }
    if selected.clicked_at < paid.order_created_at - chrono::Duration::days(30) {
        return unassigned(AttributionReason::Expired);
    }
    if selected.is_bot || selected.is_prefetch {
        return unassigned(AttributionReason::Bot);
    }
    if selected.dimensions.get("company_id") != Some(&serde_json::json!(scope.company.0))
        || selected.dimensions.get("store_id") != Some(&serde_json::json!(scope.store.0))
        || selected
            .dimensions
            .get("ad_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .is_none()
    {
        return unassigned(AttributionReason::EvidenceUnavailable);
    }
    Ok(Ok(Attribution {
        reason: AttributionReason::Attributed,
        selected: Some(selected),
    }))
}
async fn pay(
    tx: &mut Tx,
    scope: Scope,
    order: OrderId,
    row: &sqlx::postgres::PgRow,
    paid: &PaidEvent,
    event: &Conversion,
) -> Result<Outcome> {
    let digest = fingerprint(&event.business_fact()?)?;
    if let Some(previous) = row
        .try_get::<Option<Vec<u8>>, _>("paid_fingerprint")
        .map_err(db)?
    {
        return Ok(if previous == digest {
            Outcome::accepted()
        } else {
            Outcome::conflict(EventReason::PaidConflict)
        });
    }
    let attribution = match attribution(tx, scope, paid).await? {
        Ok(a) => a,
        Err(reason) => return Ok(Outcome::conflict(reason)),
    };
    let selected = attribution.selected;
    let click = selected.as_ref().map(|c| c.id.0);
    let dimensions = selected.as_ref().map(|c| c.dimensions.clone());
    let ad = dimensions
        .as_ref()
        .and_then(|d| d["ad_id"].as_str())
        .and_then(|id| Uuid::parse_str(id).ok());
    let evidence = selected
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| Failure::Unavailable)?;
    let reason = serde_json::to_value(attribution.reason).map_err(|_| Failure::Unavailable)?;
    sqlx::query("UPDATE attribution_orders SET paid_fingerprint=$2,order_created_at=$3,paid_original=$4,paid_at=$5,currency=$6,merchandise_amount=$7,tax_amount=$8,shipping_amount=$9,click_id=$10,ad_id=$11,attribution_reason=$12,policy=$13,evidence=$14,dimensions=$15 WHERE id=$1 AND paid_fingerprint IS NULL")
        .bind(order.0).bind(digest).bind(paid.order_created_at.to_rfc3339()).bind(paid.paid_at.to_rfc3339()).bind(paid.paid_at).bind(paid.currency.as_str())
        .bind(paid.merchandise_amount).bind(paid.tax_amount).bind(paid.shipping_amount).bind(click).bind(ad).bind(reason.as_str().ok_or(Failure::Unavailable)?).bind(POLICY).bind(evidence).bind(dimensions).execute(&mut **tx).await.map_err(db)?;
    for line in &paid.lines {
        sqlx::query("INSERT INTO attribution_order_lines(company_id,store_id,order_id,line_id,product_id,quantity,merchandise_amount) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(scope.company.0).bind(scope.store.0).bind(order.0).bind(&line.line_id.0).bind(&line.product_id.0).bind(i64::from(line.quantity)).bind(line.merchandise_amount).execute(&mut **tx).await.map_err(db)?;
    }
    Ok(Outcome::accepted())
}
async fn reserve_refund(
    tx: &mut Tx,
    scope: Scope,
    order: OrderId,
    refund: &RefundEvent,
    event: &Conversion,
) -> Result<std::result::Result<(Uuid, bool), EventReason>> {
    let digest = fingerprint(&event.business_fact()?)?;
    if let Some(row)=sqlx::query("SELECT id,fingerprint,order_id,applied_at IS NOT NULL AS applied FROM attribution_refunds WHERE company_id=$1 AND store_id=$2 AND external_refund_id=$3 FOR UPDATE")
        .bind(scope.company.0).bind(scope.store.0).bind(&refund.refund_id.0).fetch_optional(&mut **tx).await.map_err(db)? {
        if row.try_get::<Vec<u8>,_>("fingerprint").map_err(db)?!=digest || row.try_get::<Uuid,_>("order_id").map_err(db)?!=order.0 {
            return Ok(Err(EventReason::RefundConflict));
        }
        return Ok(Ok((row.try_get("id").map_err(db)?,row.try_get("applied").map_err(db)?)));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO attribution_refunds(id,company_id,store_id,order_id,external_refund_id,fingerprint,envelope,occurred_at,currency,merchandise_amount,tax_amount,shipping_amount) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
        .bind(id).bind(scope.company.0).bind(scope.store.0).bind(order.0).bind(&refund.refund_id.0).bind(digest).bind(serde_json::to_value(event).map_err(|_|Failure::Unavailable)?)
        .bind(refund.occurred_at).bind(refund.currency.as_str()).bind(total(refund.lines.iter().map(|l|l.merchandise_amount))?).bind(refund.tax_amount).bind(refund.shipping_amount).execute(&mut **tx).await.map_err(db)?;
    Ok(Ok((id, false)))
}
async fn balance(
    tx: &mut Tx,
    scope: Scope,
    order: OrderId,
    row: &sqlx::postgres::PgRow,
) -> Result<PaymentBalance> {
    let mut lines = Vec::new();
    for line in sqlx::query("SELECT line_id,quantity,merchandise_amount,returned_quantity,refunded_amount FROM attribution_order_lines WHERE company_id=$1 AND store_id=$2 AND order_id=$3 ORDER BY line_id")
        .bind(scope.company.0).bind(scope.store.0).bind(order.0).fetch_all(&mut **tx).await.map_err(db)? {
        lines.push(LineBalance { line_id: LineId(line.try_get("line_id").map_err(db)?), quantity:line.try_get("quantity").map_err(db)?, merchandise_amount:line.try_get("merchandise_amount").map_err(db)?, returned_quantity:line.try_get("returned_quantity").map_err(db)?, refunded_amount:line.try_get("refunded_amount").map_err(db)? });
    }
    Ok(PaymentBalance {
        paid_at: parse_time(&row.try_get::<String, _>("paid_original").map_err(db)?)?,
        currency: decode(Value::String(row.try_get("currency").map_err(db)?))?,
        tax_remaining: row.try_get::<i64, _>("tax_amount").map_err(db)?
            - row.try_get::<i64, _>("refunded_tax_amount").map_err(db)?,
        shipping_remaining: row.try_get::<i64, _>("shipping_amount").map_err(db)?
            - row
                .try_get::<i64, _>("refunded_shipping_amount")
                .map_err(db)?,
        lines,
    })
}
async fn refund_order(
    tx: &mut Tx,
    scope: Scope,
    order: OrderId,
    row: &sqlx::postgres::PgRow,
    refund: &RefundEvent,
    event: &Conversion,
    policy: CommercePolicy,
) -> Result<Outcome> {
    let (id, applied) = match reserve_refund(tx, scope, order, refund, event).await? {
        Ok(value) => value,
        Err(reason) => return Ok(Outcome::conflict(reason)),
    };
    if applied {
        return Ok(Outcome::accepted());
    }
    if row
        .try_get::<Option<Vec<u8>>, _>("paid_fingerprint")
        .map_err(db)?
        .is_none()
    {
        return Ok(Outcome::pending());
    }
    if let Some(reason) = refund_conflict(refund, &balance(tx, scope, order, row).await?, policy) {
        return Ok(Outcome::conflict(reason));
    }
    for line in &refund.lines {
        sqlx::query("UPDATE attribution_order_lines SET returned_quantity=returned_quantity+$5,refunded_amount=refunded_amount+$6 WHERE company_id=$1 AND store_id=$2 AND order_id=$3 AND line_id=$4")
            .bind(scope.company.0).bind(scope.store.0).bind(order.0).bind(&line.line_id.0).bind(i64::from(line.returned_quantity)).bind(line.merchandise_amount).execute(&mut **tx).await.map_err(db)?;
        sqlx::query("INSERT INTO attribution_refund_lines(company_id,store_id,order_id,refund_id,line_id,returned_quantity,merchandise_amount) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(scope.company.0).bind(scope.store.0).bind(order.0).bind(id).bind(&line.line_id.0).bind(i64::from(line.returned_quantity)).bind(line.merchandise_amount).execute(&mut **tx).await.map_err(db)?;
    }
    let merchandise = total(refund.lines.iter().map(|l| l.merchandise_amount))?;
    let units: i64 = refund
        .lines
        .iter()
        .map(|l| i64::from(l.returned_quantity))
        .sum();
    sqlx::query("UPDATE attribution_orders SET refunded_merchandise_amount=refunded_merchandise_amount+$2,refunded_tax_amount=refunded_tax_amount+$3,refunded_shipping_amount=refunded_shipping_amount+$4,returned_units=returned_units+$5 WHERE id=$1")
        .bind(order.0).bind(merchandise).bind(refund.tax_amount).bind(refund.shipping_amount).bind(units).execute(&mut **tx).await.map_err(db)?;
    sqlx::query("UPDATE attribution_refunds SET applied_at=clock_timestamp() WHERE id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(db)?;
    Ok(Outcome::accepted())
}
