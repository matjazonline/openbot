//! The `/ui` Channels workspace: the same shell as the mailbox, with the channel list configured
//! rather than read.
//!
//! The sidebar picks a channel and its settings are swapped into `#channel-pane` over htmx, the
//! way picking a thread swaps the mailbox's detail pane. Every write re-renders the pane and
//! sends the sidebar list along out of band, so a rename or a delete shows up immediately.

use super::*;
use crate::entities::{channel::PUBLIC_PARTICIPANT, schedule::ChannelSchedule};
use crate::use_cases::schedule::ScheduleBindingChoice;

/// Client-side behaviour this workspace adds on top of [`MAILBOX_SCRIPT`].
///
/// Kept out of the `format!` blocks below so its braces need no escaping.
pub(crate) const CHANNEL_SETTINGS_SCRIPT: &str = r##"        // The spam interlock only applies to a channel anyone can write to, so the confirmation
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
"##;

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
/// The create form and the edit form take exactly these fields, which is why they share one
/// renderer; only the URL they submit to differs.
#[derive(Debug)]
pub struct ChannelDraft<'a> {
    pub name: &'a str,
    /// What the channel is for, in one line. Shown to teammates whose mail bounced.
    pub description: &'a str,
    pub slug: &'a str,
    /// Extra addresses the channel answers on, as the comma-separated list the form submits.
    pub alias_slugs: &'a str,
    pub participant_emails: &'a str,
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
            description: "",
            slug: "",
            alias_slugs: "",
            participant_emails: "",
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
    pub schedules: &'a [ChannelSchedule],
    /// The workflow bindings this channel's schedules may name.
    pub schedule_bindings: &'a [ScheduleBindingChoice],
    pub spam_scan_enabled: bool,
    /// What the user last typed, when a save was rejected; `None` shows the stored channel.
    pub draft: Option<&'a ChannelDraft<'a>>,
    pub error: Option<&'a str>,
}

/// The pane for a channel that does not exist yet.
pub struct ChannelCreatePane<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    pub spam_scan_enabled: bool,
    pub draft: &'a ChannelDraft<'a>,
    pub error: Option<&'a str>,
}

pub fn channel_settings_page(page: &ChannelSettingsPage<'_>) -> String {
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
            "Channels",
            "Inbound addresses and their routing rules.",
            &format!(
                r##"<button type="button" class="btn btn-primary btn-xs shrink-0"
                    hx-get="/ui/channels/new?company_id={company_id}"
                    hx-target="#channel-pane" hx-swap="outerHTML" hx-sync="#channel-pane:replace"
                    hx-push-url="/ui/channels?company_id={company_id}&new=1">{plus_glyph} New Channel</button>"##,
                company_id = company.id,
                plus_glyph = icon(Icon::Plus, BUTTON_ICON),
            ),
        ),
        run_counts = super::run_counts::run_counts_bar(company.id),
        list_html = channel_settings_list(page.list, FragmentSwap::Inline),
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
                        <span class="flex w-full min-w-0 flex-wrap items-center gap-2">
                            <span class="min-w-0 truncate">{name}</span>{run_counts}{disabled_badge}
                        </span>
                        <span class="w-full truncate font-mono text-[11px] opacity-60">{address}</span>
                    </a>
                </li>
        "##,
        run_counts = super::run_counts_slot(super::RunCountKey::Channel(channel.id)),
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
    let participants = stored_participants(pane.channel);
    let aliases = stored_alias_slugs(pane.channel);
    let stored = stored_channel_draft(pane.channel, &participants, &aliases);
    let draft = pane.draft.unwrap_or(&stored);
    let company_id = pane.company.id;
    let channel_id = pane.channel.id;
    let delete_action = format!(
        r##"<button type="button" class="btn btn-error btn-outline ml-auto"
                            hx-delete="/ui/channels/{channel_id}?company_id={company_id}"
                            hx-target="#channel-pane" hx-swap="outerHTML"
                            hx-confirm="Delete channel '{name}'? Its threads go with it."
                            hx-push-url="/ui/channels?company_id={company_id}">Delete Channel</button>"##,
        name = escape_html_attr(&pane.channel.name),
    );

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
                        {delete_action}
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
            draft,
            spam_scan_enabled: pane.spam_scan_enabled,
        }),
        delete_action = delete_action,
        schedules_html = channel_schedules_card(&ChannelSchedulesCard {
            company_id,
            channel_id,
            schedules: pane.schedules,
            bindings: pane.schedule_bindings,
        }),
    )
}

pub fn channel_create_pane(pane: &ChannelCreatePane<'_>) -> String {
    format!(
        r##"
        <section id="channel-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 py-4 sm:px-6">
                <h2 class="text-xl font-bold">New channel in {company_name}</h2>
                <p class="text-xs opacity-70">A channel is an inbound address at <span class="font-mono">@{company_slug}.{app_domain_name}</span>. An agent answers it through a workflow: create an assistant channel from the agent's page.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
                {error_html}
                <form id="channel-create-form" class="space-y-4"
                    hx-post="/ui/channels?company_id={company_id}" hx-target="#channel-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
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
        fields = channel_fields(&ChannelFields {
            company: pane.company,
            app_domain_name: pane.app_domain_name,
            draft: pane.draft,
            spam_scan_enabled: pane.spam_scan_enabled,
        }),
    )
}

/// Everything about a channel except which URL its form submits to.
struct ChannelFields<'a> {
    company: &'a Company,
    app_domain_name: &'a str,
    draft: &'a ChannelDraft<'a>,
    spam_scan_enabled: bool,
}

fn channel_fields(fields: &ChannelFields<'_>) -> String {
    let draft = fields.draft;
    let slug_field = format!(
        r##"<label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Address (@{company_slug}.{app_domain_name})</span></div>
                            <input type="text" name="slug" required value="{slug}" placeholder="inbound-email-handler"
                                class="input w-full font-mono">
                        </label>"##,
        company_slug = escape_html_text(&fields.company.slug),
        app_domain_name = escape_html_text(fields.app_domain_name),
        slug = escape_html_attr(draft.slug),
    );
    format!(
        r##"
                    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Channel Name</span></div>
                            <input type="text" name="name" required value="{name}" placeholder="Inbound Email Handler" data-input="slugify"
                                class="input w-full">
                        </label>
                        {slug_field}
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
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Participant Emails</span></div>
                        <input type="text" name="participant_emails" value="{participant_emails}" autocomplete="off"
                            data-input="channel-spam-confirm"
                            placeholder="Leave blank for the company team, @public for anyone, or comma-separated emails"
                            class="input w-full">
                        <div class="label"><span class="text-[11px] opacity-60">Blank means the company team. Use <code class="font-mono">@public</code> to let anyone write in.</span></div>
                    </label>
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
        slug_field = slug_field,
        alias_slugs = escape_html_text(draft.alias_slugs),
        participant_emails = escape_html_text(draft.participant_emails),
        enabled_checked = if draft.enabled { " checked" } else { "" },
        add_3rd_party_checked = if draft.add_3rd_party { " checked" } else { "" },
        spam_html = spam_disabled_confirmation(fields.spam_scan_enabled, draft.is_public()),
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

/// A stored channel as its own form will accept it.
///
/// The channel's list-valued fields are rendered as form text and passed in so the draft can stay
/// a pure set of borrows.
fn stored_channel_draft<'a>(
    channel: &'a Channel,
    participant_emails: &'a str,
    alias_slugs: &'a str,
) -> ChannelDraft<'a> {
    ChannelDraft {
        name: &channel.name,
        description: channel.description.as_deref().unwrap_or(""),
        slug: &channel.slug,
        alias_slugs,
        participant_emails,
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

/// What the schedules card on a channel's settings renders from.
pub struct ChannelSchedulesCard<'a> {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub schedules: &'a [ChannelSchedule],
    /// The workflow bindings the channel's schedules may name, and the listed ones already do.
    pub bindings: &'a [ScheduleBindingChoice],
}

pub fn channel_schedules_card(card: &ChannelSchedulesCard<'_>) -> String {
    let ChannelSchedulesCard {
        company_id,
        channel_id,
        schedules,
        bindings,
    } = *card;
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
                                </div>
                                <div class="mt-1 flex flex-wrap items-center gap-2 text-xs opacity-70">
                                    <span class="font-medium text-primary">{cadence}</span>
                                    <span>•</span>
                                    <span>{next_run_info}</span>
                                    {binding_badge}
                                </div>
                            </div>
                            <div class="flex shrink-0 items-center gap-1">
                                <form hx-post="/ui/channels/{channel_id}/schedules/{schedule_id}/run-now"
                                    hx-target="#channel-schedules-card" hx-swap="outerHTML" class="inline">
                                    <input type="hidden" name="company_id" value="{company_id}">
                                    <input type="hidden" name="command_key" value="{command_key}">
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
                        {error}
                    </div>
                    "##,
                    name = escape_html_text(&s.name),
                    cadence = escape_html_text(&s.cadence_label()),
                    error = s
                        .last_error
                        .as_deref()
                        .map(|error| format!(
                            r#"<p class="rounded bg-error/10 px-2 py-1 text-xs text-error">Last trigger failed: {}</p>"#,
                            escape_html_text(error)
                        ))
                        .unwrap_or_default(),
                    command_key = Uuid::new_v4(),
                    schedule_id = s.id,
                    channel_id = channel_id,
                    company_id = company_id,
                    status_badge = status_badge,
                    next_run_info = next_run_info,
                    binding_badge = schedule_binding_row(s, bindings),
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
                    <p class="text-xs opacity-60">Start a workflow run on this channel on a recurring interval or once.</p>
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

                        {binding_field}

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
        binding_field = binding_field(bindings, None, FieldScale::Compact),
    )
}

/// The card's own wording for a schedule's workflow: the same badge the workspace sidebar
/// shows, with the separator that only belongs on a row that has one.
fn schedule_binding_row(schedule: &ChannelSchedule, bindings: &[ScheduleBindingChoice]) -> String {
    match binding_badge(schedule, bindings).as_str() {
        "" => String::new(),
        badge => format!("<span>•</span>{badge}"),
    }
}
