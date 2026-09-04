//! The `/ui` Companies workspace: the same shell as the mailbox, with the companies themselves in
//! the sidebar instead of one company's channels or agents.
//!
//! It is the one `/ui` workspace whose sidebar is not scoped to a company — picking an entry
//! swaps that company's settings into `#company-pane` over htmx, and every write re-renders the
//! pane with the sidebar list riding along out of band, so a rename or a delete shows up at once.
//!
//! The pane has two tabs: the company's own settings, and its team — see [`team_tab`], which is
//! rendered into [`CompanyPaneBody::Team`] rather than into a workspace of its own, because a
//! team only means anything as the team *of* a company.

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

/// Which half of a company's pane the request asked for.
///
/// The team used to be a workspace of its own; it is now the second tab of the company it belongs
/// to, so `?tab=` is what says which one to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompanyTab {
    #[default]
    Settings,
    Team,
}

impl CompanyTab {
    /// What `?tab=` names. Anything unrecognised is the settings the pane opens on.
    pub fn from_query(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("team") => CompanyTab::Team,
            _ => CompanyTab::Settings,
        }
    }
}

/// What the company pane holds below its tabs.
///
/// The team arrives pre-rendered rather than as its parts: it is [`team_tab`]'s own two columns,
/// and this pane's job is only to give them somewhere to sit. Holding the body rather than a
/// `tab` field beside it is also what keeps "Team is lit" and "the team is showing" the same
/// fact.
pub enum CompanyPaneBody<'a> {
    Settings,
    Team(&'a str),
}

impl CompanyPaneBody<'_> {
    fn tab(&self) -> CompanyTab {
        match self {
            CompanyPaneBody::Settings => CompanyTab::Settings,
            CompanyPaneBody::Team(_) => CompanyTab::Team,
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
    /// Which company the rail's other workspaces point at, and whose face it ends on.
    ///
    /// Not the same as the list's selection: opening the create form deselects the list but must
    /// not empty the rail, and a user with no company yet has nothing for it to point at.
    pub rail_company: Option<&'a Company>,
    /// Pre-rendered right-hand pane: a company's settings, the create form, or a placeholder.
    pub pane_html: &'a str,
}

/// What a company form was last submitted with, so a rejected submit comes back filled in.
///
/// The create form and the edit form take exactly these fields, which is why they share one
/// renderer; only the URL they submit to differs.
#[derive(Debug, Clone)]
pub struct CompanyDraft<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub spam_guardrail: SpamGuardrail,
    /// The picture as the picker is holding it, blank for the letter bubble. Kept as the
    /// submitted text so a rejected save comes back showing what was picked -- see
    /// [`company_fields`], which is where it becomes an [`AvatarUrl`] again.
    pub avatar_url: &'a str,
    pub memory_provider: &'a str,
    pub default_add_3rd_party: bool,
    pub default_participant_emails: String,
    pub default_retrieve_company_memory: bool,
    pub default_retrieve_agent_memory: bool,
    pub default_retrieve_user_memory: bool,
    pub default_persist_company_memory: bool,
    pub default_persist_agent_memory: bool,
    pub default_persist_user_memory: bool,
    pub model_connections: Vec<CompanyModelConnectionDraft>,
}

#[derive(Debug, Clone, Default)]
pub struct CompanyModelConnectionDraft {
    pub provider: String,
    pub models: String,
    pub is_default: bool,
    /// Safe metadata only. The renderer has no field capable of carrying the credential itself.
    pub has_api_key: bool,
    /// A deliberate request to remove this provider connection when the form is saved.
    pub remove: bool,
}

impl Default for CompanyDraft<'_> {
    fn default() -> Self {
        Self {
            name: "",
            slug: "",
            spam_guardrail: SpamGuardrail::ServerDefault,
            avatar_url: "",
            memory_provider: "",
            default_add_3rd_party: true,
            default_participant_emails: String::new(),
            default_retrieve_company_memory: false,
            default_retrieve_agent_memory: false,
            default_retrieve_user_memory: false,
            default_persist_company_memory: false,
            default_persist_agent_memory: false,
            default_persist_user_memory: false,
            model_connections: Vec::new(),
        }
    }
}

/// The settings pane for a company that already exists.
pub struct CompanyEditPane<'a> {
    pub company: &'a Company,
    pub model_connections: &'a [CompanyModelConnection],
    /// Domain the channel addresses are built on, e.g. `mailagents.com`.
    pub app_domain_name: &'a str,
    /// How much the company holds, for the summary above the form.
    pub counts: CompanyCounts,
    /// What the user last typed, when a save was rejected; `None` shows the stored company.
    pub draft: Option<&'a CompanyDraft<'a>>,
    pub error: Option<&'a str>,
    /// Only owners may change company-level configuration.
    pub editable: bool,
    /// The Resend panel, already rendered — see [`company_resend_api_section`]. It is written by its
    /// own requests and arrives pre-rendered for the same reason the team tab does: this pane's
    /// job is to give it somewhere to sit, not to know what a provider account is.
    pub resend_api_section: &'a str,
    /// Which tab the pane is open on, and — for the team — what it is showing.
    pub body: CompanyPaneBody<'a>,
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
        <aside class="ui-pane-list flex w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {header}
            {list_html}
            <div class="border-t border-base-300 p-2">
                <button type="button" class="btn btn-primary btn-sm btn-block justify-start"
                    hx-get="/ui/companies/new"
                    hx-target="#company-pane" hx-swap="outerHTML" hx-sync="#company-pane:replace"
                    hx-push-url="/ui/companies?new=1">{plus_glyph} New Company</button>
            </div>
        </aside>
        {pane_html}
        "##,
        header = sidebar_header("Companies", "Every channel, agent and thread lives in one."),
        plus_glyph = icon(Icon::Plus, BUTTON_ICON),
        list_html = company_settings_list(page.list, FragmentSwap::Inline),
        pane_html = page.pane_html,
    );

    ui_shell(&UiShell {
        title: "Companies",
        user: page.user,
        company: page.rail_company,
        section: UiSection::Companies,
        content: &content,
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
                    <a href="/ui/companies?company_id={company_id}"
                        class="flex flex-col items-start gap-0.5 {active}">
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
        <section id="company-pane"{PANE_SKELETON} data-pane-empty class="ui-pane-detail flex min-w-0 flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <p class="text-center text-sm opacity-60">{message}</p>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub fn company_edit_pane(pane: &CompanyEditPane<'_>) -> String {
    company_edit_pane_with_memory(pane, None, &ConfiguredMemoryProviders::default())
}

pub fn company_edit_pane_with_memory(
    pane: &CompanyEditPane<'_>,
    memory: Option<&MemoryConnection>,
    configured: &ConfiguredMemoryProviders,
) -> String {
    let company_id = pane.company.id;

    format!(
        r##"
        <section id="company-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-6 pt-4">
                <div class="flex items-start justify-between gap-3">
                    <div class="min-w-0">
                        <h2 class="truncate text-xl font-bold">{name}</h2>
                        <p class="truncate font-mono text-xs opacity-60">@{slug}.{app_domain_name} &middot; added {created_at}</p>
                    </div>
                    <a href="/ui?company_id={company_id}" class="btn btn-ghost btn-sm shrink-0">Open Mailbox</a>
                </div>
                {tabs}
            </div>
            {body}
        </section>
        "##,
        name = escape_html_text(&pane.company.name),
        slug = escape_html_text(&pane.company.slug),
        app_domain_name = escape_html_text(pane.app_domain_name),
        created_at = super::format_date(pane.company.created_at),
        tabs = company_tabs(company_id, pane.body.tab()),
        body = match pane.body {
            CompanyPaneBody::Settings => company_settings_body(pane, memory, configured),
            CompanyPaneBody::Team(html) => html.to_string(),
        },
    )
}

/// The pane's two halves, as ordinary links rather than htmx swaps.
///
/// A tab is a whole pane, and a plain URL is what makes one shareable and what the back button
/// already understands — the same reason the sidebar's own entries are links.
fn company_tabs(company_id: Uuid, tab: CompanyTab) -> String {
    format!(
        r##"
                <div role="tablist" class="tabs tabs-border -mb-px mt-3">
                    <a role="tab" class="tab {settings_active}" href="/ui/companies?company_id={company_id}">Settings</a>
                    <a role="tab" class="tab {team_active}" href="{team_url}">Team</a>
                </div>
        "##,
        settings_active = if tab == CompanyTab::Settings {
            "tab-active"
        } else {
            ""
        },
        team_active = if tab == CompanyTab::Team {
            "tab-active"
        } else {
            ""
        },
        team_url = team_url(company_id, TeamSelection::None),
    )
}

/// The Settings tab: what the company holds, and the form that changes it.
fn company_settings_body(
    pane: &CompanyEditPane<'_>,
    memory: Option<&MemoryConnection>,
    configured: &ConfiguredMemoryProviders,
) -> String {
    if !pane.editable {
        return r##"
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
                <div class="rounded-box border border-base-300 bg-base-200 p-5">
                    <h3 class="font-semibold">Company settings</h3>
                    <p class="mt-1 text-sm opacity-70">Only the company owner can edit these settings.</p>
                </div>
            </div>
            "##.to_string();
    }

    let stored = stored_draft(pane.company, pane.model_connections);
    let mut draft = pane.draft.cloned().unwrap_or(stored);
    // A rejected form contains only safe submitted metadata. Re-attach the stored/not-stored bit
    // from the server-side projection so a newly typed provider is never described as stored.
    for connection in &mut draft.model_connections {
        connection.has_api_key = pane
            .model_connections
            .iter()
            .any(|stored| stored.has_api_key && stored.provider.as_str() == connection.provider);
    }
    let company_id = pane.company.id;

    format!(
        r##"
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
                {error_html}
                {workspace_links}
                <form hx-put="/ui/companies/{company_id}" hx-target="#company-pane" hx-swap="outerHTML" class="space-y-4">
                    {fields}
                    {memory_status}
                    <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Save Changes</span>
                            <span class="hidden [.htmx-request_&]:inline">Saving...</span>
                        </button>
                        <button type="button" class="btn btn-ghost"
                            hx-get="/ui/companies/close"
                            hx-target="#company-pane" hx-swap="outerHTML" hx-sync="#company-pane:replace"
                            hx-push-url="/ui/companies">Cancel</button>
                        <button type="button" class="btn btn-error btn-outline ml-auto"
                            hx-delete="/ui/companies/{company_id}"
                            hx-target="#company-pane" hx-swap="outerHTML"
                            hx-confirm="Delete company '{name}'? Its channels, agents and threads go with it."
                            hx-push-url="/ui/companies">Delete Company</button>
                    </div>
                </form>
                {resend_api_section}
            </div>
        "##,
        name = escape_html_text(&pane.company.name),
        error_html = form_error_banner(pane.error),
        resend_api_section = pane.resend_api_section,
        workspace_links = workspace_links(company_id, pane.counts),
        fields = company_fields(&draft, configured),
        memory_status = memory_status(pane.company.id, memory, configured),
    )
}

pub fn company_create_pane(pane: &CompanyCreatePane<'_>) -> String {
    company_create_pane_with_memory(pane, &ConfiguredMemoryProviders::default())
}

pub fn company_create_pane_with_memory(
    pane: &CompanyCreatePane<'_>,
    configured: &ConfiguredMemoryProviders,
) -> String {
    format!(
        r##"
        <section id="company-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 py-4 sm:px-6">
                <h2 class="text-xl font-bold">New company</h2>
                <p class="text-xs opacity-70">A company owns its channels, agents and threads, and its slug is the domain part their addresses are built on.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
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
                            hx-target="#company-pane" hx-swap="outerHTML" hx-sync="#company-pane:replace"
                            hx-push-url="/ui/companies">Cancel</button>
                    </div>
                </form>
            </div>
        </section>
        "##,
        error_html = form_error_banner(pane.error),
        fields = company_fields(pane.draft, configured),
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
                        <span class="text-lg font-bold">{tasks_glyph}</span>
                        <span class="text-[11px] font-normal opacity-60">Tasks</span>
                    </a>
                    <a href="/ui?company_id={company_id}" class="btn btn-ghost h-auto flex-col gap-0 py-2">
                        <span class="text-lg font-bold">{mailbox_glyph}</span>
                        <span class="text-[11px] font-normal opacity-60">Mailbox</span>
                    </a>
                </div>
        "##,
        channels = counts.channels,
        agents = counts.agents,
        tasks_glyph = icon(Icon::Gear, "h-5 w-5"),
        mailbox_glyph = icon(Icon::Mail, "h-5 w-5"),
    )
}

/// Everything about a company except which URL its form submits to.
fn company_fields(draft: &CompanyDraft<'_>, configured: &ConfiguredMemoryProviders) -> String {
    let overrides_open = if draft.model_connections.is_empty() {
        ""
    } else {
        " open"
    };

    // Taken as text and parsed here rather than carried as a URL, so a tampered hidden field
    // cannot reach the `<img src>` the bubble draws.
    let picture = AvatarUrl::parse(draft.avatar_url).ok().flatten();
    let company_model_connections = company_model_connections(draft);

    format!(
        r##"
                    {picture_field}
                    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Company Name</span></div>
                            <input type="text" name="name" required value="{name}" placeholder="Acme Corporation"
                                data-input="slugify"
                                class="input w-full">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Slug (the address domain)</span></div>
                            <input type="text" name="slug" required value="{slug}" placeholder="acme-corporation"
                                class="input w-full font-mono">
                        </label>
                    </div>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">LLM Spam Guardrail</span></div>
                        <select name="enable_llm_spam_guardrail" class="select w-full">
                            <option value="" {default_selected}>Server default</option>
                            <option value="true" {enabled_selected}>Enabled for this company</option>
                            <option value="false" {disabled_selected}>Disabled for this company</option>
                        </select>
                        <div class="label"><span class="text-[11px] opacity-60">An extra model pass over inbound mail before an agent ever sees it.</span></div>
                    </label>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Long-term memory</span></div>
                        <select name="memory_provider" class="select w-full">
                            <option value="" {memory_disabled_selected}>Disabled</option>
                            {memory_provider_options}
                        </select>
                        <div class="label"><span class="text-[11px] opacity-60">Disabling suspends memory immediately but retains the provider connection and channel memory choices. Company deletion removes the remote memory. Hindsight extracts facts in the background, so a memory becomes recallable shortly after the reply rather than at once.</span></div>
                    </label>
                    <details class="collapse-arrow collapse border border-base-300 bg-base-200">
                        <summary class="collapse-title text-sm font-medium">New agent channel defaults</summary>
                        <div class="collapse-content space-y-4">
                            <label class="form-control"><span class="text-xs opacity-70">Participants / access</span>
                                <textarea name="default_participant_emails" rows="2" class="textarea w-full" placeholder="person@example.com, or @public">{default_participants}</textarea>
                                <span class="text-[11px] opacity-60">Blank is team-only. @public requires spam scanning.</span>
                            </label>
                            <label class="form-control"><span class="text-xs opacity-70">Allow trusted senders to add third parties</span>
                                <select name="default_add_3rd_party" class="select w-full"><option value="true"{default_third_party_yes}>Yes</option><option value="false"{default_third_party_no}>No</option></select>
                            </label>
                            <div class="grid grid-cols-1 gap-2 md:grid-cols-2">
                                <label class="label cursor-pointer justify-start gap-2"><input type="checkbox" name="default_retrieve_company_memory" value="true" class="checkbox checkbox-sm"{drc}><span>Read company memory</span></label>
                                <label class="label cursor-pointer justify-start gap-2"><input type="checkbox" name="default_persist_company_memory" value="true" class="checkbox checkbox-sm"{dpc}><span>Write company memory</span></label>
                                <label class="label cursor-pointer justify-start gap-2"><input type="checkbox" name="default_retrieve_agent_memory" value="true" class="checkbox checkbox-sm"{dra}><span>Read agent memory</span></label>
                                <label class="label cursor-pointer justify-start gap-2"><input type="checkbox" name="default_persist_agent_memory" value="true" class="checkbox checkbox-sm"{dpa}><span>Write agent memory</span></label>
                                <label class="label cursor-pointer justify-start gap-2"><input type="checkbox" name="default_retrieve_user_memory" value="true" class="checkbox checkbox-sm"{dru}><span>Read user memory</span></label>
                                <label class="label cursor-pointer justify-start gap-2"><input type="checkbox" name="default_persist_user_memory" value="true" class="checkbox checkbox-sm"{dpu}><span>Write user memory</span></label>
                            </div>
                            <p class="text-[11px] opacity-60">User memory includes authorized external participants and is isolated to this company. Grants may be saved before the selected provider is ready; they become effective only when infrastructure and agent policy allow them.</p>
                        </div>
                    </details>
                    <details class="collapse-arrow collapse border border-base-300 bg-base-200"{overrides_open}>
                        <summary class="collapse-title text-sm font-medium">Model providers</summary>
                        <div class="collapse-content space-y-4">
                            <p class="text-[11px] opacity-60">Enable provider credentials and the models agents may select. Leave an existing API key blank to keep it; use Remove provider to delete that credential deliberately. The default provider's first model is inherited by agents with no explicit selection.</p>
                            {company_model_connections}
                        </div>
                    </details>
        "##,
        picture_field = avatar_picker(&AvatarPicker {
            field_id: "company-avatar",
            avatar_url: picture.as_ref(),
            name: draft.name,
            label: "Company Picture",
            error: None,
        }),
        name = escape_html_text(draft.name),
        slug = escape_html_text(draft.slug),
        default_selected = selected_attr(draft.spam_guardrail, SpamGuardrail::ServerDefault),
        enabled_selected = selected_attr(draft.spam_guardrail, SpamGuardrail::Enabled),
        disabled_selected = selected_attr(draft.spam_guardrail, SpamGuardrail::Disabled),
        memory_disabled_selected = if draft.memory_provider.is_empty() {
            "selected"
        } else {
            ""
        },
        memory_provider_options = memory_provider_options(draft.memory_provider, configured),
        default_participants = escape_html_text(&draft.default_participant_emails),
        default_third_party_yes = if draft.default_add_3rd_party {
            " selected"
        } else {
            ""
        },
        default_third_party_no = if draft.default_add_3rd_party {
            ""
        } else {
            " selected"
        },
        drc = checked(draft.default_retrieve_company_memory),
        dra = checked(draft.default_retrieve_agent_memory),
        dru = checked(draft.default_retrieve_user_memory),
        dpc = checked(draft.default_persist_company_memory),
        dpa = checked(draft.default_persist_agent_memory),
        dpu = checked(draft.default_persist_user_memory),
    )
}

fn company_model_connections(draft: &CompanyDraft<'_>) -> String {
    let mut rows = draft.model_connections.clone();
    if rows.len() < crate::use_cases::company::MAX_COMPANY_MODEL_CONNECTIONS {
        rows.push(CompanyModelConnectionDraft {
            is_default: rows.is_empty(),
            ..CompanyModelConnectionDraft::default()
        });
    }
    rows.into_iter()
        .enumerate()
        .map(|(index, connection)| {
            let default_checked = if connection.is_default { " checked" } else { "" };
            let key_placeholder = if connection.has_api_key {
                "Leave blank to keep the stored key"
            } else {
                "Required for a new provider"
            };
            let unused_option = if connection.has_api_key {
                ""
            } else {
                r#"<option value="">Unused</option>"#
            };
            let remove_control = if connection.has_api_key || connection.remove {
                format!(
                    r#"<label class="label cursor-pointer gap-2 self-end pb-3 text-error"><input type="checkbox" name="connection_{index}_remove" value="true" class="checkbox checkbox-error checkbox-sm"{}><span class="text-xs">Remove provider</span></label>"#,
                    if connection.remove { " checked" } else { "" },
                )
            } else {
                String::new()
            };
            format!(
                r##"<fieldset class="rounded-box border border-base-300 p-3">
                    <div class="grid grid-cols-1 gap-3 md:grid-cols-[1fr_2fr_2fr_auto_auto]">
                        <label class="form-control"><span class="text-xs opacity-70">Provider</span>
                            <select name="connection_{index}_provider" class="select w-full font-mono text-sm">
                                {unused_option}
                                {provider_options}
                            </select>
                        </label>
                        <label class="form-control"><span class="text-xs opacity-70">Enabled models</span>
                            <input name="connection_{index}_models" value="{models}" placeholder="model-a, model-b" class="input w-full font-mono text-sm">
                        </label>
                        <label class="form-control"><span class="text-xs opacity-70">API key</span>
                            <input type="password" name="connection_{index}_api_key" value="" placeholder="{key_placeholder}" autocomplete="new-password" class="input w-full font-mono text-sm">
                        </label>
                        <label class="label cursor-pointer gap-2 self-end pb-3"><input type="radio" name="default_model_provider" value="{index}" class="radio radio-sm"{default_checked}><span class="text-xs">Default</span></label>
                        {remove_control}
                    </div>
                </fieldset>"##,
                unused_option = unused_option,
                provider_options = ["google", "openai", "anthropic", "groq"]
                    .into_iter()
                    .map(|provider| format!(
                        r#"<option value="{provider}"{}>{provider}</option>"#,
                        if connection.provider == provider { " selected" } else { "" }
                    ))
                    .collect::<String>(),
                models = escape_html_attr(&connection.models),
                key_placeholder = escape_html_attr(key_placeholder),
                remove_control = remove_control,
            )
        })
        .collect()
}

/// One option per known provider, disabled unless this deployment can actually run it. Rendering
/// from `MemoryProviderKind::ALL` is what keeps the form and the enum from drifting apart.
pub(super) fn memory_provider_options(
    selected: &str,
    configured: &ConfiguredMemoryProviders,
) -> String {
    MemoryProviderKind::ALL
        .into_iter()
        .map(|kind| {
            format!(
                r#"<option value="{value}" {selected} {disabled}>{label}</option>"#,
                value = kind.as_str(),
                selected = if selected == kind.as_str() {
                    "selected"
                } else {
                    ""
                },
                disabled = if configured.contains(kind) {
                    ""
                } else {
                    "disabled"
                },
                label = escape_html_text(kind.label()),
            )
        })
        .collect()
}

fn memory_status(
    company_id: Uuid,
    memory: Option<&MemoryConnection>,
    configured: &ConfiguredMemoryProviders,
) -> String {
    let Some(memory) = memory else {
        let detail = if configured.is_empty() {
            "Long-term memory is unavailable because this deployment configures no provider."
                .to_string()
        } else {
            "Long-term memory is disabled.".to_string()
        };
        return format!(
            r#"<div class="alert"><span>{}</span></div>"#,
            escape_html_text(&detail),
        );
    };
    let readiness = match (
        memory.readiness,
        memory.provisioning_phase,
        memory.last_error.as_deref(),
    ) {
        (MemoryConnectionReadiness::Ready, _, _) => "ready",
        (MemoryConnectionReadiness::Failed, _, Some(MEMORY_READINESS_TIMEOUT_ERROR)) => "timed out",
        (MemoryConnectionReadiness::Failed, _, _) => "provider failed",
        (_, _, Some(_)) => "retrying a provider error",
        (_, Some(MemoryProvisioningPhase::CreatePending), _) => "creating",
        (_, Some(MemoryProvisioningPhase::WaitingReady), _) => "waiting for readiness",
        _ => memory.readiness.as_str(),
    };
    let retry = if memory.readiness == MemoryConnectionReadiness::Failed
        && configured.contains(memory.provider)
    {
        format!(
            r##"<button type="button" class="btn btn-sm" hx-post="/ui/companies/{company_id}/memory/retry" hx-target="#company-pane" hx-swap="outerHTML" hx-disabled-elt="this"><span class="loading loading-spinner loading-xs hidden [.htmx-request_&]:inline-block"></span>Retry provisioning</button>"##
        )
    } else {
        String::new()
    };
    let error = memory
        .last_error
        .as_deref()
        .map(|error| {
            format!(
                r#"<p class="text-xs opacity-70">{}</p>"#,
                escape_html_text(error)
            )
        })
        .unwrap_or_default();
    format!(
        r#"<div class="alert flex items-center justify-between"><div><span class="badge badge-outline">{}: {}</span>{}</div>{}</div>"#,
        escape_html_text(memory.provider.label()),
        escape_html_text(readiness),
        error,
        retry,
    )
}

fn selected_attr(draft: SpamGuardrail, option: SpamGuardrail) -> &'static str {
    if draft == option { "selected" } else { "" }
}

fn checked(value: bool) -> &'static str {
    if value { " checked" } else { "" }
}

/// A stored company as the form sees it.
fn stored_draft<'a>(
    company: &'a Company,
    connections: &[CompanyModelConnection],
) -> CompanyDraft<'a> {
    CompanyDraft {
        name: &company.name,
        slug: &company.slug,
        spam_guardrail: SpamGuardrail::from_stored(company.enable_llm_spam_guardrail),
        avatar_url: company.avatar_url.as_deref().unwrap_or(""),
        memory_provider: company
            .memory_provider
            .map(MemoryProviderKind::as_str)
            .unwrap_or(""),
        default_add_3rd_party: company.channel_defaults.add_3rd_party,
        default_participant_emails: company
            .channel_defaults
            .participant_emails
            .as_ref()
            .map(|values| {
                values
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
        default_retrieve_company_memory: company.channel_defaults.retrieve_company_memory,
        default_retrieve_agent_memory: company.channel_defaults.retrieve_agent_memory,
        default_retrieve_user_memory: company.channel_defaults.retrieve_user_memory,
        default_persist_company_memory: company.channel_defaults.persist_company_memory,
        default_persist_agent_memory: company.channel_defaults.persist_agent_memory,
        default_persist_user_memory: company.channel_defaults.persist_user_memory,
        model_connections: connections
            .iter()
            .map(|connection| CompanyModelConnectionDraft {
                provider: connection.provider.to_string(),
                models: connection
                    .models
                    .iter()
                    .map(ModelName::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
                is_default: connection.is_default,
                has_api_key: connection.has_api_key,
                remove: false,
            })
            .collect(),
    }
}
