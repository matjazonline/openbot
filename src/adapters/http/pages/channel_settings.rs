//! The `/ui` Channels workspace: the same shell as the mailbox, with the channel list configured
//! rather than read.
//!
//! The sidebar picks a channel and its settings are swapped into `#channel-pane` over htmx, the
//! way picking a thread swaps the mailbox's detail pane. Every write re-renders the pane and
//! sends the sidebar list along out of band, so a rename or a delete shows up immediately.

use super::*;
use crate::entities::channel::PUBLIC_PARTICIPANT;

/// Client-side behaviour this workspace adds on top of [`MAILBOX_SCRIPT`].
///
/// Kept out of the `format!` blocks below so its braces need no escaping.
const CHANNEL_SETTINGS_SCRIPT: &str = r##"        function showChannelTab(advanced) {
            var simple = document.getElementById('channel-tab-simple');
            var advancedForm = document.getElementById('channel-tab-advanced');
            var simpleBtn = document.getElementById('channel-tab-simple-btn');
            var advancedBtn = document.getElementById('channel-tab-advanced-btn');
            if (simple) simple.classList.toggle('hidden', advanced);
            if (advancedForm) advancedForm.classList.toggle('hidden', !advanced);
            if (simpleBtn) simpleBtn.classList.toggle('tab-active', !advanced);
            if (advancedBtn) advancedBtn.classList.toggle('tab-active', advanced);
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
        // The options are radios with no `name`: naming them would group them for the browser but
        // also add a field to the submitted body, and the server still reads only `agent_ids`.
        // Clearing the siblings here is what makes the group behave like one.
        function syncChannelAgents(el) {
            var form = el.closest('form');
            if (!form) return;
            var options = form.querySelectorAll('.channel-agent-option');
            Array.prototype.forEach.call(options, function (option) {
                if (option !== el) option.checked = false;
            });
            var target = form.querySelector('input[name="agent_ids"]');
            if (!target) return;
            var checked = form.querySelectorAll('.channel-agent-option:checked');
            target.value = Array.prototype.map.call(checked, function (box) {
                return box.value;
            }).join(',');
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
}

impl Default for ChannelDraft<'_> {
    fn default() -> Self {
        Self {
            name: "",
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
    pub error: Option<&'a str>,
}

pub fn channel_settings_page(page: &ChannelSettingsPage<'_>) -> String {
    let company = page.list.company;
    let content = format!(
        r##"
        <aside class="flex w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {company_switcher}
            {list_html}
            <div class="border-t border-base-300 p-2">
                <button type="button" class="btn btn-primary btn-sm btn-block justify-start"
                    hx-get="/ui/channels/new?company_id={company_id}"
                    hx-target="#channel-pane" hx-swap="outerHTML"
                    hx-push-url="/ui/channels?company_id={company_id}&new=1">{plus_glyph} New Channel</button>
            </div>
        </aside>
        {pane_html}
        "##,
        company_switcher = company_switcher(company, page.companies, UiSection::Channels),
        plus_glyph = icon(Icon::Plus, BUTTON_ICON),
        list_html = channel_settings_list(page.list, FragmentSwap::Inline),
        company_id = company.id,
        pane_html = page.pane_html,
    );

    ui_shell(&UiShell {
        title: &format!("{} Channels", company.name),
        user: page.user,
        company_id: Some(company.id),
        section: UiSection::Channels,
        content: &content,
        script: CHANNEL_SETTINGS_SCRIPT,
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
                        hx-push-url="/ui/channels?company_id={company_id}&channel_id={channel_id}"
                        onclick="selectSidebarItem(this)">
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
        <section id="channel-pane"{PANE_SKELETON} class="flex flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <p class="text-center text-sm opacity-60">{message}</p>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub fn channel_edit_pane(pane: &ChannelEditPane<'_>) -> String {
    let participants = stored_participants(pane.channel);
    let aliases = stored_alias_slugs(pane.channel);
    let config = stored_config(pane.channel);
    let stored = stored_draft(pane.channel, &participants, &aliases, &config);
    let draft = pane.draft.unwrap_or(&stored);
    let company_id = pane.company.id;
    let channel_id = pane.channel.id;

    format!(
        r##"
        <section id="channel-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100">
            <div class="flex items-start justify-between gap-3 border-b border-base-300 px-6 py-4">
                <div class="min-w-0">
                    <h2 class="truncate text-xl font-bold">{name}</h2>
                    <p class="truncate font-mono text-xs opacity-60">{address}</p>
                </div>
                <div class="flex shrink-0 items-center gap-2">
                    <a href="/ui?company_id={company_id}&channel_id={channel_id}" class="btn btn-ghost btn-sm">Open Mailbox</a>
                    <a href="/companies/{company_id}/channels/{channel_id}/simulate" class="btn btn-outline btn-sm">Simulator</a>
                </div>
            </div>
            <div class="flex-1 overflow-y-auto px-6 py-4">
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
                            hx-target="#channel-pane" hx-swap="outerHTML"
                            hx-push-url="/ui/channels?company_id={company_id}">Cancel</button>
                        <button type="button" class="btn btn-error btn-outline ml-auto"
                            hx-delete="/ui/channels/{channel_id}?company_id={company_id}"
                            hx-target="#channel-pane" hx-swap="outerHTML"
                            hx-confirm="Delete channel '{name}'? Its threads and tasks go with it."
                            hx-push-url="/ui/channels?company_id={company_id}">Delete Channel</button>
                    </div>
                </form>
            </div>
        </section>
        "##,
        name = escape_html_text(&pane.channel.name),
        address = escape_html_text(&all_addresses(
            pane.channel,
            pane.company,
            pane.app_domain_name
        )),
        error_html = form_error_banner(pane.error),
        fields = channel_fields(&ChannelFields {
            company: pane.company,
            app_domain_name: pane.app_domain_name,
            agents: pane.agents,
            id_prefix: &channel_id.to_string(),
            draft,
            spam_scan_enabled: pane.spam_scan_enabled,
        }),
    )
}

pub fn channel_create_pane(pane: &ChannelCreatePane<'_>) -> String {
    let (simple_hidden, advanced_hidden) = if pane.draft.advanced {
        ("hidden", "")
    } else {
        ("", "hidden")
    };
    let active = |lit: bool| if lit { "tab-active" } else { "" };

    format!(
        r##"
        <section id="channel-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-6 py-4">
                <h2 class="text-xl font-bold">New channel in {company_name}</h2>
                <p class="text-xs opacity-70">A channel is an inbound address at <span class="font-mono">@{company_slug}.{app_domain_name}</span> plus the agents that answer it.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-6 py-4">
                {error_html}
                <div role="tablist" class="tabs tabs-box mb-4 w-fit">
                    <button type="button" role="tab" id="channel-tab-simple-btn" class="tab {simple_active}"
                        onclick="showChannelTab(false)">Simple</button>
                    <button type="button" role="tab" id="channel-tab-advanced-btn" class="tab {advanced_active}"
                        onclick="showChannelTab(true)">Advanced</button>
                </div>
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
                            hx-target="#channel-pane" hx-swap="outerHTML"
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
                            hx-target="#channel-pane" hx-swap="outerHTML"
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
        simple_active = active(!pane.draft.advanced),
        advanced_active = active(pane.draft.advanced),
        name = escape_html_text(pane.draft.name),
        system_prompt = escape_html_text(pane.draft.system_prompt),
        fields = channel_fields(&ChannelFields {
            company: pane.company,
            app_domain_name: pane.app_domain_name,
            agents: pane.agents,
            id_prefix: "new",
            draft: pane.draft,
            spam_scan_enabled: pane.spam_scan_enabled,
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

    format!(
        r##"
                    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Channel Name</span></div>
                            <input type="text" name="name" required value="{name}" placeholder="Inbound Email Handler"
                                oninput="this.form.slug.value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')"
                                class="input w-full">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Address (@{company_slug}.{app_domain_name})</span></div>
                            <input type="text" name="slug" required value="{slug}" placeholder="inbound-email-handler"
                                class="input w-full font-mono">
                        </label>
                    </div>
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
                            oninput="toggleChannelSpamConfirm(this)"
                            placeholder="Leave blank for the company team, @public for anyone, or comma-separated emails"
                            class="input w-full">
                        <div class="label"><span class="text-[11px] opacity-60">Blank means the company team. Use <code class="font-mono">@public</code> to let anyone write in.</span></div>
                    </label>
                    <details class="collapse-arrow collapse border border-base-300 bg-base-200"{overrides_open}>
                        <summary class="collapse-title text-sm font-medium">Custom model &amp; config</summary>
                        <div class="collapse-content space-y-4">
                            <div class="grid grid-cols-1 gap-4 md:grid-cols-3">
                                <label class="form-control w-full">
                                    <div class="label"><span class="text-xs opacity-70">LLM Provider</span></div>
                                    <input type="text" name="provider" value="{provider}" placeholder="google, openai, anthropic"
                                        class="input w-full font-mono text-sm">
                                </label>
                                <label class="form-control w-full">
                                    <div class="label"><span class="text-xs opacity-70">LLM Model</span></div>
                                    <input type="text" name="model" value="{model}" placeholder="gemini-2.5-flash, gpt-4o"
                                        class="input w-full font-mono text-sm">
                                </label>
                                <label class="form-control w-full">
                                    <div class="label"><span class="text-xs opacity-70">LLM API Key</span></div>
                                    <input type="password" name="api_key" value="{api_key}" placeholder="Overrides the company key"
                                        class="input w-full font-mono text-sm">
                                </label>
                            </div>
                            <label class="form-control w-full">
                                <div class="label"><span class="text-xs opacity-70">Channel Config (JSON)</span></div>
                                <textarea name="channel_config" rows="4" placeholder='{{ "trigger": "email", "action": "ai_reply" }}'
                                    class="textarea w-full font-mono text-xs">{channel_config}</textarea>
                            </label>
                        </div>
                    </details>
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
        provider = escape_html_text(draft.provider),
        model = escape_html_text(draft.model),
        api_key = escape_html_text(draft.api_key),
        channel_config = escape_html_text(draft.channel_config),
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

    let options: String = agents
        .iter()
        .map(|agent| {
            format!(
                r##"
                            <label class="label cursor-pointer justify-start gap-3 rounded-box px-3 py-2 hover:bg-base-300" for="agent-{id_prefix}-{agent_id}">
                                <input type="radio" id="agent-{id_prefix}-{agent_id}" value="{agent_id}" {checked}
                                    class="channel-agent-option radio radio-sm radio-primary"
                                    onchange="syncChannelAgents(this)">
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
        .collect();

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
    }
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
