use crate::application::{
    analytics::*,
    identity::{Failure, Result},
};
fn text(value: &str) -> String {
    let safe = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.into()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}
pub fn csv(report: &Analytics, grouping: Option<Grouping>) -> Result<String> {
    let group_by = match grouping {
        None => "overview",
        Some(Grouping::Day) => "day",
        Some(Grouping::Ad) => "ad",
        Some(Grouping::Campaign) => "campaign",
        Some(Grouping::AdvertisedProduct) => "advertised_product",
        Some(Grouping::SoldProduct) => "sold_product",
    };
    let fields = [
        "paid_orders",
        "attributed_paid_orders",
        "sold_units",
        "returned_units",
        "merchandise_amount",
        "refunded_merchandise_amount",
        "net_merchandise_amount",
        "tax_amount",
        "shipping_amount",
        "refunded_tax_amount",
        "refunded_shipping_amount",
        "refunds",
    ];
    let mut output = String::from(
        "group,start_utc,end_utc,timezone,group_by,click_observation_basis,financial_basis,nonadditive_measures,raw_clicks,eligible_clicks,image_loads,landing_page_loads,visible_placements,attributed_paid_orders,unattributed_paid_orders,unattributed_reasons,period_conversion_rate,rate_denominator,currency",
    );
    for f in fields {
        output.push_str(",currency_");
        output.push_str(f);
    }
    output.push_str("\r\n");
    let mut count = 0;
    for group in &report.groups {
        for index in 0..group.currencies.len().max(1) {
            count += 1;
            if count > MAX_EXPORT_ROWS {
                return Err(Failure::Invalid);
            }
            let mut row = vec![
                text(group.group.as_deref().unwrap_or("")),
                text(&report.start.to_rfc3339()),
                text(&report.end.to_rfc3339()),
                text(&report.timezone),
                text(group_by),
                text(if group.rate_denominator == "omitted_sold_product_filter" {
                    "Contextual selected-scope counts, not purchased-product clicks/observations"
                } else {
                    "Selected company/store/ad/campaign/advertised-product scope"
                }),
                text(if group.rate_denominator == "omitted_sold_product_filter" {
                    "Matching product lines for money/units; whole matching orders/refunds for counts/tax/shipping"
                } else {
                    "Selected orders/refunds and their lines"
                }),
                text(if grouping == Some(Grouping::SoldProduct) {
                    "Do not sum across product groups: clicks, observations, order/refund counts, tax/shipping"
                } else {
                    "None across groups; group measures occur only on first currency row"
                }),
            ];
            if index == 0 {
                for value in [
                    group.raw_clicks,
                    group.eligible_clicks,
                    group.image_loads,
                    group.landing_page_loads,
                    group.visible_placements,
                    group.attributed_paid_orders,
                    group.unattributed_paid_orders,
                ] {
                    row.push(value.to_string());
                }
                row.push(text(
                    &serde_json::to_string(&group.unattributed_reasons)
                        .map_err(|_| Failure::Unavailable)?,
                ));
                row.push(
                    group
                        .period_conversion_rate
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                );
                row.push(text(group.rate_denominator));
            } else {
                row.extend((0..10).map(|_| String::new()));
            }
            if let Some(currency) = group.currencies.get(index) {
                row.push(currency.currency.as_str().into());
                let value = serde_json::to_value(currency).map_err(|_| Failure::Unavailable)?;
                for field in fields {
                    row.push(
                        value[field]
                            .as_i64()
                            .ok_or(Failure::Unavailable)?
                            .to_string(),
                    );
                }
            } else {
                row.extend((0..13).map(|_| String::new()));
            }
            output.push_str(&row.join(","));
            output.push_str("\r\n");
        }
    }
    Ok(output)
}
