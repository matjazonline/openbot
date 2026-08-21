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
const AGENT_SETTINGS_SCRIPT: &str = r##"        function showAgentTab(advanced) {
            var simple = document.getElementById('agent-tab-simple');
            var advancedForm = document.getElementById('agent-tab-advanced');
            var simpleBtn = document.getElementById('agent-tab-simple-btn');
            var advancedBtn = document.getElementById('agent-tab-advanced-btn');
            if (simple) simple.classList.toggle('hidden', advanced);
            if (advancedForm) advancedForm.classList.toggle('hidden', !advanced);
            if (simpleBtn) simpleBtn.classList.toggle('tab-active', !advanced);
            if (advancedBtn) advancedBtn.classList.toggle('tab-active', advanced);
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
        }"##;

/// The agent list in the sidebar — the only part of the workspace a write has to refresh.
pub struct AgentSettingsList<'a> {
    pub company: &'a Company,
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
#[derive(Debug, Default)]
pub struct AgentDraft<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    /// The written system prompt in Advanced mode; the instructions to expand in Simple mode.
    pub system_prompt: &'a str,
    /// One line on what this agent is for, shown to sibling agents by the directory tool.
    pub description: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub api_key: &'a str,
    pub config_json: &'a str,
    pub avatar_url: &'a str,
    /// Whether the create pane should open on the Advanced tab.
    pub advanced: bool,
}

/// The settings pane for an agent that already exists.
pub struct AgentEditPane<'a> {
    pub company: &'a Company,
    pub agent: &'a Agent,
    /// The channels currently running this agent — nothing stops one being deleted out from
    /// under them, so the pane has to say who would notice.
    pub used_by: &'a [&'a Channel],
    /// What the user last typed, when a save was rejected; `None` shows the stored agent.
    pub draft: Option<&'a AgentDraft<'a>>,
    pub error: Option<&'a str>,
}

/// The pane for an agent that does not exist yet.
pub struct AgentCreatePane<'a> {
    pub company: &'a Company,
    pub draft: &'a AgentDraft<'a>,
    pub error: Option<&'a str>,
}

pub fn agent_settings_page(page: &AgentSettingsPage<'_>) -> String {
    let company = page.list.company;
    let content = format!(
        r##"
        <aside class="flex w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {header}
            {company_switcher}
            {list_html}
            <div class="border-t border-base-300 p-2">
                <button type="button" class="btn btn-primary btn-sm btn-block justify-start"
                    hx-get="/ui/agents/new?company_id={company_id}"
                    hx-target="#agent-pane" hx-swap="outerHTML"
                    hx-push-url="/ui/agents?company_id={company_id}&new=1">{plus_glyph} New Agent</button>
            </div>
        </aside>
        {pane_html}
        "##,
        header = sidebar_header("Agents", "AI responders, system prompts and model overrides."),
        company_switcher = company_switcher(company, page.companies, UiSection::Agents),
        plus_glyph = icon(Icon::Plus, BUTTON_ICON),
        list_html = agent_settings_list(page.list, FragmentSwap::Inline),
        company_id = company.id,
        pane_html = page.pane_html,
    );

    ui_shell(&UiShell {
        title: &format!("{} Agents", company.name),
        user: page.user,
        company_id: Some(company.id),
        section: UiSection::Agents,
        content: &content,
        script: AGENT_SETTINGS_SCRIPT,
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
                list.company.id,
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

fn agent_settings_entry(company_id: Uuid, agent: &Agent, selected: bool) -> String {
    format!(
        r##"
                <li>
                    <a class="flex items-center gap-3 {active}"
                        hx-get="/ui/agents/{agent_id}?company_id={company_id}"
                        hx-target="#agent-pane" hx-swap="outerHTML"
                        hx-push-url="/ui/agents?company_id={company_id}&agent_id={agent_id}"
                        onclick="selectSidebarItem(this)">
                        {avatar}
                        <span class="flex min-w-0 flex-col items-start gap-0.5">
                            <span class="flex w-full items-center gap-2">
                                <span class="min-w-0 truncate">{name}</span>
                                <span class="badge badge-ghost badge-sm shrink-0 font-mono">@{slug}</span>
                            </span>
                            <span class="w-full truncate font-mono text-[11px] opacity-60">{model}</span>
                        </span>
                    </a>
                </li>
        "##,
        active = if selected { "menu-active" } else { "" },
        agent_id = agent.id,
        avatar = avatar_bubble(agent.avatar_url.as_ref(), &agent.name, AvatarSize::Row),
        name = escape_html_text(&agent.name),
        slug = escape_html_text(&agent.slug),
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
        <section id="agent-pane"{PANE_SKELETON} class="flex flex-1 items-center justify-center bg-base-100 p-8"{oob}>
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

    format!(
        r##"
        <section id="agent-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100">
            <div class="flex items-start justify-between gap-3 border-b border-base-300 px-6 py-4">
                <div class="flex min-w-0 items-center gap-3">
                    {avatar}
                    <div class="min-w-0">
                        <h2 class="truncate text-xl font-bold">{name}</h2>
                        <p class="truncate font-mono text-xs opacity-60">@{slug} · {model}</p>
                    </div>
                </div>
            </div>
            <div class="flex-1 overflow-y-auto px-6 py-4">
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
                            hx-target="#agent-pane" hx-swap="outerHTML">Cancel</button>
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
        slug = escape_html_text(&pane.agent.slug),
        model = escape_html_text(&agent_model_summary(pane.agent)),
        error_html = form_error_banner(pane.error),
        used_by_html = used_by_channels(company_id, pane.used_by),
        delete_warning = delete_warning(pane.used_by),
        fields = agent_fields(&AgentFields {
            company_id,
            agent_id: Some(agent_id),
            draft,
        }),
    )
}

pub fn agent_create_pane(pane: &AgentCreatePane<'_>) -> String {
    let (simple_hidden, advanced_hidden) = if pane.draft.advanced {
        ("hidden", "")
    } else {
        ("", "hidden")
    };
    let active = |lit: bool| if lit { "tab-active" } else { "" };

    format!(
        r##"
        <section id="agent-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-6 py-4">
                <h2 class="text-xl font-bold">New agent in {company_name}</h2>
                <p class="text-xs opacity-70">An agent is a system prompt plus the model that answers with it. Channels run their agents in order.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-6 py-4">
                {error_html}
                <div role="tablist" class="tabs tabs-box mb-4 w-fit">
                    <button type="button" role="tab" id="agent-tab-simple-btn" class="tab {simple_active}"
                        onclick="showAgentTab(false)">Simple</button>
                    <button type="button" role="tab" id="agent-tab-advanced-btn" class="tab {advanced_active}"
                        onclick="showAgentTab(true)">Advanced</button>
                </div>
                <form id="agent-tab-simple" class="{simple_hidden} space-y-4"
                    hx-post="/ui/agents?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML"
                    hx-params="not avatar_file" hx-disabled-elt="find button[type='submit']">
                    <input type="hidden" name="form_mode" value="simple">
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Agent Name</span></div>
                        <input type="text" name="name" required value="{name}" placeholder="Support Triage"
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
                    hx-post="/ui/agents?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML"
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
        company_id = pane.company.id,
        error_html = form_error_banner(pane.error),
        simple_active = active(!pane.draft.advanced),
        advanced_active = active(pane.draft.advanced),
        name = escape_html_text(pane.draft.name),
        avatar_field = agent_avatar_field("simple", pane.draft),
        system_prompt = escape_html_text(pane.draft.system_prompt),
        fields = agent_fields(&AgentFields {
            company_id: pane.company.id,
            agent_id: None,
            draft: pane.draft,
        }),
    )
}

/// Everything about an agent except which URL its form submits to.
struct AgentFields<'a> {
    company_id: Uuid,
    /// The agent this form edits, `None` in the create pane. It namespaces every element id, so
    /// the create pane's two forms do not collide.
    agent_id: Option<Uuid>,
    draft: &'a AgentDraft<'a>,
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
    let overrides_open = if draft.provider.is_empty()
        && draft.model.is_empty()
        && draft.api_key.is_empty()
        && draft.config_json.is_empty()
    {
        ""
    } else {
        " open"
    };

    format!(
        r##"
                    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Agent Name</span></div>
                            <input type="text" name="name" required value="{name}" placeholder="Support Triage"
                                oninput="this.form.slug.value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')"
                                class="input w-full">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Handle</span></div>
                            <input type="text" name="slug" required value="{slug}" placeholder="support-triage"
                                class="input w-full font-mono">
                        </label>
                    </div>
                    {avatar_field}
                    <div class="form-control w-full">
                        <div class="label justify-between">
                            <span class="text-xs opacity-70">System Prompt</span>
                            <button type="button" class="btn btn-ghost btn-xs"
                                onclick="toggleAgentPromptGenerator('{id_prefix}')">{sparkle} Generate with AI</button>
                        </div>
                        {generator}
                        {prompt_textarea}
                    </div>
                    <details class="collapse-arrow collapse border border-base-300 bg-base-200"{overrides_open}>
                        <summary class="collapse-title text-sm font-medium">Custom model &amp; config</summary>
                        <div class="collapse-content space-y-4">
                            <div class="grid grid-cols-1 gap-4 md:grid-cols-3">
                                <label class="form-control w-full">
                                    <div class="label"><span class="text-xs opacity-70">LLM Provider</span></div>
                                    <input type="text" id="agent-provider-{id_prefix}" name="provider" value="{provider}" placeholder="google, openai, anthropic"
                                        class="input w-full font-mono text-sm">
                                </label>
                                <label class="form-control w-full">
                                    <div class="label"><span class="text-xs opacity-70">LLM Model</span></div>
                                    <input type="text" id="agent-model-{id_prefix}" name="model" value="{model}" placeholder="gemini-2.5-flash, gpt-4o"
                                        class="input w-full font-mono text-sm">
                                </label>
                                <label class="form-control w-full">
                                    <div class="label"><span class="text-xs opacity-70">LLM API Key</span></div>
                                    <input type="password" id="agent-api-key-{id_prefix}" name="api_key" value="{api_key}" placeholder="Overrides the company key"
                                        class="input w-full font-mono text-sm">
                                </label>
                            </div>
                            <label class="form-control w-full">
                                <div class="label">
                                    <span class="text-xs opacity-70">Description</span>
                                    <span class="text-xs opacity-50">Shown to other agents in this company</span>
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
        generator = prompt_generator(fields.company_id, fields.agent_id, &id_prefix),
        prompt_textarea =
            agent_prompt_textarea(&id_prefix, draft.system_prompt, FragmentSwap::Inline),
        name = escape_html_text(draft.name),
        slug = escape_html_text(draft.slug),
        provider = escape_html_text(draft.provider),
        model = escape_html_text(draft.model),
        api_key = escape_html_text(draft.api_key),
        description = escape_html_text(draft.description),
        config_json = escape_html_text(draft.config_json),
        avatar_field = agent_avatar_field(&id_prefix, draft),
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
fn prompt_generator(company_id: Uuid, agent_id: Option<Uuid>, id_prefix: &str) -> String {
    // The create pane has no agent to name, and says so by staying silent: the handler reads an
    // absent id as "answer into the new-agent form".
    let vals = match agent_id {
        Some(agent_id) => {
            format!(r##" hx-vals='{{"id_prefix": "{agent_id}"}}'"##)
        }
        None => String::new(),
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
                                    hx-post="/ui/agents/generate-prompt?company_id={company_id}"
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
fn delete_warning(used_by: &[&Channel]) -> String {
    match used_by.len() {
        0 => "No channel is running it.".to_string(),
        1 => "1 channel is running it and will stop.".to_string(),
        count => format!("{count} channels are running it and will stop."),
    }
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
        api_key: agent.api_key.as_deref().unwrap_or(""),
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
