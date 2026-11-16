use crate::application::analytics::Grouping;

fn group(by: Option<Grouping>, alias: &str, clock: &str, product: &str) -> String {
    match by {
        None => "NULL::text".into(),
        Some(Grouping::Day) => format!("to_char({clock} AT TIME ZONE $10,'YYYY-MM-DD')"),
        Some(Grouping::Ad) => format!("{alias}.dimensions->>'ad_id'"),
        Some(Grouping::Campaign) => format!("{alias}.dimensions->>'campaign_id'"),
        Some(Grouping::AdvertisedProduct) => {
            format!("{alias}.dimensions->>'advertised_product_id'")
        }
        Some(Grouping::SoldProduct) => product.into(),
    }
}
fn dimensions(alias: &str) -> String {
    format!(
        "({alias}.company_id=$1 AND ($2::uuid IS NULL OR {alias}.store_id=$2) AND ($3::uuid IS NULL OR {alias}.dimensions->>'ad_id'=$3::text) AND ($4::uuid IS NULL OR {alias}.dimensions->>'campaign_id'=$4::text) AND ($5::text IS NULL OR {alias}.dimensions->>'advertised_product_id'=$5))"
    )
}
/// Every UNION branch owns its measure's cardinality. No financial fact is
/// multiplied by click, observation, payment-line or refund-line joins.
pub fn statement(by: Option<Grouping>) -> String {
    let cg = group(by, "c", "c.clicked_at", "NULL::text");
    let vg = group(by, "v", "v.observed_at", "NULL::text");
    let og = group(by, "o", "o.paid_at", "p.product_id");
    let lg = group(by, "o", "o.paid_at", "l.product_id");
    let rg = group(by, "o", "r.occurred_at", "p.product_id");
    let rlg = group(by, "o", "r.occurred_at", "l.product_id");
    let cd = dimensions("c");
    let vd = dimensions("v");
    let od = dimensions("o");
    let products = by == Some(Grouping::SoldProduct);
    let order_products = if products {
        "SELECT DISTINCT product_id FROM attribution_order_lines WHERE company_id=o.company_id AND store_id=o.store_id AND order_id=o.id AND ($6::text IS NULL OR product_id=$6)"
    } else {
        "SELECT NULL::text AS product_id"
    };
    let refund_products = if products {
        "SELECT DISTINCT l.product_id FROM attribution_refund_lines f JOIN attribution_order_lines l ON (l.company_id,l.store_id,l.order_id,l.line_id)=(f.company_id,f.store_id,f.order_id,f.line_id) WHERE f.refund_id=r.id AND ($6::text IS NULL OR l.product_id=$6)"
    } else {
        "SELECT NULL::text AS product_id"
    };
    let group_source = if products {
        "currency IS NOT NULL"
    } else if by.is_none() {
        "false"
    } else {
        "true"
    };
    let ungrouped = by.is_none();
    let direct = if products {
        "AND d.currency IS NOT NULL"
    } else {
        ""
    };
    let context = if products {
        "UNION ALL SELECT p.group_key,d.currency,d.metric,d.amount FROM page p CROSS JOIN data d WHERE d.currency IS NULL"
    } else {
        ""
    };
    format!(
        r#"
WITH facts(group_key,currency,metric,amount) AS (
 SELECT {cg},NULL::text,m.metric,m.amount FROM attribution_clicks c
 CROSS JOIN LATERAL (VALUES ('raw_clicks',1::bigint),('eligible_clicks',CASE WHEN NOT(c.is_bot OR c.is_prefetch) THEN 1 ELSE 0 END::bigint)) m(metric,amount)
 WHERE {cd} AND c.clicked_at >= $8 AND c.clicked_at < $9
 UNION ALL
 SELECT {vg},NULL::text,
 CASE v.kind WHEN 'image_load' THEN 'image_loads' WHEN 'landing_page_load' THEN 'landing_page_loads' ELSE 'visible_placements' END,1::bigint
 FROM attribution_observations v WHERE {vd} AND v.observed_at >= $8 AND v.observed_at < $9
 UNION ALL
 SELECT {og},o.currency,m.metric,m.amount FROM attribution_orders o
 CROSS JOIN LATERAL ({order_products}) p
 CROSS JOIN LATERAL (VALUES ('paid_orders',1::bigint),
   ('attributed_paid_orders',CASE WHEN o.attribution_reason='attributed' THEN 1 ELSE 0 END::bigint),
   ('reason_'||o.attribution_reason,CASE WHEN o.attribution_reason='attributed' THEN 0 ELSE 1 END::bigint),
   ('tax_amount',o.tax_amount),('shipping_amount',o.shipping_amount)) m(metric,amount)
 WHERE {od} AND o.retired_at IS NULL AND o.paid_at >= $8 AND o.paid_at < $9 AND ($7::text IS NULL OR o.currency=$7)
 AND ($6::text IS NULL OR EXISTS(SELECT 1 FROM attribution_order_lines l WHERE l.company_id=o.company_id AND l.store_id=o.store_id AND l.order_id=o.id AND l.product_id=$6))
 UNION ALL
 SELECT {lg},o.currency,m.metric,m.amount FROM attribution_orders o
 JOIN attribution_order_lines l ON (l.company_id,l.store_id,l.order_id)=(o.company_id,o.store_id,o.id)
 CROSS JOIN LATERAL (VALUES ('sold_units',l.quantity),('merchandise_amount',l.merchandise_amount)) m(metric,amount)
 WHERE {od} AND o.retired_at IS NULL AND o.paid_at >= $8 AND o.paid_at < $9 AND ($7::text IS NULL OR o.currency=$7) AND ($6::text IS NULL OR l.product_id=$6)
 UNION ALL
 SELECT {rg},r.currency,m.metric,m.amount FROM attribution_refunds r
 JOIN attribution_orders o ON (o.company_id,o.store_id,o.id)=(r.company_id,r.store_id,r.order_id)
 CROSS JOIN LATERAL ({refund_products}) p
 CROSS JOIN LATERAL (VALUES ('refunds',1::bigint),('refunded_tax_amount',r.tax_amount),('refunded_shipping_amount',r.shipping_amount)) m(metric,amount)
 WHERE {od} AND o.retired_at IS NULL AND r.applied_at IS NOT NULL AND r.occurred_at >= $8 AND r.occurred_at < $9 AND ($7::text IS NULL OR r.currency=$7)
 AND ($6::text IS NULL OR EXISTS(SELECT 1 FROM attribution_refund_lines f JOIN attribution_order_lines l ON (l.company_id,l.store_id,l.order_id,l.line_id)=(f.company_id,f.store_id,f.order_id,f.line_id) WHERE f.refund_id=r.id AND l.product_id=$6))
 UNION ALL
 SELECT {rlg},r.currency,m.metric,m.amount FROM attribution_refunds r
 JOIN attribution_orders o ON (o.company_id,o.store_id,o.id)=(r.company_id,r.store_id,r.order_id)
 JOIN attribution_refund_lines f ON (f.company_id,f.store_id,f.order_id,f.refund_id)=(r.company_id,r.store_id,r.order_id,r.id)
 JOIN attribution_order_lines l ON (l.company_id,l.store_id,l.order_id,l.line_id)=(f.company_id,f.store_id,f.order_id,f.line_id)
 CROSS JOIN LATERAL (VALUES ('returned_units',f.returned_quantity),('refunded_merchandise_amount',f.merchandise_amount)) m(metric,amount)
 WHERE {od} AND o.retired_at IS NULL AND r.applied_at IS NOT NULL AND r.occurred_at >= $8 AND r.occurred_at < $9 AND ($7::text IS NULL OR r.currency=$7) AND ($6::text IS NULL OR l.product_id=$6)
), data AS MATERIALIZED (
 SELECT group_key,currency,metric,sum(amount) AS amount FROM facts
 GROUP BY group_key,currency,metric
), groups AS (SELECT DISTINCT group_key FROM data WHERE {group_source}
 UNION SELECT NULL::text WHERE {ungrouped}), page AS (
 SELECT group_key FROM groups WHERE $10::text IS NOT NULL AND (NOT $11::bool OR ($12::text IS NULL AND group_key IS NOT NULL) OR group_key COLLATE "C">$12 COLLATE "C")
 ORDER BY group_key COLLATE "C" NULLS FIRST LIMIT $13
), joined AS (
 SELECT p.group_key,d.currency,d.metric,d.amount FROM page p
 LEFT JOIN data d ON coalesce(d.group_key,'')=coalesce(p.group_key,'') AND (d.group_key IS NULL)=(p.group_key IS NULL) {direct}
 {context}
)
SELECT group_key,currency,metric,sum(amount)::text AS total FROM joined
GROUP BY group_key,currency,metric ORDER BY group_key COLLATE "C" NULLS FIRST,currency,metric
"#
    )
}
