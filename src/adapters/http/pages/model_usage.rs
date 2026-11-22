use super::layout::{escape_html_attr, escape_html_text};
use super::mailbox::{THEME_CONTROLLER, ui_layout};
use crate::services::model_usage::*;
use uuid::Uuid;

pub fn model_usage_page(company: Uuid, query: &UsageQuery, report: &UsageReport) -> String {
    let warning = if report.truncated {
        "<p role=\"status\" class=\"alert alert-warning\">Partial report: the scan or group limit was reached. Narrow the time range. Totals below are incomplete.</p>"
    } else {
        ""
    };
    let rows = report
        .groups
        .iter()
        .map(|group| usage_row(company, query, group))
        .collect::<String>();
    let prices = if report.prices_editable {
        report
            .prices
            .iter()
            .map(|price| price_form(company, Some(price)))
            .collect::<String>()
            + &price_form(company, None)
    } else {
        price_list(&report.prices)
    };
    let budgets = budgets_section(report);
    let content = format!(
        r#"<main class="p-6 space-y-6 h-full overflow-auto">
{THEME_CONTROLLER}
<a class="link" href="/ui/companies?company_id={company}">Company settings</a>
<h1 class="text-2xl font-bold">Model usage</h1>
<p>Calls and tokens from durable agent and workflow executions. Unconfirmed requests retain reserved token estimates; they do not prove the provider ran or billed a request.</p>
{}
<p>{} to {} · {} calls scanned</p>{warning}
<div class="overflow-x-auto"><table class="table"><caption>Usage by {}</caption><thead><tr><th scope="col">Model / workflow</th><th scope="col">Calls</th><th scope="col">Completed / unconfirmed</th><th scope="col">Reported / estimated / mixed</th><th scope="col">Input / output tokens</th><th scope="col">Estimated cost at current configured rates (USD)</th></tr></thead><tbody>{rows}</tbody></table></div>
<p>Cost uses current prices, including for historical calls. A priced subtotal excludes unpriced calls. Estimated and unconfirmed tokens also produce estimated costs.</p>
{budgets}
<h2 class="text-xl font-bold">Prices per million tokens (USD)</h2>
<p>Set both rates to calculate cost. Zero means free; no configured rate means unavailable. Changing a price recalculates historical estimates; spend already counted toward caps keeps the price it was settled at.</p>
{prices}{}
</main>"#,
        filter_form(company, query),
        escape_html_text(&report.start.to_rfc3339()),
        escape_html_text(&report.end.to_rfc3339()),
        report.scanned_calls,
        match report.grouping {
            UsageGrouping::Model => "model",
            UsageGrouping::Workflow => "workflow",
            UsageGrouping::Department => "department",
            UsageGrouping::Desk => "desk",
        },
        if report.prices_truncated {
            "<p role=\"status\">Only the first 200 configured prices are shown.</p>"
        } else {
            ""
        },
    );
    ui_layout("Model usage", &content)
}
fn filter_form(company: Uuid, query: &UsageQuery) -> String {
    format!(
        r#"<form method="get" action="/ui/companies/{company}/usage" class="flex flex-wrap gap-3 items-end" data-submit="busy-once" data-pending-label="Working…">
<label>Last days <input class="input" type="number" name="days" min="1" max="90" value="{}" required></label>
<label>Group by <select class="select" name="group"><option value="model" {}>Model</option><option value="workflow" {}>Workflow</option><option value="department" {}>Department</option>{}</select></label>{}
<label>Agent ID (optional) <input class="input" name="agent" value="{}" maxlength="36"></label>
<label>Workflow ID (optional) <input class="input" name="workflow" value="{}" maxlength="36"></label>
<label class="label cursor-pointer gap-2"><input class="checkbox" type="checkbox" name="include_archived" value="true"{}> Include archived departments</label>
<button class="btn btn-primary" type="submit"><span data-label>Show usage</span></button></form>"#,
        (query.end - query.start).num_days(),
        if query.grouping == UsageGrouping::Model {
            "selected"
        } else {
            ""
        },
        if query.grouping == UsageGrouping::Workflow {
            "selected"
        } else {
            ""
        },
        if query.grouping == UsageGrouping::Department {
            "selected"
        } else {
            ""
        },
        // The desk drill-down keeps its department.
        match query.unit {
            Some(_) => format!(
                r#"<option value="desk" {}>Desk</option>"#,
                if query.grouping == UsageGrouping::Desk {
                    "selected"
                } else {
                    ""
                }
            ),
            None => String::new(),
        },
        query.unit.map_or(String::new(), |unit| format!(
            r#"<input type="hidden" name="unit" value="{unit}">"#
        )),
        query.agent.map_or(String::new(), |id| id.to_string()),
        query.workflow.map_or(String::new(), |id| id.to_string()),
        if query.archived.included() {
            " checked"
        } else {
            ""
        },
    )
}
fn usage_row(company: Uuid, query: &UsageQuery, group: &UsageGroup) -> String {
    // A department links to its calls by desk over the same range (Plan07 02C).
    if let Some(department) = &group.department {
        let label = escape_html_text(&department.name);
        let link = match department.id {
            Some(unit) => format!(
                r#"<a class="link" href="/ui/companies/{company}/usage?days={}&amp;group=desk&amp;unit={unit}">{label}</a>"#,
                (query.end - query.start).num_days()
            ),
            None => label,
        };
        return usage_cells(link, group);
    }
    let label = match (&group.provider, &group.model, group.workflow) {
        _ if group.desk.is_some() => group
            .desk
            .as_ref()
            .map(|desk| desk.name.clone())
            .unwrap_or_default(),
        (Some(provider), Some(model), _) => format!("{provider} / {model}"),
        (_, _, Some(id)) => format!("Workflow {id}"),
        _ => "Standalone agent activity".into(),
    };
    usage_cells(escape_html_text(&label), group)
}
fn usage_cells(label: String, group: &UsageGroup) -> String {
    let cost = match &group.priced_subtotal_usd {
        Some(cost) if group.unpriced_calls > 0 => format!(
            "{cost} priced subtotal; {} unpriced calls",
            group.unpriced_calls
        ),
        Some(cost) => cost.clone(),
        None => "Unavailable — no configured rates".into(),
    };
    format!(
        "<tr><th scope=\"row\">{}</th><td>{}</td><td>{} / {}</td><td>{} / {} / {}</td><td>{} / {}</td><td>{}</td></tr>",
        label,
        group.calls,
        group.completed_calls,
        group.unconfirmed_calls,
        group.reported_calls,
        group.estimated_calls,
        group.mixed_calls,
        group.input_tokens,
        group.output_tokens,
        escape_html_text(&cost)
    )
}
/// Caps and counted spend per department for the current UTC day and month (Plan07 02C.1): the
/// figures admission enforces, from the running totals, apart from the ledger estimate above.
fn budgets_section(report: &UsageReport) -> String {
    if report.budgets.is_empty() {
        return r#"<h2 class="text-xl font-bold">Spend counted toward caps</h2><p>No department has a spend cap or counted spend this UTC day or month.</p>"#.into();
    }
    let rows: String = report
        .budgets
        .iter()
        .map(|budget| {
            let unpriced = if budget.unpriced_tokens > 0 {
                format!(
                    r#" <span class="badge badge-warning">{} unpriced tokens not counted</span>"#,
                    budget.unpriced_tokens
                )
            } else {
                String::new()
            };
            format!(
                "<tr><th scope=\"row\">{}</th><td>{}</td><td>{}</td><td>{}{unpriced}</td><td>{}</td></tr>",
                escape_html_text(&if budget.archived {
                    format!("{} (archived)", budget.name)
                } else {
                    budget.name.clone()
                }),
                if budget.period == "day" { "Today (UTC)" } else { "This month (UTC)" },
                escape_html_text(budget.cap_usd.as_deref().unwrap_or("No cap")),
                escape_html_text(&budget.spent_usd),
                escape_html_text(budget.remaining_usd.as_deref().unwrap_or("—")),
            )
        })
        .collect();
    format!(
        r#"<h2 class="text-xl font-bold">Spend counted toward caps</h2>
<p>What new runs are checked against: each settled call's priced cost when it settled, counted for its department and every department above it. Unpriced models count zero. Runs already started finish even past a cap.</p>
<div class="overflow-x-auto"><table class="table"><caption>Counted spend by department (USD)</caption><thead><tr><th scope="col">Department</th><th scope="col">Period</th><th scope="col">Cap</th><th scope="col">Counted</th><th scope="col">Remaining</th></tr></thead><tbody>{rows}</tbody></table></div>{}"#,
        if report.budgets_truncated {
            r#"<p role="status">Only the first 200 department periods are shown.</p>"#
        } else {
            ""
        }
    )
}
/// The configured prices, read-only for department managers.
fn price_list(prices: &[ModelPrice]) -> String {
    if prices.is_empty() {
        return "<p>No prices are configured.</p>".into();
    }
    let rows: String = prices
        .iter()
        .map(|price| {
            format!(
                "<tr><th scope=\"row\">{} / {}</th><td>{}</td><td>{}</td></tr>",
                escape_html_text(price.provider.as_str()),
                escape_html_text(price.model.as_str()),
                escape_html_text(&price.input.decimal()),
                escape_html_text(&price.output.decimal()),
            )
        })
        .collect();
    format!(
        r#"<p>Prices are set by the company's managers.</p><table class="table"><thead><tr><th scope="col">Model</th><th scope="col">Input</th><th scope="col">Output</th></tr></thead><tbody>{rows}</tbody></table>"#
    )
}
fn price_form(company: Uuid, price: Option<&ModelPrice>) -> String {
    let label = price.map_or("Add model price".into(), |p| {
        format!("{} / {}", p.provider, p.model)
    });
    let provider = price.map_or("", |p| p.provider.as_str());
    let model = price.map_or("", |p| p.model.as_str());
    let input = price.map_or_else(String::new, |p| p.input.decimal());
    let output = price.map_or_else(String::new, |p| p.output.decimal());
    let revision = price.map_or_else(String::new, |p| p.revision.to_string());
    format!(
        r#"<form method="post" action="/ui/companies/{company}/usage/prices" data-submit="busy-once" data-pending-label="Working…"><fieldset class="fieldset bg-base-200 p-4"><legend>{}</legend>
<input type="hidden" name="revision" value="{revision}">
<label>Provider <input class="input" name="provider" maxlength="128" value="{}" required {}></label>
<label>Model <input class="input" name="model" maxlength="256" value="{}" required {}></label>
<label>Input USD per million <input class="input" name="input" type="number" min="0" max="1000000" step="0.000001" value="{}" required></label>
<label>Output USD per million <input class="input" name="output" type="number" min="0" max="1000000" step="0.000001" value="{}" required></label>
<div><button class="btn btn-primary" type="submit" name="action" value="save"><span data-label>Save price</span></button>{}</div></fieldset></form>"#,
        escape_html_text(&label),
        escape_html_attr(provider),
        if price.is_some() { "readonly" } else { "" },
        escape_html_attr(model),
        if price.is_some() { "readonly" } else { "" },
        escape_html_attr(&input),
        escape_html_attr(&output),
        if price.is_some() {
            "<button class=\"btn\" type=\"submit\" name=\"action\" value=\"remove\" formnovalidate>Remove price</button>"
        } else {
            ""
        }
    )
}
