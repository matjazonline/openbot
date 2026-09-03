//! The `/ui` Deliveries workspace: every message the company has handed to a transport, and what
//! became of it.
//!
//! Built like the Tasks workspace next door — a filtered sidebar, one entry's detail swapped into
//! `#delivery-pane` over htmx — because it answers the other half of the same question. Tasks says
//! what the agent decided; this says whether the answer it wrote ever left the building.
//!
//! It replaces an Outbox that could only be an email outbox. What a reader sees now is the
//! transport, the purpose and the interface as columns of their own, and one row per provider
//! message rather than one opaque payload blob: a chat answer sent as three posts has three
//! provider keys, and the shape this replaces had room for one.
//!
//! Nothing here writes. The delivery worker owns these rows, so an entry offers a link to the task
//! that produced it rather than buttons that would race the transport.

use super::*;

/// One filtered page of the delivery queue, as the sidebar shows it.
pub struct DeliveryList<'a> {
    pub company: &'a Company,
    pub entries: &'a [DeliveryEntry],
    pub filter: &'a DeliveryFilter,
    /// Whether a further page follows this one, so the pager knows to offer Next.
    pub has_next: bool,
    pub selected_entry_id: Option<Uuid>,
}

/// The Deliveries workspace for one request.
pub struct DeliveriesPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub companies: &'a [Company],
    /// The company's channels, as the sidebar's channel filter offers them.
    pub channels: &'a [Channel],
    pub list: &'a DeliveryList<'a>,
    /// Pre-rendered right-hand pane: one delivery's detail, or a placeholder.
    pub pane_html: &'a str,
}

/// The pane for one delivery: where it stands, what it is carrying, and the way back to the task
/// that wrote it.
pub struct DeliveryDetailPane<'a> {
    pub company_id: Uuid,
    pub entry: &'a DeliveryEntry,
    /// The task this delivery came out of, when it is still one of the company's. Absent for a
    /// delivery queued outside a task, or whose task has since been cleared.
    pub task: Option<&'a BackgroundTask>,
    /// The channel it goes out as, when that channel still exists.
    pub channel: Option<&'a Channel>,
}

/// The `/ui/deliveries` URL for a given selection, i.e. what a click on it should leave in the
/// address bar. The list fragment takes exactly the same parameters, so the two share this.
pub fn delivery_query(
    company_id: Uuid,
    filter: &DeliveryFilter,
    selected_entry_id: Option<Uuid>,
) -> String {
    let mut params = vec![format!("company_id={company_id}")];
    if let Some(channel_id) = filter.channel_id {
        params.push(format!("channel_id={channel_id}"));
    }
    if let Some(status) = filter.status {
        params.push(format!("status={}", status.as_str()));
    }
    if let Some(transport) = filter.transport {
        params.push(format!("transport={}", transport.as_str()));
    }
    if let Some(purpose) = filter.purpose {
        params.push(format!("purpose={}", purpose.as_str()));
    }
    if filter.sort_asc {
        params.push("sort=asc".to_string());
    }
    if filter.limit() != DeliveryFilter::DEFAULT_PAGE_SIZE {
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

fn delivery_url(
    company_id: Uuid,
    filter: &DeliveryFilter,
    selected_entry_id: Option<Uuid>,
) -> String {
    format!(
        "/ui/deliveries?{}",
        delivery_query(company_id, filter, selected_entry_id)
    )
}

fn delivery_list_url(
    company_id: Uuid,
    filter: &DeliveryFilter,
    selected_entry_id: Option<Uuid>,
) -> String {
    format!(
        "/ui/deliveries/list?{}",
        delivery_query(company_id, filter, selected_entry_id)
    )
}

pub fn deliveries_page(page: &DeliveriesPage<'_>) -> String {
    let company = page.list.company;
    let content = format!(
        r##"
        <aside class="ui-pane-list flex w-80 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {header}
            {filters}
            {list_html}
        </aside>
        {pane_html}
        "##,
        header = sidebar_header("Deliveries", "Outbound messages handed to a transport."),
        filters = delivery_filter_form(company.id, page.channels, page.list.filter),
        list_html = delivery_list(page.list, FragmentSwap::Inline),
        pane_html = page.pane_html,
    );

    ui_shell(&UiShell {
        title: &format!("{} Deliveries", company.name),
        user: page.user,
        company: Some(company),
        section: UiSection::Deliveries,
        content: &content,
    })
}

/// Channel, status, transport, purpose and order, as selects that re-fetch the list between them.
///
/// The form sits outside `#delivery-list`, so a swap never takes the controls out from under the
/// pointer; a change resets to the first page simply by not sending one.
fn delivery_filter_form(company_id: Uuid, channels: &[Channel], filter: &DeliveryFilter) -> String {
    let channel_options = channel_filter_options(channels, filter.channel_id);
    let options = |chosen: bool, value: &str, label: &str| {
        format!(
            r##"<option value="{value}"{selected}>{label}</option>"##,
            selected = selected_when(chosen),
        )
    };

    let status_options: String = DeliveryStatus::ALL
        .iter()
        .map(|status| {
            options(
                filter.status == Some(*status),
                status.as_str(),
                status.label(),
            )
        })
        .collect();
    let transport_options: String = TransportKind::ALL
        .iter()
        .map(|transport| {
            options(
                filter.transport == Some(*transport),
                transport.as_str(),
                transport.label(),
            )
        })
        .collect();
    let purpose_options: String = DeliveryPurpose::ALL
        .iter()
        .map(|purpose| {
            options(
                filter.purpose == Some(*purpose),
                purpose.as_str(),
                purpose.label(),
            )
        })
        .collect();

    format!(
        r##"
            <form class="space-y-2 border-b border-base-300 px-3 pb-3"
                hx-get="/ui/deliveries/list" hx-trigger="change"
                hx-target="#delivery-list" hx-swap="outerHTML" hx-sync="#delivery-list:replace">
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
                <select name="transport" class="select select-sm w-full" aria-label="Filter by transport">
                    <option value="">All transports</option>
                    {transport_options}
                </select>
                <select name="purpose" class="select select-sm w-full" aria-label="Filter by purpose">
                    <option value="">All purposes</option>
                    {purpose_options}
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

/// One filtered page of the queue. Rendered out of band when it rides along with a pane, so a
/// reload shows the same state in both places at once.
pub fn delivery_list(list: &DeliveryList<'_>, swap: FragmentSwap) -> String {
    let company_id = list.company.id;
    let entries: String = list
        .entries
        .iter()
        .map(|entry| delivery_menu_entry(list, entry))
        .collect();

    let menu_body = if list.entries.is_empty() {
        r##"<li class="px-2 py-6 text-center text-xs opacity-60">No delivery matches these filters.</li>"##
            .to_string()
    } else {
        entries
    };

    format!(
        r##"
            <div id="delivery-list"{LIST_SKELETON} class="flex min-h-0 flex-1 flex-col"{oob}>
                <div class="flex items-center justify-between gap-2 px-3 py-2 text-[11px] opacity-60">
                    <span class="truncate">{summary}</span>
                    <button type="button" class="btn btn-ghost btn-xs" title="Reload this page of the queue"
                        hx-get="{reload_url}" hx-target="#delivery-list" hx-swap="outerHTML" hx-sync="#delivery-list:replace">{reload_glyph}</button>
                </div>
                <ul id="delivery-menu" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2">{menu_body}</ul>
                {pager}
            </div>
        "##,
        oob = swap.oob_attribute(),
        summary = escape_html_text(&delivery_page_summary(list)),
        reload_glyph = icon(Icon::Sync, BUTTON_ICON),
        reload_url = delivery_list_url(company_id, list.filter, list.selected_entry_id),
        pager = delivery_pager(list),
    )
}

/// What this page adds up to: how many deliveries, and how many are still not out the door.
fn delivery_page_summary(list: &DeliveryList<'_>) -> String {
    let count = list.entries.len();
    let page = list.filter.page();
    let counted = if count == 1 {
        "1 delivery".to_string()
    } else {
        format!("{count} deliveries")
    };

    let undelivered = list
        .entries
        .iter()
        .filter(|entry| entry.status != DeliveryStatus::Delivered)
        .count();

    if undelivered == 0 {
        format!("{counted} · page {page}")
    } else {
        format!("{counted} · {undelivered} undelivered · page {page}")
    }
}

fn delivery_menu_entry(list: &DeliveryList<'_>, entry: &DeliveryEntry) -> String {
    let company_id = list.company.id;

    format!(
        r##"
                <li>
                    <a class="flex flex-col items-start gap-0.5 {active}"
                        hx-get="/ui/deliveries/{entry_id}?company_id={company_id}"
                        hx-target="#delivery-pane" hx-swap="outerHTML"
                        hx-sync="#delivery-pane:replace"
                        hx-push-url="{push_url}"
                        data-action="select-sidebar-item">
                        <span class="flex w-full items-center gap-2">
                            <span class="badge badge-sm shrink-0 {status_style}">{status_label}</span>
                            <span class="min-w-0 truncate text-xs">{subject}</span>
                        </span>
                        <span class="w-full truncate font-mono text-[11px] opacity-60">{transport} · {purpose} · {destination} · {queued}</span>
                    </a>
                </li>
        "##,
        active = if list.selected_entry_id == Some(entry.id.as_uuid()) {
            "menu-active"
        } else {
            ""
        },
        entry_id = entry.id,
        push_url = delivery_url(company_id, list.filter, Some(entry.id.as_uuid())),
        status_style = delivery_status_style(entry.status),
        status_label = entry.status.label(),
        subject = escape_html_text(&entry.subject),
        transport = escape_html_text(entry.transport.label()),
        purpose = escape_html_text(entry.purpose.label()),
        destination = escape_html_text(&destination_of(entry)),
        queued = super::format_time(entry.created_at),
    )
}

/// Who this delivery is for: the named recipient when one was named, else the interface itself.
fn destination_of(entry: &DeliveryEntry) -> String {
    entry
        .external_destination
        .clone()
        .unwrap_or_else(|| entry.destination_label.clone())
}

/// Previous / next for the filtered list; absent entirely on a single-page list.
fn delivery_pager(list: &DeliveryList<'_>) -> String {
    let company_id = list.company.id;
    let filter = list.filter;
    let button = |page: usize, label: &str| {
        format!(
            r##"<button type="button" class="btn btn-ghost btn-xs"
                        hx-get="{url}" hx-target="#delivery-list" hx-swap="outerHTML" hx-sync="#delivery-list:replace">{label}</button>"##,
            url = delivery_list_url(company_id, &filter.on_page(page), list.selected_entry_id),
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
        r##"<nav aria-label="Delivery pages" class="flex items-center justify-between gap-2 border-t border-base-300 p-2">
                    <div>{previous}</div>
                    <div>{next}</div>
                </nav>"##
    )
}

/// The pane before a delivery is picked.
pub fn delivery_empty_pane(message: &str, swap: FragmentSwap) -> String {
    format!(
        r##"
        <section id="delivery-pane"{PANE_SKELETON} data-pane-empty class="ui-pane-detail flex min-w-0 flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <p class="text-center text-sm opacity-60">{message}</p>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub fn delivery_detail_pane(pane: &DeliveryDetailPane<'_>) -> String {
    let entry = pane.entry;
    let company_id = pane.company_id;

    format!(
        r##"
        <section id="delivery-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="flex flex-wrap items-start justify-between gap-3 border-b border-base-300 px-4 py-4 sm:px-6">
                <div class="min-w-0 grow basis-48">
                    <h2 class="flex items-center gap-2 truncate text-xl font-bold">
                        <span class="badge {status_style}">{status_label}</span>
                        <span class="truncate">{subject}</span>
                    </h2>
                    <p class="truncate font-mono text-xs opacity-60">{entry_id}</p>
                </div>
                <div class="flex shrink-0 flex-wrap items-center gap-2">
                    {task_link}
                    <button type="button" class="btn btn-ghost btn-sm btn-square text-xl leading-none" title="Reload this delivery"
                        hx-get="/ui/deliveries/{entry_id}?company_id={company_id}"
                        hx-target="#delivery-pane" hx-swap="outerHTML" hx-sync="#delivery-pane:replace">{reload_glyph}</button>
                </div>
            </div>
            <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6">
                {state}
                {facts}
                {parts}
                {task_summary}
                {last_error}
            </div>
        </section>
        "##,
        status_style = delivery_status_style(entry.status),
        status_label = entry.status.label(),
        subject = escape_html_text(&entry.subject),
        entry_id = entry.id,
        reload_glyph = icon(Icon::Sync, BUTTON_ICON),
        task_link = delivery_task_link(pane),
        state = delivery_state(entry),
        facts = delivery_facts(pane),
        parts = delivery_parts(entry),
        task_summary = delivery_task_summary(pane),
        last_error = delivery_last_error(entry),
    )
}

/// The task that produced this delivery is where its story starts, so the pane opens it in the
/// Tasks workspace rather than only naming its id.
fn delivery_task_link(pane: &DeliveryDetailPane<'_>) -> String {
    let Some(task_id) = pane.entry.task_id else {
        return String::new();
    };

    format!(
        r##"<a href="/ui/tasks?company_id={company_id}&task_id={task_id}"
                        class="btn btn-outline btn-sm">Open Task</a>"##,
        company_id = pane.company_id,
    )
}

/// The one line that says where this delivery stands, in the words the status actually means.
///
/// The two states that need a human are called out loudly: nothing retries either, and the task
/// that wrote the message completed successfully, so this pane is the only place they become
/// visible.
fn delivery_state(entry: &DeliveryEntry) -> String {
    let (style, detail) = match entry.status {
        DeliveryStatus::Delivered => (
            "alert-success",
            match entry.delivered_at {
                Some(delivered_at) => format!("Delivered {}", super::format_time(delivered_at)),
                None => "Delivered".to_string(),
            },
        ),
        DeliveryStatus::Sending => (
            "alert-info",
            "Claimed by a delivery worker and being handed to the provider now.".to_string(),
        ),
        DeliveryStatus::DeadLetter => (
            "alert-error",
            "Every attempt was used up. This will not be delivered without intervention."
                .to_string(),
        ),
        DeliveryStatus::OutcomeUnknown => (
            "alert-error",
            "The provider may or may not have accepted this. It is deliberately not retried -- \
             re-sending is how one message becomes two -- so it waits for reconciliation."
                .to_string(),
        ),
        DeliveryStatus::Retryable => (
            "alert-warning",
            format!(
                "A previous attempt failed · next attempt {}",
                super::format_time(entry.available_at)
            ),
        ),
        DeliveryStatus::Pending => (
            "alert-warning",
            format!(
                "Queued · next attempt {}",
                super::format_time(entry.available_at)
            ),
        ),
    };

    let attempts = if entry.attempt_count > 0 {
        format!(
            " · {} of {} attempts spent",
            entry.attempt_count, entry.max_attempts
        )
    } else {
        String::new()
    };

    format!(
        r##"<div class="alert {style} text-sm"><span>{detail}{attempts}</span></div>"##,
        detail = escape_html_text(&detail),
        attempts = escape_html_text(&attempts),
    )
}

/// The routing and the queue's own bookkeeping, as one table of facts.
fn delivery_facts(pane: &DeliveryDetailPane<'_>) -> String {
    let entry = pane.entry;
    let dash = || "—".to_string();

    let rows: String = [
        ("Transport", entry.transport.label().to_string()),
        ("Purpose", entry.purpose.label().to_string()),
        ("Interface", entry.destination_label.clone()),
        (
            "Recipient",
            entry.external_destination.clone().unwrap_or_else(dash),
        ),
        ("Channel", delivery_channel_name(pane)),
        ("Queued", super::format_time(entry.created_at)),
        ("Updated", super::format_time(entry.updated_at)),
        (
            "Delivered",
            entry
                .delivered_at
                .map(super::format_time)
                .unwrap_or_else(dash),
        ),
        (
            "Attempts",
            format!("{} of {}", entry.attempt_count, entry.max_attempts),
        ),
        ("Correlation", entry.correlation_id.to_string()),
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

/// What to call the channel this goes out as: the live channel when it still exists.
fn delivery_channel_name(pane: &DeliveryDetailPane<'_>) -> String {
    match pane.channel {
        Some(channel) => channel.name.clone(),
        None => pane.entry.destination_label.clone(),
    }
}

/// One row per frozen part, with the provider's own key for it.
///
/// The rendered payload is deliberately not shown. It is the transport's wire form, and the
/// envelope facts above already say what a reader needs; dumping it was how the pane this replaces
/// showed a recipient by digging through JSON.
fn delivery_parts(entry: &DeliveryEntry) -> String {
    if entry.parts.is_empty() {
        return String::new();
    }
    let (delivered, total) = entry.part_progress();

    let rows: String = entry
        .parts
        .iter()
        .map(|part| {
            format!(
                r##"<div class="flex items-baseline justify-between gap-4 px-4 py-2">
                        <dt class="flex shrink-0 items-center gap-2 text-xs">
                            <span class="badge badge-sm {style}">{label}</span>
                            <span class="opacity-60">Part {index}</span>
                        </dt>
                        <dd class="min-w-0 truncate font-mono text-xs">{key}</dd>
                    </div>"##,
                style = part_status_style(part.status),
                label = escape_html_text(part_status_label(part.status)),
                index = part.index,
                key = escape_html_text(part.provider_message_key.as_deref().unwrap_or("—")),
            )
        })
        .collect();

    format!(
        r##"<div class="space-y-2">
                    <h3 class="text-xs font-semibold uppercase opacity-60">Provider messages · {delivered} of {total} confirmed</h3>
                    <dl class="divide-y divide-base-300 rounded-box border border-base-300 bg-base-200">{rows}</dl>
                </div>"##
    )
}

/// What the task behind this delivery is doing now — the reason a delivery is still queued is
/// often the task, not the transport.
fn delivery_task_summary(pane: &DeliveryDetailPane<'_>) -> String {
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
            "This delivery's task is no longer readable here.".to_string(),
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

/// The classified reason the last attempt ended, and the provider's own words about it.
fn delivery_last_error(entry: &DeliveryEntry) -> String {
    let Some(class) = entry.last_error_class else {
        return String::new();
    };
    let detail = entry.last_error_detail.as_deref().unwrap_or("");

    format!(
        r##"<div class="alert alert-error text-xs">
                    <span><span class="badge badge-sm badge-error">{class}</span> <span class="font-mono">{detail}</span></span>
                </div>"##,
        class = escape_html_text(class.as_str()),
        detail = escape_html_text(detail),
    )
}

/// One badge style per delivery status, shared with the Tasks pane so a delivery reads the same
/// in both workspaces.
pub(crate) fn delivery_status_style(status: DeliveryStatus) -> &'static str {
    match status {
        DeliveryStatus::Pending => "badge-warning",
        DeliveryStatus::Retryable => "badge-warning",
        DeliveryStatus::Sending => "badge-info animate-pulse",
        DeliveryStatus::Delivered => "badge-success",
        DeliveryStatus::OutcomeUnknown | DeliveryStatus::DeadLetter => "badge-error",
    }
}

fn part_status_style(status: DeliveryPartStatus) -> &'static str {
    match status {
        DeliveryPartStatus::Prepared | DeliveryPartStatus::Retryable => "badge-warning",
        DeliveryPartStatus::Sending => "badge-info animate-pulse",
        DeliveryPartStatus::Delivered => "badge-success",
        DeliveryPartStatus::OutcomeUnknown | DeliveryPartStatus::Dead => "badge-error",
    }
}

fn part_status_label(status: DeliveryPartStatus) -> &'static str {
    match status {
        DeliveryPartStatus::Prepared => "Frozen",
        DeliveryPartStatus::Sending => "Sending",
        DeliveryPartStatus::Delivered => "Delivered",
        DeliveryPartStatus::OutcomeUnknown => "Unconfirmed",
        DeliveryPartStatus::Retryable => "Retrying",
        DeliveryPartStatus::Dead => "Dead",
    }
}
