//! Channel list and editor.

use super::*;

pub(crate) fn render_spam_disabled_warning(
    spam_scan_enabled: bool,
    initial_disabled: bool,
) -> String {
    if spam_scan_enabled {
        String::new()
    } else {
        let (box_class, checkbox_attr) = if initial_disabled {
            ("opacity-40 pointer-events-none grayscale", "disabled")
        } else {
            ("", "")
        };
        format!(
            r#"
        <div class="spam-disabled-box p-3 bg-warning/10 border border-warning/40 rounded-lg text-base-content text-xs space-y-2 transition-all duration-200 {box_class}">
            <div class="font-semibold flex items-center gap-1.5 text-base-content">
                <svg class="w-4 h-4 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"/>
                </svg>
                Spam scanning is disabled in server configuration
            </div>
            <div>Channels without participant email restrictions will receive incoming emails without spam filtering.</div>
            <label class="flex items-center gap-2 cursor-pointer mt-1 font-medium text-base-content">
                <input type="checkbox" name="confirm_spam_disabled" value="true" {checkbox_attr} class="rounded bg-base-200 border-base-300 text-base-content focus:ring-warning">
                <span>I am aware that spam scanning is disabled and confirm saving without participant restrictions.</span>
            </label>
        </div>
        "#
        )
    }
}

/// The create-channel form: explicit slug, participants and per-channel settings. It makes a
/// plain channel; what answers it is a workflow binding (p08 D4.7).
fn advanced_channel_form(
    company_id: Uuid,
    slug: &crate::entities::value_objects::CompanySlug,
    app_domain_name: &str,
    spam_warning_html: &str,
) -> String {
    format!(
        r##"            <form id="advanced-channel-form" hx-post="/companies/{company_id}/channels" hx-target="#channel-list" hx-swap="innerHTML" hx-disabled-elt="find button[type='submit']" class="space-y-4" data-company-id="{company_id}"
                data-after-request="reset-and-collapse" data-card="channel-form-card" data-toggle="channel-form-toggle">
                <input type="hidden" name="enabled" value="true">
                <input type="hidden" name="add_3rd_party" value="true">
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div>
                        <label for="channel_name" class="block text-xs font-medium text-base-content/80 mb-1">Channel Name</label>
                        <input type="text" id="channel_name" name="name" required
                            data-input="slugify" data-slug-target="channel_slug"
                            class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm placeholder-base-content/60 focus:outline-none focus:ring-2 focus:ring-success"
                            placeholder="Inbound Email Handler">
                    </div>
                    <div>
                        <label for="channel_slug" class="block text-xs font-medium text-base-content/80 mb-1">Slug (@{slug}.{app_domain_name})</label>
                        <input type="text" id="channel_slug" name="slug" required
                            class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm placeholder-base-content/60 focus:outline-none focus:ring-2 focus:ring-success font-mono"
                            placeholder="inbound-email-handler">
                    </div>
                </div>

                <div>
                    <label for="channel_alias_slugs" class="block text-xs font-medium text-base-content/80 mb-1">Alias Slugs (Optional)</label>
                    <input type="text" id="channel_alias_slugs" name="alias_slugs"
                        class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm placeholder-base-content/60 focus:outline-none focus:ring-2 focus:ring-success font-mono"
                        placeholder="sales, help, contact">
                    <p class="text-[11px] text-base-content/70 mt-1">Comma-separated extra addresses at <code class="text-primary">@{slug}.{app_domain_name}</code>. Replies go back out from the address the mail arrived on.</p>
                </div>

                <div>
                    <label for="participant_emails" class="block text-xs font-medium text-base-content/80 mb-1">Participant Emails (Optional - Defaults to Company Team)</label>
                    <input type="text" id="participant_emails" name="participant_emails" data-company-id="{company_id}" data-input="spam-warning" autocomplete="off"
                        class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm placeholder-base-content/60 focus:outline-none focus:ring-2 focus:ring-success"
                        placeholder="Leave blank for Company Team, @public for open access, or comma-separated emails">
                    <p class="text-[11px] text-base-content/70 mt-1">Leave blank for Company Team members. Use <code class="text-primary">@public</code> to allow anyone, or specify email addresses.</p>
                </div>
                {spam_warning_html}
                <div class="flex justify-end">
                    <button type="submit"
                        class="px-5 py-2 bg-success hover:bg-success/90 text-success-content text-sm font-semibold rounded-lg shadow-md shadow-success/20 transition cursor-pointer">
                        Create Channel
                    </button>
                </div>
            </form>"##
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
    pub spam_scan_enabled: bool,
    pub focus: ChannelsPageFocus,
}

pub fn channels_page(page: &ChannelsPage<'_>) -> String {
    let ChannelsPage {
        company,
        app_domain_name,
        spam_scan_enabled,
        focus,
        ..
    } = *page;

    let list_html = focused_channel_list(page);
    let spam_warning_html = render_spam_disabled_warning(spam_scan_enabled, true);
    let advanced_form = advanced_channel_form(
        company.id,
        &company.slug,
        app_domain_name,
        &spam_warning_html,
    );
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
                <h2 class="text-2xl font-bold text-base-content">{company_name} Channels</h2>
                <p class="text-base-content/70 text-sm mt-0.5">Manage channels for <span class="font-mono text-primary">@{slug}.{app_domain_name}</span></p>
            </div>
            <button id="channel-form-toggle" type="button" aria-controls="channel-form-card" aria-expanded="{card_expanded}"
                data-action="toggle-form-card" data-card="channel-form-card"
                class="px-4 py-2 bg-success hover:bg-success/90 text-success-content text-sm font-semibold rounded-lg shadow-md shadow-success/20 transition cursor-pointer">
                Add Channel
            </button>
        </div>

        <div id="response-message" class="mb-6"></div>

        <!-- Create Channel Card -->
        <div id="channel-form-card" class="{card_hidden} bg-base-100 border border-base-300 rounded-xl p-5 mb-8">
            <div class="flex items-center justify-between mb-4 border-b border-base-300 pb-3">
                <h3 class="text-md font-semibold text-base-content flex items-center gap-2">
                    <span class="text-base-content">+</span> Add New Channel
                </h3>
            </div>

{advanced_form}
        </div>


        <!-- Channels List Section -->
        <div>
            <h3 class="text-sm font-semibold uppercase tracking-wider text-base-content/70 mb-3">Channels</h3>
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

    base_layout(&format!("{} Channels", company.name), &content)
}

/// The channel list as the page wants it: the same rows as everywhere else, except the channel
/// named by `?edit=` which is rendered already open in its edit form.
fn focused_channel_list(page: &ChannelsPage<'_>) -> String {
    let editing = page
        .focus
        .editing_channel_id
        .filter(|id| page.channels.iter().any(|channel| channel.id == *id));
    let Some(editing_id) = editing else {
        return channel_list_fragment(page.company, page.app_domain_name, page.channels);
    };

    page.channels
        .iter()
        .map(|channel| {
            if channel.id == editing_id {
                channel_edit_fragment(
                    page.company,
                    page.app_domain_name,
                    channel,
                    page.spam_scan_enabled,
                )
            } else {
                channel_row_fragment(page.company, page.app_domain_name, channel)
            }
        })
        .collect()
}

pub fn channel_list_fragment(
    company: &Company,
    app_domain_name: &str,
    channels: &[Channel],
) -> String {
    if channels.is_empty() {
        return r##"
            <div class="bg-base-100 border border-dashed border-base-300 rounded-xl p-8 text-center">
                <p class="text-base-content/70 text-sm">No channels configured yet. Use Add Channel to create your first one.</p>
            </div>
        "##
        .to_string();
    }

    channels
        .iter()
        .map(|wf| channel_row_fragment(company, app_domain_name, wf))
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
            <a href="/companies/{company_id}/channels" class="text-xs text-primary hover:text-primary font-medium mb-1 inline-block">&larr; Back to Channels</a>
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
            "Opened by an agent in another channel",
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
            <p class="text-base-content/70 text-sm">No threads have been created for this channel yet.</p>
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

pub fn channel_row_fragment(company: &Company, app_domain_name: &str, channel: &Channel) -> String {
    let created_at_str = super::format_date(channel.created_at);
    let emails_str = match &channel.participant_emails {
        Some(emails) if !emails.is_empty() => emails.join(", "),
        _ => "None".to_string(),
    };
    let display_slug = format!("{}@{}.{}", channel.slug, company.slug, app_domain_name);
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
                    <button hx-delete="/companies/{company_id}/channels/{channel_id}" hx-target="#channel-{channel_id}" hx-swap="outerHTML" hx-confirm="Are you sure you want to delete channel '{name}'?"
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
        created_at_str = created_at_str,
        emails_str = escape_html_text(&emails_str),
    )
}

pub fn channel_edit_fragment(
    company: &Company,
    app_domain_name: &str,
    channel: &Channel,
    spam_scan_enabled: bool,
) -> String {
    let emails_str = match &channel.participant_emails {
        Some(emails) => emails.join(", "),
        None => String::new(),
    };
    let alias_slugs_str = super::channel_settings::stored_alias_slugs(channel);
    let is_public = channel
        .participant_emails
        .as_ref()
        .map(|emails| {
            emails
                .iter()
                .any(|e| e.trim().eq_ignore_ascii_case("@public"))
        })
        .unwrap_or(false);
    let spam_warning_html = render_spam_disabled_warning(spam_scan_enabled, !is_public);

    format!(
        r##"
        <form id="channel-{channel_id}" hx-put="/companies/{company_id}/channels/{channel_id}" hx-target="#channel-{channel_id}" hx-swap="outerHTML" data-company-id="{company_id}"
            class="bg-base-100 border border-success/40 rounded-xl p-4 md:p-5 space-y-4 shadow-lg">
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                    <label class="block text-xs font-medium text-base-content/80 mb-1">Channel Name</label>
                    <input type="text" name="name" value="{name}" required data-input="slugify"
                        class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm focus:outline-none focus:ring-2 focus:ring-success">
                </div>
                <div>
                    <label class="block text-xs font-medium text-base-content/80 mb-1">Slug (@{company_slug}.{app_domain_name})</label>
                    <input type="text" name="slug" value="{slug}" required
                        class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm focus:outline-none focus:ring-2 focus:ring-success font-mono">
                </div>
            </div>

            <div>
                <label class="block text-xs font-medium text-base-content/80 mb-1">Alias Slugs (Optional)</label>
                <input type="text" name="alias_slugs" value="{alias_slugs_str}"
                    class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm focus:outline-none focus:ring-2 focus:ring-success font-mono"
                    placeholder="sales, help, contact">
                <p class="text-[11px] text-base-content/70 mt-1">Comma-separated extra addresses at <code class="text-primary">@{company_slug}.{app_domain_name}</code>. Replies go back out from the address the mail arrived on.</p>
            </div>

            <div>
                <label class="block text-xs font-medium text-base-content/80 mb-1">Participant Emails (Optional - Defaults to Company Team)</label>
                <input type="text" name="participant_emails" value="{emails_str}" data-company-id="{company_id}" data-input="spam-warning" autocomplete="off"
                    class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm focus:outline-none focus:ring-2 focus:ring-success"
                    placeholder="Leave blank for Company Team, @public for open access, or comma-separated emails">
                <p class="text-[11px] text-base-content/70 mt-1">Leave blank for Company Team members. Use <code class="text-primary">@public</code> to allow anyone, or specify email addresses.</p>
            </div>
            <div>
                <label class="flex items-start gap-2.5 cursor-pointer">
                    <input type="checkbox" name="enabled" value="true" {enabled_checked}
                        class="mt-0.5 rounded bg-base-200 border-base-300 text-base-content focus:ring-success">
                    <span class="text-xs text-base-content/80">
                        <span class="font-medium">Channel enabled</span>
                        <span class="block text-[11px] text-base-content/70">Unticking this keeps the channel and its threads, but mail to its address bounces back to the sender.</span>
                    </span>
                </label>
            </div>
            <div>
                <label class="flex items-start gap-2.5 cursor-pointer">
                    <input type="checkbox" name="add_3rd_party" value="true" {add_3rd_party_checked}
                        class="mt-0.5 rounded bg-base-200 border-base-300 text-base-content focus:ring-success">
                    <span class="text-xs text-base-content/80">
                        <span class="font-medium">Add CC'd outsiders to threads</span>
                        <span class="block text-[11px] text-base-content/70">Unticking this keeps the channel internal: people outside your team are never added to a thread and are never copied on the agent's reply.</span>
                    </span>
                </label>
            </div>
            {spam_warning_html}
            <div class="flex items-center justify-end gap-2">
                <button type="button" hx-get="/companies/{company_id}/channels/{channel_id}/cancel" hx-target="#channel-{channel_id}" hx-swap="outerHTML"
                    class="px-3 py-1.5 text-xs font-medium bg-base-300 hover:bg-base-content/15 text-base-content rounded-lg transition cursor-pointer">
                    Cancel
                </button>
                <button type="submit"
                    class="px-4 py-1.5 text-xs font-semibold bg-success hover:bg-success/90 text-success-content rounded-lg transition cursor-pointer">
                    Save Changes
                </button>
            </div>
        </form>
        "##,
        channel_id = channel.id,
        company_id = company.id,
        name = escape_html_attr(&channel.name),
        slug = escape_html_attr(&channel.slug),
        enabled_checked = if channel.enabled { "checked" } else { "" },
        add_3rd_party_checked = if channel.add_3rd_party { "checked" } else { "" },
        company_slug = escape_html_text(&company.slug),
        app_domain_name = escape_html_text(app_domain_name),
        emails_str = escape_html_attr(&emails_str),
    )
}
