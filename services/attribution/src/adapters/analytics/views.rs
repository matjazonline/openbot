use super::dashboard::Form;
use crate::{
    adapters::html::escape,
    application::{
        analytics::*,
        identity::{CompanyAccess, Failure, Permission, Result, TenantPrincipal},
    },
};
use serde_json::Value;
use uuid::Uuid;

pub(super) fn page(
    company: &CompanyAccess,
    authority: &url::Url,
    title: &str,
    body: &str,
) -> String {
    let id = company.company_id.0;
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"/assets/app.css\"><link rel=\"stylesheet\" href=\"/assets/reports.css\"><script src=\"/assets/reports.js\" defer></script></head><body><a href=\"#main\">Skip to content</a><header><nav aria-label=\"Main\"><a href=\"/\">Choose company</a> <a href=\"/reports?company_id={id}\">Reports</a> <a href=\"/reports/orders?company_id={id}\">Orders and refunds</a> <a href=\"/reports/events?company_id={id}\">Merchant events</a> <a href=\"/settings?company_id={id}\">Tracking setup and keys</a> <a href=\"{}ui/companies\" rel=\"noopener noreferrer\">Company and members in Openbot</a> <a href=\"{}ui/profile\" rel=\"noopener noreferrer\">Account in Openbot</a></nav></header><main id=\"main\" tabindex=\"-1\"><h1>{}</h1><p>Company: {}</p>{body}<p id=\"report-status\" role=\"status\" aria-live=\"polite\"></p></main></body></html>",
        escape(title),
        escape(authority.as_str()),
        escape(authority.as_str()),
        escape(title),
        escape(&company.name)
    )
}
fn input(name: &str, label: &str, value: &str, kind: &str, list: Option<&str>) -> String {
    let list = list
        .map(|v| format!(" list=\"{}\"", escape(v)))
        .unwrap_or_default();
    format!(
        "<label>{}<input name=\"{}\" type=\"{}\" value=\"{}\"{}></label>",
        escape(label),
        escape(name),
        escape(kind),
        escape(value),
        list
    )
}
fn options(name: &str, label: &str, value: &str, choices: &[(&str, &str)]) -> String {
    let mut out = format!(
        "<label for=\"report-{}\">{}</label><select id=\"report-{}\" name=\"{}\">",
        escape(name),
        escape(label),
        escape(name),
        escape(name)
    );
    for (key, label) in choices {
        out += &format!(
            "<option value=\"{}\"{}>{}</option>",
            escape(key),
            if *key == value { " selected" } else { "" },
            escape(label)
        );
    }
    out + "</select>"
}
fn datalist(id: &str, page: &Value) -> String {
    let mut out = format!("<datalist id=\"{}\">", escape(id));
    for item in page["items"].as_array().into_iter().flatten() {
        if let (Some(id), Some(name)) = (item["id"].as_str(), item["name"].as_str()) {
            out += &format!(
                "<option value=\"{}\" label=\"{}\"></option>",
                escape(id),
                escape(name)
            );
        }
    }
    out + "</datalist>"
}
pub(super) fn overview(
    form: &Form,
    report: &Analytics,
    stores: &Value,
    ads: &Value,
    campaigns: &Value,
) -> Result<String> {
    let mut out = format!(
        "<form method=\"get\" action=\"/reports\"><input type=\"hidden\" name=\"company_id\" value=\"{}\"><fieldset><legend>Report filters</legend>",
        escape(&form.company_id)
    );
    out += &input(
        "start_date",
        "From date (included)",
        &form.start_date,
        "date",
        None,
    );
    out += &input(
        "end_date",
        "Until date (excluded)",
        &form.end_date,
        "date",
        None,
    );
    out += &input("timezone", "Reporting zone", &form.timezone, "text", None);
    out += &input(
        "store_id",
        "Store: choose or paste reference",
        &form.store_id,
        "text",
        Some("report-stores"),
    );
    out += &input(
        "ad_id",
        "Ad: choose or paste reference",
        &form.ad_id,
        "text",
        Some("report-ads"),
    );
    out += &input(
        "campaign_id",
        "Campaign: choose or paste reference",
        &form.campaign_id,
        "text",
        Some("report-campaigns"),
    );
    out += &input(
        "advertised_product_id",
        "Advertised product reference",
        &form.advertised_product_id,
        "text",
        None,
    );
    out += &input(
        "sold_product_id",
        "Purchased product reference",
        &form.sold_product_id,
        "text",
        None,
    );
    out += &options(
        "currency",
        "Currency",
        &form.currency,
        &[
            ("", "All, shown separately"),
            ("EUR", "EUR"),
            ("USD", "USD"),
            ("GBP", "GBP"),
            ("JPY", "JPY"),
        ],
    );
    out += &options(
        "group_by",
        "Breakdown",
        &form.group_by,
        &[
            ("", "Overview"),
            ("day", "Day"),
            ("ad", "Ad"),
            ("campaign", "Campaign"),
            ("advertised_product", "Advertised product"),
            ("sold_product", "Purchased product"),
        ],
    );
    out += "<button type=\"submit\">Update report</button></fieldset></form>";
    out += &datalist("report-stores", stores);
    out += &datalist("report-ads", ads);
    out += &datalist("report-campaigns", campaigns);
    out += "<p>Suggestions show the first 100 matching entries. You can paste another reference from tracking setup. Names in suggestions are current catalog names.</p>";
    let mut exported = form.clone();
    exported.cursor = None;
    let q = serde_urlencoded::to_string(&exported).map_err(|_| Failure::Invalid)?;
    out += &format!(
        "<p><a href=\"/reports/export?{}\">Download CSV (up to 10,000 rows)</a></p>",
        escape(&q)
    );
    out += &format!(
        "<p>Reporting zone: {}. UTC interval: {} (included) to {} (excluded).</p>",
        escape(&report.timezone),
        escape(&report.start.to_rfc3339()),
        escape(&report.end.to_rfc3339())
    );
    out += "<details><summary>How to read this report</summary><p>Clicks use click time; observations use observation time; paid orders and sales use payment time; refunds use refund time. Order creation controls the 30-day attribution window. Net merchandise is discounted merchandise less refunds in this period and can be negative. Taxes and shipping are separate. Currencies are never added.</p><p>Image loads are estimated pixel loads. Landing-page loads and visible placements are distinct observations; a visible placement requires at least 50% visibility for one continuous second. Eligible clicks exclude heuristic bot and prefetch matches. Blocked tracking and unobserved social impressions are not measured.</p><p>Period conversion rate = attributed paid orders in this period / eligible clicks in this period. It is not a click-cohort rate, can exceed 100%, and is undefined with no eligible clicks. Purchased-product filters or breakdowns omit the rate. Their clicks and observations describe the selected store/ad/campaign/advertised-product scope, not that purchased product; repeated contextual counts must not be added.</p><p>Purchased-product money and units use matching lines. Orders and refunds count once per matching group, while tax/shipping remain whole-order/refund totals. Product groups can overlap: do not sum their order/refund counts or tax/shipping to an overall total. Use Overview without a purchased-product filter for overall totals.</p></details>";
    if report.groups.is_empty() {
        out += "<p>No activity matches these filters.</p>";
    }
    for group in &report.groups {
        out += &format!(
            "<section class=\"report-group\"><h2>{}</h2>",
            escape(
                group
                    .group
                    .as_deref()
                    .unwrap_or(if form.group_by.is_empty() {
                        "Overview"
                    } else {
                        "Unattributed or no advertised product"
                    })
            )
        );
        let rate = if group.rate_denominator == "omitted_sold_product_filter" {
            "Omitted for purchased-product filtering".into()
        } else {
            group
                .period_conversion_rate
                .map(|v| format!("{:.2}%", v * 100.0))
                .unwrap_or_else(|| "Undefined — no eligible clicks".into())
        };
        out += &format!(
            "<dl><dt>Raw clicks</dt><dd>{}</dd><dt>Eligible clicks (heuristic)</dt><dd>{}</dd><dt>Estimated image loads</dt><dd>{}</dd><dt>Landing-page loads</dt><dd>{}</dd><dt>Visible placements</dt><dd>{}</dd><dt>Attributed paid orders</dt><dd>{}</dd><dt>Unattributed paid orders</dt><dd>{}</dd><dt>Period conversion rate</dt><dd>{rate}</dd></dl>",
            group.raw_clicks,
            group.eligible_clicks,
            group.image_loads,
            group.landing_page_loads,
            group.visible_placements,
            group.attributed_paid_orders,
            group.unattributed_paid_orders
        );
        if group.unattributed_paid_orders > 0 {
            out += "<ul aria-label=\"Unattributed reasons\">";
            for (reason, n) in &group.unattributed_reasons {
                if *n > 0 {
                    out += &format!("<li>{}: {n}</li>", escape(&reason.replace('_', " ")));
                }
            }
            out += "</ul>";
        }
        out += "<div class=\"report-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Financial totals\"><table class=\"report-table\"><caption>Financial totals by currency</caption><thead><tr>";
        for h in [
            "Currency",
            "Paid orders",
            "Sold units",
            "Refunds",
            "Returned units",
            "Merchandise",
            "Refunded merchandise",
            "Net merchandise",
            "Tax",
            "Refunded tax",
            "Shipping",
            "Refunded shipping",
        ] {
            out += &format!("<th scope=\"col\">{h}</th>");
        }
        out += "</tr></thead><tbody>";
        for c in &group.currencies {
            out += &format!(
                "<tr><th scope=\"row\">{}</th><td>{}</td><td>{}</td><td>{}</td><td>{}</td>",
                c.currency.as_str(),
                c.paid_orders,
                c.sold_units,
                c.refunds,
                c.returned_units
            );
            for amount in [
                c.merchandise_amount,
                c.refunded_merchandise_amount,
                c.net_merchandise_amount,
                c.tax_amount,
                c.refunded_tax_amount,
                c.shipping_amount,
                c.refunded_shipping_amount,
            ] {
                let n = amount.abs();
                let sign = if amount < 0 { "-" } else { "" };
                let value = if c.currency.exponent() == 0 {
                    amount.to_string()
                } else {
                    format!("{sign}{}.{:02}", n / 100, n % 100)
                };
                out += &format!("<td>{value}</td>");
            }
            out += "</tr>";
        }
        out += "</tbody></table></div></section>";
    }
    if let Some(cursor) = &report.next_cursor {
        let mut next = form.clone();
        next.cursor = Some(cursor.clone());
        out += &format!(
            "<p><a href=\"/reports?{}\">Next report page</a></p>",
            escape(&serde_urlencoded::to_string(&next).map_err(|_| Failure::Invalid)?)
        );
    }
    Ok(out)
}
fn display(value: &Value) -> String {
    escape(
        value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if value.is_null() {
                    "—".into()
                } else {
                    value.to_string()
                }
            })
            .as_str(),
    )
}
fn table(items: &[Value], fields: &[(&str, &str)]) -> String {
    if items.is_empty() {
        return "<p>No entries match this view.</p>".into();
    }
    let mut out="<div class=\"report-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Ledger details\"><table class=\"report-table\"><caption>Amounts in integer minor units; currencies shown separately</caption><thead><tr>".to_owned();
    for (_, label) in fields {
        out += &format!("<th scope=\"col\">{}</th>", escape(label));
    }
    out += "</tr></thead><tbody>";
    for item in items {
        out += "<tr>";
        for (key, _) in fields {
            out += &format!("<td>{}</td>", display(&item[*key]));
        }
        out += "</tr>";
    }
    out += "</tbody></table></div>";
    out
}
pub(super) fn order(order: &Value) -> Result<String> {
    let mut out = format!("<h2>Order {}</h2>", display(&order["external_order_id"]));
    out += &table(
        std::slice::from_ref(order),
        &[
            ("currency", "Currency"),
            ("order_created_at", "Created"),
            ("paid_at", "Paid"),
            ("merchandise_amount", "Merchandise"),
            ("refunded_merchandise_amount", "Refunded merchandise"),
            ("tax_amount", "Tax"),
            ("refunded_tax_amount", "Refunded tax"),
            ("shipping_amount", "Shipping"),
            ("refunded_shipping_amount", "Refunded shipping"),
            ("returned_units", "Returned units"),
            ("attribution_reason", "Attribution reason"),
            ("ad_id", "Attributed ad"),
            ("click_id", "Selected click"),
            ("policy", "Attribution policy"),
        ],
    );
    out += "<h3>Purchased lines</h3>";
    out += &table(
        order["lines"].as_array().ok_or(Failure::Unavailable)?,
        &[
            ("line_id", "Line"),
            ("product_id", "Purchased product"),
            ("quantity", "Quantity"),
            ("merchandise_amount", "Merchandise"),
        ],
    );
    out += "<h3>Applied refunds</h3>";
    Ok(out)
}
pub(super) fn ledger(
    page: &Value,
    events: Option<&str>,
    refunds: bool,
    p: &TenantPrincipal,
) -> Result<String> {
    let items = page["items"].as_array().ok_or(Failure::Unavailable)?;
    let mut out = String::new();
    if let Some(state) = events {
        out += &format!(
            "<form method=\"get\" action=\"/reports/events\"><input type=\"hidden\" name=\"company_id\" value=\"{}\">{}<button>Filter events</button></form><p>Pending events are durably stored but are not sales. Conflicts preserve the original facts. Unresolved events can be retried by an owner or administrator.</p>",
            p.access.company_id.0,
            options(
                "state",
                "Event state",
                state,
                &[
                    ("", "All states"),
                    ("pending", "Pending"),
                    ("unresolved", "Unresolved"),
                    ("conflict", "Conflict"),
                    ("accepted", "Accepted")
                ]
            )
        );
        out += &table(
            items,
            &[
                ("event_id", "Delivery reference"),
                ("kind", "Kind"),
                ("external_order_id", "Order reference"),
                ("refund_id", "Refund reference"),
                ("state", "State"),
                ("reason", "Reason"),
                ("occurred_at", "Occurred"),
                ("received_at", "Received"),
                ("attempts", "Attempts"),
                ("next_attempt_at", "Next attempt"),
                ("deadline_at", "Deadline"),
            ],
        );
        if p.access
            .permissions
            .contains(&Permission::ManageIntegrations)
        {
            for item in items.iter().filter(|i| i["state"] == "unresolved") {
                let id = item["id"].as_str().ok_or(Failure::Unavailable)?;
                out += &format!(
                    "<p><button class=\"event-retry\" data-event=\"{}\" data-company=\"{}\" data-csrf=\"{}\" data-key=\"{}\" data-created=\"{}\">Retry {}</button></p>",
                    escape(id),
                    p.access.company_id.0,
                    escape(&p.principal.session.csrf.0),
                    Uuid::new_v4(),
                    chrono::Utc::now().to_rfc3339(),
                    display(&item["event_id"])
                );
            }
        }
    } else if refunds {
        for item in items {
            out += &format!("<h4>Refund {}</h4>", display(&item["refund_id"]));
            out += &table(
                std::slice::from_ref(item),
                &[
                    ("occurred_at", "Occurred"),
                    ("currency", "Currency"),
                    ("tax_amount", "Tax refund"),
                    ("shipping_amount", "Shipping refund"),
                ],
            );
            out += &table(
                item["lines"].as_array().ok_or(Failure::Unavailable)?,
                &[
                    ("line_id", "Line"),
                    ("returned_quantity", "Returned quantity"),
                    ("merchandise_amount", "Merchandise refund"),
                ],
            );
        }
        if items.is_empty() {
            out += "<p>No applied refunds.</p>";
        }
    } else {
        out += &table(
            items,
            &[
                ("external_order_id", "Order reference"),
                ("paid_at", "Paid"),
                ("currency", "Currency"),
                ("merchandise_amount", "Merchandise"),
                ("refunded_merchandise_amount", "Refunded merchandise"),
                ("attribution_reason", "Attribution reason"),
            ],
        );
        for item in items {
            out += &format!(
                "<p><a href=\"/reports/orders/{}?company_id={}\">Inspect order {}</a></p>",
                display(&item["id"]),
                p.access.company_id.0,
                display(&item["external_order_id"])
            );
        }
    }
    Ok(out)
}
