use super::*;
use crate::adapters::http::pages::layout::escape_html_attr;
use crate::application::workflow::agent_history_views::*;

#[derive(Clone, Copy)]
pub struct HistoryAnchor {
    pub company: Uuid,
    pub run: Uuid,
    pub execution: Uuid,
}
impl HistoryAnchor {
    pub fn endpoint(self) -> String {
        format!(
            "/ui/workflow-runs/{}/steps/{}/history?company_id={}",
            self.run, self.execution, self.company
        )
    }
    pub fn region(self) -> String {
        format!("agent-history-{}", self.execution)
    }
}

pub fn agent_history_section(anchor: HistoryAnchor, page: &HistoryPage) -> String {
    let region = anchor.region();
    let entries: String = page
        .entries
        .iter()
        .map(|entry| history_row(anchor, entry))
        .collect();
    let next = page
        .next
        .as_ref()
        .map(|cursor| {
            read_button(
                anchor,
                &format!("{}&after={}", anchor.endpoint(), cursor.as_str()),
                &region,
                "Older exchanges",
            )
        })
        .unwrap_or_default();
    let latest = read_button(anchor, &anchor.endpoint(), &region, "Latest exchanges");
    let empty = if entries.is_empty() {
        "<li>No inspectable exchanges in this page.</li>"
    } else {
        ""
    };
    format!(
        r##"<section id="{region}" class="mt-4 space-y-3" aria-label="Agent history">
        <h3 class="font-semibold">Agent history</h3>
        <p class="text-xs opacity-70">Produced output is separate from workflow completion and message delivery.</p>
        <p class="text-xs">Expired payloads: {expired}. Earlier payloads expired through exchange {through}.</p>
        <ol class="space-y-2">{entries}{empty}</ol><nav aria-label="Agent history pages">{latest}{next}</nav>
        <div id="{region}-detail" aria-live="polite"></div></section>"##,
        expired = page.expired_count,
        through = page.expired_through_order
    )
}

fn read_button(anchor: HistoryAnchor, endpoint: &str, target: &str, label: &str) -> String {
    // Shared BUTTON_BUSY_SCRIPT acknowledges accepted requests and restores failed/aborted reads.
    format!(
        r##"<button type="button" class="btn btn-ghost btn-sm" hx-get="{}"
        hx-target="#{target}" hx-swap="outerHTML" hx-sync="#{}:replace" hx-disabled-elt="this">{}</button>"##,
        escape_html_attr(endpoint),
        anchor.region(),
        escape_html_text(label)
    )
}

fn history_row(anchor: HistoryAnchor, entry: &HistoryEntry) -> String {
    let status = entry
        .activity
        .map(|activity| activity.label().to_owned())
        .unwrap_or_else(|| format!("Restricted exchange ({})", entry.state.as_str()));
    let timing = timing(entry);
    let expired = if entry.payload == HistoryPayloadState::Expired {
        " · payload expired"
    } else {
        ""
    };
    let expand = entry
        .exchange
        .map(|exchange| {
            let endpoint = format!("{}&exchange={}", anchor.endpoint(), exchange.as_uuid());
            read_button(
                anchor,
                &endpoint,
                &format!("{}-detail", anchor.region()),
                &format!("Inspect exchange {}", entry.order),
            )
        })
        .unwrap_or_default();
    format!(
        r#"<li data-history-order="{}"><p>Exchange {} · {} · {timing}{expired}</p>{expand}</li>"#,
        entry.order,
        entry.order,
        escape_html_text(&status)
    )
}

pub fn agent_history_detail(anchor: HistoryAnchor, detail: &HistoryDetail) -> String {
    let entry = &detail.entry;
    let status = entry
        .activity
        .map(|activity| activity.label().to_owned())
        .unwrap_or_else(|| format!("Restricted exchange ({})", entry.state.as_str()));
    let mut body = format!(
        "<h4 class=\"font-semibold\">Exchange {} · {}</h4>",
        entry.order,
        escape_html_text(&status)
    );
    body.push_str(&format!(
        "<p class=\"text-xs opacity-70\">{}</p>",
        timing(entry)
    ));
    if let Some(provenance) = &entry.provenance {
        body.push_str(&format!(
            "<p>Agent <code>{}</code> · revision <code>{}</code> · manifest <code>{}</code></p>",
            provenance.agent.as_uuid(),
            provenance.revision.as_uuid(),
            provenance.manifest.as_uuid()
        ));
    }
    if entry.payload == HistoryPayloadState::Expired {
        body.push_str(
            "<p>History payload expired. The retained metadata records its original outcome.</p>",
        );
    }
    if let Some(status) = &detail.status {
        body.push_str(&format!(
            "<p>Workflow: {}{} · desk response: {} · delivery: {}</p>",
            escape_html_text(&status.workflow),
            status
                .waiting_reason
                .as_ref()
                .map(|reason| format!(" (waiting: {})", escape_html_text(reason)))
                .unwrap_or_default(),
            escape_html_text(
                status
                    .channel_response
                    .as_deref()
                    .unwrap_or("not applicable")
            ),
            escape_html_text(status.delivery.as_deref().unwrap_or("not applicable"))
        ));
    }
    if let Some(source) = &entry.source {
        body.push_str(&source_link(anchor.company, source));
    }
    if let Some(payload) = &detail.payload {
        body.push_str(&payload_body(anchor, payload));
    }
    format!(
        r#"<article id="{}-detail" class="space-y-3" aria-live="polite" tabindex="-1">{body}</article>"#,
        anchor.region()
    )
}

fn timing(entry: &HistoryEntry) -> String {
    format!(
        "Created {}{}{}",
        format_date_time(entry.timing.created_at),
        entry
            .timing
            .activated_at
            .map(|at| format!(" · started {}", format_date_time(at)))
            .unwrap_or_default(),
        entry
            .timing
            .finished_at
            .map(|at| format!(" · finished {}", format_date_time(at)))
            .unwrap_or_default()
    )
}

fn source_link(company: Uuid, source: &HistorySource) -> String {
    let checkpoint = source
        .checkpoint
        .map(|id| format!(" · checkpoint <code>{id}</code>"))
        .unwrap_or_default();
    format!(
        r#"<a class="link" href="/ui/workflow-runs?company_id={company}&amp;run_id={run}">Source run</a> ·
        <a class="link" href="/ui/workflow-runs/{run}/steps/{execution}?company_id={company}">Source step and output</a>{checkpoint}"#,
        run = source.run.as_uuid(),
        execution = source.execution
    )
}

fn json(value: &serde_json::Value) -> String {
    // These are bounded native payloads; retain the complete JSON rather than truncating provenance.
    format!(
        r#"<pre class="overflow-auto bg-base-200 p-3 text-xs">{}</pre>"#,
        escape_html_text(&serde_json::to_string_pretty(value).unwrap_or_default())
    )
}

fn payload_body(anchor: HistoryAnchor, payload: &HistoryPayload) -> String {
    let output = payload
        .output
        .as_ref()
        .map(json)
        .unwrap_or_else(|| "<p>No logical output produced.</p>".into());
    let mut body = format!(
        "<details><summary>Original input</summary>{}</details><details><summary>Produced output</summary>{output}</details>",
        json(&payload.input)
    );
    if let Some(frozen) = &payload.frozen {
        let safe = serde_json::to_value(frozen).unwrap_or(serde_json::Value::Null);
        body.push_str(&format!(
            "<details><summary>Exact frozen context and model request</summary>{}</details>",
            json(&safe)
        ));
        body.push_str("<ul aria-label=\"Selected source availability\">");
        for source in &payload.selected_sources {
            let content = match source {
                HistorySelectedSource::Available { exchange, source } => {
                    let order = frozen
                        .history
                        .selected
                        .iter()
                        .find(|selected| selected.id == *exchange)
                        .map(|selected| selected.position.get().to_string())
                        .unwrap_or_else(|| exchange.as_uuid().to_string());
                    let inspect = read_button(
                        anchor,
                        &format!("{}&exchange={}", anchor.endpoint(), exchange.as_uuid()),
                        &format!("{}-detail", anchor.region()),
                        &format!("Inspect exchange {order}"),
                    );
                    format!(
                        "{} · {} {inspect}",
                        exchange.as_uuid(),
                        source_link(anchor.company, source)
                    )
                }
                HistorySelectedSource::Expired { exchange } => format!(
                    "{} · source payload expired; frozen copy retained",
                    exchange.as_uuid()
                ),
                HistorySelectedSource::Unavailable { exchange } => {
                    format!("{} · source unavailable", exchange.as_uuid())
                }
            };
            body.push_str(&format!("<li>{content}</li>"));
        }
        body.push_str("</ul>");
    } else {
        body.push_str("<p>Preparation has not been frozen.</p>");
    }
    body
}

pub fn agent_history_loader(anchor: HistoryAnchor) -> String {
    let region = anchor.region();
    format!(
        r##"<details class="mt-4"><summary>Agent history</summary>
        <section id="{region}" aria-label="Agent history" hx-get="{}" hx-trigger="revealed once"
          hx-target="this" hx-swap="outerHTML" hx-sync="#{region}:replace">
          <p role="status">Loading agent history…</p></section></details>"##,
        escape_html_attr(&anchor.endpoint())
    )
}
