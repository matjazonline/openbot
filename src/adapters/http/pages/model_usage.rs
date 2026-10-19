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
    let rows = report.groups.iter().map(usage_row).collect::<String>();
    let prices = report
        .prices
        .iter()
        .map(|price| price_form(company, Some(price)))
        .collect::<String>();
    let content = format!(
        r#"<main class="p-6 space-y-6 h-full overflow-auto">
{THEME_CONTROLLER}
<a class="link" href="/ui/companies?company_id={company}">Company settings</a>
<h1 class="text-2xl font-bold">Model usage</h1>
<p>Calls and tokens from durable agent and workflow executions. Unconfirmed requests retain reserved token estimates; they do not prove the provider ran or billed a request.</p>
{}
<p>{} to {} · {} calls scanned</p>{warning}
<div class="overflow-x-auto"><table class="table"><caption>Usage by {}</caption><thead><tr><th scope="col">Model / workflow</th><th scope="col">Calls</th><th scope="col">Completed / unconfirmed</th><th scope="col">Reported / estimated / mixed</th><th scope="col">Input / output tokens</th><th scope="col">Estimated cost at current configured rates (USD)</th></tr></thead><tbody>{rows}</tbody></table></div>
<p>Cost uses current prices, including for historical calls. A priced subtotal excludes unpriced calls. Estimated and unconfirmed tokens also produce estimated costs. No monetary caps apply.</p>
<h2 class="text-xl font-bold">Prices per million tokens (USD)</h2>
<p>Set both rates to calculate cost. Zero means free; no configured rate means unavailable. Changing a price recalculates historical estimates.</p>
{prices}{}{}
</main>"#,
        filter_form(company, query),
        escape_html_text(&report.start.to_rfc3339()),
        escape_html_text(&report.end.to_rfc3339()),
        report.scanned_calls,
        if report.grouping == UsageGrouping::Model {
            "model"
        } else {
            "workflow"
        },
        if report.prices_truncated {
            "<p role=\"status\">Only the first 200 configured prices are shown.</p>"
        } else {
            ""
        },
        price_form(company, None)
    );
    ui_layout("Model usage", &content)
}
fn filter_form(company: Uuid, query: &UsageQuery) -> String {
    format!(
        r#"<form method="get" action="/ui/companies/{company}/usage" class="flex flex-wrap gap-3 items-end" data-submit="busy-once" data-pending-label="Working…">
<label>Last days <input class="input" type="number" name="days" min="1" max="90" value="{}" required></label>
<label>Group by <select class="select" name="group"><option value="model" {}>Model</option><option value="workflow" {}>Workflow</option></select></label>
<label>Agent ID (optional) <input class="input" name="agent" value="{}" maxlength="36"></label>
<label>Workflow ID (optional) <input class="input" name="workflow" value="{}" maxlength="36"></label>
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
        query.agent.map_or(String::new(), |id| id.to_string()),
        query.workflow.map_or(String::new(), |id| id.to_string())
    )
}
fn usage_row(group: &UsageGroup) -> String {
    let label = match (&group.provider, &group.model, group.workflow) {
        (Some(provider), Some(model), _) => format!("{provider} / {model}"),
        (_, _, Some(id)) => format!("Workflow {id}"),
        _ => "Standalone agent activity".into(),
    };
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
        escape_html_text(&label),
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
