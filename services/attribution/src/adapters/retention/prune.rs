use super::*;

// Child tables precede parents: no ON DELETE cascade ever has work to do.
const FINANCIAL: [&str; 5] = [
    "attribution_merchant_events",
    "attribution_refund_lines",
    "attribution_refunds",
    "attribution_order_lines",
    "attribution_orders",
];
const OTHER: [&str; 12] = [
    "attribution_checkouts",
    "attribution_observations",
    "attribution_clicks",
    "attribution_links",
    "attribution_ads",
    "attribution_campaigns",
    "attribution_credentials",
    "attribution_installations",
    "attribution_domains",
    "attribution_management_receipts",
    "attribution_audit",
    "attribution_stores",
];

pub(super) async fn purge(tx: &mut Tx, company: Uuid, remaining: &mut i64) -> Result<bool> {
    for table in FINANCIAL.into_iter().chain(OTHER) {
        let predicate = if table == "attribution_credentials" {
            "NOT EXISTS(SELECT 1 FROM attribution_credentials c WHERE c.company_id=t.company_id AND c.rotated_to=t.id)"
        } else {
            "true"
        };
        delete(tx, company, table, predicate, remaining).await?;
        // A full chunk must stop before any parent is touched. Rotation chains may
        // expose the next leaf only after this statement: resume the same table.
        if *remaining == 0 {
            return Ok(false);
        }
        let left: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS(SELECT 1 FROM {table} WHERE company_id=$1)"
        ))
        .bind(company)
        .fetch_one(&mut **tx)
        .await
        .map_err(db)?;
        if left {
            return Ok(false);
        }
    }
    sqlx::query("UPDATE attribution_tenants SET purged_at=clock_timestamp(),display_name=NULL,status_valid_until=NULL,observation_number=0,reconcile_execution=NULL,reconcile_lease_until=NULL,reconcile_failures=0,last_reconcile_failure=NULL WHERE company_id=$1")
        .bind(company).execute(&mut **tx).await.map_err(db)?;
    Ok(true)
}

pub(super) async fn retained(
    tx: &mut Tx,
    company: Uuid,
    policy: RetentionPolicy,
    remaining: &mut i64,
) -> Result<()> {
    // transaction_timestamp fixes all cutoffs for one batch. Eligibility is strict:
    // a fact exactly at its configured boundary is still retained.
    for (table, predicate) in [
        (
            "attribution_observations",
            format!(
                "t.received_at < transaction_timestamp()-interval '{} days'",
                policy.observations
            ),
        ),
        (
            "attribution_checkouts",
            format!(
                "t.received_at < transaction_timestamp()-interval '{} days' AND NOT EXISTS(SELECT 1 FROM attribution_merchant_events e WHERE e.company_id=t.company_id AND e.store_id=t.store_id AND e.envelope->>'checkout_id'=t.checkout_id AND (e.state IN ('pending','unresolved') OR e.execution IS NOT NULL))",
                policy.clicks
            ),
        ),
        (
            "attribution_clicks",
            format!(
                "t.clicked_at < transaction_timestamp()-interval '{} days' AND NOT EXISTS(SELECT 1 FROM attribution_checkouts c WHERE c.company_id=t.company_id AND c.store_id=t.store_id AND c.click_id=t.id)",
                policy.clicks
            ),
        ),
        (
            "attribution_merchant_events",
            format!(
                "t.received_at < transaction_timestamp()-interval '{} days' AND t.state IN ('accepted','conflict') AND t.execution IS NULL",
                policy.events
            ),
        ),
        (
            "attribution_management_receipts",
            "t.created_at < transaction_timestamp()-interval '48 hours'".to_owned(),
        ),
        (
            "attribution_audit",
            format!(
                "t.created_at < transaction_timestamp()-interval '{} days'",
                policy.audit
            ),
        ),
    ] {
        delete(tx, company, table, &predicate, remaining).await?;
    }
    if *remaining == 0 {
        return Ok(());
    }
    // Retire the whole financial fact before any of its parts disappear. Retain
    // terminal deliveries for their own configured lifetime as well.
    let marked=sqlx::query(r#"UPDATE attribution_orders SET retired_at=clock_timestamp() WHERE id IN (
        SELECT o.id FROM attribution_orders o WHERE o.company_id=$1 AND o.retired_at IS NULL
        AND greatest(o.received_at,o.paid_at)<transaction_timestamp()-make_interval(days=>$2)
        AND NOT EXISTS(SELECT 1 FROM attribution_merchant_events e WHERE e.company_id=o.company_id AND e.order_id=o.id
            AND (e.state IN ('pending','unresolved') OR e.execution IS NOT NULL OR e.received_at>=transaction_timestamp()-make_interval(days=>$3)))
        ORDER BY o.id LIMIT $4 FOR UPDATE
    )"#).bind(company).bind(policy.financial as i32).bind(policy.events as i32).bind(*remaining)
        .execute(&mut **tx).await.map_err(db)?.rows_affected();
    *remaining -= marked as i64;
    for table in FINANCIAL {
        let predicate = if table == "attribution_orders" {
            "t.retired_at IS NOT NULL".to_owned()
        } else {
            "EXISTS(SELECT 1 FROM attribution_orders o WHERE o.company_id=t.company_id AND o.id=t.order_id AND o.retired_at IS NOT NULL)".to_owned()
        };
        delete(tx, company, table, &predicate, remaining).await?;
        if *remaining == 0 {
            return Ok(());
        }
    }
    Ok(())
}
