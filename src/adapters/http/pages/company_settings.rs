//! The `/ui` Companies workspace: the same shell as the mailbox, with the companies themselves in
//! the sidebar instead of one company's channels or agents.
//!
//! It is the one `/ui` workspace whose sidebar is not scoped to a company — picking an entry
//! swaps that company's settings into `#company-pane` over htmx, and every write re-renders the
//! pane with the sidebar list riding along out of band, so a rename or a delete shows up at once.

use super::*;

/// Whether the company's spam guardrail is forced on, forced off, or left to the server default.
///
/// A checkbox can only say "on" or "absent", and absent already means "server default" here — so
/// the three states are a `<select>`, and this is what its values parse into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpamGuardrail {
    ServerDefault,
    Enabled,
    Disabled,
}

impl SpamGuardrail {
    /// What the stored column means, so the pane opens on the state the company is actually in.
    pub fn from_stored(stored: Option<bool>) -> Self {
        match stored {
            None => SpamGuardrail::ServerDefault,
            Some(true) => SpamGuardrail::Enabled,
            Some(false) => SpamGuardrail::Disabled,
        }
    }

    /// The submitted `<option>` value, parsed back. Anything unrecognised is the default.
    pub fn from_form(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("true") => SpamGuardrail::Enabled,
            Some("false") => SpamGuardrail::Disabled,
            _ => SpamGuardrail::ServerDefault,
        }
    }

    /// What the use case stores: `None` leaves the company on the server's own setting.
    pub fn stored(self) -> Option<bool> {
        match self {
            SpamGuardrail::ServerDefault => None,
            SpamGuardrail::Enabled => Some(true),
            SpamGuardrail::Disabled => Some(false),
        }
    }
}

/// The company list in the sidebar — the only part of the workspace a write has to refresh.
pub struct CompanySettingsList<'a> {
    pub companies: &'a [Company],
    pub selected_company_id: Option<Uuid>,
}

/// The Companies workspace for one request.
pub struct CompanySettingsPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub list: &'a CompanySettingsList<'a>,
    /// Which company the rail's other workspaces point at.
    ///
    /// Not the same as the list's selection: opening the create form deselects the list but must
    /// not empty the rail, and a user with no company yet has nothing for it to point at.
    pub rail_company_id: Option<Uuid>,
    /// Pre-rendered right-hand pane: a company's settings, the create form, or a placeholder.
    pub pane_html: &'a str,
}

/// What a company form was last submitted with, so a rejected submit comes back filled in.
///
/// The create form and the edit form take exactly these fields, which is why they share one
/// renderer; only the URL they submit to differs.
#[derive(Debug)]
pub struct CompanyDraft<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub api_key: &'a str,
    pub spam_guardrail: SpamGuardrail,
}

impl Default for CompanyDraft<'_> {
    fn default() -> Self {
        Self {
            name: "",
            slug: "",
            provider: "",
            model: "",
            api_key: "",
            spam_guardrail: SpamGuardrail::ServerDefault,
        }
    }
}

/// The settings pane for a company that already exists.
pub struct CompanyEditPane<'a> {
    pub company: &'a Company,
    /// Domain the channel addresses are built on, e.g. `mailagents.com`.
    pub app_domain_name: &'a str,
    /// How much the company holds, for the summary above the form.
    pub counts: CompanyCounts,
    /// What the user last typed, when a save was rejected; `None` shows the stored company.
    pub draft: Option<&'a CompanyDraft<'a>>,
    pub error: Option<&'a str>,
}

/// The pane for a company that does not exist yet.
pub struct CompanyCreatePane<'a> {
    pub draft: &'a CompanyDraft<'a>,
    pub error: Option<&'a str>,
}

/// What a company currently contains, as the pane summarises it.
///
/// Both numbers are counts of the same shape, so they are named rather than passed as a pair.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompanyCounts {
    pub channels: usize,
    pub agents: usize,
}

pub fn company_settings_page(page: &CompanySettingsPage<'_>) -> String {
    let content = format!(
        r##"
        <aside class="flex w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
            <div class="border-b border-base-300 px-4 py-4">
                <h2 class="text-sm font-bold uppercase tracking-wider opacity-70">Companies</h2>
                <p class="text-[11px] opacity-60">Every channel, agent and thread lives in one.</p>
            </div>
            {list_html}
            <div class="border-t border-base-300 p-2">
                <button type="button" class="btn btn-primary btn-sm btn-block justify-start"
                    hx-get="/ui/companies/new"
                    hx-target="#company-pane" hx-swap="outerHTML"
                    hx-push-url="/ui/companies?new=1">＋ New Company</button>
            </div>
        </aside>
        {pane_html}
        "##,
        list_html = company_settings_list(page.list, FragmentSwap::Inline),
        pane_html = page.pane_html,
    );

    ui_shell(&UiShell {
        title: "Companies",
        user: page.user,
        company_id: page.rail_company_id,
        section: UiSection::Companies,
        content: &content,
        script: "",
    })
}

/// One entry per company, keyed `#company-menu` so the mailbox's selection highlighting applies
/// here unchanged.
///
/// Rendered out of band after a write, so a created, renamed or deleted company shows up without
/// the pane having to reload the whole workspace.
pub fn company_settings_list(list: &CompanySettingsList<'_>, swap: FragmentSwap) -> String {
    let entries: String = list
        .companies
        .iter()
        .map(|company| {
            company_settings_entry(company, list.selected_company_id == Some(company.id))
        })
        .collect();

    let menu_body = if list.companies.is_empty() {
        r##"<li class="px-2 py-6 text-center text-xs opacity-60">No companies yet. Create your first one below.</li>"##
            .to_string()
    } else {
        entries
    };

    format!(
        r##"<ul id="company-menu" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2"{oob}>{menu_body}</ul>"##,
        oob = swap.oob_attribute(),
    )
}

fn company_settings_entry(company: &Company, selected: bool) -> String {
    format!(
        r##"
                <li>
                    <a class="flex flex-col items-start gap-0.5 {active}"
                        hx-get="/ui/companies/{company_id}"
                        hx-target="#company-pane" hx-swap="outerHTML"
                        hx-push-url="/ui/companies?company_id={company_id}"
                        onclick="selectSidebarItem(this)">
                        <span class="w-full truncate">{name}</span>
                        <span class="w-full truncate font-mono text-[11px] opacity-60">/{slug}</span>
                    </a>
                </li>
        "##,
        active = if selected { "menu-active" } else { "" },
        company_id = company.id,
        name = escape_html_text(&company.name),
        slug = escape_html_text(&company.slug),
    )
}

/// The pane before a company is picked.
pub fn company_settings_empty_pane(message: &str, swap: FragmentSwap) -> String {
    format!(
        r##"
        <section id="company-pane" class="flex flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <p class="text-center text-sm opacity-60">{message}</p>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub fn company_edit_pane(pane: &CompanyEditPane<'_>) -> String {
    let stored = stored_draft(pane.company);
    let draft = pane.draft.unwrap_or(&stored);
    let company_id = pane.company.id;

    format!(
        r##"
        <section id="company-pane" class="flex flex-1 flex-col bg-base-100">
            <div class="flex items-start justify-between gap-3 border-b border-base-300 px-6 py-4">
                <div class="min-w-0">
                    <h2 class="truncate text-xl font-bold">{name}</h2>
                    <p class="truncate font-mono text-xs opacity-60">@{slug}.{app_domain_name} &middot; added {created_at}</p>
                </div>
                <div class="flex shrink-0 items-center gap-2">
                    <a href="/ui?company_id={company_id}" class="btn btn-ghost btn-sm">Open Mailbox</a>
                    <a href="/ui/team?company_id={company_id}" class="btn btn-outline btn-sm">Team &amp; Invites</a>
                </div>
            </div>
            <div class="flex-1 overflow-y-auto px-6 py-4">
                {error_html}
                {workspace_links}
                <form hx-put="/ui/companies/{company_id}" hx-target="#company-pane" hx-swap="outerHTML" class="space-y-4">
                    {fields}
                    <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Save Changes</span>
                            <span class="hidden [.htmx-request_&]:inline">Saving...</span>
                        </button>
                        <button type="button" class="btn btn-ghost"
                            hx-get="/ui/companies/close"
                            hx-target="#company-pane" hx-swap="outerHTML"
                            hx-push-url="/ui/companies">Cancel</button>
                        <button type="button" class="btn btn-error btn-outline ml-auto"
                            hx-delete="/ui/companies/{company_id}"
                            hx-target="#company-pane" hx-swap="outerHTML"
                            hx-confirm="Delete company '{name}'? Its channels, agents and threads go with it."
                            hx-push-url="/ui/companies">Delete Company</button>
                    </div>
                </form>
            </div>
        </section>
        "##,
        name = escape_html_text(&pane.company.name),
        slug = escape_html_text(&pane.company.slug),
        app_domain_name = escape_html_text(pane.app_domain_name),
        created_at = pane.company.created_at.format("%b %d, %Y"),
        error_html = form_error_banner(pane.error),
        workspace_links = workspace_links(company_id, pane.counts),
        fields = company_fields(draft),
    )
}

pub fn company_create_pane(pane: &CompanyCreatePane<'_>) -> String {
    format!(
        r##"
        <section id="company-pane" class="flex flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-6 py-4">
                <h2 class="text-xl font-bold">New company</h2>
                <p class="text-xs opacity-70">A company owns its channels, agents and threads, and its slug is the domain part their addresses are built on.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-6 py-4">
                {error_html}
                <form class="space-y-4" hx-post="/ui/companies" hx-target="#company-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    {fields}
                    <div class="flex items-center gap-3">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Create Company</span>
                            <span class="hidden [.htmx-request_&]:inline">Creating...</span>
                        </button>
                        <button type="button" class="btn btn-ghost"
                            hx-get="/ui/companies/close"
                            hx-target="#company-pane" hx-swap="outerHTML"
                            hx-push-url="/ui/companies">Cancel</button>
                    </div>
                </form>
            </div>
        </section>
        "##,
        error_html = form_error_banner(pane.error),
        fields = company_fields(pane.draft),
    )
}

/// Where the rest of `/ui` is for this company, with what it currently holds.
///
/// The workspace is reached from the rail too, but the rail is icons only — this says how much is
/// actually in there.
fn workspace_links(company_id: Uuid, counts: CompanyCounts) -> String {
    format!(
        r##"
                <div class="mb-4 grid grid-cols-2 gap-2 sm:grid-cols-4">
                    <a href="/ui/channels?company_id={company_id}" class="btn btn-ghost h-auto flex-col gap-0 py-2">
                        <span class="text-lg font-bold">{channels}</span>
                        <span class="text-[11px] font-normal opacity-60">Channels</span>
                    </a>
                    <a href="/ui/agents?company_id={company_id}" class="btn btn-ghost h-auto flex-col gap-0 py-2">
                        <span class="text-lg font-bold">{agents}</span>
                        <span class="text-[11px] font-normal opacity-60">Agents</span>
                    </a>
                    <a href="/ui/tasks?company_id={company_id}" class="btn btn-ghost h-auto flex-col gap-0 py-2">
                        <span class="text-lg font-bold">⚙</span>
                        <span class="text-[11px] font-normal opacity-60">Tasks</span>
                    </a>
                    <a href="/ui?company_id={company_id}" class="btn btn-ghost h-auto flex-col gap-0 py-2">
                        <span class="text-lg font-bold">✉</span>
                        <span class="text-[11px] font-normal opacity-60">Mailbox</span>
                    </a>
                </div>
        "##,
        channels = counts.channels,
        agents = counts.agents,
    )
}

/// Everything about a company except which URL its form submits to.
fn company_fields(draft: &CompanyDraft<'_>) -> String {
    let overrides_open =
        if draft.provider.is_empty() && draft.model.is_empty() && draft.api_key.is_empty() {
            ""
        } else {
            " open"
        };

    format!(
        r##"
                    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="label-text text-xs opacity-70">Company Name</span></div>
                            <input type="text" name="name" required value="{name}" placeholder="Acme Corporation"
                                oninput="this.form.slug.value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')"
                                class="input input-bordered w-full">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="label-text text-xs opacity-70">Slug (the address domain)</span></div>
                            <input type="text" name="slug" required value="{slug}" placeholder="acme-corporation"
                                class="input input-bordered w-full font-mono">
                        </label>
                    </div>
                    <label class="form-control w-full">
                        <div class="label"><span class="label-text text-xs opacity-70">LLM Spam Guardrail</span></div>
                        <select name="enable_llm_spam_guardrail" class="select select-bordered w-full">
                            <option value="" {default_selected}>Server default</option>
                            <option value="true" {enabled_selected}>Enabled for this company</option>
                            <option value="false" {disabled_selected}>Disabled for this company</option>
                        </select>
                        <div class="label"><span class="label-text-alt text-[11px] opacity-60">An extra model pass over inbound mail before an agent ever sees it.</span></div>
                    </label>
                    <details class="collapse-arrow collapse border border-base-300 bg-base-200"{overrides_open}>
                        <summary class="collapse-title text-sm font-medium">Default model &amp; key</summary>
                        <div class="collapse-content space-y-4">
                            <p class="text-[11px] opacity-60">What every channel and agent in this company falls back to when it sets nothing of its own.</p>
                            <div class="grid grid-cols-1 gap-4 md:grid-cols-3">
                                <label class="form-control w-full">
                                    <div class="label"><span class="label-text text-xs opacity-70">LLM Provider</span></div>
                                    <input type="text" name="provider" value="{provider}" placeholder="google, openai, anthropic"
                                        class="input input-bordered w-full font-mono text-sm">
                                </label>
                                <label class="form-control w-full">
                                    <div class="label"><span class="label-text text-xs opacity-70">LLM Model</span></div>
                                    <input type="text" name="model" value="{model}" placeholder="gemini-2.5-flash, gpt-4o"
                                        class="input input-bordered w-full font-mono text-sm">
                                </label>
                                <label class="form-control w-full">
                                    <div class="label"><span class="label-text text-xs opacity-70">LLM API Key</span></div>
                                    <input type="password" name="api_key" value="{api_key}" placeholder="Overrides the server key"
                                        class="input input-bordered w-full font-mono text-sm">
                                </label>
                            </div>
                        </div>
                    </details>
        "##,
        name = escape_html_text(draft.name),
        slug = escape_html_text(draft.slug),
        default_selected = selected_attr(draft.spam_guardrail, SpamGuardrail::ServerDefault),
        enabled_selected = selected_attr(draft.spam_guardrail, SpamGuardrail::Enabled),
        disabled_selected = selected_attr(draft.spam_guardrail, SpamGuardrail::Disabled),
        provider = escape_html_text(draft.provider),
        model = escape_html_text(draft.model),
        api_key = escape_html_text(draft.api_key),
    )
}

fn selected_attr(draft: SpamGuardrail, option: SpamGuardrail) -> &'static str {
    if draft == option { "selected" } else { "" }
}

/// A stored company as the form sees it.
fn stored_draft(company: &Company) -> CompanyDraft<'_> {
    CompanyDraft {
        name: &company.name,
        slug: &company.slug,
        provider: company.provider.as_deref().unwrap_or(""),
        model: company.model.as_deref().unwrap_or(""),
        api_key: company.api_key.as_deref().unwrap_or(""),
        spam_guardrail: SpamGuardrail::from_stored(company.enable_llm_spam_guardrail),
    }
}
