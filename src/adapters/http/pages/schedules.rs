use super::*;
use crate::entities::{
    agent::Agent,
    channel::Channel,
    company::Company,
    schedule::{
        ChannelSchedule, RunAsSelection, ScheduleDeliveryMode, ScheduleRun, ScheduleRunAsChoices,
        ScheduleType,
    },
};

pub struct SchedulesPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub companies: &'a [Company],
    pub company: &'a Company,
    pub schedules: &'a [ChannelSchedule],
    pub selected_schedule_id: Option<Uuid>,
    /// The team, so a row can name whoever its schedule runs as.
    pub run_as: &'a ScheduleRunAsChoices,
    pub runs_html: &'a str,
    pub pane_html: &'a str,
}

pub struct ScheduleRunsColumnProps<'a> {
    pub company_id: Uuid,
    pub schedule: &'a ChannelSchedule,
    pub channel: Option<&'a Channel>,
    pub runs: &'a [ScheduleRun],
    pub selected_thread_id: Option<Uuid>,
    pub page: usize,
    pub has_next: bool,
}

pub struct ScheduleThreadPaneProps<'a> {
    pub company_id: Uuid,
    pub schedule: &'a ChannelSchedule,
    pub channel: Option<&'a Channel>,
    pub agent: Option<&'a Agent>,
    pub thread_id: Uuid,
    pub subject: &'a str,
    pub messages: &'a [ThreadMessageView],
}

pub struct ScheduleFormPaneProps<'a> {
    pub company_id: Uuid,
    pub channels: &'a [Channel],
    pub schedule: Option<&'a ChannelSchedule>,
    pub run_as: &'a ScheduleRunAsChoices,
    pub error: Option<&'a str>,
}

pub fn schedules_page(page: &SchedulesPage<'_>) -> String {
    let company = page.company;
    let list_html = schedules_sidebar_list(
        company.id,
        page.schedules,
        page.selected_schedule_id,
        page.run_as,
        FragmentSwap::Inline,
    );

    let content = format!(
        r##"
        <aside class="ui-pane-list flex w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {header}
            {list_html}
            <div class="border-t border-base-300 p-2">
                <button type="button" class="btn btn-primary btn-sm btn-block justify-start"
                    hx-get="/ui/schedules/new?company_id={company_id}"
                    hx-target="#schedule-pane" hx-swap="outerHTML" hx-sync="#schedule-pane:replace"
                    hx-push-url="/ui/schedules?company_id={company_id}&new=1">{plus_glyph} New Schedule</button>
            </div>
        </aside>
        <div id="schedules-workspace"{empty} class="ui-pane-detail ui-split flex flex-1 min-w-0">
            {runs_html}
            {pane_html}
        </div>
        "##,
        header = sidebar_header("Schedules", "Cron triggers and automated agent runs."),
        // With no schedule resolved there is nothing on the right to show, so a phone opens on
        // the list. The create form arrives with one still selected, and is a detail worth
        // opening on.
        empty = if page.selected_schedule_id.is_none() {
            " data-pane-empty"
        } else {
            ""
        },
        list_html = list_html,
        plus_glyph = icon(Icon::Plus, BUTTON_ICON),
        company_id = company.id,
        runs_html = page.runs_html,
        pane_html = page.pane_html,
    );

    ui_shell(&UiShell {
        title: &format!("{} Schedules", company.name),
        user: page.user,
        company: Some(company),
        section: UiSection::Schedules,
        content: &content,
    })
}

pub fn schedules_sidebar_list(
    company_id: Uuid,
    schedules: &[ChannelSchedule],
    selected_id: Option<Uuid>,
    run_as: &ScheduleRunAsChoices,
    swap: FragmentSwap,
) -> String {
    let entries: String = schedules
        .iter()
        .map(|schedule| {
            let active = if selected_id == Some(schedule.id) {
                "menu-active"
            } else {
                ""
            };
            let status_badge = if schedule.enabled {
                r#"<span class="badge badge-success badge-xs"></span>"#
            } else {
                r#"<span class="badge badge-ghost badge-xs opacity-50"></span>"#
            };

            format!(
                r##"
                <li>
                    <a class="flex flex-col items-start gap-1 {active}"
                        hx-get="/ui/schedules?company_id={company_id}&schedule_id={schedule_id}"
                        hx-target="#schedules-workspace"
                        hx-sync="#schedules-workspace:replace"
                        hx-push-url="/ui/schedules?company_id={company_id}&schedule_id={schedule_id}"
                        data-action="select-sidebar-item">
                        <div class="flex w-full min-w-0 items-center justify-between gap-1">
                            <span class="min-w-0 truncate font-semibold text-sm">{name}</span>
                            {status_badge}
                        </div>
                        <div class="flex w-full items-center justify-between gap-1 text-[11px] opacity-70">
                            <span class="font-mono">{cadence}</span>
                            {run_as_badge}
                        </div>
                    </a>
                </li>
                "##,
                active = active,
                company_id = company_id,
                schedule_id = schedule.id,
                name = escape_html_text(&schedule.name),
                cadence = escape_html_text(&schedule.cadence_label()),
                status_badge = status_badge,
                run_as_badge = run_as_badge(schedule, run_as),
            )
        })
        .collect();

    let menu_body = if schedules.is_empty() {
        r#"<li class="px-2 py-6 text-center text-xs opacity-60">No schedules yet. Create your first automated schedule below.</li>"#.to_string()
    } else {
        entries
    };

    format!(
        r##"<ul id="schedules-menu" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2"{oob}>{menu_body}</ul>"##,
        oob = swap.oob_attribute(),
    )
}

/// When this schedule next runs, in its own zone -- the zone is named because a reader cannot
/// otherwise tell whose morning "09:00" is.
fn next_run_label(schedule: &ChannelSchedule) -> String {
    match (schedule.enabled, schedule.next_run_at) {
        (true, Some(next)) => format!("Next run {}", schedule.in_zone(next, "%b %d, %H:%M %Z")),
        (false, _) => match schedule.last_run_at {
            Some(last) => format!(
                "Paused — last ran {}",
                schedule.in_zone(last, "%b %d, %H:%M %Z")
            ),
            None => "Paused — never run".to_string(),
        },
        (true, None) => "Completed".to_string(),
    }
}

/// A trigger that could not be launched leaves its reason on the row; showing it here is the only
/// way an operator learns a run failed without reading the server log.
fn schedule_error_banner(schedule: &ChannelSchedule) -> String {
    match schedule.last_error.as_deref() {
        Some(error) => format!(
            r#"<p class="rounded bg-error/10 px-2 py-1 text-xs text-error" title="{full}">Last trigger failed: {short}</p>"#,
            full = escape_html_text(error),
            short = escape_html_text(&truncate_chars(error, 80)),
        ),
        None => String::new(),
    }
}

fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let kept: String = value.chars().take(limit).collect();
    format!("{kept}…")
}

/// The live owner stays mounted while SSE replaces its child column.
///
/// Replacing the element that owns `sse-connect` opens a disconnect/reconnect window in which a
/// task-completion notification can be lost. HTTP responses replace this whole region; live
/// events replace only [`schedule_runs_column_fragment`].
pub fn schedule_runs_column(props: &ScheduleRunsColumnProps<'_>, swap: FragmentSwap) -> String {
    format!(
        r##"<div id="schedule-runs-live" class="contents"{oob}
            hx-ext="sse" sse-connect="/ui/schedules/{schedule_id}/events?company_id={company_id}&page={page}"
            sse-swap="schedule-runs" hx-target="#schedule-runs-column" hx-swap="outerHTML">{column}</div>"##,
        oob = swap.oob_attribute(),
        schedule_id = props.schedule.id,
        company_id = props.company_id,
        page = props.page,
        column = schedule_runs_column_fragment(props),
    )
}

pub(crate) fn schedule_runs_column_fragment(props: &ScheduleRunsColumnProps<'_>) -> String {
    let schedule = props.schedule;
    let company_id = props.company_id;
    let channel_name = props.channel.map(|c| c.name.as_str()).unwrap_or("Channel");

    let toggle_label = if schedule.enabled { "Pause" } else { "Resume" };
    let toggle_val = if schedule.enabled { "false" } else { "true" };

    let delivery_label = match schedule.delivery_mode {
        ScheduleDeliveryMode::MailboxOnly => "Mailbox Only",
        ScheduleDeliveryMode::EmailParticipants => "Email: Participants",
        ScheduleDeliveryMode::EmailCustom => "Email: Custom",
    };

    let rows: String = if props.runs.is_empty() {
        r#"<div class="p-8 text-center text-xs opacity-60">No execution runs yet. Click "Run Now" to trigger the first run.</div>"#.to_string()
    } else {
        props
            .runs
            .iter()
            .map(|run| {
                let active = if props.selected_thread_id == Some(run.thread_id) {
                    "bg-base-300"
                } else {
                    ""
                };

                let now = Utc::now();
                let activity = run.activity(now);
                let badge = schedule_run_activity_badge(activity);

                let snippet = run
                    .latest_response
                    .as_deref()
                    .unwrap_or("Waiting for agent reply...");

                format!(
                    r##"
                    <a class="thread-row block border-b border-base-300 px-4 py-3 hover:bg-base-200 transition-colors cursor-pointer {active}"
                        hx-get="/ui/schedules/thread/{thread_id}?company_id={company_id}&schedule_id={schedule_id}"
                        hx-target="#schedule-pane" hx-swap="outerHTML"
                        hx-sync="#schedule-pane:replace"
                        data-thread-id="{thread_id}"
                        data-action="select-thread-row">
                        <div class="flex items-center justify-between gap-1">
                            <span class="text-xs font-semibold truncate text-base-content">{subject}</span>
                            {badge}
                        </div>
                        <p class="mt-1 line-clamp-2 text-xs opacity-70">{snippet}</p>
                        <div class="mt-2 flex items-center justify-between text-[11px] opacity-50 font-mono">
                            <span>{time}</span>
                            <span>{msg_count} msgs</span>
                        </div>
                    </a>
                    "##,
                    thread_id = run.thread_id,
                    company_id = company_id,
                    schedule_id = schedule.id,
                    subject = escape_html_text(&run.subject),
                    badge = badge,
                    snippet = escape_html_text(snippet),
                    time = super::format_date_time(run.created_at),
                    msg_count = run.message_count,
                    active = active,
                )
            })
            .collect()
    };

    let prev_disabled = if props.page <= 1 {
        "btn-disabled opacity-40"
    } else {
        ""
    };
    let next_disabled = if !props.has_next {
        "btn-disabled opacity-40"
    } else {
        ""
    };
    let prev_page = props.page.saturating_sub(1).max(1);
    let next_page = props.page + 1;

    format!(
        r##"
        <section id="schedule-runs-column"{PANE_SKELETON} class="ui-pane-stacked flex w-80 shrink-0 flex-col border-r border-base-300 bg-base-100">
            <div class="border-b border-base-300 p-4 space-y-2">
                <div class="flex items-start justify-between gap-2">
                    <div class="min-w-0">
                        <h2 class="truncate text-base font-bold">{schedule_name}</h2>
                        <p class="truncate text-xs opacity-60 font-mono">#{channel_name}</p>
                    </div>
                    <div class="flex items-center gap-1 shrink-0">
                        <form hx-post="/ui/schedules/{schedule_id}/run-now"
                            hx-target="#schedule-runs-live" hx-swap="outerHTML" class="inline">
                            <input type="hidden" name="company_id" value="{company_id}">
                            <input type="hidden" name="page" value="{page}">
                            <button type="submit" class="btn btn-outline btn-xs" title="Run schedule immediately">
                                Run Now
                            </button>
                        </form>
                        <form hx-post="/ui/schedules/{schedule_id}/toggle"
                            hx-target="#schedule-runs-live" hx-swap="outerHTML" class="inline">
                            <input type="hidden" name="company_id" value="{company_id}">
                            <input type="hidden" name="page" value="{page}">
                            <input type="hidden" name="enabled" value="{toggle_val}">
                            <button type="submit" class="btn btn-ghost btn-xs">
                                {toggle_label}
                            </button>
                        </form>
                        <button type="button" class="btn btn-ghost btn-xs"
                            hx-get="/ui/schedules/{schedule_id}/edit?company_id={company_id}"
                            hx-target="#schedule-pane" hx-swap="outerHTML" hx-sync="#schedule-pane:replace">
                            Edit
                        </button>
                    </div>
                </div>
                <div class="flex items-center justify-between text-xs pt-1">
                    <span class="badge badge-sm badge-neutral">{cadence}</span>
                    <span class="badge badge-sm badge-ghost opacity-70">{delivery}</span>
                </div>
                <p class="text-xs opacity-60">{next_run}</p>
                {error_banner}
            </div>

            <div id="schedule-runs-list" data-thread-list class="flex-1 overflow-y-auto">
                {rows}
            </div>

            <div class="flex items-center justify-between border-t border-base-300 px-3 py-2 text-xs">
                <button class="btn btn-ghost btn-xs {prev_disabled}"
                    hx-get="/ui/schedules?company_id={company_id}&schedule_id={schedule_id}&page={prev_page}"
                    hx-target="#schedules-workspace" hx-sync="#schedules-workspace:replace">« Newer</button>
                <span class="opacity-60 font-mono">Page {page}</span>
                <button class="btn btn-ghost btn-xs {next_disabled}"
                    hx-get="/ui/schedules?company_id={company_id}&schedule_id={schedule_id}&page={next_page}"
                    hx-target="#schedules-workspace" hx-sync="#schedules-workspace:replace">Older »</button>
            </div>
        </section>
        "##,
        schedule_id = schedule.id,
        schedule_name = escape_html_text(&schedule.name),
        channel_name = escape_html_text(channel_name),
        company_id = company_id,
        cadence = escape_html_text(&schedule.cadence_label()),
        delivery = delivery_label,
        next_run = escape_html_text(&next_run_label(schedule)),
        error_banner = schedule_error_banner(schedule),
        rows = rows,
        page = props.page,
        prev_page = prev_page,
        next_page = next_page,
        prev_disabled = prev_disabled,
        next_disabled = next_disabled,
    )
}

pub fn schedule_thread_pane(props: &ScheduleThreadPaneProps<'_>) -> String {
    let bubbles: String = props
        .messages
        .iter()
        .map(|msg| {
            message_bubble_chat(
                msg,
                props.agent,
                None,
                MessageScope {
                    company_id: props.company_id,
                    channel_id: props.channel.map(|channel| channel.id).unwrap_or_default(),
                },
            )
        })
        .collect();

    let channel_name = props.channel.map(|c| c.name.as_str()).unwrap_or("Channel");
    let channel_id = props.channel.map(|channel| channel.id).unwrap_or_default();
    // Reuse the mailbox's live thread stream. Starting after the newest rendered message closes
    // the snapshot/subscribe race without replaying bubbles already present in this pane.
    let after = props
        .messages
        .last()
        .map(|message| format!("&after={}", message.cursor()))
        .unwrap_or_default();

    format!(
        r##"
        <section id="schedule-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100" data-thread-id="{thread_id}"
            hx-ext="sse"
            sse-connect="/ui/events?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}{after}">
            <div class="flex flex-wrap items-center justify-between gap-3 border-b border-base-300 px-4 py-4 sm:px-6">
                <div class="min-w-0 grow basis-48">
                    <h2 class="truncate text-lg font-bold">{subject}</h2>
                    <p class="text-xs opacity-60">Generated by <span class="font-semibold">{schedule_name}</span> in <span class="font-mono">#{channel_name}</span></p>
                </div>
            </div>

            <div id="message-scroll" class="flex-1 overflow-y-auto px-4 py-4 sm:px-6 space-y-4"
                sse-swap="message" hx-swap="beforeend">
                {bubbles}
            </div>

            <div id="thread-activity" sse-swap="activity" hx-target="this" hx-swap="innerHTML"></div>

            <div class="border-t border-base-300 p-4">
                <form hx-post="/ui/schedules/thread/{thread_id}/reply?company_id={company_id}&schedule_id={schedule_id}"
                    hx-target="#schedule-pane" hx-swap="outerHTML" class="flex items-end gap-2">
                    <textarea name="reply_text" rows="1" required
                        placeholder="Reply to agent in this run thread (Enter to send, Shift+Enter for new line)..."
                        class="textarea textarea-sm flex-1 font-mono text-xs max-h-40"
                        data-keydown="composer" data-input="auto-grow-composer"></textarea>
                    <button type="submit" class="btn btn-primary btn-sm">Reply</button>
                </form>
            </div>
        </section>
        "##,
        thread_id = props.thread_id,
        subject = escape_html_text(props.subject),
        schedule_id = props.schedule.id,
        schedule_name = escape_html_text(&props.schedule.name),
        channel_name = escape_html_text(channel_name),
        company_id = props.company_id,
        channel_id = channel_id,
        after = after,
        bubbles = bubbles,
    )
}

/// The zones offered on the schedule form. A short list rather than all ~600 IANA names: these
/// cover the common cases, and any other valid name still round-trips if it is already stored.
const TIMEZONE_CHOICES: &[&str] = &[
    "UTC",
    "Europe/Ljubljana",
    "Europe/London",
    "Europe/Berlin",
    "Europe/Lisbon",
    "America/New_York",
    "America/Chicago",
    "America/Denver",
    "America/Los_Angeles",
    "America/Sao_Paulo",
    "Asia/Dubai",
    "Asia/Kolkata",
    "Asia/Singapore",
    "Asia/Tokyo",
    "Australia/Sydney",
];

fn timezone_options(selected: &str) -> String {
    // A stored zone outside the shortlist still has to come back selected, so it is offered too.
    let extra = (!TIMEZONE_CHOICES.contains(&selected)).then_some(selected);

    TIMEZONE_CHOICES
        .iter()
        .copied()
        .chain(extra)
        .map(|zone| {
            format!(
                r#"<option value="{value}" {selected_attr}>{label}</option>"#,
                value = escape_html_text(zone),
                label = escape_html_text(zone),
                selected_attr = if zone == selected { "selected" } else { "" },
            )
        })
        .collect()
}

/// Who a schedule runs as, in one line beside its cadence.
///
/// A run that acts as nobody says nothing: that is what every schedule did before the attribution
/// existed, so naming it would be noise on most rows.
pub(crate) fn run_as_badge(schedule: &ChannelSchedule, choices: &ScheduleRunAsChoices) -> String {
    match choices.selection(schedule.run_as_user_id) {
        RunAsSelection::System => String::new(),
        RunAsSelection::Choosable(account) | RunAsSelection::Locked(account) => format!(
            r#"<span class="truncate" title="Runs as {full}">as {label}</span>"#,
            full = escape_html_text(account.email.as_str()),
            label = escape_html_text(account.label()),
        ),
        RunAsSelection::Departed => {
            r#"<span class="text-error">member has left the team</span>"#.to_string()
        }
    }
}

/// How much room a form has for the run-as control: the channel card's compact fields, or the
/// full-width workspace form.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FieldScale {
    Compact,
    Full,
}

impl FieldScale {
    fn select_class(self) -> &'static str {
        match self {
            Self::Compact => "select select-sm w-full",
            Self::Full => "select w-full",
        }
    }

    fn input_class(self) -> &'static str {
        match self {
            Self::Compact => "input input-sm w-full",
            Self::Full => "input w-full",
        }
    }

    fn label_class(self) -> &'static str {
        match self {
            Self::Compact => "label py-1",
            Self::Full => "label",
        }
    }
}

/// The "Run as" control.
///
/// Usually a picker of the members this caller may attribute runs to. A schedule the *owner*
/// attributed to somebody else renders locked instead, resubmitting the stored id, so an admin
/// saving an unrelated edit cannot silently re-point the run at themselves — the one case where
/// leaving a value alone is allowed but choosing it is not.
pub(crate) fn run_as_field(
    choices: &ScheduleRunAsChoices,
    stored: Option<Uuid>,
    scale: FieldScale,
) -> String {
    let selection = choices.selection(stored);

    if let RunAsSelection::Locked(account) = selection {
        return format!(
            r#"
            <div class="form-control w-full">
                <div class="{label_class}">
                    <span class="text-xs opacity-70">Run as</span>
                    <span class="text-xs opacity-50">Only the company owner can change this</span>
                </div>
                <input type="hidden" name="run_as_user_id" value="{user_id}">
                <input type="text" class="{input_class}" value="{label}" disabled>
            </div>
            "#,
            label_class = scale.label_class(),
            input_class = scale.input_class(),
            user_id = account.user_id,
            label = escape_html_text(account.label()),
        );
    }

    let selected_id = match selection {
        RunAsSelection::Choosable(account) => Some(account.user_id),
        _ => None,
    };
    let options: String = choices
        .choosable()
        .map(|account| {
            format!(
                r#"<option value="{value}" {selected}>{label} ({email})</option>"#,
                value = account.user_id,
                selected = if selected_id == Some(account.user_id) {
                    "selected"
                } else {
                    ""
                },
                label = escape_html_text(account.label()),
                email = escape_html_text(account.email.as_str()),
            )
        })
        .collect();
    let note = match selection {
        RunAsSelection::Departed => {
            r#"<span class="text-xs text-error">The member this ran as has left the team; pick another to start it running again.</span>"#
        }
        _ => r#"<span class="text-xs opacity-50">Their memory, their name on the thread</span>"#,
    };

    format!(
        r#"
        <label class="form-control w-full">
            <div class="{label_class}">
                <span class="text-xs opacity-70">Run as</span>
                {note}
            </div>
            <select name="run_as_user_id" class="{select_class}">
                <option value="" {system_selected}>Nobody — an unattributed system run</option>
                {options}
            </select>
        </label>
        "#,
        label_class = scale.label_class(),
        select_class = scale.select_class(),
        system_selected = if selected_id.is_none() {
            "selected"
        } else {
            ""
        },
        note = note,
        options = options,
    )
}

/// Deleting is offered only on a stored schedule, and only from its own edit form, where the name
/// on screen is the one the confirmation names.
fn delete_schedule_button(schedule_id: Option<Uuid>, company_id: Uuid) -> String {
    match schedule_id {
        Some(id) => format!(
            r##"<button type="button" class="btn btn-ghost btn-sm text-error ml-auto"
                hx-delete="/ui/schedules/{id}"
                hx-vals='{{"company_id": "{company_id}"}}'
                hx-confirm="Delete this schedule? Runs already recorded stay in the mailbox."
                hx-target="#schedules-workspace">Delete</button>"##
        ),
        None => String::new(),
    }
}

pub fn schedule_form_pane(props: &ScheduleFormPaneProps<'_>) -> String {
    let company_id = props.company_id;
    let (is_edit, title, action_url) = match props.schedule {
        Some(s) => (
            true,
            format!("Edit Schedule: {}", s.name),
            format!("/ui/schedules/{}?company_id={}", s.id, company_id),
        ),
        None => (
            false,
            "New Automated Schedule".to_string(),
            format!("/ui/schedules?company_id={}", company_id),
        ),
    };

    let method = if is_edit { "hx-put" } else { "hx-post" };

    let name = props.schedule.map(|s| s.name.as_str()).unwrap_or("");
    let is_oneoff = props
        .schedule
        .is_some_and(|s| s.schedule_type == ScheduleType::OneOff);
    let interval_val = props
        .schedule
        .and_then(|s| s.interval_seconds)
        .unwrap_or(3600);
    let subject_template = props
        .schedule
        .map(|s| s.subject_template.as_str())
        .unwrap_or("[Scheduled Report] {{date}}");
    let prompt_template = props
        .schedule
        .map(|s| s.prompt_template.as_str())
        .unwrap_or("");
    let delivery_mode = props
        .schedule
        .map(|s| s.delivery_mode.as_str())
        .unwrap_or("mailbox_only");
    let selected_timezone = props.schedule.map(|s| s.timezone.name()).unwrap_or("UTC");
    let recipients_str = props
        .schedule
        .map(|s| {
            s.recipient_emails
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let channel_options: String = props
        .channels
        .iter()
        .map(|ch| {
            let selected = if props.schedule.is_some_and(|s| s.channel_id == ch.id) {
                "selected"
            } else {
                ""
            };
            format!(
                r#"<option value="{}" {}>#{} ({})</option>"#,
                ch.id,
                selected,
                escape_html_text(&ch.slug),
                escape_html_text(&ch.name)
            )
        })
        .collect();

    let (interval_hidden, oneoff_hidden) = if is_oneoff {
        ("hidden", "")
    } else {
        ("", "hidden")
    };
    let custom_recipients_hidden = if delivery_mode == "email_custom" {
        ""
    } else {
        "hidden"
    };

    format!(
        r##"
        <section id="schedule-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 py-4 sm:px-6">
                <h2 class="text-xl font-bold">{title}</h2>
                <p class="text-xs opacity-70">Automate your channel agents to run periodically or at an exact date and time.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6 space-y-4">
                {error_html}
                <form {method}="{action_url}" hx-target="#schedules-workspace" class="space-y-4 max-w-2xl">
                    <input type="hidden" name="company_id" value="{company_id}">
                    
                    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Schedule Name</span></div>
                            <input type="text" name="name" required value="{name}" placeholder="Daily Operations Report" class="input w-full">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Target Channel</span></div>
                            <select name="channel_id" required class="select w-full">
                                {channel_options}
                            </select>
                        </label>
                    </div>

                    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Schedule Type</span></div>
                            <select name="schedule_type" class="select w-full" data-action="toggle-schedule-type">
                                <option value="interval" {interval_selected}>Recurring Interval</option>
                                <option value="one_off" {oneoff_selected}>One-Off Scheduled Run</option>
                            </select>
                        </label>

                        <div class="schedule-interval-box {interval_hidden}">
                            <label class="form-control w-full">
                                <div class="label"><span class="text-xs opacity-70">Repeat Cadence</span></div>
                                <select name="interval_seconds" class="select w-full">
                                    <option value="900" {i900}>Every 15 minutes</option>
                                    <option value="1800" {i1800}>Every 30 minutes</option>
                                    <option value="3600" {i3600}>Every hour (1h)</option>
                                    <option value="21600" {i21600}>Every 6 hours</option>
                                    <option value="43200" {i43200}>Every 12 hours</option>
                                    <option value="86400" {i86400}>Every day (24h)</option>
                                    <option value="604800" {i604800}>Every week (7d)</option>
                                </select>
                            </label>
                        </div>

                        <div class="schedule-oneoff-box {oneoff_hidden}">
                            <label class="form-control w-full">
                                <div class="label"><span class="text-xs opacity-70">Run At (Date &amp; Time UTC)</span></div>
                                <input type="datetime-local" name="scheduled_at" class="input w-full">
                            </label>
                        </div>
                    </div>

                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Thread Subject (supports <code class="font-mono">{{date}}</code>, <code class="font-mono">{{time}}</code>)</span></div>
                        <input type="text" name="subject_template" required value="{subject_template}" class="input w-full font-mono text-xs">
                    </label>

                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Agent Prompt / Task Instructions</span></div>
                        <textarea name="prompt_template" required rows="4" placeholder="Describe the task or report the agent should generate for this scheduled run..." class="textarea w-full font-mono text-xs">{prompt_template}</textarea>
                    </label>

                    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label">
                                <span class="text-xs opacity-70">Timezone</span>
                                <span class="text-xs opacity-50">Templates and the cadence follow this zone</span>
                            </div>
                            <select name="timezone" class="select w-full">
                                {timezone_options}
                            </select>
                        </label>
                        {run_as_field}
                    </div>

                    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Output Delivery Mode</span></div>
                            <select name="delivery_mode" class="select w-full" data-action="toggle-schedule-delivery">
                                <option value="mailbox_only" {d_mailbox}>Mailbox Only (In-App Review)</option>
                                <option value="email_participants" {d_participants}>Post to Mailbox &amp; Email Participants</option>
                                <option value="email_custom" {d_custom}>Post to Mailbox &amp; Email Custom List</option>
                            </select>
                        </label>
                        <div class="schedule-custom-recipients-box {custom_recipients_hidden}">
                            <label class="form-control w-full">
                                <div class="label"><span class="text-xs opacity-70">Recipient Emails (comma-separated)</span></div>
                                <input type="text" name="recipient_emails" value="{recipients_str}" placeholder="team@company.com, client@example.com" class="input w-full">
                            </label>
                        </div>
                    </div>

                    <div class="flex items-center gap-3 pt-4 border-t border-base-300">
                        <button type="submit" class="btn btn-primary">{submit_label}</button>
                        <button type="button" class="btn btn-ghost"
                            hx-get="/ui/schedules/close?company_id={company_id}"
                            hx-target="#schedule-pane" hx-swap="outerHTML" hx-sync="#schedule-pane:replace"
                            hx-push-url="/ui/schedules?company_id={company_id}">Cancel</button>
                        {delete_button}
                    </div>
                </form>
            </div>
        </section>
        "##,
        title = title,
        method = method,
        action_url = action_url,
        company_id = company_id,
        error_html = form_error_banner(props.error),
        name = escape_html_text(name),
        channel_options = channel_options,
        interval_selected = if !is_oneoff { "selected" } else { "" },
        oneoff_selected = if is_oneoff { "selected" } else { "" },
        interval_hidden = interval_hidden,
        oneoff_hidden = oneoff_hidden,
        i900 = if interval_val == 900 { "selected" } else { "" },
        i1800 = if interval_val == 1800 { "selected" } else { "" },
        i3600 = if interval_val == 3600 { "selected" } else { "" },
        i21600 = if interval_val == 21600 {
            "selected"
        } else {
            ""
        },
        i43200 = if interval_val == 43200 {
            "selected"
        } else {
            ""
        },
        i86400 = if interval_val == 86400 {
            "selected"
        } else {
            ""
        },
        i604800 = if interval_val == 604800 {
            "selected"
        } else {
            ""
        },
        timezone_options = timezone_options(selected_timezone),
        run_as_field = run_as_field(
            props.run_as,
            props.schedule.and_then(|schedule| schedule.run_as_user_id),
            FieldScale::Full,
        ),
        subject_template = escape_html_text(subject_template),
        prompt_template = escape_html_text(prompt_template),
        d_mailbox = if delivery_mode == "mailbox_only" {
            "selected"
        } else {
            ""
        },
        d_participants = if delivery_mode == "email_participants" {
            "selected"
        } else {
            ""
        },
        d_custom = if delivery_mode == "email_custom" {
            "selected"
        } else {
            ""
        },
        custom_recipients_hidden = custom_recipients_hidden,
        recipients_str = escape_html_text(&recipients_str),
        delete_button = delete_schedule_button(props.schedule.map(|s| s.id), company_id),
        submit_label = if is_edit {
            "Save Changes"
        } else {
            "Create Schedule"
        },
    )
}

pub fn schedules_empty_pane(message: &str, swap: FragmentSwap) -> String {
    format!(
        r##"
        <section id="schedule-pane"{PANE_SKELETON} data-pane-empty class="flex min-w-0 flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <div class="text-center space-y-2">
                <p class="text-sm opacity-60">{message}</p>
            </div>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub(crate) const SCHEDULES_SCRIPT: &str = r##"        function toggleScheduleType(select) {
            var form = select.closest('form');
            if (!form) return;
            var isInterval = select.value === 'interval';
            var intervalBox = form.querySelector('.schedule-interval-box');
            var oneoffBox = form.querySelector('.schedule-oneoff-box');
            if (intervalBox) intervalBox.classList.toggle('hidden', !isInterval);
            if (oneoffBox) oneoffBox.classList.toggle('hidden', isInterval);
        }

        function toggleScheduleDelivery(select) {
            var form = select.closest('form');
            if (!form) return;
            var isCustom = select.value === 'email_custom';
            var customBox = form.querySelector('.schedule-custom-recipients-box');
            if (customBox) customBox.classList.toggle('hidden', !isCustom);
        }

        document.body.addEventListener('htmx:afterSettle', function (event) {
            if (!event.target || (event.target.id !== 'schedule-runs-column' && event.target.id !== 'schedule-runs-live' && event.target.id !== 'schedule-pane')) return;
            var pane = document.getElementById('schedule-pane');
            if (!pane || !pane.dataset.threadId) return;
            var selected = document.querySelector('#schedule-runs-list .thread-row[data-thread-id="' + pane.dataset.threadId + '"]');
            if (selected) selectThreadRow(selected);
        });"##;
