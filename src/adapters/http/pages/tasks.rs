//! Background task list, task detail rows and execution-parameter rendering.

use super::*;

pub struct TaskPageLink {
    pub href: String,
    pub hx_get: String,
}

pub struct TaskPagination {
    pub current_page: usize,
    pub limit: usize,
    pub previous: Option<TaskPageLink>,
    pub next: Option<TaskPageLink>,
}

/// Totals across the tasks on this page.
pub(crate) struct TaskTokenTotals {
    pub(crate) prompt: usize,
    pub(crate) completion: usize,
    pub(crate) total: usize,
}

pub(crate) fn total_token_usage(tasks: &[BackgroundTask]) -> TaskTokenTotals {
    tasks.iter().filter_map(|task| task.token_usage()).fold(
        TaskTokenTotals {
            prompt: 0,
            completion: 0,
            total: 0,
        },
        |acc, usage| TaskTokenTotals {
            prompt: acc.prompt + usage.prompt_tokens,
            completion: acc.completion + usage.completion_tokens,
            total: acc.total + usage.total_tokens,
        },
    )
}

fn channel_filter_options(channels: &[Channel], current: Option<Uuid>) -> String {
    let mut options = String::from("<option value=\"\">All Channels</option>");
    for channel in channels {
        let selected = if current == Some(channel.id) {
            "selected"
        } else {
            ""
        };
        options.push_str(&format!(
            "<option value=\"{}\" {}>{} (/{})</option>",
            channel.id,
            selected,
            escape_html_text(&channel.name),
            escape_html_text(&channel.slug)
        ));
    }
    options
}

fn status_filter_options(current: Option<TaskStatus>) -> String {
    let current = current.as_ref().map(|s| s.as_str()).unwrap_or("");
    [
        ("", "All Statuses"),
        ("pending", "Pending"),
        ("processing", "Processing"),
        ("completed", "Completed"),
        ("failed", "Failed"),
        ("dead_letter", "Dead Letter"),
        ("stopped", "Stopped"),
    ]
    .iter()
    .map(|(value, label)| {
        let selected = if current == *value { "selected" } else { "" };
        format!(
            "<option value=\"{}\" {}>{}</option>",
            value, selected, label
        )
    })
    .collect()
}

/// Running total of tokens consumed by the tasks currently listed.
fn task_token_meter_card(totals: &TaskTokenTotals) -> String {
    let (total_prompt_tokens, total_completion_tokens, total_tokens_meter) =
        (totals.prompt, totals.completion, totals.total);
    format!(
        r##"
        <!-- Token Meter Summary Card -->
        <div class="bg-slate-900/80 border border-indigo-900/60 rounded-xl p-4 mb-6 flex flex-wrap items-center justify-between gap-4 shadow-sm">
            <div class="flex items-center gap-3">
                <div class="p-2.5 bg-indigo-950/80 border border-indigo-700/50 rounded-lg text-indigo-400">
                    {meter_glyph}
                </div>
                <div>
                    <h4 class="text-xs font-semibold uppercase tracking-wider text-slate-300">Token Meter Summary</h4>
                    <p class="text-xs text-slate-400">Total tokens consumed by tasks on this page</p>
                </div>
            </div>
            <div class="flex items-center gap-3 text-xs font-mono">
                <div class="bg-slate-950/80 px-3 py-1.5 rounded-lg border border-slate-800">
                    <span class="text-slate-400">Prompt Tokens:</span>
                    <span class="text-indigo-300 font-bold ml-1.5">{total_prompt_tokens}</span>
                </div>
                <div class="bg-slate-950/80 px-3 py-1.5 rounded-lg border border-slate-800">
                    <span class="text-slate-400">Completion Tokens:</span>
                    <span class="text-indigo-300 font-bold ml-1.5">{total_completion_tokens}</span>
                </div>
                <div class="bg-indigo-950/90 px-3.5 py-1.5 rounded-lg border border-indigo-700/80">
                    <span class="text-indigo-200 font-semibold">Total Tokens:</span>
                    <span class="text-white font-extrabold text-sm ml-1.5">{total_tokens_meter}</span>
                </div>
            </div>
        </div>
"##,
        meter_glyph = icon(Icon::Graph, "h-5 w-5"),
    )
}

/// Channel / status / sort controls; each one re-submits the form and swaps the task list.
fn task_filter_bar(
    company_id: Uuid,
    limit: usize,
    wf_options: &str,
    status_options: &str,
    sort_asc: bool,
) -> String {
    let sort_desc_selected = if !sort_asc { "selected" } else { "" };
    let sort_asc_selected = if sort_asc { "selected" } else { "" };
    format!(
        r##"
        <!-- Filter & Sort Bar -->
        <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-4 mb-6">
            <form hx-get="/companies/{company_id}/tasks/filter" hx-target="#task-list" hx-swap="innerHTML" class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <input type="hidden" name="limit" value="{limit}">
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Filter by Channel</label>
                    <select name="channel_id" data-action="submit-form"
                        class="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
                        {wf_options}
                    </select>
                </div>
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Filter by Status</label>
                    <select name="status" data-action="submit-form"
                        class="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
                        {status_options}
                    </select>
                </div>
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Sort by Time</label>
                    <select name="sort" data-action="submit-form"
                        class="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
                        <option value="desc" {sort_desc_selected}>Newest First</option>
                        <option value="asc" {sort_asc_selected}>Oldest First</option>
                    </select>
                </div>
            </form>
        </div>
"##
    )
}

pub fn company_tasks_page(
    company: &Company,
    channels: &[Channel],
    tasks: &[BackgroundTask],
    current_wf: Option<Uuid>,
    current_status: Option<TaskStatus>,
    sort_asc: bool,
    pagination: &TaskPagination,
) -> String {
    let task_list_html = task_list_fragment(company.id, tasks, pagination);
    let totals = total_token_usage(tasks);
    let token_meter_card = task_token_meter_card(&totals);
    let filter_bar = task_filter_bar(
        company.id,
        pagination.limit,
        &channel_filter_options(channels, current_wf),
        &status_filter_options(current_status),
        sort_asc,
    );

    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <a href="/companies" class="text-xs text-indigo-400 hover:text-indigo-300 font-medium mb-1 inline-block">&larr; Back to Companies</a>
                <h2 class="text-2xl font-bold text-white">{company_name} Background Tasks</h2>
                <p class="text-slate-400 text-sm mt-0.5">Monitor, stop, or resume background processing tasks for <span class="font-mono text-indigo-300">/{slug}</span></p>
            </div>
        </div>

        <div id="response-message" class="mb-6"></div>

{token_meter_card}
{filter_bar}
        <!-- Task List Section -->
        <div>
            <h3 class="text-sm font-semibold uppercase tracking-wider text-slate-400 mb-3">Tasks</h3>
            <div id="task-list" class="space-y-3">
                {task_list_html}
            </div>
        </div>
        "##,
        company_name = escape_html_text(&company.name),
        slug = escape_html_text(&company.slug),
        task_list_html = task_list_html,
    );

    base_layout(&format!("{} Tasks", company.name), &content)
}

pub fn task_list_fragment(
    company_id: Uuid,
    tasks: &[BackgroundTask],
    pagination: &TaskPagination,
) -> String {
    let tasks_html = if tasks.is_empty() {
        r##"
            <div class="bg-slate-900/40 border border-dashed border-slate-700/80 rounded-xl p-8 text-center">
                <p class="text-slate-400 text-sm">No tasks matching the selected filters.</p>
            </div>
        "##
        .to_string()
    } else {
        tasks
            .iter()
            .map(|t| task_row_fragment(company_id, t))
            .collect()
    };

    let link = |link: &TaskPageLink, label: &str| {
        format!(
            r##"<a href="{href}" hx-get="{hx_get}" hx-target="#task-list" hx-swap="innerHTML"
                class="px-3 py-2 text-xs font-semibold bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700 rounded-lg transition">{label}</a>"##,
            href = escape_html_attr(&link.href),
            hx_get = escape_html_attr(&link.hx_get),
            label = label,
        )
    };
    let previous = pagination
        .previous
        .as_ref()
        .map(|value| link(value, "&larr; Previous"))
        .unwrap_or_default();
    let next = pagination
        .next
        .as_ref()
        .map(|value| link(value, "Next &rarr;"))
        .unwrap_or_default();
    let pagination_html = if pagination.previous.is_some() || pagination.next.is_some() {
        format!(
            r##"<nav aria-label="Task pagination" class="flex items-center justify-between pt-3">
                <div>{previous}</div>
                <span class="text-xs font-mono text-slate-400">Page {page}</span>
                <div>{next}</div>
            </nav>"##,
            page = pagination.current_page,
        )
    } else {
        String::new()
    };

    format!("{tasks_html}{pagination_html}")
}

pub(crate) fn sanitize_json_payload(value: &serde_json::Value) -> serde_json::Value {
    let mut cloned = value.clone();
    sanitize_json_mut(&mut cloned);
    cloned
}

pub(crate) fn sanitize_json_mut(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k.eq_ignore_ascii_case("api_key")
                    || k.eq_ignore_ascii_case("apikey")
                    || k.eq_ignore_ascii_case("secret")
                {
                    if let serde_json::Value::String(s) = v
                        && !s.is_empty()
                    {
                        *v = serde_json::Value::String("***masked***".to_string());
                    }
                } else {
                    sanitize_json_mut(v);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                sanitize_json_mut(v);
            }
        }
        _ => {}
    }
}

pub fn find_task_for_message<'a>(
    msg: &Message,
    tasks: &'a [BackgroundTask],
    preferred_task_id: Option<Uuid>,
    thread_id: Option<Uuid>,
) -> Option<&'a BackgroundTask> {
    let is_agent = msg.role == MessageRole::Agent || msg.direction == MessageDirection::Outbound;

    for task in tasks {
        if let Some(tid) = thread_id
            && task.thread_id.is_some()
            && task.thread_id != Some(tid)
        {
            continue;
        }

        let payload = &task.payload;

        if is_agent {
            if let Some(outbound_id) = payload
                .get("execution_result")
                .and_then(|r| r.get("outbound_message_id"))
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("outbound_message_id").and_then(|v| v.as_str()))
                && outbound_id == msg.message_id.as_str()
            {
                return Some(task);
            }

            if let Some(resp) = payload
                .get("execution_result")
                .and_then(|r| r.get("response"))
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("response").and_then(|v| v.as_str()))
                && !resp.is_empty()
                && resp == msg.clean_text_body
            {
                return Some(task);
            }
        } else {
            if let Some(inbound_msg_id) = payload
                .get("inbound_message")
                .and_then(|m| m.get("message_id"))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    payload
                        .get("parsed_email")
                        .and_then(|p| p.get("message_id"))
                        .and_then(|v| v.as_str())
                })
                .or_else(|| payload.get("inbound_message_id").and_then(|v| v.as_str()))
                && inbound_msg_id == msg.message_id.as_str()
            {
                return Some(task);
            }

            if let Some(inbound_id_str) = payload
                .get("inbound_message")
                .and_then(|m| m.get("id"))
                .and_then(|v| v.as_str())
                && inbound_id_str == msg.id.to_string()
            {
                return Some(task);
            }
        }
    }

    if let Some(pref_id) = preferred_task_id
        && let Some(task) = tasks.iter().find(|t| t.id == pref_id)
    {
        return Some(task);
    }

    if let Some(tid) = thread_id {
        let thread_tasks: Vec<&BackgroundTask> =
            tasks.iter().filter(|t| t.thread_id == Some(tid)).collect();

        if thread_tasks.len() == 1 {
            return Some(thread_tasks[0]);
        } else if !thread_tasks.is_empty() {
            let closest = thread_tasks
                .iter()
                .min_by_key(|t| (t.created_at - msg.created_at).num_seconds().abs());
            if let Some(t) = closest {
                return Some(t);
            }
        }
    }

    tasks.first()
}

const BADGE_INDIGO: &str = "bg-indigo-950/80 text-indigo-300 border border-indigo-800/50";
const BADGE_PURPLE: &str = "bg-purple-950/80 text-purple-300 border border-purple-800/50";
const BADGE_PURPLE_STRONG: &str = "bg-purple-950/80 text-purple-300 border border-purple-800/60";
const BADGE_SLATE: &str = "bg-slate-800 text-slate-300 border border-slate-700";
const BADGE_EMERALD: &str = "bg-emerald-950/80 text-emerald-300 border border-emerald-800/50";
const BADGE_CYAN: &str = "bg-cyan-950/80 text-cyan-300 border border-cyan-800/60";
const BADGE_TEAL: &str = "bg-teal-950/80 text-teal-300 border border-teal-800/60";
const BADGE_TOKENS: &str =
    "bg-indigo-950/90 text-indigo-200 border border-indigo-700/60 font-semibold";

/// One summary chip above the raw task payload.
fn badge(style: &str, label: impl std::fmt::Display) -> String {
    let label = super::escape_html_text(&label.to_string());
    format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono {style}">{label}</span>"#)
}

/// Read a string at `path` (e.g. `["execution_parameters", "provider"]`), if present and a string.
fn json_str<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    current.as_str()
}

fn json_u64(value: &serde_json::Value, path: &[&str]) -> Option<u64> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    current.as_u64()
}

/// Which model ran, from the recorded execution parameters — or, for a message with no execution
/// of its own, whatever the channel/company would have used.
fn model_badges(payload: &serde_json::Value) -> Vec<String> {
    let mut badges = Vec::new();
    if payload.get("execution_parameters").is_some() {
        if let Some(provider) = json_str(payload, &["execution_parameters", "provider"]) {
            badges.push(badge(BADGE_INDIGO, format!("Provider: {provider}")));
        }
        if let Some(model) = json_str(payload, &["execution_parameters", "model"]) {
            badges.push(badge(BADGE_INDIGO, format!("Model: {model}")));
        }
        if let Some(agent) = json_str(payload, &["execution_parameters", "agent_name"]) {
            badges.push(badge(BADGE_PURPLE, format!("Agent: {agent}")));
        }
        return badges;
    }

    let inherited = |key: &str| {
        json_str(payload, &["channel", key]).or_else(|| json_str(payload, &["company", key]))
    };
    if let Some(provider) = inherited("provider") {
        badges.push(badge(BADGE_INDIGO, format!("Provider: {provider}")));
    }
    if let Some(model) = inherited("model") {
        badges.push(badge(BADGE_INDIGO, format!("Model: {model}")));
    }
    badges
}

fn email_badges(payload: &serde_json::Value) -> Vec<String> {
    let mut badges = Vec::new();
    if payload.get("parsed_email").is_none() {
        return badges;
    }
    if let Some(sender) = json_str(payload, &["parsed_email", "sender"]) {
        badges.push(badge(BADGE_SLATE, format!("Sender: {sender}")));
    }
    if let Some(subject) = json_str(payload, &["parsed_email", "subject"]) {
        badges.push(badge(BADGE_SLATE, format!("Subject: {subject}")));
    }
    badges
}

/// Was any non-empty agent configuration in play, from either the run or the channel?
fn config_badge(payload: &serde_json::Value) -> Option<String> {
    let is_populated = |path: &[&str]| {
        let mut current = payload;
        for key in path {
            match current.get(key) {
                Some(next) => current = next,
                None => return false,
            }
        }
        !current.is_null() && current != &serde_json::json!({})
    };
    let present = is_populated(&["execution_parameters", "config"])
        || is_populated(&["channel", "channel_config"]);
    present.then(|| badge(BADGE_EMERALD, "Config: Present"))
}

fn token_badge(payload: &serde_json::Value) -> Option<String> {
    let usage = payload
        .get("execution_result")
        .and_then(|result| result.get("token_usage"))
        .or_else(|| payload.get("token_usage"))?;
    let (prompt, completion, total) = (
        usage.get("prompt_tokens").and_then(|v| v.as_u64())?,
        usage.get("completion_tokens").and_then(|v| v.as_u64())?,
        usage.get("total_tokens").and_then(|v| v.as_u64())?,
    );
    Some(badge(
        BADGE_TOKENS,
        format!(
            "{glyph} Token Meter: {total} tokens (Prompt: {prompt} | Completion: {completion})",
            glyph = icon(Icon::Graph, BUTTON_ICON),
        ),
    ))
}

/// Diagnostics the runner attached to the response: timing, tool use, and why generation stopped.
fn metadata_badges(metadata: &serde_json::Value) -> Vec<String> {
    let mut badges = Vec::new();

    if metadata.get("execution_diagnostics").is_some() {
        if let Some(duration_ms) = json_u64(metadata, &["execution_diagnostics", "duration_ms"]) {
            badges.push(badge(BADGE_CYAN, format!("Duration: {duration_ms} ms")));
        }
        if let Some(source) = json_str(metadata, &["execution_diagnostics", "token_usage_source"]) {
            badges.push(badge(BADGE_SLATE, format!("Token Usage: {source}")));
        }
        if let Some(chars) = json_u64(metadata, &["execution_diagnostics", "response_characters"]) {
            badges.push(badge(BADGE_SLATE, format!("Response: {chars} chars")));
        }
        if let Some(tool_calls) = json_u64(metadata, &["execution_diagnostics", "tool_call_count"])
            .filter(|count| *count > 0)
        {
            badges.push(badge(
                BADGE_PURPLE_STRONG,
                format!("Tool Calls: {tool_calls}"),
            ));
        }
    }

    if let (Some(events), Some(llm_calls)) = (
        json_u64(metadata, &["observability", "summary", "total_events"]),
        json_u64(metadata, &["observability", "summary", "total_llm_calls"]),
    ) {
        badges.push(badge(
            BADGE_TEAL,
            format!("Observed: {events} events / {llm_calls} LLM calls"),
        ));
    }

    badges.push(finish_reason_badge(metadata));
    badges
}

/// Why generation stopped. `length`/`max_tokens` is called out loudly because it means the reply
/// was cut off rather than finished.
fn finish_reason_badge(metadata: &serde_json::Value) -> String {
    let reason = metadata
        .get("finish_reason")
        .or_else(|| metadata.get("stop_reason"))
        .and_then(|v| v.as_str());
    let Some(reason) = reason else {
        return badge(BADGE_PURPLE, "Metadata: Present");
    };
    let (style, label) = match reason {
        "length" | "max_tokens" => (
            "bg-amber-950/90 text-amber-300 border border-amber-700/80 font-bold",
            format!("Finish Reason: {reason} (TRUNCATED MID-SENTENCE)"),
        ),
        "stop" | "end_turn" => (
            "bg-emerald-950/90 text-emerald-300 border border-emerald-700/80 font-semibold",
            format!("Finish Reason: {reason}"),
        ),
        other => (
            "bg-slate-800 text-slate-300 border border-slate-700 font-semibold",
            format!("Finish Reason: {other}"),
        ),
    };
    badge(
        style,
        format!("{glyph} {label}", glyph = icon(Icon::Goal, BUTTON_ICON)),
    )
}

/// The collapsible "Task Execution Parameters" block shown under a message: summary chips over the
/// full (secret-scrubbed) task payload.
pub fn render_message_task_parameters_html(payload: &serde_json::Value) -> String {
    let sanitized_payload = sanitize_json_payload(payload);
    let payload_str = super::escape_html_text(
        &serde_json::to_string_pretty(&sanitized_payload).unwrap_or_else(|_| payload.to_string()),
    );

    let mut badges = model_badges(&sanitized_payload);
    badges.extend(email_badges(&sanitized_payload));
    badges.extend(config_badge(&sanitized_payload));
    badges.extend(token_badge(&sanitized_payload));
    if let Some(metadata) = sanitized_payload
        .get("execution_result")
        .and_then(|result| result.get("metadata"))
        .or_else(|| sanitized_payload.get("metadata"))
    {
        badges.extend(metadata_badges(metadata));
    }

    let summary_badges_html = if badges.is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="flex flex-wrap gap-1.5 mb-2">{}</div>"#,
            badges.join("")
        )
    };

    format!(
        r##"
        <details class="mt-3 border-t border-slate-800/80 pt-3 group">
            <summary class="cursor-pointer text-xs font-semibold text-slate-400 hover:text-indigo-300 transition flex items-center gap-1.5 select-none">
                <span class="text-indigo-400 group-open:rotate-90 transition-transform">{disclosure}</span>
                <span>Task Execution Parameters</span>
            </summary>
            <div class="mt-2.5">
                {summary_badges_html}
                <pre class="bg-slate-950 p-3 rounded-lg text-emerald-300 font-mono text-[11px] border border-slate-800/80 overflow-x-auto whitespace-pre-wrap max-h-96">{payload_str}</pre>
            </div>
        </details>
        "##,
        disclosure = icon(Icon::ChevronRight, "h-3 w-3"),
    )
}

/// What state a task is in, as one pill.
///
/// `glyph` is `None` for the states that speak for themselves and `Some` for the two that are
/// parked on somebody outside the queue -- those share an hourglass because they are the same kind
/// of wait, differing only in who is being waited on.
fn status_pill(tint: &str, label: &str, glyph: Option<Icon>) -> String {
    format!(
        r##"<span class="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-semibold border {tint}">{glyph}{label}</span>"##,
        glyph = match glyph {
            Some(glyph) => icon(glyph, BUTTON_ICON),
            None => String::new(),
        },
    )
}

pub fn task_row_fragment(company_id: Uuid, task: &BackgroundTask) -> String {
    let created_at_str = super::format_time(task.created_at);
    let status_badge = match task.status {
        TaskStatus::Pending => status_pill(
            "bg-amber-950 text-amber-300 border-amber-700/50",
            "Pending",
            None,
        ),
        TaskStatus::Processing => status_pill(
            "bg-indigo-950 text-indigo-300 border-indigo-700/50 animate-pulse",
            "Processing",
            None,
        ),
        TaskStatus::PendingApproval => status_pill(
            "bg-sky-950 text-sky-300 border-sky-700/50",
            "Awaiting Approval",
            Some(Icon::Hourglass),
        ),
        TaskStatus::WaitingForThirdPartyReply => status_pill(
            "bg-cyan-950 text-cyan-300 border-cyan-700/50",
            "Awaiting 3rd Party Reply",
            Some(Icon::Hourglass),
        ),
        TaskStatus::Completed => status_pill(
            "bg-emerald-950 text-emerald-300 border-emerald-700/50",
            "Completed",
            None,
        ),
        TaskStatus::Failed => status_pill(
            "bg-rose-950 text-rose-300 border-rose-700/50",
            "Failed",
            None,
        ),
        TaskStatus::DeadLetter => status_pill(
            "bg-purple-950 text-purple-300 border-purple-700/50",
            "Dead Letter",
            None,
        ),
        TaskStatus::Stopped => status_pill(
            "bg-slate-800 text-slate-400 border-slate-600",
            "Stopped",
            None,
        ),
    };

    let action_button = match task.status {
        TaskStatus::Pending | TaskStatus::Processing | TaskStatus::Failed => format!(
            r##"<button hx-post="/companies/{company_id}/tasks/{task_id}/stop" hx-target="#task-{task_id}" hx-swap="outerHTML"
                class="px-3 py-1.5 text-xs font-semibold bg-rose-950 hover:bg-rose-900 text-rose-300 border border-rose-800/50 rounded-lg transition cursor-pointer">
                Stop Task
            </button>"##,
            company_id = company_id,
            task_id = task.id
        ),
        TaskStatus::Stopped | TaskStatus::DeadLetter => format!(
            r##"<button hx-post="/companies/{company_id}/tasks/{task_id}/resume" hx-target="#task-{task_id}" hx-swap="outerHTML"
                class="px-3 py-1.5 text-xs font-semibold bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg transition cursor-pointer shadow-md shadow-emerald-600/30">
                Resume Task
            </button>"##,
            company_id = company_id,
            task_id = task.id
        ),
        _ => String::new(),
    };

    let thread_link = match task.thread_id {
        Some(tid) => format!(
            r##"<a href="/companies/{company_id}/channels/{channel_id}/simulate?thread_id={tid}"
                class="px-3 py-1.5 text-xs font-semibold bg-indigo-600/90 hover:bg-indigo-500 text-white rounded-lg transition flex items-center gap-1 shadow-sm whitespace-nowrap">
                <span>Open Thread</span>
            </a>"##,
            company_id = company_id,
            channel_id = task.channel_id,
            tid = tid
        ),
        None => String::new(),
    };

    let thread_info = match task.thread_id {
        Some(tid) => format!(
            r##" • Thread: <a href="/companies/{company_id}/channels/{channel_id}/simulate?thread_id={tid}" class="font-mono text-emerald-400 hover:text-emerald-300 underline font-medium">{tid}</a>"##,
            company_id = company_id,
            channel_id = task.channel_id,
            tid = tid
        ),
        None => String::new(),
    };

    let error_html = match &task.last_error {
        Some(err) if !err.is_empty() => format!(
            r##"<div class="mt-2 text-xs font-mono bg-slate-950/80 p-2 rounded border border-rose-900/50 text-rose-300">Error: {err}</div>"##,
            err = escape_html_text(err),
        ),
        _ => String::new(),
    };

    let token_meter_badge = if let Some(tu) = task.token_usage() {
        format!(
            r##"<div class="mt-2 inline-flex items-center gap-2 px-2.5 py-1 rounded-md bg-indigo-950/80 border border-indigo-800/70 text-indigo-300 font-mono text-xs shadow-sm">
                <span class="inline-flex items-center gap-1.5 text-indigo-400 font-semibold">{glyph} Token Meter:</span>
                <span class="text-white font-bold">{total} total</span>
                <span class="text-slate-400 text-[11px]">(Prompt: {prompt} • Completion: {completion})</span>
            </div>"##,
            glyph = icon(Icon::Graph, BUTTON_ICON),
            total = tu.total_tokens,
            prompt = tu.prompt_tokens,
            completion = tu.completion_tokens,
        )
    } else {
        String::new()
    };

    let parameters_html = render_message_task_parameters_html(&task.payload);

    format!(
        r##"
        <div id="task-{task_id}" class="bg-slate-900/80 border border-slate-700/70 rounded-xl p-4 md:p-5 hover:border-slate-600 transition shadow-sm">
            <div class="flex items-center justify-between">
                <div>
                    <div class="flex items-center gap-3">
                        <span class="font-mono text-xs text-slate-300 font-semibold">{task_id}</span>
                        {status_badge}
                        <span class="text-xs text-slate-400 font-mono">Retries: {retry_count}/{max_retries}</span>
                    </div>
                    <p class="text-xs text-slate-400 mt-1">Type: <span class="font-mono text-indigo-300">{task_type}</span> • Enqueued {created_at_str}{thread_info}</p>
                    {token_meter_badge}
                </div>
                <div class="flex items-center gap-2">
                    {thread_link}
                    {action_button}
                </div>
            </div>
            {error_html}
            {parameters_html}
        </div>
        "##,
        task_id = task.id,
        status_badge = status_badge,
        retry_count = task.retry_count,
        max_retries = task.max_retries,
        task_type = escape_html_text(&task.task_type),
        created_at_str = created_at_str,
        thread_info = thread_info,
        token_meter_badge = token_meter_badge,
        thread_link = thread_link,
        action_button = action_button,
        error_html = error_html,
        parameters_html = parameters_html,
    )
}
