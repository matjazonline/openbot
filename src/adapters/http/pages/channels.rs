//! Channel list, editor, and the agent-selection controls they share.

use super::*;

pub fn render_agents_selection(
    company_id: Uuid,
    agents: &[Agent],
    selected_ids: Option<&[Uuid]>,
    container_id: &str,
) -> String {
    render_agents_selection_full(company_id, agents, selected_ids, container_id, None)
}

/// One selectable agent card. Picking it writes the agent id into the hidden `agent_ids` input
/// that the surrounding channel form actually submits.
fn agent_radio_card(
    group_name: &str,
    value: &str,
    checked: bool,
    title_class: &str,
    title: &str,
    subtitle_class: &str,
    subtitle: &str,
) -> String {
    let checked = if checked { "checked" } else { "" };
    format!(
        r#"
        <label class="flex items-center gap-2 p-2 bg-slate-800/80 border border-slate-700/80 rounded-lg cursor-pointer hover:bg-slate-700/60 transition">
            <input type="radio" name="{group_name}" value="{value}" {checked}
                data-action="pick-agent-radio"
                class="border-slate-700 text-indigo-600 focus:ring-indigo-500">
            <div class="text-xs flex flex-col">
                <span class="font-medium {title_class}">{title}</span>
                <span class="{subtitle_class} font-mono text-[10px]">{subtitle}</span>
            </div>
        </label>
        "#
    )
}

pub fn render_agents_selection_full(
    company_id: Uuid,
    agents: &[Agent],
    selected_ids: Option<&[Uuid]>,
    container_id: &str,
    error_msg: Option<&str>,
) -> String {
    let initial_id = match selected_ids {
        Some(ids) if !ids.is_empty() => ids[0].to_string(),
        _ => String::new(),
    };
    let agents_selection_id = format!("agents-selection-{container_id}");
    // Unique per render so several of these on one page don't share a radio group.
    let group_name = format!("agent_radio_{}_{}", container_id, Uuid::new_v4().simple());

    let mut agent_cards = agent_radio_card(
        &group_name,
        "",
        initial_id.is_empty(),
        "text-slate-300",
        "None",
        "text-slate-500",
        "Use channel fallback / custom agent",
    );
    agent_cards.push_str(r#"<div class="sm:col-span-2 md:col-span-3 mt-2 text-[10px] font-bold uppercase tracking-wide text-slate-500">Agent library</div>"#);
    let library_options = agents
        .iter()
        .filter(|agent| agent.is_library())
        .map(|agent| {
            format!(
                r#"<option value="{id}"{selected}>{name} (@{slug})</option>"#,
                id = agent.id,
                selected = if selected_ids.is_some_and(|ids| ids.contains(&agent.id)) {
                    " selected"
                } else {
                    ""
                },
                name = escape_html_text(&agent.name),
                slug = escape_html_text(&agent.slug),
            )
        })
        .collect::<String>();
    if library_options.is_empty() {
        agent_cards
            .push_str(r#"<p class="text-xs text-slate-500">No library agents available.</p>"#);
    } else {
        agent_cards.push_str(&format!(r#"<select class="library-agent-select sm:col-span-2 md:col-span-3 bg-slate-800 border border-slate-700 rounded-lg p-2 text-sm" data-action="pick-agent-library"><option value="">Choose a library agent…</option>{library_options}</select>"#));
    }
    agent_cards.push_str(r#"<div class="sm:col-span-2 md:col-span-3 mt-2 text-[10px] font-bold uppercase tracking-wide text-slate-500">Custom company agents</div>"#);
    for agent in agents.iter().filter(|agent| !agent.is_library()) {
        agent_cards.push_str(&agent_radio_card(
            &group_name,
            &agent.id.to_string(),
            selected_ids.is_some_and(|ids| ids.contains(&agent.id)),
            "text-white",
            &agent.name,
            "text-slate-400",
            &format!("@{}", agent.slug),
        ));
    }
    if !agents.iter().any(|agent| !agent.is_library()) {
        agent_cards
            .push_str(r#"<p class="text-xs text-slate-500">No custom company agents yet.</p>"#);
    }

    let error_html = error_msg.map_or_else(String::new, |message| {
        format!(
            r#"<div class="alert alert-error text-xs">{}</div>"#,
            escape_html_text(message)
        )
    });

    format!(
        r#"
        <div id="{agents_selection_id}" data-agents-selection class="space-y-3">
            <input type="hidden" name="agent_ids" value="{initial_id}">
            <div class="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-2 mt-1">
                {agent_cards}
            </div>

            <div>
                <a href="/ui/agents?company_id={company_id}&new=1"
                    class="text-xs text-emerald-400 hover:text-emerald-300 font-medium cursor-pointer inline-flex items-center gap-1">
                    <span>Manage agents in the Agents workspace</span>
                </a>
            </div>
            {error_html}
        </div>
        "#
    )
}

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
        <div class="spam-disabled-box p-3 bg-amber-500/10 border border-amber-500/30 rounded-lg text-amber-300 text-xs space-y-2 transition-all duration-200 {box_class}">
            <div class="font-semibold flex items-center gap-1.5 text-amber-400">
                <svg class="w-4 h-4 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"/>
                </svg>
                Spam scanning is disabled in server configuration
            </div>
            <div>Channels without participant email restrictions will receive incoming emails without spam filtering.</div>
            <label class="flex items-center gap-2 cursor-pointer mt-1 font-medium text-amber-200">
                <input type="checkbox" name="confirm_spam_disabled" value="true" {checkbox_attr} class="rounded bg-slate-800 border-slate-700 text-amber-500 focus:ring-amber-500">
                <span>I am aware that spam scanning is disabled and confirm saving without participant restrictions.</span>
            </label>
        </div>
        "#
        )
    }
}

/// "Simple" create-channel form: a name and plain-language instructions, from which the
/// server generates the agent and its system prompt.
fn simple_channel_form(company_id: Uuid) -> String {
    format!(
        r##"            <form id="simple-channel-form" hx-post="/companies/{company_id}/channels" hx-target="#channel-list" hx-swap="innerHTML" hx-disabled-elt="find button[type='submit']" class="space-y-4" data-company-id="{company_id}"
                data-after-request="reset-and-collapse" data-card="channel-form-card" data-toggle="channel-form-toggle">
                <input type="hidden" name="form_mode" value="simple">
                <input type="hidden" name="enabled" value="true">
                <input type="hidden" name="add_3rd_party" value="true">
                <div>
                    <label for="simple_channel_name" class="block text-xs font-medium text-slate-300 mb-1">Channel Name</label>
                    <input type="text" id="simple_channel_name" name="name" required
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500"
                        placeholder="Inbound Email Handler">
                </div>
                <div>
                    <label for="simple_system_prompt" class="block text-xs font-medium text-slate-300 mb-1">Agent Instructions</label>
                    <textarea id="simple_system_prompt" name="system_prompt" rows="4" required
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono text-xs"
                        placeholder="Describe the agent's role, responsibilities, rules, and tone. A complete system prompt will be generated when you create the channel."></textarea>
                </div>
                <div class="flex justify-end">
                    <button type="submit"
                        class="px-5 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-emerald-600/30 transition cursor-pointer flex items-center gap-2 [.htmx-request_&]:pointer-events-none [.htmx-request_&]:opacity-80">
                        <svg class="animate-spin h-4 w-4 text-white hidden [.htmx-request_&]:inline-block shrink-0" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" aria-hidden="true">
                            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                        </svg>
                        <span class="[.htmx-request_&]:hidden">Create Channel</span>
                        <span class="hidden [.htmx-request_&]:inline">Creating...</span>
                    </button>
                </div>
            </form>"##
    )
}

/// "Advanced" create-channel form: explicit slug, agent selection, participants and per-channel
/// LLM overrides.
fn advanced_channel_form(
    company_id: Uuid,
    slug: &crate::entities::value_objects::CompanySlug,
    app_domain_name: &str,
    agents_selection_html: &str,
    spam_warning_html: &str,
    memory_fields_html: &str,
) -> String {
    format!(
        r##"            <form id="advanced-channel-form" hx-post="/companies/{company_id}/channels" hx-target="#channel-list" hx-swap="innerHTML" class="hidden space-y-4" data-company-id="{company_id}"
                data-after-request="reset-and-collapse" data-card="channel-form-card" data-toggle="channel-form-toggle">
                <input type="hidden" name="form_mode" value="advanced">
                <input type="hidden" name="enabled" value="true">
                <input type="hidden" name="add_3rd_party" value="true">
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div>
                        <label for="channel_name" class="block text-xs font-medium text-slate-300 mb-1">Channel Name</label>
                        <input type="text" id="channel_name" name="name" required
                            data-input="slugify" data-slug-target="channel_slug"
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500"
                            placeholder="Inbound Email Handler">
                    </div>
                    <div>
                        <label for="channel_slug" class="block text-xs font-medium text-slate-300 mb-1">Slug (@{slug}.{app_domain_name})</label>
                        <input type="text" id="channel_slug" name="slug" required
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                            placeholder="inbound-email-handler">
                    </div>
                </div>

                <div>
                    <label for="channel_alias_slugs" class="block text-xs font-medium text-slate-300 mb-1">Alias Slugs (Optional)</label>
                    <input type="text" id="channel_alias_slugs" name="alias_slugs"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                        placeholder="sales, help, contact">
                    <p class="text-[11px] text-slate-400 mt-1">Comma-separated extra addresses at <code class="text-indigo-300">@{slug}.{app_domain_name}</code>. Replies go back out from the address the mail arrived on.</p>
                </div>

                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Select Agent</label>
                    {agents_selection_html}
                </div>

                <div>
                    <label for="participant_emails" class="block text-xs font-medium text-slate-300 mb-1">Participant Emails (Optional - Defaults to Company Team)</label>
                    <input type="text" id="participant_emails" name="participant_emails" data-company-id="{company_id}" data-input="spam-warning" autocomplete="off"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500"
                        placeholder="Leave blank for Company Team, @public for open access, or comma-separated emails">
                    <p class="text-[11px] text-slate-400 mt-1">Leave blank for Company Team members. Use <code class="text-indigo-300">@public</code> to allow anyone, or specify email addresses.</p>
                </div>
                {memory_fields_html}
                {spam_warning_html}
                <div class="flex justify-end">
                    <button type="submit"
                        class="px-5 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-emerald-600/30 transition cursor-pointer">
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
    pub agents: &'a [Agent],
    pub spam_scan_enabled: bool,
    pub focus: ChannelsPageFocus,
}

pub fn channels_page(page: &ChannelsPage<'_>) -> String {
    let ChannelsPage {
        company,
        app_domain_name,
        agents,
        spam_scan_enabled,
        focus,
        ..
    } = *page;

    let list_html = focused_channel_list(page);
    let agents_selection_html = render_agents_selection(company.id, agents, None, "new");
    let spam_warning_html = render_spam_disabled_warning(spam_scan_enabled, true);
    let memory_fields_html = classic_memory_fields(company.memory_provider.is_some(), None);
    let simple_form = simple_channel_form(company.id);
    let advanced_form = advanced_channel_form(
        company.id,
        &company.slug,
        app_domain_name,
        &agents_selection_html,
        &spam_warning_html,
        &memory_fields_html,
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
                <a href="/companies" class="text-xs text-indigo-400 hover:text-indigo-300 font-medium mb-1 inline-block">&larr; Back to Companies</a>
                <h2 class="text-2xl font-bold text-white">{company_name} Channels</h2>
                <p class="text-slate-400 text-sm mt-0.5">Manage channels for <span class="font-mono text-indigo-300">@{slug}.{app_domain_name}</span></p>
            </div>
            <button id="channel-form-toggle" type="button" aria-controls="channel-form-card" aria-expanded="{card_expanded}"
                data-action="toggle-form-card" data-card="channel-form-card"
                class="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-emerald-600/30 transition cursor-pointer">
                Add Channel
            </button>
        </div>

        <div id="response-message" class="mb-6"></div>

        <!-- Create Channel Card -->
        <div id="channel-form-card" class="{card_hidden} bg-slate-900/70 border border-slate-700/80 rounded-xl p-5 mb-8">
            <div class="flex items-center justify-between mb-4 border-b border-slate-800 pb-3">
                <h3 class="text-md font-semibold text-white flex items-center gap-2">
                    <span class="text-emerald-400">+</span> Add New Channel
                </h3>
                <div class="flex items-center bg-slate-800/80 p-1 rounded-lg border border-slate-700/50 text-xs font-medium">
                    <button type="button" id="tab-simple-btn" data-action="show-channel-form-tab" data-tab="simple"
                        class="px-3 py-1 rounded-md text-white bg-indigo-600 font-semibold transition cursor-pointer">
                        Simple
                    </button>
                    <button type="button" id="tab-advanced-btn" data-action="show-channel-form-tab" data-tab="advanced"
                        class="px-3 py-1 rounded-md text-slate-400 hover:text-white transition cursor-pointer">
                        Advanced
                    </button>
                </div>
            </div>

            <!-- Simple Create Channel Form (Default) -->
{simple_form}

            <!-- Advanced Create Channel Form (Hidden by default) -->
{advanced_form}
        </div>


        <!-- Channels List Section -->
        <div>
            <h3 class="text-sm font-semibold uppercase tracking-wider text-slate-400 mb-3">Channels</h3>
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
        return channel_list_fragment(
            page.company,
            page.app_domain_name,
            page.channels,
            page.agents,
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
                    page.agents,
                    page.spam_scan_enabled,
                )
            } else {
                channel_row_fragment(page.company, page.app_domain_name, channel, page.agents)
            }
        })
        .collect()
}

pub fn channel_list_fragment(
    company: &Company,
    app_domain_name: &str,
    channels: &[Channel],
    agents: &[Agent],
) -> String {
    if channels.is_empty() {
        return r##"
            <div class="bg-slate-900/40 border border-dashed border-slate-700/80 rounded-xl p-8 text-center">
                <p class="text-slate-400 text-sm">No channels configured yet. Use Add Channel to create your first one.</p>
            </div>
        "##
        .to_string();
    }

    channels
        .iter()
        .map(|wf| channel_row_fragment(company, app_domain_name, wf, agents))
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
            <a href="/companies/{company_id}/channels" class="text-xs text-indigo-400 hover:text-indigo-300 font-medium mb-1 inline-block">&larr; Back to Channels</a>
            <h2 class="text-2xl font-bold text-white">{channel_name} Threads</h2>
            <p class="text-slate-400 text-sm mt-0.5">Newest conversations for <span class="font-mono text-emerald-300">/{channel_slug}</span></p>
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
    let participants = if thread.participant_emails.is_empty() {
        "No participants".to_string()
    } else {
        escape_html_text(&thread.participant_emails.join(", "))
    };

    format!(
        r##"
                    <article class="bg-slate-900/80 border border-slate-700/70 rounded-xl p-4 md:p-5 hover:border-indigo-700/70 transition shadow-sm">
                        <div class="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-3">
                            <div class="min-w-0">
                                <h3 class="flex items-center gap-1.5 font-semibold text-white break-words">{channel_glyph}{subject}</h3>
                                <p class="text-xs text-slate-400 mt-1 break-words">{participants}</p>
                                <p class="text-[11px] font-mono text-slate-500 mt-2">{thread_id}</p>
                            </div>
                            <div class="sm:text-right shrink-0">
                                <p class="text-xs text-slate-400">Updated {updated_at}</p>
                                <a href="/companies/{company_id}/channels/{channel_id}/simulate?thread_id={thread_id}"
                                    class="inline-block mt-2 px-3 py-1.5 text-xs font-medium bg-indigo-900/80 hover:bg-indigo-800 text-indigo-200 border border-indigo-700/50 rounded-lg transition">
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
        <div class="bg-slate-900/40 border border-dashed border-slate-700/80 rounded-xl p-8 text-center">
            <p class="text-slate-400 text-sm">No threads have been created for this channel yet.</p>
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
                    class="px-4 py-2 text-sm font-medium bg-slate-800 hover:bg-slate-700 disabled:opacity-60 text-slate-200 border border-slate-700 rounded-lg transition cursor-pointer">
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
    agents: &[Agent],
) -> String {
    let created_at_str = super::format_date(channel.created_at);
    let emails_str = match &channel.participant_emails {
        Some(emails) if !emails.is_empty() => emails.join(", "),
        _ => "None".to_string(),
    };
    let assigned_agents_str = match &channel.agent_ids {
        Some(ids) if !ids.is_empty() => {
            let matches: Vec<String> = agents
                .iter()
                .filter(|a| ids.contains(&a.id))
                .map(|a| format!("{} (@{})", a.name, a.slug))
                .collect();
            if matches.is_empty() {
                format!("{} agent(s)", ids.len())
            } else {
                matches.join(", ")
            }
        }
        _ => "None".to_string(),
    };
    let config_str = "Configured on active agent";
    let provider_str = "Configured on active agent";
    let model_str = "Configured on active agent";
    let api_key_str = "Company connection";
    let display_slug = format!("{}@{}.{}", channel.slug, company.slug, app_domain_name);
    let disabled_badge = if channel.enabled {
        ""
    } else {
        r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-medium bg-slate-800 text-slate-400 border border-slate-600">Disabled</span>"#
    };
    let delete_action = if channel.owner_agent_id.is_some() {
        String::new()
    } else {
        format!(
            r##"<button hx-delete="/companies/{company_id}/channels/{channel_id}" hx-target="#channel-{channel_id}" hx-swap="outerHTML" hx-confirm="Are you sure you want to delete channel '{name}'?"
                        class="px-3 py-1.5 text-xs font-medium bg-rose-950/80 hover:bg-rose-900/90 text-rose-300 border border-rose-800/50 rounded-lg transition cursor-pointer">
                        Delete
                    </button>"##,
            company_id = company.id,
            channel_id = channel.id,
            name = escape_html_attr(&channel.name),
        )
    };

    format!(
        r##"
        <div id="channel-{channel_id}" class="bg-slate-900/80 border border-slate-700/70 rounded-xl p-4 md:p-5 flex flex-col gap-3 hover:border-slate-600 transition shadow-sm">
            <div class="flex items-center justify-between">
                <div>
                    <div class="flex items-center gap-3">
                        <h4 class="text-md font-semibold text-white">{name}</h4>
                        <span class="px-2.5 py-0.5 rounded-full text-xs font-mono bg-emerald-950/90 text-emerald-300 border border-emerald-700/50">{display_slug}</span>
                        {disabled_badge}
                    </div>
                    <p class="text-xs text-slate-400 mt-1">Created on {created_at_str}</p>
                </div>
                <div class="flex flex-wrap items-center justify-end gap-2">
                    <a href="/companies/{company_id}/channels/{channel_id}/threads"
                        class="px-3 py-1.5 text-xs font-medium bg-emerald-900/80 hover:bg-emerald-800 text-emerald-200 border border-emerald-700/50 rounded-lg transition">
                        Threads
                    </a>
                    <a href="/companies/{company_id}/tasks?channel_id={channel_id}"
                        class="px-3 py-1.5 text-xs font-medium bg-amber-900/80 hover:bg-amber-800 text-amber-200 border border-amber-700/50 rounded-lg transition">
                        Task Executions
                    </a>
                    <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                        class="px-3 py-1.5 text-xs font-medium bg-indigo-900/80 hover:bg-indigo-800 text-indigo-200 border border-indigo-700/50 rounded-lg transition">
                        New Thread
                    </a>
                    <button hx-get="/companies/{company_id}/channels/{channel_id}/edit" hx-target="#channel-{channel_id}" hx-swap="outerHTML"
                        class="px-3 py-1.5 text-xs font-medium bg-slate-700 hover:bg-slate-600 text-slate-200 rounded-lg transition cursor-pointer">
                        Edit
                    </button>
                    {delete_action}
                </div>
            </div>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-2 text-xs bg-slate-950/60 p-3 rounded-lg border border-slate-800 font-mono">
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">Provider:</span>
                    <span class="text-slate-300">{provider_str}</span>
                </div>
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">Model:</span>
                    <span class="text-slate-300">{model_str}</span>
                </div>
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">API Key:</span>
                    <span class="text-slate-300">{api_key_str}</span>
                </div>
            </div>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-2 text-xs bg-slate-950/60 p-3 rounded-lg border border-slate-800 font-mono">
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">Assigned Agents:</span>
                    <span class="text-indigo-300">{assigned_agents_str}</span>
                </div>
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">Participants:</span>
                    <span class="text-slate-300">{emails_str}</span>
                </div>
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">Config:</span>
                    <pre class="text-slate-300 whitespace-pre-wrap text-[11px]">{config_str}</pre>
                </div>
            </div>
        </div>
        "##,
        company_id = company.id,
        channel_id = channel.id,
        name = escape_html_attr(&channel.name),
        display_slug = escape_html_text(&display_slug),
        created_at_str = created_at_str,
        provider_str = escape_html_text(provider_str),
        model_str = escape_html_text(model_str),
        api_key_str = api_key_str,
        assigned_agents_str = escape_html_text(&assigned_agents_str),
        emails_str = escape_html_text(&emails_str),
        config_str = escape_html_text(config_str),
        delete_action = delete_action,
    )
}

pub fn channel_edit_fragment(
    company: &Company,
    app_domain_name: &str,
    channel: &Channel,
    agents: &[Agent],
    spam_scan_enabled: bool,
) -> String {
    let emails_str = match &channel.participant_emails {
        Some(emails) => emails.join(", "),
        None => String::new(),
    };
    let alias_slugs_str = super::channel_settings::stored_alias_slugs(channel);
    let owner = channel
        .owner_agent_id
        .and_then(|owner_id| agents.iter().find(|agent| agent.id == owner_id));
    let agents_selection_html = owner.map_or_else(
        || {
            render_agents_selection(
                company.id,
                agents,
                channel.agent_ids.as_deref(),
                &channel.id.to_string(),
            )
        },
        |owner| {
            format!(
                r#"<input type="hidden" name="agent_ids" value="{owner_id}"><p class="rounded-lg border border-slate-700 bg-slate-800 p-3 text-xs"><span class="font-medium">{owner_name}</span> <span class="font-mono text-slate-400">@{owner_slug}</span><span class="mt-1 block text-slate-400">The owner remains the position-0 agent while this channel is enabled or disabled.</span></p>"#,
                owner_id = owner.id,
                owner_name = escape_html_text(&owner.name),
                owner_slug = escape_html_text(&owner.slug),
            )
        },
    );
    let slug_readonly = if owner.is_some() { " readonly" } else { "" };
    let name_slug_behavior = if owner.is_some() {
        ""
    } else {
        r#" data-input="slugify""#
    };
    let slug_help = owner.map_or_else(String::new, |owner| {
        format!(
            r#"<p class="mt-1 text-[11px] text-slate-400">The primary address follows the agent handle. <a class="text-indigo-300" href="/ui/agents?company_id={}&amp;agent_id={}">Rename {} in Agents</a>.</p>"#,
            company.id,
            owner.id,
            escape_html_text(&owner.name),
        )
    });
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
    let memory_fields_html =
        classic_memory_fields(company.memory_provider.is_some(), Some(channel));

    format!(
        r##"
        <form id="channel-{channel_id}" hx-put="/companies/{company_id}/channels/{channel_id}" hx-target="#channel-{channel_id}" hx-swap="outerHTML" data-company-id="{company_id}"
            class="bg-slate-900 border border-emerald-500/60 rounded-xl p-4 md:p-5 space-y-4 shadow-lg">
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Channel Name</label>
                    <input type="text" name="name" value="{name}" required{name_slug_behavior}
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500">
                </div>
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Slug (@{company_slug}.{app_domain_name})</label>
                    <input type="text" name="slug" value="{slug}" required{slug_readonly}
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono">
                    {slug_help}
                </div>
            </div>

            <div>
                <label class="block text-xs font-medium text-slate-300 mb-1">Alias Slugs (Optional)</label>
                <input type="text" name="alias_slugs" value="{alias_slugs_str}"
                    class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                    placeholder="sales, help, contact">
                <p class="text-[11px] text-slate-400 mt-1">Comma-separated extra addresses at <code class="text-indigo-300">@{company_slug}.{app_domain_name}</code>. Replies go back out from the address the mail arrived on.</p>
            </div>

            <div>
                <label class="block text-xs font-medium text-slate-300 mb-1">Select Agents (Multiple allowed)</label>
                {agents_selection_html}
            </div>

            <div>
                <label class="block text-xs font-medium text-slate-300 mb-1">Participant Emails (Optional - Defaults to Company Team)</label>
                <input type="text" name="participant_emails" value="{emails_str}" data-company-id="{company_id}" data-input="spam-warning" autocomplete="off"
                    class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500"
                    placeholder="Leave blank for Company Team, @public for open access, or comma-separated emails">
                <p class="text-[11px] text-slate-400 mt-1">Leave blank for Company Team members. Use <code class="text-indigo-300">@public</code> to allow anyone, or specify email addresses.</p>
            </div>
            {memory_fields_html}
            <div>
                <label class="flex items-start gap-2.5 cursor-pointer">
                    <input type="checkbox" name="enabled" value="true" {enabled_checked}
                        class="mt-0.5 rounded bg-slate-800 border-slate-700 text-emerald-500 focus:ring-emerald-500">
                    <span class="text-xs text-slate-300">
                        <span class="font-medium">Channel enabled</span>
                        <span class="block text-[11px] text-slate-400">Unticking this keeps the channel and its threads, but mail to its address bounces back to the sender.</span>
                    </span>
                </label>
            </div>
            <div>
                <label class="flex items-start gap-2.5 cursor-pointer">
                    <input type="checkbox" name="add_3rd_party" value="true" {add_3rd_party_checked}
                        class="mt-0.5 rounded bg-slate-800 border-slate-700 text-emerald-500 focus:ring-emerald-500">
                    <span class="text-xs text-slate-300">
                        <span class="font-medium">Add CC'd outsiders to threads</span>
                        <span class="block text-[11px] text-slate-400">Unticking this keeps the channel internal: people outside your team are never added to a thread and are never copied on the agent's reply.</span>
                    </span>
                </label>
            </div>
            {spam_warning_html}
            <div class="flex items-center justify-end gap-2">
                <button type="button" hx-get="/companies/{company_id}/channels/{channel_id}/cancel" hx-target="#channel-{channel_id}" hx-swap="outerHTML"
                    class="px-3 py-1.5 text-xs font-medium bg-slate-700 hover:bg-slate-600 text-slate-200 rounded-lg transition cursor-pointer">
                    Cancel
                </button>
                <button type="submit"
                    class="px-4 py-1.5 text-xs font-semibold bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg transition cursor-pointer">
                    Save Changes
                </button>
            </div>
        </form>
        "##,
        channel_id = channel.id,
        company_id = company.id,
        name = escape_html_attr(&channel.name),
        name_slug_behavior = name_slug_behavior,
        slug = escape_html_attr(&channel.slug),
        slug_readonly = slug_readonly,
        slug_help = slug_help,
        enabled_checked = if channel.enabled { "checked" } else { "" },
        add_3rd_party_checked = if channel.add_3rd_party { "checked" } else { "" },
        company_slug = escape_html_text(&company.slug),
        app_domain_name = escape_html_text(app_domain_name),
        emails_str = escape_html_attr(&emails_str),
        agents_selection_html = agents_selection_html,
    )
}

fn classic_memory_fields(memory_available: bool, channel: Option<&Channel>) -> String {
    let state_notice = if memory_available {
        "The selected provider is ready; grants are effective when agent policy also permits memory."
    } else {
        "Grants can be saved now and remain inactive until the selected company provider is ready."
    };
    let checked = |enabled: bool| if enabled { " checked" } else { "" };
    let retrieve_company = checked(channel.is_some_and(|c| c.retrieve_company_memory));
    let retrieve_agent = checked(channel.is_some_and(|c| c.retrieve_agent_memory));
    let retrieve_user = checked(channel.is_some_and(|c| c.retrieve_user_memory));
    let persist_company = checked(channel.is_some_and(|c| c.persist_company_memory));
    let persist_agent = checked(channel.is_some_and(|c| c.persist_agent_memory));
    let persist_user = checked(channel.is_some_and(|c| c.persist_user_memory));
    format!(
        r##"<fieldset class="rounded-lg border border-slate-700 bg-slate-800/50 p-4">
                <legend class="px-1 text-xs font-semibold text-slate-300">Memory</legend>
                <p class="mb-3 text-[11px] text-slate-400">{state_notice} User memory includes authorized external senders and remains isolated to this company.</p>
                <div class="grid max-w-sm grid-cols-4 items-center gap-x-2 gap-y-2 text-xs text-slate-300">
                    <span></span>
                    <span class="text-center">Company</span>
                    <span class="text-center">Agent</span>
                    <span class="text-center">User</span>
                    <span>Retrieve</span>
                    <input aria-label="Retrieve company memory" type="checkbox" name="retrieve_company_memory" value="true" class="justify-self-center"{retrieve_company}>
                    <input aria-label="Retrieve agent memory" type="checkbox" name="retrieve_agent_memory" value="true" class="justify-self-center"{retrieve_agent}>
                    <input aria-label="Retrieve user memory" type="checkbox" name="retrieve_user_memory" value="true" class="justify-self-center"{retrieve_user}>
                    <span>Persist</span>
                    <input aria-label="Persist company memory" type="checkbox" name="persist_company_memory" value="true" class="justify-self-center"{persist_company}>
                    <input aria-label="Persist agent memory" type="checkbox" name="persist_agent_memory" value="true" class="justify-self-center"{persist_agent}>
                    <input aria-label="Persist user memory" type="checkbox" name="persist_user_memory" value="true" class="justify-self-center"{persist_user}>
                </div>
            </fieldset>"##,
    )
}
