//! The `/ui` Outbox workspace: every email the company has handed to the transport, and what
//! became of it.
//!
//! Built like the Tasks workspace next door — a filtered sidebar, one entry's detail swapped into
//! `#outbox-pane` over htmx — because it answers the other half of the same question. Tasks says
//! what the agent decided; this says whether the mail it wrote ever left the building.
//!
//! Nothing here writes. The poller owns these rows, so an entry offers a link to the task that
//! produced it rather than buttons that would race the transport.

use super::*;

/// One filtered page of the outbox, as the sidebar shows it.
pub struct OutboxList<'a> {
    pub company: &'a Company,
    pub entries: &'a [OutboxEntry],
    pub filter: &'a OutboxFilter,
    /// Whether a further page follows this one, so the pager knows to offer Next.
    pub has_next: bool,
    pub selected_entry_id: Option<Uuid>,
}

/// The Outbox workspace for one request.
pub struct OutboxPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub companies: &'a [Company],
    /// The company's channels, as the sidebar's channel filter offers them.
    pub channels: &'a [Channel],
    pub list: &'a OutboxList<'a>,
    /// Pre-rendered right-hand pane: one entry's detail, or a placeholder.
    pub pane_html: &'a str,
}

/// The pane for one queued email: the envelope, the transport's verdict, and the way back to the
/// task that wrote it.
pub struct OutboxDetailPane<'a> {
    pub company_id: Uuid,
    pub entry: &'a OutboxEntry,
    /// The task this email came out of, when it is still one of the company's. Absent for mail
    /// queued outside a task, or whose task has since been cleared.
    pub task: Option<&'a BackgroundTask>,
    /// The channel it goes out as, when that channel still exists. The pane falls back to the name
    /// recorded in the payload, so a deleted channel still reads as something.
    pub channel: Option<&'a Channel>,
}

/// The `/ui/outbox` URL for a given selection, i.e. what a click on it should leave in the address
/// bar. The list fragment takes exactly the same parameters, so the two share this.
pub fn outbox_query(
    company_id: Uuid,
    filter: &OutboxFilter,
    selected_entry_id: Option<Uuid>,
) -> String {
    let mut params = vec![format!("company_id={company_id}")];
    if let Some(channel_id) = filter.channel_id {
        params.push(format!("channel_id={channel_id}"));
    }
    if let Some(status) = filter.status {
        params.push(format!("status={}", status.as_str()));
    }
    if filter.sort_asc {
        params.push("sort=asc".to_string());
    }
    if filter.limit() != OutboxFilter::DEFAULT_PAGE_SIZE {
        params.push(format!("limit={}", filter.limit()));
    }
    if filter.page() > 1 {
        params.push(format!("page={}", filter.page()));
    }
    if let Some(entry_id) = selected_entry_id {
        params.push(format!("entry_id={entry_id}"));
    }
    params.join("&")
}

fn outbox_url(company_id: Uuid, filter: &OutboxFilter, selected_entry_id: Option<Uuid>) -> String {
    format!(
        "/ui/outbox?{}",
        outbox_query(company_id, filter, selected_entry_id)
    )
}

fn outbox_list_url(
    company_id: Uuid,
    filter: &OutboxFilter,
    selected_entry_id: Option<Uuid>,
) -> String {
    format!(
        "/ui/outbox/list?{}",
        outbox_query(company_id, filter, selected_entry_id)
    )
}

pub fn outbox_page(page: &OutboxPage<'_>) -> String {
    let company = page.list.company;
    let content = format!(
        r##"
        <aside class="flex w-80 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {header}
            {company_switcher}
            {filters}
            {list_html}
        </aside>
        {pane_html}
        "##,
        header = sidebar_header("Outbox", "Queued and delivered outbound messages."),
        company_switcher = company_switcher(company, page.companies, UiSection::Outbox),
        filters = outbox_filter_form(company.id, page.channels, page.list.filter),
        list_html = outbox_list(page.list, FragmentSwap::Inline),
        pane_html = page.pane_html,
    );

    ui_shell(&UiShell {
        title: &format!("{} Outbox", company.name),
        user: page.user,
        company_id: Some(company.id),
        section: UiSection::Outbox,
        content: &content,
        script: "",
    })
}

/// Status and order, as two selects that re-fetch the list between them.
///
/// The form sits outside `#outbox-list`, so a swap never takes the controls out from under the
/// pointer; a change resets to the first page simply by not sending one.
fn outbox_filter_form(company_id: Uuid, channels: &[Channel], filter: &OutboxFilter) -> String {
    let channel_options = channel_filter_options(channels, filter.channel_id);

    let status_options: String = OutboxStatus::ALL
        .iter()
        .map(|status| {
            format!(
                r##"<option value="{value}"{selected}>{label}</option>"##,
                value = status.as_str(),
                selected = selected_when(filter.status == Some(*status)),
                label = status.label(),
            )
        })
        .collect();

    format!(
        r##"
            <form class="space-y-2 border-b border-base-300 px-3 pb-3"
                hx-get="/ui/outbox/list" hx-trigger="change"
                hx-target="#outbox-list" hx-swap="outerHTML">
                <input type="hidden" name="company_id" value="{company_id}">
                <input type="hidden" name="limit" value="{limit}">
                <select name="channel_id" class="select select-sm w-full" aria-label="Filter by channel">
                    <option value="">All channels</option>
                    {channel_options}
                </select>
                <select name="status" class="select select-sm w-full" aria-label="Filter by delivery status">
                    <option value="">All statuses</option>
                    {status_options}
                </select>
                <select name="sort" class="select select-sm w-full" aria-label="Sort by time">
                    <option value="desc"{newest_selected}>Newest first</option>
                    <option value="asc"{oldest_selected}>Oldest first</option>
                </select>
            </form>
        "##,
        limit = filter.limit(),
        newest_selected = selected_when(!filter.sort_asc),
        oldest_selected = selected_when(filter.sort_asc),
    )
}

/// One filtered page of the outbox. Rendered out of band when it rides along with a pane, so a
/// reload shows the same state in both places at once.
pub fn outbox_list(list: &OutboxList<'_>, swap: FragmentSwap) -> String {
    let company_id = list.company.id;
    let entries: String = list
        .entries
        .iter()
        .map(|entry| outbox_menu_entry(list, entry))
        .collect();

    let menu_body = if list.entries.is_empty() {
        r##"<li class="px-2 py-6 text-center text-xs opacity-60">No queued email matches these filters.</li>"##
            .to_string()
    } else {
        entries
    };

    format!(
        r##"
            <div id="outbox-list"{LIST_SKELETON} class="flex min-h-0 flex-1 flex-col"{oob}>
                <div class="flex items-center justify-between gap-2 px-3 py-2 text-[11px] opacity-60">
                    <span class="truncate">{summary}</span>
                    <button type="button" class="btn btn-ghost btn-xs" title="Reload this page of the outbox"
                        hx-get="{reload_url}" hx-target="#outbox-list" hx-swap="outerHTML">{reload_glyph}</button>
                </div>
                <ul id="outbox-menu" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2">{menu_body}</ul>
                {pager}
            </div>
        "##,
        oob = swap.oob_attribute(),
        summary = escape_html_text(&outbox_page_summary(list)),
        reload_glyph = icon(Icon::Sync, BUTTON_ICON),
        reload_url = outbox_list_url(company_id, list.filter, list.selected_entry_id),
        pager = outbox_pager(list),
    )
}

/// What this page adds up to: how many emails, and how many of them are still not out the door.
fn outbox_page_summary(list: &OutboxList<'_>) -> String {
    let count = list.entries.len();
    let page = list.filter.page();
    let counted = if count == 1 {
        "1 email".to_string()
    } else {
        format!("{count} emails")
    };

    let undelivered = list
        .entries
        .iter()
        .filter(|entry| entry.status != OutboxStatus::Sent)
        .count();

    if undelivered == 0 {
        format!("{counted} · page {page}")
    } else {
        format!("{counted} · {undelivered} unsent · page {page}")
    }
}

fn outbox_menu_entry(list: &OutboxList<'_>, entry: &OutboxEntry) -> String {
    let company_id = list.company.id;

    format!(
        r##"
                <li>
                    <a class="flex flex-col items-start gap-0.5 {active}"
                        hx-get="/ui/outbox/{entry_id}?company_id={company_id}"
                        hx-target="#outbox-pane" hx-swap="outerHTML"
                        hx-push-url="{push_url}"
                        onclick="selectSidebarItem(this)">
                        <span class="flex w-full items-center gap-2">
                            <span class="badge badge-sm shrink-0 {status_style}">{status_label}</span>
                            <span class="min-w-0 truncate text-xs">{subject}</span>
                        </span>
                        <span class="w-full truncate font-mono text-[11px] opacity-60">{recipient} · {queued}</span>
                    </a>
                </li>
        "##,
        active = if list.selected_entry_id == Some(entry.id) {
            "menu-active"
        } else {
            ""
        },
        entry_id = entry.id,
        push_url = outbox_url(company_id, list.filter, Some(entry.id)),
        status_style = outbox_status_style(entry.status),
        status_label = entry.status.label(),
        subject = escape_html_text(entry.subject().unwrap_or("(no subject)")),
        recipient = escape_html_text(entry.recipient().unwrap_or("unknown recipient")),
        queued = super::format_time(entry.created_at),
    )
}

/// Previous / next for the filtered list; absent entirely on a single-page list.
fn outbox_pager(list: &OutboxList<'_>) -> String {
    let company_id = list.company.id;
    let filter = list.filter;
    let button = |page: usize, label: &str| {
        format!(
            r##"<button type="button" class="btn btn-ghost btn-xs"
                        hx-get="{url}" hx-target="#outbox-list" hx-swap="outerHTML">{label}</button>"##,
            url = outbox_list_url(company_id, &filter.on_page(page), list.selected_entry_id),
        )
    };

    // Which way "back" runs depends on the order the list is in, so the labels follow the sort.
    let (back, forward) = (
        icon(Icon::ArrowLeft, BUTTON_ICON),
        icon(Icon::ArrowRight, BUTTON_ICON),
    );
    let (previous_label, next_label) = if filter.sort_asc {
        (format!("{back} Older"), format!("Newer {forward}"))
    } else {
        (format!("{back} Newer"), format!("Older {forward}"))
    };
    let previous = if filter.page() > 1 {
        button(filter.page() - 1, &previous_label)
    } else {
        String::new()
    };
    let next = if list.has_next {
        button(filter.page().saturating_add(1), &next_label)
    } else {
        String::new()
    };

    if previous.is_empty() && next.is_empty() {
        return String::new();
    }

    format!(
        r##"<nav aria-label="Outbox pages" class="flex items-center justify-between gap-2 border-t border-base-300 p-2">
                    <div>{previous}</div>
                    <div>{next}</div>
                </nav>"##
    )
}

/// The pane before an email is picked.
pub fn outbox_empty_pane(message: &str, swap: FragmentSwap) -> String {
    format!(
        r##"
        <section id="outbox-pane"{PANE_SKELETON} class="flex flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <p class="text-center text-sm opacity-60">{message}</p>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub fn outbox_detail_pane(pane: &OutboxDetailPane<'_>) -> String {
    let entry = pane.entry;
    let company_id = pane.company_id;

    format!(
        r##"
        <section id="outbox-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100">
            <div class="flex items-start justify-between gap-3 border-b border-base-300 px-6 py-4">
                <div class="min-w-0">
                    <h2 class="flex items-center gap-2 truncate text-xl font-bold">
                        <span class="badge {status_style}">{status_label}</span>
                        <span class="truncate">{subject}</span>
                    </h2>
                    <p class="truncate font-mono text-xs opacity-60">{entry_id}</p>
                </div>
                <div class="flex shrink-0 items-center gap-2">
                    {task_link}
                    <button type="button" class="btn btn-ghost btn-sm btn-square text-xl leading-none" title="Reload this email"
                        hx-get="/ui/outbox/{entry_id}?company_id={company_id}"
                        hx-target="#outbox-pane" hx-swap="outerHTML">{reload_glyph}</button>
                </div>
            </div>
            <div class="flex-1 space-y-4 overflow-y-auto px-6 py-4">
                {delivery}
                {facts}
                {task_summary}
                {last_error}
                {payload}
            </div>
        </section>
        "##,
        status_style = outbox_status_style(entry.status),
        status_label = entry.status.label(),
        subject = escape_html_text(entry.subject().unwrap_or("(no subject)")),
        entry_id = entry.id,
        reload_glyph = icon(Icon::Sync, BUTTON_ICON),
        task_link = outbox_task_link(pane),
        delivery = outbox_delivery_state(entry),
        facts = outbox_facts(pane),
        task_summary = outbox_task_summary(pane),
        last_error = outbox_last_error(entry),
        payload = outbox_payload(entry),
    )
}

/// The task that wrote this email is where its story starts, so the pane opens it in the Tasks
/// workspace rather than only naming its id.
fn outbox_task_link(pane: &OutboxDetailPane<'_>) -> String {
    let Some(task_id) = pane.entry.task_id else {
        return String::new();
    };

    format!(
        r##"<a href="/ui/tasks?company_id={company_id}&task_id={task_id}"
                        class="btn btn-outline btn-sm">Open Task</a>"##,
        company_id = pane.company_id,
    )
}

/// The one line that says where this email stands, in the words the status actually means.
///
/// A failed row is called out loudly: nothing retries it, and the task that wrote it completed
/// successfully, so this pane is the only place the undelivered mail becomes visible.
fn outbox_delivery_state(entry: &OutboxEntry) -> String {
    let (style, detail) = match entry.status {
        OutboxStatus::Sent => (
            "alert-success",
            match entry.sent_at {
                Some(sent_at) => format!("Delivered {}", super::format_time(sent_at)),
                None => "Delivered".to_string(),
            },
        ),
        OutboxStatus::Sending => (
            "alert-info",
            "Claimed by a sender and being handed to the provider now.".to_string(),
        ),
        OutboxStatus::Failed => (
            "alert-error",
            "Every attempt was used up. This email will not be delivered without intervention."
                .to_string(),
        ),
        OutboxStatus::Pending => (
            "alert-warning",
            format!(
                "Queued · next attempt {}",
                super::format_time(entry.available_at)
            ),
        ),
    };

    let attempts = if entry.retry_count > 0 {
        format!(" · {} failed attempt(s)", entry.retry_count)
    } else {
        String::new()
    };

    format!(
        r##"<div class="alert {style} text-sm"><span>{detail}{attempts}</span></div>"##,
        detail = escape_html_text(&detail),
        attempts = escape_html_text(&attempts),
    )
}

/// The envelope and the queue's own bookkeeping, as one table of facts.
fn outbox_facts(pane: &OutboxDetailPane<'_>) -> String {
    let entry = pane.entry;
    let dash = || "—".to_string();
    let cc = entry.recipients_cc();
    let cc = if cc.is_empty() { dash() } else { cc.join(", ") };

    let rows: String = [
        ("To", entry.recipient().unwrap_or("—").to_string()),
        ("Cc", cc),
        ("Channel", outbox_channel_name(pane)),
        ("Queued", super::format_time(entry.created_at)),
        ("Updated", super::format_time(entry.updated_at)),
        (
            "Sent",
            entry.sent_at.map(super::format_time).unwrap_or_else(dash),
        ),
        ("Attempts", entry.retry_count.to_string()),
        (
            "Provider message",
            entry.provider_message_id.clone().unwrap_or_else(dash),
        ),
        ("Idempotency key", entry.idempotency_key.clone()),
    ]
    .into_iter()
    .map(|(label, value)| {
        format!(
            r##"<div class="flex items-baseline justify-between gap-4 px-4 py-2">
                        <dt class="text-xs opacity-60">{label}</dt>
                        <dd class="min-w-0 truncate font-mono text-xs">{value}</dd>
                    </div>"##,
            value = escape_html_text(&value),
        )
    })
    .collect();

    format!(
        r##"<dl class="divide-y divide-base-300 rounded-box border border-base-300 bg-base-200">{rows}</dl>"##
    )
}

/// What to call the channel this email goes out as: the live channel when it still exists, else
/// the name the payload recorded when the email was composed.
fn outbox_channel_name(pane: &OutboxDetailPane<'_>) -> String {
    match pane.channel {
        Some(channel) => channel.name.clone(),
        None => pane.entry.channel_name().unwrap_or("—").to_string(),
    }
}

/// What the task behind this email is doing now — the reason an email is still queued is often the
/// task, not the transport.
fn outbox_task_summary(pane: &OutboxDetailPane<'_>) -> String {
    let Some(task_id) = pane.entry.task_id else {
        return String::new();
    };

    let (badge, task_type) = match pane.task {
        Some(task) => (
            format!(
                r##"<span class="badge badge-sm {style}">{label}</span>"##,
                style = task_status_style(task.status),
                label = task_status_label(task.status),
            ),
            escape_html_text(&task.task_type),
        ),
        None => (
            r##"<span class="badge badge-sm badge-ghost">Unavailable</span>"##.to_string(),
            "This email's task is no longer readable here.".to_string(),
        ),
    };

    format!(
        r##"<div class="space-y-2">
                    <h3 class="text-xs font-semibold uppercase opacity-60">Task</h3>
                    <a class="flex items-center gap-2 rounded-box border border-base-300 bg-base-200 px-4 py-2 text-xs hover:border-primary"
                        href="/ui/tasks?company_id={company_id}&task_id={task_id}">
                        {badge}
                        <span class="min-w-0 truncate">{task_type}</span>
                        <span class="ml-auto shrink-0 font-mono opacity-60">{task_id}</span>
                    </a>
                </div>"##,
        company_id = pane.company_id,
    )
}

fn outbox_last_error(entry: &OutboxEntry) -> String {
    match entry.last_error.as_deref() {
        Some(error) if !error.is_empty() => format!(
            r##"<div class="alert alert-error text-xs"><span class="font-mono">{}</span></div>"##,
            escape_html_text(error)
        ),
        _ => String::new(),
    }
}

/// The email as it will actually be sent, secret-scrubbed the same way a task payload is.
fn outbox_payload(entry: &OutboxEntry) -> String {
    let sanitized = sanitize_json_payload(&entry.payload);
    let payload =
        serde_json::to_string_pretty(&sanitized).unwrap_or_else(|_| sanitized.to_string());

    format!(
        r##"
                <details class="rounded-box border border-base-300 bg-base-200">
                    <summary class="cursor-pointer px-4 py-2 text-xs font-semibold uppercase opacity-60">Payload</summary>
                    <pre class="overflow-x-auto px-4 pb-4 font-mono text-[11px] whitespace-pre-wrap">{payload}</pre>
                </details>
        "##,
        payload = escape_html_text(&payload),
    )
}

/// One badge style per delivery status, shared with the Tasks pane so an email reads the same
/// in both workspaces.
pub(crate) fn outbox_status_style(status: OutboxStatus) -> &'static str {
    match status {
        OutboxStatus::Pending => "badge-warning",
        OutboxStatus::Sending => "badge-info animate-pulse",
        OutboxStatus::Sent => "badge-success",
        OutboxStatus::Failed => "badge-error",
    }
}
