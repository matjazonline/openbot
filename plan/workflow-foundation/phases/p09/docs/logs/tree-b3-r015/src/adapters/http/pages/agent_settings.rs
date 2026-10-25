//! The `/ui` Agents workspace: the same shell as the mailbox and the Channels workspace, with the
//! company's agents configured rather than read.
//!
//! The sidebar picks an agent and its settings are swapped into `#agent-pane` over htmx, the way
//! picking a channel swaps `#channel-pane`. Every write re-renders the pane and sends the sidebar
//! list along out of band, so a rename or a delete shows up immediately.

use super::*;
use crate::entities::skill::Skill;

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
            if (preview) preview.textContent = '@' + (input.value || 'agent-handle');
        }

        function updateSimpleAgentAddressPreview(input) {
            var preview = document.getElementById('agent-address-preview');
            if (preview) preview.textContent = '@' + (slugifyValue(input.value) || 'agent-handle');
        }"##;

/// The agent list in the sidebar — the only part of the workspace a write has to refresh.
pub struct AgentSettingsList<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    pub agents: &'a [Agent],
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
#[derive(Debug, Clone, Default)]
pub struct AgentDraft<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    /// The written system prompt in Advanced mode; the instructions to expand in Simple mode.
    pub system_prompt: &'a str,
    /// One line on what this agent is for, shown to sibling agents by the directory tool.
    pub description: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub skill_ids: Vec<Uuid>,
    pub avatar_url: &'a str,
    /// Whether the create pane should open on the Advanced tab.
    pub advanced: bool,
}

/// The settings pane for an agent that already exists.
pub struct AgentEditPane<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    pub model_connections: &'a [CompanyModelConnection],
    pub agent: &'a Agent,
    /// What the user last typed, when a save was rejected; `None` shows the stored agent.
    pub draft: Option<&'a AgentDraft<'a>>,
    pub error: Option<&'a str>,
    pub capability_options: AgentCapabilityOptions<'a>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AgentCapabilityOptions<'a> {
    pub skills: &'a [Skill],
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
    pub capability_options: AgentCapabilityOptions<'a>,
}

pub fn agent_settings_page(page: &AgentSettingsPage<'_>) -> String {
    let company = page.list.company;
    let content = format!(
        r##"
        <aside class="ui-pane-list flex w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {header}
            {run_counts}
            {list_html}
        </aside>
        {pane_html}
        "##,
        header = sidebar_header_with_action(
            "Agents",
            "AI responders, system prompts and model overrides.",
            &format!(
                r##"<button type="button" class="btn btn-primary btn-xs shrink-0"
                    hx-get="/ui/agents/new?company_id={company_id}"
                    hx-target="#agent-pane" hx-swap="outerHTML" hx-sync="#agent-pane:replace"
                    hx-push-url="/ui/agents?company_id={company_id}&new=1">{plus_glyph} New Agent</button>"##,
                company_id = company.id,
                plus_glyph = icon(Icon::Plus, BUTTON_ICON),
            ),
        ),
        run_counts = super::run_counts::run_counts_bar(company.id),
        list_html = agent_settings_list(page.list, FragmentSwap::Inline),
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
                agent,
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

fn agent_settings_entry(company: &Company, agent: &Agent, selected: bool) -> String {
    let address = format!("@{}", agent.slug);
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
                        <span class="flex min-w-0 flex-1 flex-col items-start gap-0.5">
                            <span class="flex w-full min-w-0 flex-wrap items-center gap-2">
                                <span class="min-w-0 truncate">{name}</span>
                            </span>
                            <span class="w-full truncate font-mono text-[11px] opacity-60">{slug}</span>
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
    let address = format!("@{}", pane.agent.slug);

    format!(
        r##"
        <section id="agent-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 py-4 sm:px-6">
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div class="flex min-w-0 grow basis-48 items-center gap-3">
                        {avatar}
                        <div class="min-w-0">
                            <h2 class="truncate text-xl font-bold">{name}</h2>
                            <p class="truncate font-mono text-xs opacity-60">{address} · {model}</p>
                            <p class="truncate text-xs opacity-50">{creator}</p>
                        </div>
                    </div>
                </div>
            </div>
            {body}
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
        body = agent_settings_body(pane),
    )
}

/// The Settings tab: the agent's own form, and what deleting it would cost.
fn agent_settings_body(pane: &AgentEditPane<'_>) -> String {
    let stored = stored_draft(pane.agent);
    let draft = pane.draft.unwrap_or(&stored);
    let company_id = pane.company.id;
    let agent_id = pane.agent.id;

    format!(
        r##"
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
                {error_html}
                <form method="post" action="/ui/agents/{agent_id}?company_id={company_id}" hx-disabled-elt="find button[type='submit']" hx-put="/ui/agents/{agent_id}?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML"
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
                            hx-confirm="Delete agent '{name}'? This cannot be undone."
                            hx-push-url="/ui/agents?company_id={company_id}">Delete Agent</button>
                    </div>
                </form>
                {assistant_html}
                {workflow_cards}
            </div>
        "##,
        assistant_html = assistant_channel_card(company_id, pane.agent, None),
        workflow_cards = super::workflow_agent_cards(company_id, agent_id),
        name = escape_html_attr(&pane.agent.name),
        error_html = form_error_banner(pane.error),
        fields = agent_fields(&AgentFields {
            scope: AgentFormScope::Company(company_id),
            agent_id: Some(agent_id),
            draft,
            model_connections: pane.model_connections,
            capability_options: pane.capability_options,
        }),
    )
}

/// What a "Create assistant channel" submission produced, for its card to say.
pub enum AssistantChannelOutcome<'a> {
    Created { channel: &'a str, address: &'a str },
    Refused(&'a str),
}

/// "Create assistant channel" (p08 D2.4): a new team channel this agent answers, with every
/// message run through the chosen reply mode's workflow. The command key is fresh with every
/// render, so a double submit creates one channel.
pub fn assistant_channel_card(
    company_id: Uuid,
    agent: &Agent,
    outcome: Option<AssistantChannelOutcome<'_>>,
) -> String {
    let agent_id = agent.id;
    let banner = match outcome {
        None => String::new(),
        Some(AssistantChannelOutcome::Created { channel, address }) => format!(
            r#"<div role="status" class="alert alert-success text-sm">Created the team channel {channel} ({address}); it now answers through its workflow.</div>"#,
            channel = escape_html_text(channel),
            address = escape_html_text(address),
        ),
        Some(AssistantChannelOutcome::Refused(message)) => form_error_banner(Some(message)),
    };
    format!(
        r##"
                <section id="assistant-channel-card" class="mt-6 space-y-3 rounded-box border border-base-300 p-4">
                    <div>
                        <h3 class="font-semibold">Assistant channel</h3>
                        <p class="text-sm opacity-70">Create a team channel (never public) that this agent answers. Autonomous replies send the agent's answer directly; reviewed replies wait for your approval first.</p>
                    </div>
                    {banner}
                    <form hx-post="/ui/agents/{agent_id}/assistant-channel?company_id={company_id}"
                        hx-target="#assistant-channel-card" hx-swap="outerHTML"
                        hx-disabled-elt="find button[type='submit']" class="grid gap-3 sm:grid-cols-2">
                        <input type="hidden" name="command_key" value="{command_key}">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Channel name</span></div>
                            <input class="input w-full" name="channel_name" required maxlength="120" value="{name}">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Channel address</span></div>
                            <input class="input w-full font-mono" name="channel_slug" required maxlength="64" placeholder="{slug}-assistant">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Replies</span></div>
                            <select class="select w-full" name="reply_mode">
                                <option value="autonomous">Autonomous</option>
                                <option value="reviewed">Reviewed by me</option>
                            </select>
                        </label>
                        <div class="flex items-end">
                            <button type="submit" class="btn btn-outline">Create assistant channel</button>
                        </div>
                    </form>
                </section>
        "##,
        slug = escape_html_attr(&agent.slug),
        name = escape_html_attr(&agent.name),
        command_key = Uuid::new_v4(),
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
    // A handle, not a mail address: an agent has none of its own (p08 D4.7).
    let preview_handle = format!("@{preview_slug}");

    format!(
        r##"
        <section id="agent-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 py-4 sm:px-6">
                <h2 class="text-xl font-bold">New agent in {company_name}</h2>
                <p class="text-xs opacity-70">An agent is a system prompt plus the model that answers with it. A workflow runs it as a model step.</p>
                <p class="mt-1 font-mono text-xs text-primary"><span id="agent-address-preview">{preview_handle}</span></p>
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
                    method="post" action="/ui/agents?company_id={company_id}" hx-post="/ui/agents?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML"
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
                    <button type="submit" class="btn btn-primary">
                        <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">Create Agent</span>
                        <span class="hidden [.htmx-request_&]:inline">Creating...</span>
                    </button>
                </form>
                <form id="agent-tab-advanced" class="{advanced_hidden} space-y-4"
                    method="post" action="/ui/agents?company_id={company_id}" hx-post="/ui/agents?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML"
                    hx-params="not avatar_file" hx-disabled-elt="find button[type='submit']">
                    <input type="hidden" name="form_mode" value="advanced">
                    {fields}
                    <button type="submit" class="btn btn-primary">
                        <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">Create Agent</span>
                        <span class="hidden [.htmx-request_&]:inline">Creating...</span>
                    </button>
                </form>
            </div>
        </section>
        "##,
        company_name = escape_html_text(&pane.company.name),
        preview_handle = escape_html_text(&preview_handle),
        company_id = pane.company.id,
        error_html = form_error_banner(pane.error),
        easy_tab_button = easy_html.tab_button,
        easy_form = easy_html.form,
        simple_hidden = hidden(AgentCreateTab::Simple),
        advanced_hidden = hidden(AgentCreateTab::Advanced),
        simple_active = active(AgentCreateTab::Simple),
        advanced_active = active(AgentCreateTab::Advanced),
        name = escape_html_text(pane.draft.name),
        avatar_field = agent_avatar_field("simple", pane.draft),
        system_prompt = escape_html_text(pane.draft.system_prompt),
        fields = agent_fields(&AgentFields {
            scope: AgentFormScope::Company(pane.company.id),
            agent_id: None,
            draft: pane.draft,
            model_connections: pane.model_connections,
            capability_options: pane.capability_options,
        }),
    )
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
                    method="post" action="/ui/agents/from-library?company_id={company_id}" hx-post="/ui/agents/from-library?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML"
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
    capability_options: AgentCapabilityOptions<'a>,
}

#[derive(Clone, Copy)]
enum AgentFormScope {
    Company(Uuid),
    Library,
}

pub fn library_agent_fields(draft: &AgentDraft<'_>, agent_id: Option<Uuid>) -> String {
    library_agent_fields_with_capabilities(draft, agent_id, AgentCapabilityOptions::default())
}

pub fn library_agent_fields_with_capabilities(
    draft: &AgentDraft<'_>,
    agent_id: Option<Uuid>,
    capability_options: AgentCapabilityOptions<'_>,
) -> String {
    agent_fields(&AgentFields {
        scope: AgentFormScope::Library,
        agent_id,
        draft,
        model_connections: &[],
        capability_options,
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
        if draft.provider.is_empty() && draft.model.is_empty() && draft.skill_ids.is_empty() {
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
                        <summary class="collapse-title text-sm font-medium">Capabilities &amp; advanced options</summary>
                        <div class="collapse-content space-y-4">
                            {capabilities}
                            {model_connection_fields}
                            <label class="form-control w-full">
                                <div class="label">
                                    <span class="text-xs opacity-70">Description</span>
                                    <span class="text-xs opacity-50">{description_help}</span>
                                </div>
                                <input type="text" id="agent-description-{id_prefix}" name="description" value="{description}" placeholder="Answers supplier capacity and delivery-date questions"
                                    class="input w-full text-sm">
                            </label>

                        </div>
                    </details>
        "##,
        sparkle = icon(Icon::Sparkle, BUTTON_ICON),
        generator = prompt_generator(fields.scope, fields.agent_id, &id_prefix),
        capabilities = capability_picker(fields),
        prompt_textarea =
            agent_prompt_textarea(&id_prefix, draft.system_prompt, FragmentSwap::Inline),
        name = escape_html_text(draft.name),
        slug = escape_html_text(draft.slug),
        description_help = description_help,
        description = escape_html_text(draft.description),
        avatar_field = agent_avatar_field(&id_prefix, draft),
    )
}

fn csv_uuids(ids: &[Uuid]) -> String {
    ids.iter()
        .map(Uuid::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn capability_picker(fields: &AgentFields<'_>) -> String {
    let draft = fields.draft;
    let skills = skill_picker(fields.capability_options.skills, draft, fields.scope);

    format!(
        r##"<section class="space-y-5 rounded-box border border-base-300 bg-base-100 p-4" data-agent-capability-picker>
            <div><h3 class="font-semibold">Agent capabilities</h3><p class="text-xs opacity-60">Explicitly choose what this agent may use.</p></div>
            {skills}
        </section>"##
    )
}

fn skill_picker(skills: &[Skill], draft: &AgentDraft<'_>, scope: AgentFormScope) -> String {
    let cards = skills
        .iter()
        .filter(|skill| match scope {
            AgentFormScope::Company(company_id) => skill.company_id == Some(company_id),
            AgentFormScope::Library => skill.company_id.is_none(),
        })
        .map(|skill| {
            format!(
                r##"<label class="flex cursor-pointer items-start gap-3 rounded-box border border-base-300 p-3">
                    <input type="checkbox" class="checkbox checkbox-primary mt-0.5" value="{id}"
                        data-action="capability-multi-select"{checked}>
                    <span><span class="font-medium">{name}</span><span class="block text-xs opacity-60">{description}</span></span>
                </label>"##,
                id = skill.id,
                checked = if draft.skill_ids.contains(&skill.id) { " checked" } else { "" },
                name = escape_html_text(&skill.name),
                description = escape_html_text(&skill.description),
            )
        })
        .collect::<String>();
    let empty = if cards.is_empty() {
        r#"<p class="text-xs opacity-60">No skills are available yet.</p>"#
    } else {
        ""
    };
    format!(
        r##"<section class="space-y-2" data-capability-multi-select>
            <input type="hidden" name="skill_ids" value="{value}">
            <div class="flex items-center justify-between"><h4 class="text-sm font-semibold">Skills</h4>{browse}</div>
            <div class="grid grid-cols-1 gap-2 md:grid-cols-2">{cards}</div>{empty}</section>"##,
        value = escape_html_attr(&csv_uuids(&draft.skill_ids)),
        browse = match scope {
            AgentFormScope::Company(company_id) => format!(
                r#"<a class="btn btn-ghost btn-xs" href="/ui/companies?company_id={company_id}&amp;tab=skills">Browse library</a>"#
            ),
            AgentFormScope::Library => String::new(),
        },
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
                                    method="post" action="{url}" hx-post="{url}"
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

/// A stored agent as the form sees it.
fn stored_draft(agent: &Agent) -> AgentDraft<'_> {
    AgentDraft {
        name: &agent.name,
        slug: &agent.slug,
        system_prompt: agent.system_prompt.as_deref().unwrap_or(""),
        description: agent.description.as_deref().unwrap_or(""),
        provider: agent.provider.as_deref().unwrap_or(""),
        model: agent.model.as_deref().unwrap_or(""),
        skill_ids: Vec::new(),
        avatar_url: agent
            .avatar_url
            .as_ref()
            .map(AvatarUrl::as_str)
            .unwrap_or(""),
        advanced: true,
    }
}

#[cfg(test)]
mod capability_tests {
    use chrono::Utc;

    use super::*;
    use crate::entities::{
        creation::CreationProvenance, skill::SkillInstruction, value_objects::SkillSlug,
    };

    fn skill(company_id: Option<Uuid>, name: &str) -> Skill {
        Skill {
            id: Uuid::new_v4(),
            company_id,
            slug: SkillSlug::parse(&name.to_ascii_lowercase()).unwrap(),
            name: name.into(),
            description: format!("{name} description"),
            trigger: "When needed".into(),
            instructions: vec![SkillInstruction::Prompt {
                text: "Reply".into(),
            }],
            created_by: CreationProvenance::system(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn the_agent_form_offers_no_tool_grants_or_harness_choice() {
        let html = library_agent_fields(&AgentDraft::default(), None);
        assert!(!html.contains("granted_tool_ids"));
        assert!(!html.contains("name=\"harness_kind\""));
        assert!(!html.contains("outreach_"));
    }

    #[test]
    fn a_library_agent_picker_offers_only_global_skills() {
        let global = skill(None, "Global");
        let company = skill(Some(Uuid::new_v4()), "Company");
        let options = [global, company];
        let html = library_agent_fields_with_capabilities(
            &AgentDraft::default(),
            None,
            AgentCapabilityOptions { skills: &options },
        );
        assert!(html.contains("Global description"));
        assert!(!html.contains("Company description"));
    }
}
