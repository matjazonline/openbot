//! The `/ui` Channels workspace: the same shell as the mailbox, with the channel list configured
//! rather than read.
//!
//! The sidebar picks a channel and its settings are swapped into `#channel-pane` over htmx, the
//! way picking a thread swaps the mailbox's detail pane. Every write re-renders the pane and
//! sends the sidebar list along out of band, so a rename or a delete shows up immediately.

use super::*;
use crate::entities::{
    channel::PUBLIC_PARTICIPANT,
    schedule::{ChannelSchedule, ScheduleDeliveryMode},
};

/// Client-side behaviour this workspace adds on top of [`MAILBOX_SCRIPT`].
///
/// Kept out of the `format!` blocks below so its braces need no escaping.
pub(crate) const CHANNEL_SETTINGS_SCRIPT: &str = r##"        function showChannelTab(mode) {
            var easy = document.getElementById('channel-tab-easy');
            var simple = document.getElementById('channel-tab-simple');
            var advancedForm = document.getElementById('channel-tab-advanced');
            var easyBtn = document.getElementById('channel-tab-easy-btn');
            var simpleBtn = document.getElementById('channel-tab-simple-btn');
            var advancedBtn = document.getElementById('channel-tab-advanced-btn');
            if (easy) easy.classList.toggle('hidden', mode !== 'easy');
            if (simple) simple.classList.toggle('hidden', mode !== 'simple');
            if (advancedForm) advancedForm.classList.toggle('hidden', mode !== 'advanced');
            if (easyBtn) easyBtn.classList.toggle('tab-active', mode === 'easy');
            if (simpleBtn) simpleBtn.classList.toggle('tab-active', mode === 'simple');
            if (advancedBtn) advancedBtn.classList.toggle('tab-active', mode === 'advanced');
        }

        // The spam interlock only applies to a channel anyone can write to, so the confirmation
        // stays inert until the participants field actually names @public.
        function toggleChannelSpamConfirm(input) {
            var form = input.closest('form');
            if (!form) return;
            var box = form.querySelector('.spam-confirm-box');
            if (!box) return;
            var checkbox = box.querySelector('input[type="checkbox"]');
            var isPublic = input.value.toLowerCase().includes('@public');
            box.classList.toggle('opacity-40', !isPublic);
            box.classList.toggle('pointer-events-none', !isPublic);
            if (checkbox) {
                checkbox.disabled = !isPublic;
                if (!isPublic) checkbox.checked = false;
            }
        }

        // A channel can run several agents in order, but a urlencoded form keeps only the last
        // value of a repeated name — so the options feed one hidden comma-separated field.
        //
        // Library definitions are picked from a modal of cards and land in a hidden field; custom
        // company definitions remain compact radios. Both feed the same field the server reads.
        function syncChannelAgents(el) {
            var form = el.closest('form');
            if (!form) return;
            var fromLibrary = el.classList.contains('channel-library-agent-field');
            var options = form.querySelectorAll('.channel-agent-option');
            Array.prototype.forEach.call(options, function (option) {
                if (fromLibrary || option !== el) option.checked = false;
            });
            if (!fromLibrary) setChannelLibraryAgent(form, '', '');
            var target = form.querySelector('input[name="agent_ids"]');
            if (!target) return;
            var checked = form.querySelector('.channel-agent-option:checked');
            target.value = fromLibrary ? el.value : (checked ? checked.value : '');
        }

        // The library choice lives in a hidden field, so the button that opens the modal and the
        // card inside it are what has to be kept in step with the value.
        function setChannelLibraryAgent(form, id, name) {
            var field = form.querySelector('.channel-library-agent-field');
            if (!field) return;
            field.value = id;
            var label = form.querySelector('.channel-library-agent-label');
            if (label) label.textContent = name || label.dataset.placeholder;
            var cards = form.querySelectorAll('.channel-library-agent-card');
            Array.prototype.forEach.call(cards, function (card) {
                card.classList.toggle('border-primary', !!id && card.dataset.agentId === id);
            });
        }

        // Picking a card is the whole modal: it fills the hidden field, clears any radio through
        // the shared sync, and closes.
        function pickChannelLibraryAgent(card) {
            var form = card.closest('form');
            if (!form) return;
            setChannelLibraryAgent(form, card.dataset.agentId, card.dataset.agentName);
            var field = form.querySelector('.channel-library-agent-field');
            if (field) syncChannelAgents(field);
            var dialog = card.closest('dialog');
            if (dialog) dialog.close();
        }"##;

/// The channel list in the sidebar — the only part of the workspace a write has to refresh.
pub struct ChannelSettingsList<'a> {
    pub company: &'a Company,
    /// Domain the channel addresses are built on, e.g. `mailagents.com`.
    pub app_domain_name: &'a str,
    pub channels: &'a [Channel],
    pub selected_channel_id: Option<Uuid>,
}

/// The Channels workspace for one request.
pub struct ChannelSettingsPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub companies: &'a [Company],
    pub list: &'a ChannelSettingsList<'a>,
    /// Pre-rendered right-hand pane: a channel's settings, the create form, or a placeholder.
    pub pane_html: &'a str,
}

/// What a channel form was last submitted with, so a rejected submit comes back filled in.
///
/// The Advanced create form and the edit form take exactly these fields, which is why they share
/// one renderer; only the URL they submit to differs.
#[derive(Debug)]
pub struct ChannelDraft<'a> {
    pub name: &'a str,
    /// What the channel is for, in one line. Shown to teammates whose mail bounced.
    pub description: &'a str,
    pub slug: &'a str,
    /// Extra addresses the channel answers on, as the comma-separated list the form submits.
    pub alias_slugs: &'a str,
    /// Simple mode only: the instructions the server expands into an agent.
    pub system_prompt: &'a str,
    pub participant_emails: &'a str,
    pub agent_ids: &'a [Uuid],
    pub provider: &'a str,
    pub model: &'a str,
    pub api_key: &'a str,
    pub channel_config: &'a str,
    /// Whether the create pane should open on the Advanced tab.
    pub advanced: bool,
    /// Whether the channel takes traffic. A new channel starts enabled, which is why this type
    /// hand-writes `Default` instead of deriving it.
    pub enabled: bool,
    /// Whether CC'd outsiders may join this channel's threads. Starts on, like `enabled`.
    pub add_3rd_party: bool,
    pub retrieve_company_memory: bool,
    pub retrieve_agent_memory: bool,
    pub retrieve_user_memory: bool,
    pub persist_company_memory: bool,
    pub persist_agent_memory: bool,
    pub persist_user_memory: bool,
    pub memory_persistence_mode: &'a str,
    pub memory_recall_mode: &'a str,
    pub memory_max_results: String,
}

impl Default for ChannelDraft<'_> {
    fn default() -> Self {
        Self {
            name: "",
            description: "",
            slug: "",
            alias_slugs: "",
            system_prompt: "",
            participant_emails: "",
            agent_ids: &[],
            provider: "",
            model: "",
            api_key: "",
            channel_config: "",
            advanced: false,
            enabled: true,
            add_3rd_party: true,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            memory_persistence_mode: "audience_only",
            memory_recall_mode: "fast",
            memory_max_results: "5".into(),
        }
    }
}

impl ChannelDraft<'_> {
    /// Whether the participants field currently names `@public` — the only case the spam
    /// interlock applies to.
    ///
    /// Deliberately the same test `toggleChannelSpamConfirm` makes, so the state the server
    /// renders and the state typing produces cannot disagree.
    fn is_public(&self) -> bool {
        self.participant_emails
            .to_lowercase()
            .contains(PUBLIC_PARTICIPANT)
    }
}

/// The settings pane for a channel that already exists.
pub struct ChannelEditPane<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    pub channel: &'a Channel,
    pub agents: &'a [Agent],
    pub schedules: &'a [ChannelSchedule],
    pub spam_scan_enabled: bool,
    /// What the user last typed, when a save was rejected; `None` shows the stored channel.
    pub draft: Option<&'a ChannelDraft<'a>>,
    pub error: Option<&'a str>,
}

/// The pane for a channel that does not exist yet.
pub struct ChannelCreatePane<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    pub agents: &'a [Agent],
    pub spam_scan_enabled: bool,
    pub draft: &'a ChannelDraft<'a>,
    /// Easy is activated explicitly after an Easy submission is rejected.
    pub easy: bool,
    pub error: Option<&'a str>,
}

pub fn channel_settings_page(page: &ChannelSettingsPage<'_>) -> String {
    let company = page.list.company;
    let content = format!(
        r##"
        <aside class="ui-pane-list flex w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {header}
            {list_html}
            <div class="border-t border-base-300 p-2">
                <button type="button" class="btn btn-primary btn-sm btn-block justify-start"
                    hx-get="/ui/channels/new?company_id={company_id}"
                    hx-target="#channel-pane" hx-swap="outerHTML" hx-sync="#channel-pane:replace"
                    hx-push-url="/ui/channels?company_id={company_id}&new=1">{plus_glyph} New Channel</button>
            </div>
        </aside>
        {pane_html}
        "##,
        header = sidebar_header("Channels", "Inbound addresses and their routing rules."),
        plus_glyph = icon(Icon::Plus, BUTTON_ICON),
        list_html = channel_settings_list(page.list, FragmentSwap::Inline),
        company_id = company.id,
        pane_html = page.pane_html,
    );

    ui_shell(&UiShell {
        title: &format!("{} Channels", company.name),
        user: page.user,
        company: Some(company),
        section: UiSection::Channels,
        content: &content,
    })
}

/// One entry per channel, keyed `#channel-menu` so the mailbox's selection highlighting applies
/// here unchanged.
///
/// Rendered out of band after a write, so a created, renamed or deleted channel shows up without
/// the pane having to reload the whole workspace.
pub fn channel_settings_list(list: &ChannelSettingsList<'_>, swap: FragmentSwap) -> String {
    let entries: String = list
        .channels
        .iter()
        .map(|channel| {
            channel_settings_entry(
                list.company.id,
                channel,
                &channel.inbound_address(&list.company.slug, list.app_domain_name),
                list.selected_channel_id == Some(channel.id),
            )
        })
        .collect();

    let menu_body = if list.channels.is_empty() {
        r##"<li class="px-2 py-6 text-center text-xs opacity-60">No channels yet. Create your first one below.</li>"##
            .to_string()
    } else {
        entries
    };

    format!(
        r##"<ul id="channel-menu" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2"{oob}>{menu_body}</ul>"##,
        oob = swap.oob_attribute(),
    )
}

fn channel_settings_entry(
    company_id: Uuid,
    channel: &Channel,
    address: &EmailAddress,
    selected: bool,
) -> String {
    format!(
        r##"
                <li>
                    <a class="flex flex-col items-start gap-0.5 {active}"
                        hx-get="/ui/channels/{channel_id}?company_id={company_id}"
                        hx-target="#channel-pane" hx-swap="outerHTML"
                        hx-sync="#channel-pane:replace"
                        hx-push-url="/ui/channels?company_id={company_id}&channel_id={channel_id}"
                        data-action="select-sidebar-item">
                        <span class="flex w-full min-w-0 items-center gap-2">
                            <span class="min-w-0 truncate">{name}</span>{disabled_badge}
                        </span>
                        <span class="w-full truncate font-mono text-[11px] opacity-60">{address}</span>
                    </a>
                </li>
        "##,
        active = if selected { "menu-active" } else { "" },
        channel_id = channel.id,
        name = escape_html_text(&channel.name),
        disabled_badge = disabled_badge(channel),
        address = escape_html_text(address),
    )
}

/// The "off" marker on a channel, so the state is visible without opening its settings.
pub fn disabled_badge(channel: &Channel) -> &'static str {
    if channel.enabled {
        ""
    } else {
        r#"<span class="badge badge-ghost badge-sm shrink-0 opacity-70">Disabled</span>"#
    }
}

/// The pane before a channel is picked.
pub fn channel_settings_empty_pane(message: &str, swap: FragmentSwap) -> String {
    format!(
        r##"
        <section id="channel-pane"{PANE_SKELETON} data-pane-empty class="ui-pane-detail flex min-w-0 flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <p class="text-center text-sm opacity-60">{message}</p>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub fn channel_edit_pane(pane: &ChannelEditPane<'_>) -> String {
    channel_edit_pane_with_memory(pane, pane.company.memory_provider.is_some())
}

pub fn channel_edit_pane_with_memory(pane: &ChannelEditPane<'_>, memory_ready: bool) -> String {
    let participants = stored_participants(pane.channel);
    let aliases = stored_alias_slugs(pane.channel);
    let config = stored_config(pane.channel);
    let stored = stored_draft(pane.channel, &participants, &aliases, &config);
    let draft = pane.draft.unwrap_or(&stored);
    let company_id = pane.company.id;
    let channel_id = pane.channel.id;

    format!(
        r##"
        <section id="channel-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="flex flex-wrap items-start justify-between gap-3 border-b border-base-300 px-4 py-4 sm:px-6">
                <div class="min-w-0 grow basis-48">
                    <h2 class="truncate text-xl font-bold">{name}</h2>
                    <p class="truncate font-mono text-xs opacity-60">{address}</p>
                    <p class="truncate text-xs opacity-50">{creator}</p>
                </div>
                <div class="flex shrink-0 flex-wrap items-center gap-2">
                    <a href="/ui?company_id={company_id}&channel_id={channel_id}" class="btn btn-ghost btn-sm">Open Mailbox</a>
                    <a href="/companies/{company_id}/channels/{channel_id}/simulate" class="btn btn-outline btn-sm">Simulator</a>
                </div>
            </div>
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6 space-y-6">
                {error_html}
                <form hx-put="/ui/channels/{channel_id}?company_id={company_id}" hx-target="#channel-pane" hx-swap="outerHTML" class="space-y-4">
                    <input type="hidden" name="form_mode" value="advanced">
                    {fields}
                    <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Save Changes</span>
                            <span class="hidden [.htmx-request_&]:inline">Saving...</span>
                        </button>
                        <button type="button" class="btn btn-ghost"
                            hx-get="/ui/channels/close?company_id={company_id}"
                            hx-target="#channel-pane" hx-swap="outerHTML" hx-sync="#channel-pane:replace"
                            hx-push-url="/ui/channels?company_id={company_id}">Cancel</button>
                        <button type="button" class="btn btn-error btn-outline ml-auto"
                            hx-delete="/ui/channels/{channel_id}?company_id={company_id}"
                            hx-target="#channel-pane" hx-swap="outerHTML"
                            hx-confirm="Delete channel '{name}'? Its threads and tasks go with it."
                            hx-push-url="/ui/channels?company_id={company_id}">Delete Channel</button>
                    </div>
                </form>

                {schedules_html}
            </div>
        </section>
        "##,
        name = escape_html_text(&pane.channel.name),
        address = escape_html_text(&all_addresses(
            pane.channel,
            pane.company,
            pane.app_domain_name
        )),
        creator = escape_html_text(&pane.channel.created_by.label()),
        error_html = form_error_banner(pane.error),
        fields = channel_fields(&ChannelFields {
            company: pane.company,
            app_domain_name: pane.app_domain_name,
            agents: pane.agents,
            id_prefix: &channel_id.to_string(),
            draft,
            spam_scan_enabled: pane.spam_scan_enabled,
            memory_ready,
        }),
        schedules_html = channel_schedules_card(company_id, channel_id, pane.schedules),
    )
}

pub fn channel_create_pane(pane: &ChannelCreatePane<'_>) -> String {
    channel_create_pane_with_memory(pane, pane.company.memory_provider.is_some())
}

pub fn channel_create_pane_with_memory(pane: &ChannelCreatePane<'_>, memory_ready: bool) -> String {
    let (easy_hidden, simple_hidden, advanced_hidden) = if pane.easy {
        ("", "hidden", "hidden")
    } else if pane.draft.advanced {
        ("hidden", "hidden", "")
    } else {
        ("hidden", "", "hidden")
    };
    let active = |lit: bool| if lit { "tab-active" } else { "" };

    format!(
        r##"
        <section id="channel-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 py-4 sm:px-6">
                <h2 class="text-xl font-bold">New channel in {company_name}</h2>
                <p class="text-xs opacity-70">A channel is an inbound address at <span class="font-mono">@{company_slug}.{app_domain_name}</span> plus the agents that answer it.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
                {error_html}
                <div role="tablist" class="tabs tabs-box mb-4 w-fit">
                    <button type="button" role="tab" id="channel-tab-easy-btn" class="tab {easy_active}"
                        data-action="show-channel-tab" data-tab="easy">Easy</button>
                    <button type="button" role="tab" id="channel-tab-simple-btn" class="tab {simple_active}"
                        data-action="show-channel-tab" data-tab="simple">Simple</button>
                    <button type="button" role="tab" id="channel-tab-advanced-btn" class="tab {advanced_active}"
                        data-action="show-channel-tab" data-tab="advanced">Advanced</button>
                </div>
                <form id="channel-tab-easy" class="{easy_hidden} space-y-4"
                    hx-post="/ui/channels/easy?company_id={company_id}" hx-target="#channel-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    {library_picker}
                    <div class="flex items-center gap-3">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Create Channels</span>
                            <span class="hidden [.htmx-request_&]:inline">Creating...</span>
                        </button>
                        <button type="button" class="btn btn-ghost"
                            hx-get="/ui/channels/close?company_id={company_id}"
                            hx-target="#channel-pane" hx-swap="outerHTML" hx-sync="#channel-pane:replace"
                            hx-push-url="/ui/channels?company_id={company_id}">Cancel</button>
                    </div>
                </form>
                <form id="channel-tab-simple" class="{simple_hidden} space-y-4"
                    hx-post="/ui/channels?company_id={company_id}" hx-target="#channel-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    <input type="hidden" name="form_mode" value="simple">
                    <!-- Simple mode has no on/off controls, and an absent checkbox reads as "off". -->
                    <input type="hidden" name="enabled" value="true">
                    <input type="hidden" name="add_3rd_party" value="true">
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Channel Name</span></div>
                        <input type="text" name="name" required value="{name}" placeholder="Inbound Email Handler"
                            class="input w-full">
                    </label>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Description</span></div>
                        <input type="text" name="description" value="{description}" autocomplete="off"
                            placeholder="Answers supplier capacity and delivery-date questions."
                            class="input w-full">
                        <div class="label"><span class="text-[11px] opacity-60">One line, shown to teammates who mail an address that does not exist.</span></div>
                    </label>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Agent Instructions</span></div>
                        <textarea name="system_prompt" rows="6" required
                            placeholder="Describe the agent's role, responsibilities, rules and tone. A full system prompt is generated when the channel is created."
                            class="textarea w-full font-mono text-xs">{system_prompt}</textarea>
                    </label>
                    <div class="flex items-center gap-3">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Create Channel</span>
                            <span class="hidden [.htmx-request_&]:inline">Creating...</span>
                        </button>
                        <button type="button" class="btn btn-ghost"
                            hx-get="/ui/channels/close?company_id={company_id}"
                            hx-target="#channel-pane" hx-swap="outerHTML" hx-sync="#channel-pane:replace"
                            hx-push-url="/ui/channels?company_id={company_id}">Cancel</button>
                    </div>
                </form>
                <form id="channel-tab-advanced" class="{advanced_hidden} space-y-4"
                    hx-post="/ui/channels?company_id={company_id}" hx-target="#channel-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    <input type="hidden" name="form_mode" value="advanced">
                    {fields}
                    <div class="flex items-center gap-3">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Create Channel</span>
                            <span class="hidden [.htmx-request_&]:inline">Creating...</span>
                        </button>
                        <button type="button" class="btn btn-ghost"
                            hx-get="/ui/channels/close?company_id={company_id}"
                            hx-target="#channel-pane" hx-swap="outerHTML" hx-sync="#channel-pane:replace"
                            hx-push-url="/ui/channels?company_id={company_id}">Cancel</button>
                    </div>
                </form>
            </div>
        </section>
        "##,
        company_name = escape_html_text(&pane.company.name),
        company_slug = escape_html_text(&pane.company.slug),
        app_domain_name = escape_html_text(pane.app_domain_name),
        company_id = pane.company.id,
        error_html = form_error_banner(pane.error),
        easy_active = active(pane.easy),
        simple_active = active(!pane.easy && !pane.draft.advanced),
        advanced_active = active(!pane.easy && pane.draft.advanced),
        easy_hidden = easy_hidden,
        name = escape_html_text(pane.draft.name),
        description = escape_html_text(pane.draft.description),
        system_prompt = escape_html_text(pane.draft.system_prompt),
        library_picker =
            agent_library_multi_select(pane.agents, pane.draft.agent_ids, "library_agent_ids",),
        fields = channel_fields(&ChannelFields {
            company: pane.company,
            app_domain_name: pane.app_domain_name,
            agents: pane.agents,
            id_prefix: "new",
            draft: pane.draft,
            spam_scan_enabled: pane.spam_scan_enabled,
            memory_ready,
        }),
    )
}

/// Everything about a channel except which URL its form submits to.
struct ChannelFields<'a> {
    company: &'a Company,
    app_domain_name: &'a str,
    agents: &'a [Agent],
    /// Namespaces the element ids, so the create pane's two forms do not collide.
    id_prefix: &'a str,
    draft: &'a ChannelDraft<'a>,
    spam_scan_enabled: bool,
    memory_ready: bool,
}

fn channel_fields(fields: &ChannelFields<'_>) -> String {
    let draft = fields.draft;
    let overrides_open = if draft.provider.is_empty()
        && draft.model.is_empty()
        && draft.api_key.is_empty()
        && draft.channel_config.is_empty()
    {
        ""
    } else {
        " open"
    };
    let model_connection_fields = model_connection_fields(&ModelConnectionFields {
        agent_id_suffix: None,
        provider: draft.provider,
        model: draft.model,
        api_key: draft.api_key,
        api_key_placeholder: "Overrides the company key",
    });

    format!(
        r##"
                    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Channel Name</span></div>
                            <input type="text" name="name" required value="{name}" placeholder="Inbound Email Handler"
                                data-input="slugify"
                                class="input w-full">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Address (@{company_slug}.{app_domain_name})</span></div>
                            <input type="text" name="slug" required value="{slug}" placeholder="inbound-email-handler"
                                class="input w-full font-mono">
                        </label>
                    </div>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Description</span></div>
                        <input type="text" name="description" value="{description}" autocomplete="off"
                            placeholder="Answers supplier capacity and delivery-date questions."
                            class="input w-full">
                        <div class="label"><span class="text-[11px] opacity-60">What this channel is for, in one line. Shown to teammates who mail an address that does not exist, so they can find the channel they meant.</span></div>
                    </label>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Alias Addresses (@{company_slug}.{app_domain_name})</span></div>
                        <input type="text" name="alias_slugs" value="{alias_slugs}" autocomplete="off"
                            placeholder="sales, help, contact"
                            class="input w-full font-mono">
                        <div class="label"><span class="text-[11px] opacity-60">Comma-separated. Mail to any of these reaches this channel, and replies go back out from the address it arrived on.</span></div>
                    </label>
                    <div class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Agents</span></div>
                        {agents_html}
                    </div>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Participant Emails</span></div>
                        <input type="text" name="participant_emails" value="{participant_emails}" autocomplete="off"
                            data-input="channel-spam-confirm"
                            placeholder="Leave blank for the company team, @public for anyone, or comma-separated emails"
                            class="input w-full">
                        <div class="label"><span class="text-[11px] opacity-60">Blank means the company team. Use <code class="font-mono">@public</code> to let anyone write in.</span></div>
                    </label>
                    <details class="collapse-arrow collapse border border-base-300 bg-base-200"{overrides_open}>
                        <summary class="collapse-title text-sm font-medium">Custom model &amp; config</summary>
                        <div class="collapse-content space-y-4">
                            {model_connection_fields}
                            <label class="form-control w-full">
                                <div class="label"><span class="text-xs opacity-70">Channel Config (JSON)</span></div>
                                <textarea name="channel_config" rows="4" placeholder='{{ "trigger": "email", "action": "ai_reply" }}'
                                    class="textarea w-full font-mono text-xs">{channel_config}</textarea>
                            </label>
                        </div>
                    </details>
                    {memory_fields}
                    <div class="form-control w-full">
                        <label class="flex cursor-pointer items-start gap-3 rounded-lg border border-base-300 bg-base-200 p-3">
                            <input type="checkbox" name="enabled" value="true" class="checkbox checkbox-sm mt-0.5"{enabled_checked}>
                            <span class="text-xs">
                                <span class="font-semibold">Channel enabled</span>
                                <span class="block opacity-70">Unticking this keeps the channel and its threads, but mail sent to its address bounces back to the sender, and other channels can no longer hand work to it.</span>
                            </span>
                        </label>
                    </div>
                    <div class="form-control w-full">
                        <label class="flex cursor-pointer items-start gap-3 rounded-lg border border-base-300 bg-base-200 p-3">
                            <input type="checkbox" name="add_3rd_party" value="true" class="checkbox checkbox-sm mt-0.5"{add_3rd_party_checked}>
                            <span class="text-xs">
                                <span class="font-semibold">Add CC'd outsiders to threads</span>
                                <span class="block opacity-70">Unticking this keeps the channel internal: people outside your team who are CC'd on a message are never added to the thread and are never copied on the agent's reply, and mail they send to this channel bounces.</span>
                            </span>
                        </label>
                    </div>
                    {spam_html}
        "##,
        name = escape_html_text(draft.name),
        description = escape_html_text(draft.description),
        company_slug = escape_html_text(&fields.company.slug),
        app_domain_name = escape_html_text(fields.app_domain_name),
        slug = escape_html_text(draft.slug),
        alias_slugs = escape_html_text(draft.alias_slugs),
        agents_html = agent_radios(
            fields.company.id,
            fields.agents,
            draft.agent_ids,
            fields.id_prefix
        ),
        participant_emails = escape_html_text(draft.participant_emails),
        channel_config = escape_html_text(draft.channel_config),
        memory_fields = memory_fields(fields.memory_ready, draft),
        enabled_checked = if draft.enabled { " checked" } else { "" },
        add_3rd_party_checked = if draft.add_3rd_party { " checked" } else { "" },
        spam_html = spam_disabled_confirmation(fields.spam_scan_enabled, draft.is_public()),
    )
}

/// The company's agents as radios feeding one hidden `agent_ids` field.
///
/// Order matters — a channel runs its agents in sequence — so the hidden field is the source of
/// truth and `syncChannelAgents` keeps it in the order the options are listed. A channel stored
/// with several agents still renders them all selected; picking one collapses the field to it.
fn agent_radios(company_id: Uuid, agents: &[Agent], selected: &[Uuid], id_prefix: &str) -> String {
    if agents.is_empty() {
        return format!(
            r##"<div class="rounded-box border border-dashed border-base-300 p-4 text-center text-xs opacity-70">
                            <input type="hidden" name="agent_ids" value="">
                            This company has no agents yet. The channel falls back to its own model settings —
                            or <a href="/ui/agents?company_id={company_id}&amp;new=1" class="link">create an agent</a> first.
                        </div>"##
        );
    }

    let render_options = |group: &[&Agent]| -> String {
        group
        .iter()
        .map(|agent| {
            format!(
                r##"
                            <label class="label cursor-pointer justify-start gap-3 rounded-box px-3 py-2 hover:bg-base-300" for="agent-{id_prefix}-{agent_id}">
                                <input type="radio" id="agent-{id_prefix}-{agent_id}" value="{agent_id}" {checked}
                                    class="channel-agent-option radio radio-sm radio-primary"
                                    data-action="sync-channel-agents">
                                <span class="flex min-w-0 flex-col items-start">
                                    <span class="truncate text-sm">{name}</span>
                                    <span class="truncate font-mono text-[11px] opacity-60">@{slug}</span>
                                </span>
                            </label>
                "##,
                agent_id = agent.id,
                checked = if selected.contains(&agent.id) {
                    "checked"
                } else {
                    ""
                },
                name = escape_html_text(&agent.name),
                slug = escape_html_text(&agent.slug),
            )
        })
        .collect()
    };
    let library = agents
        .iter()
        .filter(|agent| agent.is_library())
        .collect::<Vec<_>>();
    let custom = agents
        .iter()
        .filter(|agent| !agent.is_library())
        .collect::<Vec<_>>();
    let library_options = if library.is_empty() {
        r#"<p class="px-3 py-2 text-xs opacity-60">No library agents are available.</p>"#
            .to_string()
    } else {
        library_agent_picker(&library, selected, id_prefix)
    };
    let custom_options = if custom.is_empty() {
        format!(
            r#"<p class="px-3 py-2 text-xs opacity-60">No custom agents yet. <a class="link" href="/ui/agents?company_id={company_id}&amp;new=1">Create one</a>.</p>"#
        )
    } else {
        render_options(&custom)
    };
    let options = format!(
        r#"<div class="px-3 pt-2 text-[11px] font-bold uppercase opacity-60">Agent library</div>{library_options}<div class="mt-2 px-3 pt-2 text-[11px] font-bold uppercase opacity-60">Custom company agents</div>{custom_options}"#
    );

    format!(
        r##"<div class="rounded-box border border-base-300 bg-base-200 p-1">
                            <input type="hidden" name="agent_ids" value="{initial}">
                            <div class="grid grid-cols-1 gap-1 sm:grid-cols-2">
                                {options}
                            </div>
                        </div>"##,
        initial = selected
            .iter()
            .map(Uuid::to_string)
            .collect::<Vec<_>>()
            .join(","),
    )
}

/// The agent library as a modal of cards, opened from the button that shows the current pick.
///
/// A `<select>` can only carry a name, and a library definition is chosen on what it *does* — so
/// the choice is made against cards carrying the avatar, the handle and the description, and it
/// lands in a hidden field the shared `syncChannelAgents` treats like any other option.
fn library_agent_picker(library: &[&Agent], selected: &[Uuid], id_prefix: &str) -> String {
    const PLACEHOLDER: &str = "Choose a library agent…";

    let picked = library.iter().find(|agent| selected.contains(&agent.id));
    let label = picked.map_or_else(
        || PLACEHOLDER.to_string(),
        |agent| escape_html_text(&agent.name),
    );
    let cards = library
        .iter()
        .map(|agent| library_agent_card(agent, picked.is_some_and(|p| p.id == agent.id)))
        .collect::<String>();

    format!(
        r##"<div class="mx-3 my-2">
                                    <input type="hidden" class="channel-library-agent-field" value="{picked_id}">
                                    <button type="button" class="btn btn-block justify-start font-normal"
                                        data-action="open-dialog" data-dialog="library-agents-{id_prefix}">
                                        <span class="channel-library-agent-label truncate" data-placeholder="{PLACEHOLDER}">{label}</span>
                                    </button>
                                    <dialog id="library-agents-{id_prefix}" class="modal">
                                        <div class="modal-box max-w-3xl">
                                            <h3 class="text-lg font-bold">Agent library</h3>
                                            <p class="pt-1 pb-4 text-sm opacity-70">Ready-made agents this channel can run.</p>
                                            <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
                                                {cards}
                                            </div>
                                            <div class="modal-action">
                                                <button type="button" class="btn btn-ghost btn-sm"
                                                    data-agent-id="" data-agent-name=""
                                                    data-action="pick-channel-library-agent">Clear</button>
                                                <button type="button" class="btn btn-sm"
                                                    data-action="close-dialog">Close</button>
                                            </div>
                                        </div>
                                        <button type="button" class="modal-backdrop"
                                            data-action="close-dialog">close</button>
                                    </dialog>
                                </div>"##,
        picked_id = picked.map(|agent| agent.id.to_string()).unwrap_or_default(),
    )
}

/// One library definition as a card: what it is called, what it answers as, and what it is for.
fn library_agent_card(agent: &Agent, picked: bool) -> String {
    let description = agent.description.as_deref().unwrap_or_default().trim();
    let description_html = if description.is_empty() {
        String::new()
    } else {
        format!(
            r#"<span class="line-clamp-2 text-xs opacity-70">{}</span>"#,
            escape_html_text(description)
        )
    };

    format!(
        r##"<button type="button" data-agent-id="{id}" data-agent-name="{name}"
                                                    data-action="pick-channel-library-agent"
                                                    class="channel-library-agent-card flex items-start gap-3 rounded-box border-2 {border} bg-base-200 p-3 text-left hover:border-primary">
                                                    {avatar}
                                                    <span class="flex min-w-0 flex-col gap-0.5">
                                                        <span class="truncate text-sm font-semibold">{name}</span>
                                                        <span class="truncate font-mono text-[11px] opacity-60">@{slug}</span>
                                                        {description_html}
                                                    </span>
                                                </button>"##,
        id = agent.id,
        border = if picked {
            "border-primary"
        } else {
            "border-base-300"
        },
        avatar = avatar_bubble(agent.avatar_url.as_ref(), &agent.name, AvatarSize::Row),
        name = escape_html_text(&agent.name),
        slug = escape_html_text(&agent.slug),
    )
}

/// The interlock for a server with spam scanning turned off.
///
/// `ChannelUseCases` only demands the confirmation for a `@public` channel, so the box is inert
/// until the participants field names one — `is_public` is the state the first render lands on
/// and `toggleChannelSpamConfirm` takes it from there.
fn spam_disabled_confirmation(spam_scan_enabled: bool, is_public: bool) -> String {
    if spam_scan_enabled {
        return String::new();
    }

    let (dimmed, disabled) = if is_public {
        ("", "")
    } else {
        (" opacity-40 pointer-events-none", " disabled")
    };

    format!(
        r##"
                    <div class="spam-confirm-box alert alert-warning text-sm transition-opacity{dimmed}">
                        <div class="space-y-2">
                            <div class="font-semibold">Spam scanning is disabled in server configuration</div>
                            <div class="text-xs">A channel with no participant restrictions receives mail without spam filtering.</div>
                            <label class="flex cursor-pointer items-center gap-3">
                                <input type="checkbox" name="confirm_spam_disabled" value="true" class="checkbox checkbox-sm"{disabled}>
                                <span class="text-xs">I understand, and confirm saving a <code class="font-mono">@public</code> channel anyway.</span>
                            </label>
                        </div>
                    </div>
    "##
    )
}

/// A stored channel as the form sees it.
///
/// The two fields the channel does not hold as text — its participants and its config JSON — are
/// rendered by [`stored_participants`] and [`stored_config`] and passed in, so the draft can stay
/// a pure set of borrows.
fn stored_draft<'a>(
    channel: &'a Channel,
    participant_emails: &'a str,
    alias_slugs: &'a str,
    channel_config: &'a str,
) -> ChannelDraft<'a> {
    ChannelDraft {
        name: &channel.name,
        description: channel.description.as_deref().unwrap_or(""),
        slug: &channel.slug,
        alias_slugs,
        system_prompt: "",
        participant_emails,
        agent_ids: channel.agent_ids.as_deref().unwrap_or(&[]),
        provider: channel.provider.as_deref().unwrap_or(""),
        model: channel.model.as_deref().unwrap_or(""),
        api_key: channel.api_key.as_deref().unwrap_or(""),
        channel_config,
        advanced: true,
        enabled: channel.enabled,
        add_3rd_party: channel.add_3rd_party,
        retrieve_company_memory: channel.retrieve_company_memory,
        retrieve_agent_memory: channel.retrieve_agent_memory,
        retrieve_user_memory: channel.retrieve_user_memory,
        persist_company_memory: channel.persist_company_memory,
        persist_agent_memory: channel.persist_agent_memory,
        persist_user_memory: channel.persist_user_memory,
        memory_persistence_mode: channel.memory_persistence_mode.as_str(),
        memory_recall_mode: channel.memory_recall_mode.as_str(),
        memory_max_results: channel.memory_max_results.to_string(),
    }
}

fn memory_fields(memory_ready: bool, draft: &ChannelDraft<'_>) -> String {
    let disabled = if memory_ready { "" } else { " disabled" };
    let checked = |value: bool| if value { " checked" } else { "" };
    let recall_selected = |value: &str| {
        if draft.memory_recall_mode == value {
            " selected"
        } else {
            ""
        }
    };
    let persistence_selected = |value: &str| {
        if draft.memory_persistence_mode == value {
            " selected"
        } else {
            ""
        }
    };
    format!(
        r##"<fieldset class="rounded-lg border border-base-300 bg-base-200 p-4"{disabled}>
                        <legend class="px-1 text-xs font-semibold">Memory</legend>
                        <p class="mb-3 text-[11px] opacity-60">Controls become authoritative only when the company's selected provider is ready.</p>
                        <div class="grid max-w-sm grid-cols-4 items-center gap-x-2 gap-y-2 text-xs">
                            <span></span>
                            <span class="text-center opacity-70">Company</span>
                            <span class="text-center opacity-70">Agent</span>
                            <span class="text-center opacity-70">User</span>
                            <span class="opacity-70">Retrieve</span>
                            <input aria-label="Retrieve company memory" type="checkbox" name="retrieve_company_memory" value="true" class="checkbox checkbox-sm justify-self-center"{retrieve_company}{disabled}>
                            <input aria-label="Retrieve agent memory" type="checkbox" name="retrieve_agent_memory" value="true" class="checkbox checkbox-sm justify-self-center"{retrieve_agent}{disabled}>
                            <input aria-label="Retrieve user memory" type="checkbox" name="retrieve_user_memory" value="true" class="checkbox checkbox-sm justify-self-center"{retrieve_user}{disabled}>
                            <span class="opacity-70">Persist</span>
                            <input aria-label="Persist company memory" type="checkbox" name="persist_company_memory" value="true" class="checkbox checkbox-sm justify-self-center"{persist_company}{disabled}>
                            <input aria-label="Persist agent memory" type="checkbox" name="persist_agent_memory" value="true" class="checkbox checkbox-sm justify-self-center"{persist_agent}{disabled}>
                            <input aria-label="Persist user memory" type="checkbox" name="persist_user_memory" value="true" class="checkbox checkbox-sm justify-self-center"{persist_user}{disabled}>
                        </div>
                        <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
                            <label class="form-control w-full">
                                <div class="label"><span class="text-xs opacity-70">Persistence mode</span></div>
                                <select name="memory_persistence_mode" class="select w-full"{disabled}>
                                    <option value="audience_only"{audience_only_selected}>Audience only</option>
                                    <option value="scope_specific_facts"{scope_specific_selected}>Scope-specific facts</option>
                                </select>
                            </label>
                            <label class="form-control w-full">
                                <div class="label"><span class="text-xs opacity-70">Recall mode</span></div>
                                <select name="memory_recall_mode" class="select w-full"{disabled}>
                                    <option value="fast"{fast_selected}>Fast</option>
                                    <option value="thinking"{thinking_selected}>Thinking</option>
                                </select>
                            </label>
                            <label class="form-control w-full">
                                <div class="label"><span class="text-xs opacity-70">Maximum results</span></div>
                                <input name="memory_max_results" type="number" min="1" max="20" value="{max_results}" class="input w-full"{disabled}>
                            </label>
                        </div>
                    </fieldset>"##,
        retrieve_company = checked(draft.retrieve_company_memory),
        retrieve_agent = checked(draft.retrieve_agent_memory),
        retrieve_user = checked(draft.retrieve_user_memory),
        persist_company = checked(draft.persist_company_memory),
        persist_agent = checked(draft.persist_agent_memory),
        persist_user = checked(draft.persist_user_memory),
        audience_only_selected = persistence_selected("audience_only"),
        scope_specific_selected = persistence_selected("scope_specific_facts"),
        fast_selected = recall_selected("fast"),
        thinking_selected = recall_selected("thinking"),
        max_results = escape_html_text(&draft.memory_max_results),
    )
}

/// Every address the channel answers on, canonical first, for the pane's identity line.
fn all_addresses(channel: &Channel, company: &Company, app_domain_name: &str) -> String {
    channel
        .inbound_addresses(&company.slug, app_domain_name)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The channel's alias addresses as the comma-separated list the form submits.
pub fn stored_alias_slugs(channel: &Channel) -> String {
    channel
        .alias_slugs
        .iter()
        .map(|slug| slug.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The channel's participants as the comma-separated list the form submits.
pub fn stored_participants(channel: &Channel) -> String {
    match &channel.participant_emails {
        Some(emails) => emails.join(", "),
        None => String::new(),
    }
}

/// The channel's config as the JSON text the form submits.
pub fn stored_config(channel: &Channel) -> String {
    match &channel.channel_config {
        Some(config) => serde_json::to_string_pretty(config).unwrap_or_else(|_| config.to_string()),
        None => String::new(),
    }
}

pub fn channel_schedules_card(
    company_id: Uuid,
    channel_id: Uuid,
    schedules: &[ChannelSchedule],
) -> String {
    let rows_html: String = if schedules.is_empty() {
        r#"<div class="rounded-lg border border-dashed border-base-300 p-4 text-center text-xs opacity-60">No automated schedules configured for this channel yet.</div>"#.to_string()
    } else {
        schedules
            .iter()
            .map(|s| {
                let status_badge = if s.enabled {
                    r#"<span class="badge badge-success badge-sm">Active</span>"#
                } else {
                    r#"<span class="badge badge-ghost badge-sm opacity-60">Paused</span>"#
                };
                let delivery_badge = match s.delivery_mode {
                    ScheduleDeliveryMode::MailboxOnly => {
                        r#"<span class="badge badge-neutral badge-sm">Mailbox Only</span>"#
                    }
                    ScheduleDeliveryMode::EmailParticipants => {
                        r#"<span class="badge badge-info badge-sm">Email: Participants</span>"#
                    }
                    ScheduleDeliveryMode::EmailCustom => {
                        r#"<span class="badge badge-primary badge-sm">Email: Custom</span>"#
                    }
                };
                let next_run_info = match s.next_run_at {
                    Some(next) => format!("Next run: {}", s.in_zone(next, "%b %d, %H:%M %Z")),
                    None => "Completed".to_string(),
                };
                let toggle_label = if s.enabled { "Pause" } else { "Resume" };
                let toggle_value = if s.enabled { "false" } else { "true" };

                format!(
                    r##"
                    <div class="flex flex-col gap-2 rounded-lg border border-base-300 bg-base-100 p-3">
                        <div class="flex items-start justify-between gap-2">
                            <div class="min-w-0">
                                <div class="flex items-center gap-2 flex-wrap">
                                    <span class="font-semibold text-sm truncate">{name}</span>
                                    {status_badge}
                                    {delivery_badge}
                                </div>
                                <div class="mt-1 flex flex-wrap items-center gap-2 text-xs opacity-70">
                                    <span class="font-medium text-primary">{cadence}</span>
                                    <span>•</span>
                                    <span>{next_run_info}</span>
                                </div>
                            </div>
                            <div class="flex shrink-0 items-center gap-1">
                                <form hx-post="/ui/channels/{channel_id}/schedules/{schedule_id}/run-now"
                                    hx-target="#channel-schedules-card" hx-swap="outerHTML" class="inline">
                                    <input type="hidden" name="company_id" value="{company_id}">
                                    <button type="submit" class="btn btn-outline btn-xs" title="Trigger this schedule immediately">
                                        Run Now
                                    </button>
                                </form>
                                <form hx-post="/ui/channels/{channel_id}/schedules/{schedule_id}/toggle"
                                    hx-target="#channel-schedules-card" hx-swap="outerHTML" class="inline">
                                    <input type="hidden" name="company_id" value="{company_id}">
                                    <input type="hidden" name="enabled" value="{toggle_value}">
                                    <button type="submit" class="btn btn-ghost btn-xs">
                                        {toggle_label}
                                    </button>
                                </form>
                                <form hx-post="/ui/channels/{channel_id}/schedules/{schedule_id}/delete"
                                    hx-target="#channel-schedules-card" hx-swap="outerHTML"
                                    hx-confirm="Delete schedule '{name}'?" class="inline">
                                    <input type="hidden" name="company_id" value="{company_id}">
                                    <button type="submit" class="btn btn-ghost btn-xs text-error">
                                        ✕
                                    </button>
                                </form>
                            </div>
                        </div>
                        <div class="rounded bg-base-200 p-2 text-xs font-mono opacity-80 truncate" title="{subject}">
                            <span class="font-semibold text-base-content/70">Subject:</span> {subject}
                        </div>
                        <div class="rounded bg-base-200 p-2 text-xs font-mono opacity-80 line-clamp-2" title="{prompt}">
                            <span class="font-semibold text-base-content/70">Prompt:</span> {prompt}
                        </div>
                    </div>
                    "##,
                    name = escape_html_text(&s.name),
                    cadence = escape_html_text(&s.cadence_label()),
                    subject = escape_html_text(&s.subject_template),
                    prompt = escape_html_text(&s.prompt_template),
                    schedule_id = s.id,
                    channel_id = channel_id,
                    company_id = company_id,
                    status_badge = status_badge,
                    delivery_badge = delivery_badge,
                    next_run_info = next_run_info,
                    toggle_label = toggle_label,
                    toggle_value = toggle_value,
                )
            })
            .collect()
    };

    format!(
        r##"
        <div id="channel-schedules-card" class="rounded-box border border-base-300 bg-base-200 p-4 space-y-4">
            <div class="flex items-center justify-between">
                <div>
                    <h3 class="text-sm font-bold">Automated Schedules &amp; Triggers</h3>
                    <p class="text-xs opacity-60">Run this channel's agents on a recurring interval or as a one-off scheduled report.</p>
                </div>
            </div>

            <div class="space-y-2">
                {rows_html}
            </div>

            <details class="collapse collapse-arrow border border-base-300 bg-base-100 rounded-lg">
                <summary class="collapse-title text-xs font-semibold">+ New Schedule</summary>
                <div class="collapse-content">
                    <form hx-post="/ui/channels/{channel_id}/schedules" hx-target="#channel-schedules-card" hx-swap="outerHTML" class="space-y-3 pt-2">
                        <input type="hidden" name="company_id" value="{company_id}">
                        
                        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
                            <label class="form-control w-full">
                                <div class="label py-1"><span class="text-xs opacity-70">Schedule Name</span></div>
                                <input type="text" name="name" required placeholder="Daily Operations Report" class="input input-sm w-full">
                            </label>
                            <label class="form-control w-full">
                                <div class="label py-1"><span class="text-xs opacity-70">Schedule Type</span></div>
                                <select name="schedule_type" class="select select-sm w-full" data-action="toggle-schedule-type">
                                    <option value="interval" selected>Recurring Interval</option>
                                    <option value="one_off">One-Off Scheduled Run</option>
                                </select>
                            </label>
                        </div>

                        <div class="schedule-interval-box">
                            <label class="form-control w-full">
                                <div class="label py-1"><span class="text-xs opacity-70">Repeat Cadence</span></div>
                                <select name="interval_seconds" class="select select-sm w-full">
                                    <option value="900">Every 15 minutes</option>
                                    <option value="1800">Every 30 minutes</option>
                                    <option value="3600" selected>Every hour</option>
                                    <option value="21600">Every 6 hours</option>
                                    <option value="43200">Every 12 hours</option>
                                    <option value="86400">Every day (24h)</option>
                                    <option value="604800">Every week (7d)</option>
                                </select>
                            </label>
                        </div>

                        <div class="schedule-oneoff-box hidden">
                            <label class="form-control w-full">
                                <div class="label py-1"><span class="text-xs opacity-70">Run At (Date &amp; Time)</span></div>
                                <input type="datetime-local" name="scheduled_at" class="input input-sm w-full">
                            </label>
                        </div>

                        <label class="form-control w-full">
                            <div class="label py-1"><span class="text-xs opacity-70">Thread Subject (supports <code class="font-mono">{{date}}</code>, <code class="font-mono">{{time}}</code>)</span></div>
                            <input type="text" name="subject_template" required value="[Scheduled Report] {{date}}" class="input input-sm w-full font-mono text-xs">
                        </label>

                        <label class="form-control w-full">
                            <div class="label py-1"><span class="text-xs opacity-70">Agent Prompt / Task Instructions</span></div>
                            <textarea name="prompt_template" required rows="3" placeholder="Describe what the agent should analyze or generate for this scheduled run..." class="textarea textarea-sm w-full font-mono text-xs"></textarea>
                        </label>

                        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
                            <label class="form-control w-full">
                                <div class="label py-1"><span class="text-xs opacity-70">Output Delivery Mode</span></div>
                                <select name="delivery_mode" class="select select-sm w-full" data-action="toggle-schedule-delivery">
                                    <option value="mailbox_only" selected>Mailbox Only (In-App Review)</option>
                                    <option value="email_participants">Post to Mailbox &amp; Email Participants</option>
                                    <option value="email_custom">Post to Mailbox &amp; Email Custom List</option>
                                </select>
                            </label>
                            <div class="schedule-custom-recipients-box hidden">
                                <label class="form-control w-full">
                                    <div class="label py-1"><span class="text-xs opacity-70">Recipient Emails (comma-separated)</span></div>
                                    <input type="text" name="recipient_emails" placeholder="team@company.com, client@example.com" class="input input-sm w-full">
                                </label>
                            </div>
                        </div>

                        <div class="flex items-center gap-2 pt-2">
                            <button type="submit" class="btn btn-primary btn-sm">Create Schedule</button>
                        </div>
                    </form>
                </div>
            </details>
        </div>
        "##,
        rows_html = rows_html,
        channel_id = channel_id,
        company_id = company_id,
    )
}
