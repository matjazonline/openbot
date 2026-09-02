//! The `/ui` Agents workspace: the same shell as the mailbox and the Channels workspace, with the
//! company's agents configured rather than read.
//!
//! The sidebar picks an agent and its settings are swapped into `#agent-pane` over htmx, the way
//! picking a channel swaps `#channel-pane`. Every write re-renders the pane and sends the sidebar
//! list along out of band, so a rename or a delete shows up immediately.

use super::*;

/// Client-side behaviour this workspace adds on top of [`MAILBOX_SCRIPT`].
///
/// Kept out of the `format!` blocks below so its braces need no escaping.
pub const AGENT_SETTINGS_SCRIPT: &str = r##"        // The create pane holds one form per tab -- Easy, Simple and Advanced -- and shows the
        // picked one. A tab whose form the server did not render is simply absent.
        function showAgentTab(tab) {
            ['easy', 'simple', 'advanced'].forEach(function (name) {
                var form = document.getElementById('agent-tab-' + name);
                var button = document.getElementById('agent-tab-' + name + '-btn');
                if (form) form.classList.toggle('hidden', name !== tab);
                if (button) button.classList.toggle('tab-active', name === tab);
            });
        }

        // The generator is off to one side until asked for: writing a prompt by hand is still the
        // normal way to fill the field.
        function toggleAgentPromptGenerator(prefix) {
            var box = document.getElementById('agent-generator-' + prefix);
            if (!box) return;
            box.classList.toggle('hidden');
            if (!box.classList.contains('hidden')) {
                var input = document.getElementById('agent-instructions-' + prefix);
                if (input) input.focus();
            }
        }

        function updateAgentAddressPreview(input) {
            var preview = document.getElementById(input.dataset.addressPreview);
            if (preview) preview.textContent = (input.value || 'agent-handle') + preview.dataset.addressSuffix;
        }

        function updateSimpleAgentAddressPreview(input) {
            var preview = document.getElementById('agent-address-preview');
            if (preview) preview.textContent = (slugifyValue(input.value) || 'agent-handle') + preview.dataset.addressSuffix;
        }"##;

/// The agent list in the sidebar — the only part of the workspace a write has to refresh.
pub struct AgentSettingsList<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    pub agents: &'a [Agent],
    pub channels: &'a [Channel],
    pub selected_agent_id: Option<Uuid>,
}

/// The Agents workspace for one request.
pub struct AgentSettingsPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub companies: &'a [Company],
    pub list: &'a AgentSettingsList<'a>,
    /// Pre-rendered right-hand pane: an agent's settings, the create form, or a placeholder.
    pub pane_html: &'a str,
}

/// What an agent form was last submitted with, so a rejected submit comes back filled in.
///
/// The Advanced create form and the edit form take exactly these fields, which is why they share
/// one renderer; only the URL they submit to differs.
#[derive(Debug)]
pub struct AgentDraft<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    /// The written system prompt in Advanced mode; the instructions to expand in Simple mode.
    pub system_prompt: &'a str,
    /// One line on what this agent is for, shown to sibling agents by the directory tool.
    pub description: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub run_timeout_secs: Option<u32>,
    pub memory_enabled: bool,
    pub memory_persistence_mode: &'a str,
    pub memory_recall_mode: &'a str,
    pub memory_max_results: u8,
    pub config_json: &'a str,
    pub avatar_url: &'a str,
    /// Whether the create pane should open on the Advanced tab.
    pub advanced: bool,
}

/// A blank agent as its own form will accept it, which is why this is hand-written rather than
/// derived: `memory_max_results` renders into a `min="1"` number field, so a derived `0` makes the
/// empty create form fail browser validation before it can be submitted — silently, because the
/// field sits inside a collapsed `<details>` and cannot be focused to report itself.
impl Default for AgentDraft<'_> {
    fn default() -> Self {
        Self {
            name: "",
            slug: "",
            system_prompt: "",
            description: "",
            provider: "",
            model: "",
            run_timeout_secs: None,
            memory_enabled: false,
            memory_persistence_mode: MemoryPersistenceMode::default().as_str(),
            memory_recall_mode: MemoryRecallMode::default().as_str(),
            memory_max_results: default_memory_max_results(),
            config_json: "",
            avatar_url: "",
            advanced: false,
        }
    }
}

/// The settings pane for an agent that already exists.
pub struct AgentEditPane<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    pub model_connections: &'a [CompanyModelConnection],
    pub agent: &'a Agent,
    /// The channels currently running this agent — nothing stops one being deleted out from
    /// under them, so the pane has to say who would notice.
    pub used_by: &'a [&'a Channel],
    /// What the user last typed, when a save was rejected; `None` shows the stored agent.
    pub draft: Option<&'a AgentDraft<'a>>,
    pub error: Option<&'a str>,
}

/// Which of the create pane's forms opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCreateTab {
    /// Pick ready-made definitions out of the agent library.
    Easy,
    /// A name and instructions, expanded into a system prompt on create.
    Simple,
    /// Every stored field, written by hand.
    Advanced,
}

/// The pane for an agent that does not exist yet.
pub struct AgentCreatePane<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    pub model_connections: &'a [CompanyModelConnection],
    /// The global definitions the Easy tab offers; empty drops the tab entirely.
    pub library_agents: &'a [Agent],
    /// Which of them are ticked, so a rejected pick comes back with the selection intact.
    pub selected_library_agent_ids: &'a [Uuid],
    pub tab: AgentCreateTab,
    pub draft: &'a AgentDraft<'a>,
    pub error: Option<&'a str>,
}

pub fn agent_settings_page(page: &AgentSettingsPage<'_>) -> String {
    let company = page.list.company;
    let content = format!(
        r##"
        <aside class="ui-pane-list flex w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {header}
            {list_html}
            <div class="border-t border-base-300 p-2">
                <button type="button" class="btn btn-primary btn-sm btn-block justify-start"
                    hx-get="/ui/agents/new?company_id={company_id}"
                    hx-target="#agent-pane" hx-swap="outerHTML" hx-sync="#agent-pane:replace"
                    hx-push-url="/ui/agents?company_id={company_id}&new=1">{plus_glyph} New Agent</button>
            </div>
        </aside>
        {pane_html}
        "##,
        header = sidebar_header(
            "Agents",
            "AI responders, system prompts and model overrides."
        ),
        plus_glyph = icon(Icon::Plus, BUTTON_ICON),
        list_html = agent_settings_list(page.list, FragmentSwap::Inline),
        company_id = company.id,
        pane_html = page.pane_html,
    );

    ui_shell(&UiShell {
        title: &format!("{} Agents", company.name),
        user: page.user,
        company: Some(company),
        section: UiSection::Agents,
        content: &content,
    })
}

/// One entry per agent, rendered out of band after a write so a created, renamed or deleted agent
/// shows up without the pane having to reload the whole workspace.
pub fn agent_settings_list(list: &AgentSettingsList<'_>, swap: FragmentSwap) -> String {
    let entries: String = list
        .agents
        .iter()
        .map(|agent| {
            agent_settings_entry(
                list.company,
                list.app_domain_name,
                agent,
                list.channels,
                list.selected_agent_id == Some(agent.id),
            )
        })
        .collect();

    let menu_body = if list.agents.is_empty() {
        r##"<li class="px-2 py-6 text-center text-xs opacity-60">No agents yet. Create your first one below.</li>"##
            .to_string()
    } else {
        entries
    };

    format!(
        r##"<ul id="agent-menu" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2"{oob}>{menu_body}</ul>"##,
        oob = swap.oob_attribute(),
    )
}

fn agent_settings_entry(
    company: &Company,
    app_domain_name: &str,
    agent: &Agent,
    channels: &[Channel],
    selected: bool,
) -> String {
    let address = agent_display_address(agent, channels.iter(), company, app_domain_name);
    format!(
        r##"
                <li>
                    <a class="flex items-center gap-3 {active}"
                        hx-get="/ui/agents/{agent_id}?company_id={company_id}"
                        hx-target="#agent-pane" hx-swap="outerHTML"
                        hx-sync="#agent-pane:replace"
                        hx-push-url="/ui/agents?company_id={company_id}&agent_id={agent_id}"
                        data-action="select-sidebar-item">
                        {avatar}
                        <span class="flex min-w-0 flex-col items-start gap-0.5">
                            <span class="flex w-full items-center gap-2">
                                <span class="min-w-0 truncate">{name}</span>
                                <span class="badge badge-ghost badge-sm shrink-0 font-mono">{slug}</span>
                            </span>
                            <span class="w-full truncate font-mono text-[11px] opacity-60">{model}</span>
                        </span>
                    </a>
                </li>
        "##,
        active = if selected { "menu-active" } else { "" },
        company_id = company.id,
        agent_id = agent.id,
        avatar = avatar_bubble(agent.avatar_url.as_ref(), &agent.name, AvatarSize::Row),
        name = escape_html_text(&agent.name),
        slug = escape_html_text(&address),
        model = escape_html_text(&agent_model_summary(agent)),
    )
}

fn agent_display_address<'a>(
    agent: &Agent,
    channels: impl Iterator<Item = &'a Channel>,
    company: &Company,
    app_domain_name: &str,
) -> String {
    let channels = channels.collect::<Vec<_>>();
    let owned = channels
        .iter()
        .copied()
        .find(|channel| channel.owner_agent_id == Some(agent.id));
    let channel = owned.or_else(|| {
        let mut matches = channels.iter().copied().filter(|channel| {
            channel
                .agent_ids
                .as_deref()
                .and_then(|ids| ids.first())
                .is_some_and(|id| *id == agent.id)
        });
        let only = matches.next();
        (matches.next().is_none()).then_some(only).flatten()
    });
    channel.map_or_else(
        || format!("@{}", agent.slug),
        |channel| {
            channel
                .inbound_address(&company.slug, app_domain_name)
                .to_string()
        },
    )
}

/// The line under an agent's name: which model it answers with, or that it takes the company's.
fn agent_model_summary(agent: &Agent) -> String {
    match (agent.provider.as_deref(), agent.model.as_deref()) {
        (Some(provider), Some(model)) => format!("{provider} / {model}"),
        (Some(provider), None) => provider.to_string(),
        (None, Some(model)) => model.to_string(),
        (None, None) => "company default model".to_string(),
    }
}

/// The pane before an agent is picked.
pub fn agent_settings_empty_pane(message: &str, swap: FragmentSwap) -> String {
    format!(
        r##"
        <section id="agent-pane"{PANE_SKELETON} data-pane-empty class="ui-pane-detail flex min-w-0 flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <p class="text-center text-sm opacity-60">{message}</p>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub fn agent_edit_pane(pane: &AgentEditPane<'_>) -> String {
    let config = stored_agent_config(pane.agent);
    let stored = stored_draft(pane.agent, &config);
    let draft = pane.draft.unwrap_or(&stored);
    let company_id = pane.company.id;
    let agent_id = pane.agent.id;
    let address = agent_display_address(
        pane.agent,
        pane.used_by.iter().copied(),
        pane.company,
        pane.app_domain_name,
    );

    format!(
        r##"
        <section id="agent-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="flex flex-wrap items-start justify-between gap-3 border-b border-base-300 px-4 py-4 sm:px-6">
                <div class="flex min-w-0 grow basis-48 items-center gap-3">
                    {avatar}
                    <div class="min-w-0">
                        <h2 class="truncate text-xl font-bold">{name}</h2>
                        <p class="truncate font-mono text-xs opacity-60">{address} · {model}</p>
                        <p class="truncate text-xs opacity-50">{creator}</p>
                    </div>
                </div>
            </div>
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
                {error_html}
                {used_by_html}
                <form hx-put="/ui/agents/{agent_id}?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML"
                    hx-params="not avatar_file" class="space-y-4">
                    <input type="hidden" name="form_mode" value="advanced">
                    {fields}
                    <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Save Changes</span>
                            <span class="hidden [.htmx-request_&]:inline">Saving...</span>
                        </button>
                        <button type="button" class="btn btn-ghost"
                            hx-get="/ui/agents/{agent_id}?company_id={company_id}"
                            hx-target="#agent-pane" hx-swap="outerHTML" hx-sync="#agent-pane:replace">Cancel</button>
                        <button type="button" class="btn btn-error btn-outline ml-auto"
                            hx-delete="/ui/agents/{agent_id}?company_id={company_id}"
                            hx-target="#agent-pane" hx-swap="outerHTML"
                            hx-confirm="Delete agent '{name}'? {delete_warning}"
                            hx-push-url="/ui/agents?company_id={company_id}">Delete Agent</button>
                    </div>
                </form>
            </div>
        </section>
        "##,
        avatar = avatar_bubble(
            pane.agent.avatar_url.as_ref(),
            &pane.agent.name,
            AvatarSize::Header
        ),
        name = escape_html_text(&pane.agent.name),
        address = escape_html_text(&address),
        model = escape_html_text(&agent_model_summary(pane.agent)),
        creator = escape_html_text(&pane.agent.created_by.label()),
        error_html = form_error_banner(pane.error),
        used_by_html = used_by_channels(company_id, pane.used_by),
        delete_warning = escape_html_attr(&delete_warning(agent_id, pane.used_by)),
        fields = agent_fields(&AgentFields {
            scope: AgentFormScope::Company(company_id),
            agent_id: Some(agent_id),
            draft,
            model_connections: pane.model_connections,
        }),
    )
}

pub fn agent_create_pane(pane: &AgentCreatePane<'_>) -> String {
    let hidden = |tab: AgentCreateTab| if pane.tab == tab { "" } else { "hidden" };
    let active = |tab: AgentCreateTab| if pane.tab == tab { "tab-active" } else { "" };
    let easy_html = agent_easy_tab(pane, hidden(AgentCreateTab::Easy));
    let preview_slug = if pane.draft.slug.is_empty() {
        "agent-handle"
    } else {
        pane.draft.slug
    };
    let preview_address = Channel::address_for(
        &crate::entities::value_objects::ChannelSlug::from(preview_slug),
        &pane.company.slug,
        pane.app_domain_name,
    );
    let address_suffix = format!("@{}.{}", pane.company.slug, pane.app_domain_name);

    format!(
        r##"
        <section id="agent-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 py-4 sm:px-6">
                <h2 class="text-xl font-bold">New agent in {company_name}</h2>
                <p class="text-xs opacity-70">An agent is a system prompt plus the model that answers with it. Channels run their agents in order.</p>
                <p class="mt-1 font-mono text-xs text-primary"><span id="agent-address-preview" data-address-suffix="{address_suffix}">{preview_address}</span></p>
            </div>
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
                {error_html}
                <div role="tablist" class="tabs tabs-box mb-4 w-fit">
                    {easy_tab_button}
                    <button type="button" role="tab" id="agent-tab-simple-btn" class="tab {simple_active}"
                        data-action="show-agent-tab" data-tab="simple">Simple</button>
                    <button type="button" role="tab" id="agent-tab-advanced-btn" class="tab {advanced_active}"
                        data-action="show-agent-tab" data-tab="advanced">Advanced</button>
                </div>
                {easy_form}
                <form id="agent-tab-simple" class="{simple_hidden} space-y-4"
                    hx-post="/ui/agents?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML"
                    hx-params="not avatar_file" hx-disabled-elt="find button[type='submit']">
                    <input type="hidden" name="form_mode" value="simple">
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Agent Name</span></div>
                        <input type="text" name="name" required value="{name}" placeholder="Support Triage"
                            data-input="agent-simple-address-preview"
                            class="input w-full">
                    </label>
                    {avatar_field}
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Agent Instructions</span></div>
                        <textarea name="system_prompt" rows="6" required
                            placeholder="Describe the agent's role, responsibilities, rules and tone. A full system prompt is generated when the agent is created."
                            class="textarea w-full font-mono text-xs">{system_prompt}</textarea>
                    </label>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Run timeout (seconds)</span></div>
                        <input type="number" name="run_timeout_secs" min="1" max="3600"
                            value="{simple_run_timeout}" placeholder="Use deployment default"
                            class="input w-full">
                    </label>
                    <button type="submit" class="btn btn-primary">
                        <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">Create Agent</span>
                        <span class="hidden [.htmx-request_&]:inline">Creating...</span>
                    </button>
                </form>
                <form id="agent-tab-advanced" class="{advanced_hidden} space-y-4"
                    hx-post="/ui/agents/new/channel?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML"
                    hx-params="not avatar_file" hx-disabled-elt="find button[type='submit']">
                    <input type="hidden" name="form_mode" value="advanced">
                    {agent_step}
                    {fields}
                    <button type="submit" class="btn btn-primary">
                        <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">Next: Channel</span>
                        <span class="hidden [.htmx-request_&]:inline">Loading channel...</span>
                    </button>
                </form>
            </div>
        </section>
        "##,
        company_name = escape_html_text(&pane.company.name),
        preview_address = escape_html_text(&preview_address),
        address_suffix = escape_html_attr(&address_suffix),
        company_id = pane.company.id,
        error_html = form_error_banner(pane.error),
        easy_tab_button = easy_html.tab_button,
        easy_form = easy_html.form,
        simple_hidden = hidden(AgentCreateTab::Simple),
        advanced_hidden = hidden(AgentCreateTab::Advanced),
        simple_active = active(AgentCreateTab::Simple),
        advanced_active = active(AgentCreateTab::Advanced),
        agent_step = create_steps(AgentCreateStep::Agent),
        name = escape_html_text(pane.draft.name),
        avatar_field = agent_avatar_field("simple", pane.draft),
        system_prompt = escape_html_text(pane.draft.system_prompt),
        simple_run_timeout = pane
            .draft
            .run_timeout_secs
            .map(|seconds| seconds.to_string())
            .unwrap_or_default(),
        fields = agent_fields(&AgentFields {
            scope: AgentFormScope::Company(pane.company.id),
            agent_id: None,
            draft: pane.draft,
            model_connections: pane.model_connections,
        }),
    )
}

/// The pane for the second half of an Advanced create: the personal channel the new agent will
/// answer on.
///
/// Nothing has been written when this renders. The agent from the first step rides along in hidden
/// fields (see [`carried_agent_fields`]) so one submit can create the pair in the transaction they
/// have always been created in.
pub struct AgentChannelStepPane<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    /// The first step, as it was submitted and normalized.
    pub agent: &'a AgentDraft<'a>,
    pub draft: &'a ChannelDraft<'a>,
    pub spam_scan_enabled: bool,
    pub memory_ready: bool,
    pub error: Option<&'a str>,
}

/// Which half of an Advanced create the pane is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentCreateStep {
    Agent,
    Channel,
}

/// The two-step progress line both halves carry, so the first one says there is a second.
fn create_steps(current: AgentCreateStep) -> String {
    let step = |lit: bool| if lit { " step-primary" } else { "" };
    format!(
        r##"<ul class="steps steps-horizontal w-full max-w-xs text-xs">
                        <li class="step step-primary">Agent</li>
                        <li class="step{channel}">Channel</li>
                    </ul>"##,
        channel = step(current == AgentCreateStep::Channel),
    )
}

pub fn agent_channel_step_pane(pane: &AgentChannelStepPane<'_>) -> String {
    let address = Channel::address_for(
        &crate::entities::value_objects::ChannelSlug::from(pane.agent.slug),
        &pane.company.slug,
        pane.app_domain_name,
    );

    format!(
        r##"
        <section id="agent-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 py-4 sm:px-6">
                <h2 class="text-xl font-bold">New agent in {company_name}</h2>
                <p class="text-xs opacity-70">{agent_name} answers on its own channel. Set it up here; the agent and the channel are created together.</p>
                <p class="mt-1 font-mono text-xs text-primary">{address}</p>
            </div>
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
                {error_html}
                <form class="space-y-4"
                    hx-post="/ui/agents/new/create?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML"
                    hx-params="not avatar_file" hx-disabled-elt="find button[type='submit']">
                    {steps}
                    {carried}
                    {fields}
                    <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                        <button type="button" class="btn btn-ghost"
                            hx-post="/ui/agents/new/agent?company_id={company_id}" hx-include="closest form"
                            hx-target="#agent-pane" hx-swap="outerHTML" hx-sync="#agent-pane:replace"
                            hx-disabled-elt="this">Back</button>
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Create Agent</span>
                            <span class="hidden [.htmx-request_&]:inline">Creating...</span>
                        </button>
                        <button type="button" class="btn btn-ghost ml-auto"
                            hx-get="/ui/agents/new?company_id={company_id}"
                            hx-target="#agent-pane" hx-swap="outerHTML" hx-sync="#agent-pane:replace">Cancel</button>
                    </div>
                </form>
            </div>
        </section>
        "##,
        company_name = escape_html_text(&pane.company.name),
        agent_name = escape_html_text(pane.agent.name),
        address = escape_html_text(&address),
        company_id = pane.company.id,
        error_html = form_error_banner(pane.error),
        steps = create_steps(AgentCreateStep::Channel),
        carried = carried_agent_fields(pane.agent),
        fields = channel_fields(&ChannelFields {
            company: pane.company,
            app_domain_name: pane.app_domain_name,
            agents: &[],
            id_prefix: "new-agent-channel",
            draft: pane.draft,
            spam_scan_enabled: pane.spam_scan_enabled,
            memory_ready: pane.memory_ready,
            owner: ChannelOwner::Pending {
                name: pane.agent.name,
                slug: pane.agent.slug,
            },
        }),
    )
}

/// The first step's agent, as the hidden fields the channel step submits it back in.
///
/// Prefixed because the channel form beside them owns the unprefixed `name`, `slug`,
/// `description` and `system_prompt`; `CarriedAgent` in the route module is the other half of this
/// naming and turns them back into the ordinary agent form.
fn carried_agent_fields(draft: &AgentDraft<'_>) -> String {
    let hidden = |name: &str, value: &str| {
        format!(
            r##"<input type="hidden" name="{name}" value="{value}">"##,
            value = escape_html_attr(value),
        )
    };

    [
        hidden("agent_name", draft.name),
        hidden("agent_slug", draft.slug),
        hidden("agent_system_prompt", draft.system_prompt),
        hidden("agent_description", draft.description),
        hidden("agent_provider", draft.provider),
        hidden("agent_model", draft.model),
        hidden(
            "agent_run_timeout_secs",
            &draft
                .run_timeout_secs
                .map(|seconds| seconds.to_string())
                .unwrap_or_default(),
        ),
        // An unticked checkbox submits no key at all, and that is exactly how the agent form
        // reads a disabled memory policy -- so an off switch is an absent field here too.
        if draft.memory_enabled {
            hidden("agent_memory_enabled", "true")
        } else {
            String::new()
        },
        hidden(
            "agent_memory_persistence_mode",
            draft.memory_persistence_mode,
        ),
        hidden("agent_memory_recall_mode", draft.memory_recall_mode),
        hidden(
            "agent_memory_max_results",
            &draft.memory_max_results.to_string(),
        ),
        hidden("agent_config_json", draft.config_json),
        hidden("agent_avatar_url", draft.avatar_url),
    ]
    .concat()
}

/// The Easy tab, as its two pieces: the button in the tab list and the form under it. Both are
/// empty when the library has nothing to offer, so the tab disappears rather than opening onto an
/// empty picker.
struct EasyTab {
    tab_button: String,
    form: String,
}

fn agent_easy_tab(pane: &AgentCreatePane<'_>, hidden: &str) -> EasyTab {
    let picker = agent_library_multi_select(
        pane.library_agents,
        pane.selected_library_agent_ids,
        "library_agent_ids",
    );
    if picker.is_empty() {
        return EasyTab {
            tab_button: String::new(),
            form: String::new(),
        };
    }

    EasyTab {
        tab_button: format!(
            r##"<button type="button" role="tab" id="agent-tab-easy-btn" class="tab {active}"
                        data-action="show-agent-tab" data-tab="easy">Easy</button>"##,
            active = if pane.tab == AgentCreateTab::Easy {
                "tab-active"
            } else {
                ""
            },
        ),
        form: format!(
            r##"<form id="agent-tab-easy" class="{hidden} space-y-4"
                    hx-post="/ui/agents/from-library?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    {picker}
                    <button type="submit" class="btn btn-primary">
                        <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">Create Agents</span>
                        <span class="hidden [.htmx-request_&]:inline">Creating...</span>
                    </button>
                </form>"##,
            company_id = pane.company.id,
        ),
    }
}

/// Everything about an agent except which URL its form submits to.
struct AgentFields<'a> {
    scope: AgentFormScope,
    /// The agent this form edits, `None` in the create pane. It namespaces every element id, so
    /// the create pane's two forms do not collide.
    agent_id: Option<Uuid>,
    draft: &'a AgentDraft<'a>,
    model_connections: &'a [CompanyModelConnection],
}

#[derive(Clone, Copy)]
enum AgentFormScope {
    Company(Uuid),
    Library,
}

pub fn library_agent_fields(draft: &AgentDraft<'_>, agent_id: Option<Uuid>) -> String {
    agent_fields(&AgentFields {
        scope: AgentFormScope::Library,
        agent_id,
        draft,
        model_connections: &[],
    })
}

/// The element-id namespace one form's fields share.
fn id_prefix(agent_id: Option<Uuid>) -> String {
    agent_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "new".to_string())
}

fn agent_fields(fields: &AgentFields<'_>) -> String {
    let draft = fields.draft;
    let id_prefix = id_prefix(fields.agent_id);
    let overrides_open =
        if draft.provider.is_empty() && draft.model.is_empty() && draft.config_json.is_empty() {
            ""
        } else {
            " open"
        };
    let description_help = match fields.scope {
        AgentFormScope::Company(_) => "Shown to other agents in this company",
        AgentFormScope::Library => "Shown to agents in companies that select it",
    };
    let model_connection_fields = if matches!(fields.scope, AgentFormScope::Library) {
        format!(
            r##"
        <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
            <label class="form-control w-full"><div class="label"><span class="text-xs opacity-70">Provider</span></div>
                <input id="agent-provider-{id_prefix}" name="provider" value="{provider}" class="input w-full" placeholder="Inherit company provider"></label>
            <label class="form-control w-full"><div class="label"><span class="text-xs opacity-70">Model</span></div>
                <input id="agent-model-{id_prefix}" name="model" value="{model}" class="input w-full" placeholder="Inherit company model"></label>
        </div>"##,
            provider = escape_html_attr(draft.provider),
            model = escape_html_attr(draft.model),
        )
    } else {
        company_model_selection(fields.model_connections, draft, &id_prefix)
    };
    let run_timeout_value = draft
        .run_timeout_secs
        .map(|seconds| seconds.to_string())
        .unwrap_or_default();

    format!(
        r##"
                    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Agent Name</span></div>
                            <input type="text" name="name" required value="{name}" placeholder="Support Triage"
                                data-input="slugify"
                                class="input w-full">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Handle</span></div>
                            <input type="text" name="slug" required value="{slug}" placeholder="support-triage"
                                data-input="agent-address-preview" data-address-preview="agent-address-preview"
                                class="input w-full font-mono">
                        </label>
                    </div>
                    {avatar_field}
                    <div class="form-control w-full">
                        <div class="label justify-between">
                            <span class="text-xs opacity-70">System Prompt</span>
                            <button type="button" class="btn btn-ghost btn-xs"
                                data-action="toggle-agent-prompt" data-prefix="{id_prefix}">{sparkle} Generate with AI</button>
                        </div>
                        {generator}
                        {prompt_textarea}
                    </div>
                    <details class="collapse-arrow collapse border border-base-300 bg-base-200"{overrides_open}>
                        <summary class="collapse-title text-sm font-medium">Custom model &amp; config</summary>
                        <div class="collapse-content space-y-4">
                            {model_connection_fields}
                            <label class="form-control w-full">
                                <div class="label"><span class="text-xs opacity-70">Run timeout (seconds)</span></div>
                                <input type="number" name="run_timeout_secs" min="1" max="3600"
                                    value="{run_timeout_value}" placeholder="Use deployment default"
                                    class="input w-full">
                                <div class="label"><span class="text-xs opacity-60">Leave blank to inherit AGENT_RUN_TIMEOUT_SECS.</span></div>
                            </label>
                            <div class="grid grid-cols-1 gap-4 md:grid-cols-3">
                                <label class="form-control md:col-span-3">
                                    <span class="label cursor-pointer justify-start gap-3">
                                        <input type="checkbox" name="memory_enabled" value="true" class="checkbox"{memory_enabled_checked}>
                                        <span><span class="font-medium">Enable memory for this agent</span><span class="block text-xs opacity-60">Channel grants still control which company, agent, and user scopes it may read or write.</span></span>
                                    </span>
                                </label>
                                <label class="form-control w-full">
                                    <div class="label"><span class="text-xs opacity-70">Memory persistence</span></div>
                                    <select name="memory_persistence_mode" class="select w-full">
                                        <option value="audience_only"{memory_audience_selected}>Audience only</option>
                                        <option value="scope_specific_facts"{memory_scope_selected}>Scope-specific facts</option>
                                    </select>
                                </label>
                                <label class="form-control w-full">
                                    <div class="label"><span class="text-xs opacity-70">Memory recall</span></div>
                                    <select name="memory_recall_mode" class="select w-full">
                                        <option value="fast"{memory_fast_selected}>Fast</option>
                                        <option value="thinking"{memory_thinking_selected}>Thinking</option>
                                    </select>
                                </label>
                                <label class="form-control w-full">
                                    <div class="label"><span class="text-xs opacity-70">Maximum memory results</span></div>
                                    <input name="memory_max_results" type="number" min="1" max="20" value="{memory_max_results}" class="input w-full">
                                </label>
                            </div>
                            <label class="form-control w-full">
                                <div class="label">
                                    <span class="text-xs opacity-70">Description</span>
                                    <span class="text-xs opacity-50">{description_help}</span>
                                </div>
                                <input type="text" id="agent-description-{id_prefix}" name="description" value="{description}" placeholder="Answers supplier capacity and delivery-date questions"
                                    class="input w-full text-sm">
                            </label>
                            <label class="form-control w-full">
                                <div class="label"><span class="text-xs opacity-70">Agent Config (JSON)</span></div>
                                <textarea name="config_json" rows="4" placeholder='{{ "temperature": 0.2 }}'
                                    class="textarea w-full font-mono text-xs">{config_json}</textarea>
                            </label>
                        </div>
                    </details>
        "##,
        sparkle = icon(Icon::Sparkle, BUTTON_ICON),
        generator = prompt_generator(fields.scope, fields.agent_id, &id_prefix),
        prompt_textarea =
            agent_prompt_textarea(&id_prefix, draft.system_prompt, FragmentSwap::Inline),
        name = escape_html_text(draft.name),
        slug = escape_html_text(draft.slug),
        description_help = description_help,
        description = escape_html_text(draft.description),
        config_json = escape_html_text(draft.config_json),
        avatar_field = agent_avatar_field(&id_prefix, draft),
        memory_audience_selected = if draft.memory_persistence_mode == "audience_only" {
            " selected"
        } else {
            ""
        },
        memory_scope_selected = if draft.memory_persistence_mode == "scope_specific_facts" {
            " selected"
        } else {
            ""
        },
        memory_fast_selected = if draft.memory_recall_mode == "fast" {
            " selected"
        } else {
            ""
        },
        memory_thinking_selected = if draft.memory_recall_mode == "thinking" {
            " selected"
        } else {
            ""
        },
        memory_max_results = draft.memory_max_results,
        memory_enabled_checked = if draft.memory_enabled { " checked" } else { "" },
    )
}

fn company_model_selection(
    connections: &[CompanyModelConnection],
    draft: &AgentDraft<'_>,
    id_prefix: &str,
) -> String {
    let provider_options: String = connections
        .iter()
        .map(|connection| {
            format!(
                r#"<option value="{provider}"{selected}>{provider}{default}</option>"#,
                provider = escape_html_attr(connection.provider.as_str()),
                selected = if connection.provider.as_str() == draft.provider {
                    " selected"
                } else {
                    ""
                },
                default = if connection.is_default {
                    " (default)"
                } else {
                    ""
                },
            )
        })
        .collect();
    let model_options: String = connections
        .iter()
        .flat_map(|connection| {
            connection.models.iter().map(move |model| {
                format!(
                    r#"<option value="{model}" data-provider="{provider}"{selected}>{provider} / {model}</option>"#,
                    provider = escape_html_attr(connection.provider.as_str()),
                    model = escape_html_attr(model.as_str()),
                    selected = if connection.provider.as_str() == draft.provider
                        && model.as_str() == draft.model
                    {
                        " selected"
                    } else {
                        ""
                    },
                )
            })
        })
        .collect();
    format!(
        r##"<div class="grid grid-cols-1 gap-4 md:grid-cols-2">
            <label class="form-control w-full"><div class="label"><span class="text-xs opacity-70">Provider</span></div>
                <select id="agent-provider-{id_prefix}" name="provider" class="select w-full font-mono text-sm">
                    <option value="">Inherit company default</option>{provider_options}
                </select>
            </label>
            <label class="form-control w-full"><div class="label"><span class="text-xs opacity-70">Model</span></div>
                <select id="agent-model-{id_prefix}" name="model" class="select w-full font-mono text-sm">
                    <option value="">Inherit company default</option>{model_options}
                </select>
                <div class="label"><span class="text-[11px] opacity-60">Select a model belonging to the provider; invalid pairs are refused on save.</span></div>
            </label>
        </div>"##
    )
}

/// The agent's picture field.
///
/// `id_prefix` is what keeps the two forms on the create pane -- Simple and Advanced, both
/// rendered, one hidden -- from swapping each other's field when a file is picked.
fn agent_avatar_field(id_prefix: &str, draft: &AgentDraft<'_>) -> String {
    let stored = AvatarUrl::parse(draft.avatar_url).ok().flatten();

    avatar_picker(&AvatarPicker {
        field_id: &format!("agent-avatar-{id_prefix}"),
        avatar_url: stored.as_ref(),
        name: draft.name,
        label: "Agent Picture",
        error: None,
    })
}

/// The system prompt field itself.
///
/// One renderer for the two ways it appears: in the form on a normal render, and swapped in out of
/// band when [`prompt_generator`] has just written one — so a generated prompt lands in a field
/// identical to the one it replaces.
pub fn agent_prompt_textarea(id_prefix: &str, system_prompt: &str, swap: FragmentSwap) -> String {
    format!(
        r##"<textarea id="agent-prompt-{id_prefix}" name="system_prompt" rows="10"
                            placeholder="You are a support triage agent. Read the incoming mail, decide..."
                            class="textarea w-full font-mono text-xs"{oob}>{system_prompt}</textarea>"##,
        id_prefix = escape_html_text(id_prefix),
        oob = swap.oob_attribute(),
        system_prompt = escape_html_text(system_prompt),
    )
}

/// The "Generate with AI" box: instructions in, a written system prompt out.
///
/// It cannot be a `<form>` — it sits inside the agent form already — so the button names what to
/// send with `hx-include`, pulling the model overrides along so the generation runs on whichever
/// model this agent is being pointed at.
fn prompt_generator(scope: AgentFormScope, agent_id: Option<Uuid>, id_prefix: &str) -> String {
    // The create pane has no agent to name, and says so by staying silent: the handler reads an
    // absent id as "answer into the new-agent form".
    let vals = match agent_id {
        Some(agent_id) => {
            format!(r##" hx-vals='{{"id_prefix": "{agent_id}"}}'"##)
        }
        None => String::new(),
    };

    let url = match scope {
        AgentFormScope::Company(company_id) => {
            format!("/ui/agents/generate-prompt?company_id={company_id}")
        }
        AgentFormScope::Library => "/ui/agent-library/generate-prompt".to_string(),
    };

    format!(
        r##"
                        <div id="agent-generator-{id_prefix}" class="mb-2 hidden space-y-2 rounded-box border border-base-300 bg-base-200 p-3">
                            <p class="text-xs opacity-70">Describe what this agent should do. The instructions are expanded into a full system prompt, replacing what is in the field below.</p>
                            <textarea id="agent-instructions-{id_prefix}" name="instructions" rows="3"
                                placeholder="Answer billing questions politely, and escalate anything about refunds."
                                class="textarea w-full text-xs"></textarea>
                            <div class="flex items-center gap-3">
                                <button type="button" class="btn btn-primary btn-outline btn-sm"
                                    hx-post="{url}"
                                    hx-include="#agent-instructions-{id_prefix}, #agent-provider-{id_prefix}, #agent-model-{id_prefix}, #agent-api-key-{id_prefix}"{vals}
                                    hx-target="#agent-generator-status-{id_prefix}" hx-swap="innerHTML"
                                    hx-disabled-elt="this">
                                    <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                                    <span class="[.htmx-request_&]:hidden">Generate</span>
                                    <span class="hidden [.htmx-request_&]:inline">Writing prompt...</span>
                                </button>
                                <div id="agent-generator-status-{id_prefix}" class="min-w-0 flex-1 text-xs"></div>
                            </div>
                        </div>
        "##
    )
}

/// What the generator answers with: a note in its own status line, and the written prompt swapped
/// into the form's field out of band.
///
/// The prompt travels as escaped HTML text rather than through a `<script>`, so nothing a model
/// writes — `</script>` included — can escape the field it lands in.
pub fn agent_prompt_generated(id_prefix: &str, system_prompt: &str) -> String {
    format!(
        r##"<span class="inline-flex items-center gap-1.5 text-success">{glyph} Prompt written into the field below.</span>{textarea}"##,
        glyph = icon(Icon::Check, BUTTON_ICON),
        textarea = agent_prompt_textarea(id_prefix, system_prompt, FragmentSwap::OutOfBand),
    )
}

/// Why the generator could not write a prompt, in the same status line.
pub fn agent_prompt_failed(message: &str) -> String {
    format!(
        r##"<span class="text-error">{message}</span>"##,
        message = escape_html_text(message),
    )
}

/// The channels that would lose this agent if it were deleted, each a link into its settings.
fn used_by_channels(company_id: Uuid, channels: &[&Channel]) -> String {
    if channels.is_empty() {
        return String::new();
    }

    let chips: String = channels
        .iter()
        .map(|channel| {
            format!(
                r##"<a href="/ui/channels?company_id={company_id}&channel_id={channel_id}" class="badge badge-outline badge-sm">{name}</a>"##,
                channel_id = channel.id,
                name = escape_html_text(&channel.name),
            )
        })
        .collect();

    format!(
        r##"
                <div class="mb-4 flex flex-wrap items-center gap-2 text-xs opacity-70">
                    <span>Run by</span>
                    {chips}
                </div>
        "##
    )
}

/// What deleting this agent would cost, as the confirmation dialog puts it.
fn delete_warning(agent_id: Uuid, used_by: &[&Channel]) -> String {
    let owned = used_by
        .iter()
        .filter(|channel| channel.owner_agent_id == Some(agent_id))
        .count();
    let blockers = used_by
        .iter()
        .filter(|channel| {
            channel.owner_agent_id != Some(agent_id)
                && channel.enabled
                && channel
                    .agent_ids
                    .as_deref()
                    .and_then(|ids| ids.first())
                    .is_some_and(|id| *id == agent_id)
        })
        .count();
    let detached = used_by.len().saturating_sub(owned + blockers);
    if used_by.is_empty() {
        return "No channel is running it.".to_string();
    }

    let mut effects = Vec::new();
    if owned > 0 {
        effects.push(format!(
            "{} owned personal channel{} and all of its data will be deleted",
            owned,
            if owned == 1 { "" } else { "s" }
        ));
    }
    if blockers > 0 {
        effects.push(format!(
            "{} enabled position-0 channel{} must be reassigned first",
            blockers,
            if blockers == 1 { "" } else { "s" }
        ));
    }
    if detached > 0 {
        effects.push(format!(
            "{} other channel assignment{} will be removed",
            detached,
            if detached == 1 { "" } else { "s" }
        ));
    }
    format!("{}.", effects.join("; "))
}

/// A stored agent as the form sees it.
///
/// The one field the agent does not hold as text — its config JSON — is rendered by
/// [`stored_agent_config`] and passed in, so the draft can stay a pure set of borrows.
fn stored_draft<'a>(agent: &'a Agent, config_json: &'a str) -> AgentDraft<'a> {
    AgentDraft {
        name: &agent.name,
        slug: &agent.slug,
        system_prompt: agent.system_prompt.as_deref().unwrap_or(""),
        description: agent.description.as_deref().unwrap_or(""),
        provider: agent.provider.as_deref().unwrap_or(""),
        model: agent.model.as_deref().unwrap_or(""),
        run_timeout_secs: agent.run_timeout_secs,
        memory_enabled: agent.memory_enabled,
        memory_persistence_mode: agent.memory_persistence_mode.as_str(),
        memory_recall_mode: agent.memory_recall_mode.as_str(),
        memory_max_results: agent.memory_max_results,
        config_json,
        avatar_url: agent
            .avatar_url
            .as_ref()
            .map(AvatarUrl::as_str)
            .unwrap_or(""),
        advanced: true,
    }
}

/// The agent's config as the JSON text the form submits.
pub fn stored_agent_config(agent: &Agent) -> String {
    match &agent.config_json {
        Some(config) => serde_json::to_string_pretty(config).unwrap_or_else(|_| config.to_string()),
        None => String::new(),
    }
}
