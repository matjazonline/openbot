//! The reply-handling settings page: whether outside replies run the agent or wait for the team.
//!
//! Not to be confused with `manual_handoffs`, whose pages live in [`super::attention`].

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::entities::{
    task::{TaskOwner, TaskOwnerCandidate},
    thread_handoff::{ExternalReplyHandling, ExternalReplyHandlingPolicy},
    transport::PrincipalId,
};

use super::{
    ThreadHandoff, ThreadHandoffDraft, ThreadHandoffMark, ThreadHandoffState,
    attention::{age_label, due_label, priority_class},
    base_layout, escape_html_attr, escape_html_text,
};

/// What a reader calls each policy, as distinct from the `as_str` the database stores.
const fn policy_label(policy: ExternalReplyHandling) -> &'static str {
    match policy {
        ExternalReplyHandling::Automatic => "Automatic",
        ExternalReplyHandling::ManualHandoff => "Manual handoff",
    }
}

/// The one sentence that says what choosing this does to a customer's reply.
const fn policy_consequence(policy: ExternalReplyHandling) -> &'static str {
    match policy {
        ExternalReplyHandling::Automatic => "Outside replies run the channel's agent immediately.",
        ExternalReplyHandling::ManualHandoff => {
            "Outside replies to existing threads are filed and wait for the team."
        }
    }
}

/// The effective policy *and where it came from*, which is the support question this page answers.
fn effective_description(policy: &ExternalReplyHandlingPolicy) -> String {
    let provenance = if policy.is_inherited() {
        "inherited from company"
    } else {
        "channel override"
    };
    format!("{} ({provenance})", policy_label(policy.effective()))
}

fn selected(value: bool) -> &'static str {
    if value { "selected" } else { "" }
}

fn policy_option(value: ExternalReplyHandling, is_selected: bool) -> String {
    format!(
        r#"<option value="{value}" {selected}>{label} — {consequence}</option>"#,
        value = escape_html_attr(value.as_str()),
        selected = selected(is_selected),
        label = escape_html_text(policy_label(value)),
        consequence = escape_html_text(policy_consequence(value)),
    )
}

pub fn reply_handling_policy_page(
    company_id: uuid::Uuid,
    channel_id: uuid::Uuid,
    policy: &ExternalReplyHandlingPolicy,
) -> String {
    let company_options = [
        ExternalReplyHandling::Automatic,
        ExternalReplyHandling::ManualHandoff,
    ]
    .into_iter()
    .map(|value| policy_option(value, policy.company_default == value))
    .collect::<Vec<_>>()
    .join("");
    let channel_options = [
        ExternalReplyHandling::Automatic,
        ExternalReplyHandling::ManualHandoff,
    ]
    .into_iter()
    .map(|value| policy_option(value, policy.channel_override == Some(value)))
    .collect::<Vec<_>>()
    .join("");
    let body = format!(
        r#"<header class="mb-6"><h1 class="text-2xl font-bold">Reply handling policy</h1>
           <p class="text-slate-400">Effective policy: {effective}</p></header>
           <div id="policy-result"></div>
           <form method="post" action="/ui/reply-handling/company" class="mb-5 space-y-3 rounded-xl border border-slate-700 p-4">
             <input type="hidden" name="company_id" value="{company_id}">
             <input type="hidden" name="channel_id" value="{channel_id}">
             <label class="block font-semibold">Company default</label>
             <select name="policy" class="rounded-lg bg-slate-950 p-2">
               {company_options}
             </select>
             <button class="rounded-lg bg-indigo-700 px-4 py-2" type="submit">Save company default</button>
           </form>
           <form method="post" action="/ui/reply-handling/channel" class="space-y-3 rounded-xl border border-slate-700 p-4">
             <input type="hidden" name="company_id" value="{company_id}">
             <input type="hidden" name="channel_id" value="{channel_id}">
             <label class="block font-semibold">Channel override</label>
             <select name="policy_override" class="rounded-lg bg-slate-950 p-2">
               <option value="inherit" {inherit}>Inherit company default — this channel follows the company as it changes.</option>
               {channel_options}
             </select>
             <button class="rounded-lg bg-indigo-700 px-4 py-2" type="submit">Save channel override</button>
           </form>"#,
        effective = escape_html_text(&effective_description(policy)),
        company_id = escape_html_attr(&company_id.to_string()),
        channel_id = escape_html_attr(&channel_id.to_string()),
        inherit = selected(policy.is_inherited()),
    );
    base_layout("Reply handling policy", &body)
}

pub fn reply_handling_policy_saved(company_id: uuid::Uuid, channel_id: uuid::Uuid) -> String {
    format!(
        r#"<div class="rounded-xl border border-emerald-700 p-4">Reply handling saved.
        <a class="ml-2 underline" href="/ui/reply-handling?company_id={company_id}&amp;channel_id={channel_id}">Reply handling settings</a></div>"#,
        company_id = escape_html_attr(&company_id.to_string()),
        channel_id = escape_html_attr(&channel_id.to_string()),
    )
}

/// What the four buttons do, served from `/assets/app.js` like every other behaviour here.
///
/// Not an inline `<script>` and not an `onclick`: the page CSP blocks both, so either would be
/// markup that never runs. The button carries its request in `data-*` attributes and the delegated
/// click dispatcher in [`super::mailbox`] calls this with the control that was pressed.
pub(crate) const THREAD_HANDOFF_SCRIPT: &str = r##"
function postThreadHandoff(control) {
    var row = control.closest('[data-handoff-base]');
    if (!row) return;
    var action = control.dataset.handoffAction;
    var body = {
        command_id: crypto.randomUUID(),
        expected_version: Number(row.dataset.handoffVersion),
        expected_generation: row.dataset.handoffGeneration,
    };
    // Claim and reassign are the one responsibility route rather than an action of their own, and
    // they carry the whole attribute pair so a claim cannot silently drop a priority or a due time
    // somebody else set.
    var path = '/' + action;
    if (action === 'claim' || action === 'reassign') {
        path = '';
        body.priority = row.dataset.handoffPriority;
        body.due_at = row.dataset.handoffDueAt || null;
        body.operation = { kind: action };
        if (action === 'reassign') {
            var picker = row.querySelector('[data-handoff-reassign-to]');
            if (!picker || !picker.value) return;
            body.operation.to = picker.value;
        }
    }
    if (action === 'draft') {
        var form = document.getElementById('ask-agent-form');
        body.note_ids = form
            ? Array.from(form.querySelectorAll('input[name="note_ids"]:checked')).map(function (input) { return input.value; })
            : [];
    }
    if (action === 'send') {
        body.draft_version = Number(row.dataset.handoffDraftVersion);
    }
    control.disabled = true;
    fetch(row.dataset.handoffBase + path, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
    }).then(function (response) {
        if (!response.ok) return response.text().then(function (text) { throw new Error(text); });
        if (window.htmx) {
            window.htmx.ajax('GET', row.dataset.handoffPane, { target: '#detail-pane', swap: 'outerHTML' });
        }
    }).catch(function (error) {
        control.disabled = false;
        var slot = document.getElementById('thread-handoff-error');
        if (slot) slot.textContent = String(error.message || error);
    });
}
"##;

/// One button in the handoff action row.
///
/// Every value the request needs lives on the row, so the button carries only which action it is.
fn handoff_button(label: &str, action: &str, style: &str) -> String {
    format!(
        r#"<button type="button" class="btn btn-sm {style}"
            data-action="thread-handoff" data-handoff-action="{action}">{label}</button>"#,
        style = escape_html_attr(style),
        action = escape_html_attr(action),
        label = escape_html_text(label),
    )
}

/// What the open thread says about its held customer reply.
///
/// Takes the entity rather than a flattened view model on purpose: every button carries the
/// handoff's own `expected_version` and `expected_generation`, so a button rendered from a stale
/// page fails loudly instead of acting on the generation that replaced it. That is the
/// user-visible half of the whole fencing design.
pub struct ThreadHandoffBannerView<'a> {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub handoff: &'a ThreadHandoff,
    /// Who is responsible, already resolved to a label -- `None` renders "Channel team".
    pub responsible_label: Option<&'a str>,
    /// The signed-in reader, for the claim/act decision.
    pub viewer_principal_id: Option<PrincipalId>,
    pub viewer_manages_tasks: bool,
    /// The draft this generation has, when its run produced one.
    pub draft: Option<ThreadHandoffDraft>,
    /// Who a manager may hand this reply to. Empty for everybody else, and empty is a valid
    /// answer: a channel with one member has nobody to reassign to.
    pub reassign_candidates: &'a [TaskOwnerCandidate],
    pub as_of: DateTime<Utc>,
    pub error: Option<&'a str>,
}

/// The fallback the attention projection uses for `responsible_principal_id IS NULL`.
///
/// Shared with `src/adapters/persistence/attention.rs`'s `COALESCE(..., 'Channel team')` by
/// agreement rather than by type, because that one is SQL. Two different words for one state is
/// how a support conversation goes wrong.
const CHANNEL_TEAM: &str = "Channel team";

/// The handoff banner for an open thread: what state it is in, who owns it, how long it has
/// waited, and what can be done about it.
///
/// This is the **only** function producing the action row -- Phase 4's `thread_handoff_actions`
/// moved in here whole. There must not be a second set of these buttons anywhere.
///
/// Two conditional behaviours, each one a decision:
///
/// - **Unclaimed**: the primary button is Claim and `Generate draft` is not rendered at all,
///   because the drafting command refuses an unclaimed handoff and a button that always fails is
///   worse than no button. A one-line hint says why.
/// - **Claimed by somebody else**, viewer is not a manager: read-only -- state, responsibility,
///   age, priority, due -- and no buttons, because every command would answer `NotFound`. A
///   manager additionally gets Reassign. **Send is offered only to the assigned reviewer**, which
///   for a handoff draft is the responsible principal fixed at draft time: a manager who is not
///   that person -- the company owner included -- sees that the draft exists but gets no Send
///   button, because the review machinery would answer them `NotFound`.
pub fn thread_handoff_banner(view: &ThreadHandoffBannerView<'_>) -> String {
    let handoff = view.handoff;
    let mark = ThreadHandoffMark::of(handoff);
    let base = format!(
        "/api/companies/{}/thread-handoffs/{}",
        view.company_id, handoff.id
    );
    let pane_href = format!(
        "/ui/messages?company_id={}&channel_id={}&thread_id={}",
        view.company_id, view.channel_id, view.thread_id
    );

    format!(
        r#"<section id="thread-handoff-actions" class="border-t border-warning/40 px-4 py-3 sm:px-6"
            data-handoff-id="{handoff_id}" data-handoff-state="{state}">
            <p class="mb-2 text-xs font-semibold uppercase opacity-70">Held customer reply</p>
            <div class="mb-2 flex flex-wrap items-center gap-2 text-xs">
                <span class="badge badge-xs {state_class}">{state_label}</span>
                <span class="badge badge-xs {priority_class}">{priority}</span>
                <span class="opacity-70">{responsible} · {age} · {due}</span>
            </div>
            <div class="flex flex-wrap items-center gap-2" data-handoff-base="{base}"
                data-handoff-pane="{pane_href}" data-handoff-version="{version}"
                data-handoff-generation="{generation}" data-handoff-priority="{priority_value}"
                data-handoff-due-at="{due_value}"{draft_version}>{actions}</div>
            <p id="thread-handoff-error" class="mt-2 text-xs text-error">{error}</p>
        </section>"#,
        handoff_id = escape_html_attr(&handoff.id.to_string()),
        state = escape_html_attr(handoff.state.as_str()),
        state_class = mark.badge_class(),
        state_label = escape_html_text(mark.label()),
        priority_class = priority_class(handoff.priority),
        priority = escape_html_text(&handoff.priority.to_string()),
        responsible = escape_html_text(view.responsible_label.unwrap_or(
            // "Channel team" is reserved for `responsible_principal_id IS NULL`. A claimed handoff
            // whose principal no longer resolves to a name is still claimed, and saying otherwise
            // would tell the reader this reply is theirs to take.
            if handoff.responsible_principal_id.is_some() {
                "A teammate"
            } else {
                CHANNEL_TEAM
            }
        )),
        age = escape_html_text(&age_label(handoff.generation_opened_at, view.as_of)),
        due = escape_html_text(&due_label(handoff.due_at)),
        base = escape_html_attr(&base),
        pane_href = escape_html_attr(&pane_href),
        version = handoff.version,
        generation = escape_html_attr(&handoff.generation.to_string()),
        // The responsibility route takes the whole attribute pair on every command, so a claim
        // restates what is already stored rather than dropping a priority somebody else set.
        priority_value = escape_html_attr(handoff.priority.as_str()),
        due_value = escape_html_attr(
            &handoff
                .due_at
                .map(|due| due.to_rfc3339())
                .unwrap_or_default()
        ),
        draft_version = view.draft.map_or_else(String::new, |draft| format!(
            r#" data-handoff-draft-version="{}""#,
            draft.draft_version
        )),
        actions = handoff_actions(view),
        error = escape_html_text(view.error.unwrap_or_default()),
    )
}

/// The buttons, which are entirely a function of the state and who is reading.
fn handoff_actions(view: &ThreadHandoffBannerView<'_>) -> String {
    let handoff = view.handoff;
    let is_responsible = view.viewer_principal_id.is_some()
        && view.viewer_principal_id == handoff.responsible_principal_id;
    let claimed_by_somebody_else = handoff.responsible_principal_id.is_some() && !is_responsible;
    // Read-only: a teammate who is neither responsible nor a manager would be answered `NotFound`
    // by every one of these commands, and a signed-out reader has no principal to act as.
    if view.viewer_principal_id.is_none()
        || (claimed_by_somebody_else && !view.viewer_manages_tasks)
    {
        return String::new();
    }

    match handoff.state {
        ThreadHandoffState::NeedsInstruction if handoff.responsible_principal_id.is_none() => {
            // Not a disabled Generate draft: an unclaimed reply has a next step, and it is
            // claiming it.
            format!(
                r#"{claim}{dismiss}<span class="basis-full text-xs opacity-70">Claim this reply before drafting an answer.</span>"#,
                claim = handoff_button("Claim", "claim", "btn-primary"),
                dismiss = handoff_button("Dismiss", "dismiss", "btn-ghost"),
            )
        }
        ThreadHandoffState::NeedsInstruction => {
            let mut row = String::new();
            if is_responsible {
                row.push_str(&handoff_button(
                    "Generate draft",
                    "draft",
                    "btn-outline btn-info",
                ));
            } else {
                row.push_str(&reassign_control(view));
            }
            row.push_str(&handoff_button("Dismiss", "dismiss", "btn-ghost"));
            row
        }
        // A run is in flight and the answer to "stop it" is to let it finish or let it fail, so
        // there is nothing to press -- not even a disabled button pretending otherwise.
        ThreadHandoffState::Drafting => {
            r#"<span class="text-xs opacity-70">Drafting… the agent is writing an answer.</span>"#
                .to_string()
        }
        ThreadHandoffState::DraftReady => {
            let mut row = String::new();
            // Both need the draft's own version as their fence, so neither is offered until the
            // run's draft has actually been read back.
            if let (true, Some(draft)) = (is_responsible, view.draft) {
                row.push_str(&handoff_button("Send", "send", "btn-primary"));
                // Edit and send needs somewhere to edit, and this banner has no field for one: it
                // opens the draft in the review surface that already has one, whose
                // Save-and-approve is the same pair of commands `POST .../send-edited` performs.
                row.push_str(&format!(
                    r#"<a class="btn btn-sm btn-outline" href="/ui/response-reviews/{draft_id}?company_id={company_id}">Edit and send</a>"#,
                    draft_id = escape_html_attr(&draft.draft_id.to_string()),
                    company_id = view.company_id,
                ));
            } else if claimed_by_somebody_else {
                row.push_str(&reassign_control(view));
            }
            row.push_str(&handoff_button("Dismiss", "dismiss", "btn-ghost"));
            row
        }
        // A resolved or dismissed handoff is not loaded onto the pane at all; stated rather than
        // wildcarded so a new state cannot silently render an empty row.
        ThreadHandoffState::Resolved | ThreadHandoffState::Dismissed => String::new(),
    }
}

/// A manager's hand-over control: who to, and the button that sends it.
///
/// Only humans, and only those the channel already lets in -- the command refuses a principal who
/// could not open the thread, so offering one would be offering an error. An empty list is a real
/// answer and says so rather than rendering a button with nothing behind it.
fn reassign_control(view: &ThreadHandoffBannerView<'_>) -> String {
    let options = view
        .reassign_candidates
        .iter()
        .filter_map(|candidate| match candidate.owner {
            TaskOwner::Human(principal)
                if Some(principal) != view.handoff.responsible_principal_id
                    && Some(principal) != view.viewer_principal_id =>
            {
                Some(format!(
                    r#"<option value="{id}">{label}</option>"#,
                    id = escape_html_attr(&principal.as_uuid().to_string()),
                    label = escape_html_text(&candidate.label),
                ))
            }
            _ => None,
        })
        .collect::<String>();
    if options.is_empty() {
        return r#"<span class="text-xs opacity-70">Nobody else on this channel to reassign to.</span>"#
            .to_string();
    }
    format!(
        r#"<select class="select select-sm" data-handoff-reassign-to aria-label="Reassign to">{options}</select>{button}"#,
        button = handoff_button("Reassign", "reassign", "btn-outline"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(
        company_default: ExternalReplyHandling,
        channel_override: Option<ExternalReplyHandling>,
    ) -> String {
        reply_handling_policy_page(
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            &ExternalReplyHandlingPolicy::resolve(company_default, channel_override),
        )
    }

    /// The `<option value="…">` that carries `selected`, for one `<select>` of the page.
    fn selected_values(html: &str, select_name: &str) -> Vec<String> {
        let after = html
            .split_once(&format!(r#"<select name="{select_name}""#))
            .expect("the select is rendered")
            .1;
        let block = after.split_once("</select>").expect("the select closes").0;
        block
            .split("<option value=\"")
            .skip(1)
            .filter_map(|option| {
                let (value, rest) = option.split_once('"')?;
                rest.split_once('>')?
                    .0
                    .contains("selected")
                    .then(|| value.to_string())
            })
            .collect()
    }

    #[test]
    fn every_company_and_override_combination_marks_exactly_one_option_per_select() {
        for company_default in [
            ExternalReplyHandling::Automatic,
            ExternalReplyHandling::ManualHandoff,
        ] {
            for channel_override in [
                None,
                Some(ExternalReplyHandling::Automatic),
                Some(ExternalReplyHandling::ManualHandoff),
            ] {
                let html = page(company_default, channel_override);
                assert_eq!(
                    selected_values(&html, "policy"),
                    vec![company_default.as_str().to_string()],
                    "company default {company_default:?}"
                );
                let expected_override = channel_override
                    .map_or_else(|| "inherit".to_string(), |value| value.as_str().to_string());
                assert_eq!(
                    selected_values(&html, "policy_override"),
                    vec![expected_override],
                    "override {channel_override:?}"
                );
            }
        }
    }

    #[test]
    fn the_header_says_which_of_the_two_settings_the_effective_value_came_from() {
        assert!(
            page(
                ExternalReplyHandling::Automatic,
                Some(ExternalReplyHandling::ManualHandoff)
            )
            .contains("Effective policy: Manual handoff (channel override)")
        );
        assert!(
            page(ExternalReplyHandling::Automatic, None)
                .contains("Effective policy: Automatic (inherited from company)")
        );
        // An override equal to the company default is still an override, and says so.
        assert!(
            page(
                ExternalReplyHandling::Automatic,
                Some(ExternalReplyHandling::Automatic)
            )
            .contains("Effective policy: Automatic (channel override)")
        );
    }

    #[test]
    fn each_option_carries_the_consequence_of_choosing_it() {
        let html = page(ExternalReplyHandling::Automatic, None);
        assert!(html.contains("Outside replies run the channel&#39;s agent immediately."));
        assert!(
            html.contains("Outside replies to existing threads are filed and wait for the team.")
        );
    }
}
