//! Channel list and editor.

use super::*;
use crate::entities::department::Department;

/// Both page shells submit the same required-owner/Public contract.
fn advanced_channel_form(page: &ChannelsPage<'_>) -> String {
    let draft = super::channel_settings::ChannelDraft::default();
    let fields = super::channel_settings::channel_fields(&super::channel_settings::ChannelFields {
        company: page.company,
        app_domain_name: page.app_domain_name,
        draft: &draft,
        spam_scan_enabled: page.spam_scan_enabled,
        departments: page.departments,
    });
    format!(
        r##"<form id="advanced-channel-form" hx-post="/companies/{company_id}/channels" hx-target="#channel-list" hx-swap="innerHTML" hx-disabled-elt="find button[type='submit']" class="space-y-4" data-company-id="{company_id}"
                data-after-request="reset-and-collapse" data-card="channel-form-card" data-toggle="channel-form-toggle">
            {fields}
            <div class="flex justify-end">
                <button type="submit" class="btn btn-success">Create Desk</button>
            </div>
        </form>"##,
        company_id = page.company.id,
    )
}

/// What the channels page opens with.
///
/// The mailbox's "New Channel" / "Edit Channel" buttons link into these states instead of
/// dropping the user on a page where the form they asked for is still collapsed.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelsPageFocus {
    pub create_form_open: bool,
    pub editing_channel_id: Option<Uuid>,
}

/// Everything the full channels page renders from.
pub struct ChannelsPage<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    pub channels: &'a [Channel],
    /// The company's departments: the list groups desks under them and badges each desk.
    pub departments: &'a [Department],
    pub spam_scan_enabled: bool,
    pub focus: ChannelsPageFocus,
}

pub fn channels_page(page: &ChannelsPage<'_>) -> String {
    let ChannelsPage {
        company,
        app_domain_name,
        focus,
        ..
    } = *page;

    let list_html = focused_channel_list(page);
    let advanced_form = advanced_channel_form(page);
    let (card_hidden, card_expanded) = if focus.create_form_open {
        ("", "true")
    } else {
        ("hidden", "false")
    };
    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <a href="/companies" class="text-xs text-primary hover:text-primary font-medium mb-1 inline-block">&larr; Back to Companies</a>
                <h2 class="text-2xl font-bold text-base-content">{company_name} Desks</h2>
                <p class="text-base-content/70 text-sm mt-0.5">Manage desks for <span class="font-mono text-primary">@{slug}.{app_domain_name}</span></p>
            </div>
            <button id="channel-form-toggle" type="button" aria-controls="channel-form-card" aria-expanded="{card_expanded}"
                data-action="toggle-form-card" data-card="channel-form-card"
                class="px-4 py-2 bg-success hover:bg-success/90 text-success-content text-sm font-semibold rounded-lg shadow-md shadow-success/20 transition cursor-pointer">
                Add Desk
            </button>
        </div>

        <div id="response-message" class="mb-6"></div>

        <!-- Create Channel Card -->
        <div id="channel-form-card" class="{card_hidden} bg-base-100 border border-base-300 rounded-xl p-5 mb-8">
            <div class="flex items-center justify-between mb-4 border-b border-base-300 pb-3">
                <h3 class="text-md font-semibold text-base-content flex items-center gap-2">
                    <span class="text-base-content">+</span> Add New Desk
                </h3>
            </div>

{advanced_form}
        </div>


        <!-- Channels List Section -->
        <div>
            <h3 class="text-sm font-semibold uppercase tracking-wider text-base-content/70 mb-3">Desks</h3>
            <div id="channel-list" class="space-y-3">
                {list_html}
            </div>
        </div>
        "##,
        company_name = escape_html_text(&company.name),
        slug = escape_html_text(&company.slug),
        app_domain_name = escape_html_text(app_domain_name),
        list_html = list_html,
    );

    base_layout(&format!("{} Desks", company.name), &content)
}

/// The channel list as the page wants it: the same rows as everywhere else, except the channel
/// named by `?edit=` which is rendered already open in its edit form.
fn focused_channel_list(page: &ChannelsPage<'_>) -> String {
    let editing = page
        .focus
        .editing_channel_id
        .filter(|id| page.channels.iter().any(|channel| channel.id == *id));
    let Some(editing_id) = editing else {
        return channel_list_fragment(
            page.company,
            page.app_domain_name,
            page.channels,
            page.departments,
        );
    };

    page.channels
        .iter()
        .map(|channel| {
            if channel.id == editing_id {
                channel_edit_fragment(
                    page.company,
                    page.app_domain_name,
                    channel,
                    page.departments,
                    page.spam_scan_enabled,
                )
            } else {
                channel_row_fragment(
                    page.company,
                    page.app_domain_name,
                    channel,
                    department_name(page.departments, channel).as_deref(),
                )
            }
        })
        .collect()
}

/// The name of the department that owns `channel`, when it is one of `departments`.
/// The desk's department as its row names it: an archived one says so (Plan08 02E.2).
pub fn department_name(departments: &[Department], channel: &Channel) -> Option<String> {
    let owner = channel.department_id;
    departments
        .iter()
        .find(|department| department.id == owner)
        .map(Department::label)
}

pub fn channel_list_fragment(
    company: &Company,
    app_domain_name: &str,
    channels: &[Channel],
    departments: &[Department],
) -> String {
    if channels.is_empty() {
        return r##"
            <div class="bg-base-100 border border-dashed border-base-300 rounded-xl p-8 text-center">
                <p class="text-base-content/70 text-sm">No desks configured yet. Use Add Desk to create your first one.</p>
            </div>
        "##
        .to_string();
    }

    super::grouped_by_department(channels, departments)
        .into_iter()
        .map(|(heading, desks)| {
            let rows: String = desks
                .into_iter()
                .map(|channel| {
                    channel_row_fragment(
                        company,
                        app_domain_name,
                        channel,
                        department_name(departments, channel).as_deref(),
                    )
                })
                .collect();
            match heading {
                Some(heading) => format!(
                    r#"<h4 class="desk-group pt-2 text-xs font-semibold uppercase tracking-wider text-base-content/70">{}</h4>{rows}"#,
                    escape_html_text(&heading)
                ),
                None => rows,
            }
        })
        .collect()
}

pub fn channel_threads_page(
    company: &Company,
    channel: &Channel,
    app_domain_name: &str,
    threads: &[Thread],
    next_cursor: Option<&str>,
) -> String {
    let list_html = channel_thread_list_fragment(
        company.id,
        channel.id,
        app_domain_name,
        threads,
        next_cursor,
        false,
    );
    let content = format!(
        r##"
        <div class="mb-6">
            <a href="/companies/{company_id}/channels" class="text-xs text-primary hover:text-primary font-medium mb-1 inline-block">&larr; Back to Desks</a>
            <h2 class="text-2xl font-bold text-base-content">{channel_name} Threads</h2>
            <p class="text-base-content/70 text-sm mt-0.5">Newest conversations for <span class="font-mono text-base-content">/{channel_slug}</span></p>
        </div>
        <div id="thread-list" class="space-y-3">
            {list_html}
        </div>
        "##,
        company_id = company.id,
        channel_name = escape_html_text(&channel.name),
        channel_slug = escape_html_text(&channel.slug),
        list_html = list_html,
    );

    base_layout(&format!("{} Threads", channel.name), &content)
}

/// One thread card in the admin channel list.
///
/// Extracted from the list body so the glyph has somewhere to live that is not another level of
/// closure inside an already long function.
fn channel_thread_card(
    company_id: Uuid,
    channel_id: Uuid,
    app_domain_name: &str,
    thread: &Thread,
) -> String {
    let participant_addresses = thread
        .participant_projection
        .subjects_for(TransportKind::Email);
    let participants = if participant_addresses.is_empty() {
        "No participants".to_string()
    } else {
        escape_html_text(&participant_addresses.join(", "))
    };

    format!(
        r##"
                    <article class="bg-base-100 border border-base-300 rounded-xl p-4 md:p-5 hover:border-primary/40 transition shadow-sm">
                        <div class="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-3">
                            <div class="min-w-0">
                                <h3 class="flex items-center gap-1.5 font-semibold text-base-content break-words">{channel_glyph}{subject}</h3>
                                <p class="text-xs text-base-content/70 mt-1 break-words">{participants}</p>
                                <p class="text-[11px] font-mono text-base-content/70 mt-2">{thread_id}</p>
                            </div>
                            <div class="sm:text-right shrink-0">
                                <p class="text-xs text-base-content/70">Updated {updated_at}</p>
                                <a href="/companies/{company_id}/channels/{channel_id}/simulate?thread_id={thread_id}"
                                    class="inline-block mt-2 px-3 py-1.5 text-xs font-medium bg-primary/10 hover:bg-primary/20 text-primary border border-primary/40 rounded-lg transition">
                                    Open Thread
                                </a>
                            </div>
                        </div>
                    </article>
                    "##,
        channel_glyph = other_channel_glyph(
            opened_by_another_channel(thread, app_domain_name),
            "Opened by an agent in another desk",
        ),
        subject = escape_html_text(&thread.subject),
        participants = participants,
        thread_id = thread.id,
        updated_at = super::format_date_time(thread.updated_at),
        company_id = company_id,
        channel_id = channel_id,
    )
}

pub fn channel_thread_list_fragment(
    company_id: Uuid,
    channel_id: Uuid,
    app_domain_name: &str,
    threads: &[Thread],
    next_cursor: Option<&str>,
    out_of_band_pagination: bool,
) -> String {
    let cards = if threads.is_empty() && !out_of_band_pagination {
        r##"
        <div class="bg-base-100 border border-dashed border-base-300 rounded-xl p-8 text-center">
            <p class="text-base-content/70 text-sm">No threads have been created for this desk yet.</p>
        </div>
        "##
        .to_string()
    } else {
        threads
            .iter()
            .map(|thread| channel_thread_card(company_id, channel_id, app_domain_name, thread))
            .collect()
    };

    let oob = if out_of_band_pagination {
        " hx-swap-oob=\"outerHTML\""
    } else {
        ""
    };
    let pagination = match next_cursor {
        Some(cursor) => format!(
            r##"
            <div id="thread-pagination" class="pt-3 text-center"{oob}>
                <button hx-get="/companies/{company_id}/channels/{channel_id}/threads/list?cursor={cursor}"
                    hx-target="#thread-list" hx-swap="beforeend" hx-disabled-elt="this"
                    class="px-4 py-2 text-sm font-medium bg-base-200 hover:bg-base-300 disabled:opacity-60 text-base-content border border-base-300 rounded-lg transition cursor-pointer">
                    Load older threads
                </button>
            </div>
            "##,
            company_id = company_id,
            channel_id = channel_id,
            cursor = escape_html_attr(cursor),
            oob = oob,
        ),
        None => format!(r##"<div id="thread-pagination"{oob}></div>"##),
    };

    format!("{cards}{pagination}")
}

pub fn channel_row_fragment(
    company: &Company,
    app_domain_name: &str,
    channel: &Channel,
    department: Option<&str>,
) -> String {
    let created_at_str = super::format_date(channel.created_at);
    let emails_str = match &channel.participant_emails {
        Some(emails) if !emails.is_empty() => emails.join(", "),
        _ => "None".to_string(),
    };
    let display_slug = channel.inbound_address(&company.slug, app_domain_name);
    let disabled_badge = if channel.enabled {
        ""
    } else {
        r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-medium bg-base-200 text-base-content/70 border border-base-content/30">Disabled</span>"#
    };

    format!(
        r##"
        <div id="channel-{channel_id}" class="bg-base-100 border border-base-300 rounded-xl p-4 md:p-5 flex flex-col gap-3 hover:border-base-content/30 transition shadow-sm">
            <div class="flex items-center justify-between">
                <div>
                    <div class="flex items-center gap-3">
                        <h4 class="text-md font-semibold text-base-content">{name}</h4>
                        <span class="px-2.5 py-0.5 rounded-full text-xs font-mono bg-success/10 text-base-content border border-success/40">{display_slug}</span>
                        {department_badge}
                        {disabled_badge}
                    </div>
                    <p class="text-xs text-base-content/70 mt-1">Created on {created_at_str}</p>
                </div>
                <div class="flex flex-wrap items-center justify-end gap-2">
                    <a href="/companies/{company_id}/channels/{channel_id}/threads"
                        class="px-3 py-1.5 text-xs font-medium bg-success/10 hover:bg-success/20 text-base-content border border-success/40 rounded-lg transition">
                        Threads
                    </a>
                    <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                        class="px-3 py-1.5 text-xs font-medium bg-primary/10 hover:bg-primary/20 text-primary border border-primary/40 rounded-lg transition">
                        New Thread
                    </a>
                    <button hx-get="/companies/{company_id}/channels/{channel_id}/edit" hx-target="#channel-{channel_id}" hx-swap="outerHTML"
                        class="px-3 py-1.5 text-xs font-medium bg-base-300 hover:bg-base-content/15 text-base-content rounded-lg transition cursor-pointer">
                        Edit
                    </button>
                    <button hx-delete="/companies/{company_id}/channels/{channel_id}" hx-target="#channel-{channel_id}" hx-swap="outerHTML" hx-confirm="Are you sure you want to delete desk '{name}'?"
                        class="px-3 py-1.5 text-xs font-medium bg-error/10 hover:bg-error/20 text-base-content border border-error/40 rounded-lg transition cursor-pointer">
                        Delete
                    </button>
                </div>
            </div>
            <div class="text-xs bg-base-300 p-3 rounded-lg border border-base-300 font-mono">
                <span class="text-base-content/70 block font-sans text-[11px] font-semibold uppercase">Participants:</span>
                <span class="text-base-content/80">{emails_str}</span>
            </div>
        </div>
        "##,
        company_id = company.id,
        channel_id = channel.id,
        name = escape_html_attr(&channel.name),
        display_slug = escape_html_text(&display_slug),
        department_badge = department
            .map(|name| format!(
                r#"<span class="desk-department badge badge-outline badge-sm">{}</span>"#,
                escape_html_text(name)
            ))
            .unwrap_or_default(),
        created_at_str = created_at_str,
        emails_str = escape_html_text(&emails_str),
    )
}

pub fn channel_edit_fragment(
    company: &Company,
    app_domain_name: &str,
    channel: &Channel,
    departments: &[Department],
    spam_scan_enabled: bool,
) -> String {
    let participants = super::channel_settings::stored_participants(channel);
    let aliases = super::channel_settings::stored_alias_slugs(channel);
    let draft = super::channel_settings::stored_channel_draft(channel, &participants, &aliases);
    let fields = super::channel_settings::channel_fields(&super::channel_settings::ChannelFields {
        company,
        app_domain_name,
        draft: &draft,
        spam_scan_enabled,
        departments,
    });
    format!(
        r##"<form id="channel-{channel_id}" hx-put="/companies/{company_id}/channels/{channel_id}" hx-target="#channel-{channel_id}" hx-swap="outerHTML" hx-disabled-elt="find button[type='submit']" data-company-id="{company_id}"
            class="bg-base-100 border border-success/40 rounded-xl p-4 md:p-5 space-y-4 shadow-lg">
            {fields}
            <div class="flex items-center justify-end gap-2">
                <button type="button" hx-get="/companies/{company_id}/channels/{channel_id}/cancel" hx-target="#channel-{channel_id}" hx-swap="outerHTML" class="btn btn-sm">Cancel</button>
                <button type="submit" class="btn btn-sm btn-success">Save Changes</button>
            </div>
        </form>"##,
        channel_id = channel.id,
        company_id = company.id,
    )
}
