use super::*;
/// Sticky header that replaces the "new simulation" form once a thread is live.
pub(crate) fn simulation_active_banner(
    company_id: Uuid,
    channel_id: Uuid,
    thread_id_str: &str,
    status_label: &str,
) -> String {
    format!(
        r##"
        <div id="simulation-form-container" hx-swap-oob="outerHTML">
            <div class="bg-base-100 border border-base-300 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                <div class="flex items-center gap-2 text-sm text-base-content/80 font-medium">
                    <span class="inline-block w-2.5 h-2.5 rounded-full bg-success animate-pulse"></span>
                    <span>{status_label}</span>
                    <span class="text-xs text-base-content/70 font-mono">({thread_id_str})</span>
                </div>
                <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                   class="px-4 py-2 bg-primary hover:bg-primary/90 text-primary-content text-xs font-semibold rounded-lg shadow-md transition flex items-center gap-1.5 cursor-pointer">
                    <span class="inline-flex items-center gap-1.5">{sync_glyph} Simulate New Thread</span>
                </a>
            </div>
        </div>
        "##,
        sync_glyph = icon(Icon::Sync, BUTTON_ICON),
    )
}

/// The workflow side of one message (p08 D4.6): for an inbound message every binding's outcome,
/// for an agent's reply the run that wrote it.
fn message_workflow_html(msg: &ThreadMessageView, outcomes: &[&MessageBindingOutcome]) -> String {
    if msg.is_agent() {
        return match msg.run_ids.first() {
            Some(run_id) => format!(
                r##"<div class="text-xs font-mono text-base-content/70">Written by run <span class="text-primary">{run}</span></div>"##,
                run = short_id(*run_id),
            ),
            None => String::new(),
        };
    }
    if outcomes.is_empty() {
        return r##"<div class="text-xs text-base-content/70">No workflow binding has answered this message yet.</div>"##.to_string();
    }
    let cards: String = outcomes
        .iter()
        .map(|outcome| binding_outcome_card(outcome))
        .collect();
    format!(
        r##"<div class="space-y-2">
                            <h5 class="text-[11px] font-semibold uppercase text-base-content/70">Workflow outcomes</h5>
                            {cards}
                        </div>"##
    )
}

fn binding_outcome_card(outcome: &MessageBindingOutcome) -> String {
    let (badge, detail) = match &outcome.outcome {
        BindingOutcome::Admitted(run) => (
            format!(
                r##"<span class="badge badge-sm {style}">{label}</span>"##,
                style = run_state_style(run.state),
                label = escape_html_text(&run_state_label(run.state)),
            ),
            admitted_run_detail(run),
        ),
        BindingOutcome::Refused { reason } => (
            r##"<span class="badge badge-sm badge-ghost">Refused</span>"##.to_string(),
            format!(
                r##"<div class="text-xs font-mono">{reason}</div>"##,
                reason = escape_html_text(reason)
            ),
        ),
        BindingOutcome::Failed { reason } => (
            r##"<span class="badge badge-sm badge-error">Admission failed</span>"##.to_string(),
            format!(
                r##"<div class="text-xs font-mono">{reason}</div>"##,
                reason = escape_html_text(reason)
            ),
        ),
    };
    format!(
        r##"<div class="rounded-lg border border-base-300 bg-base-200 p-3 space-y-2">
                                <div class="flex flex-wrap items-center gap-2 text-xs">
                                    {badge}
                                    <span class="font-semibold">{title}</span>
                                    <span class="ml-auto font-mono text-base-content/70">binding {binding}</span>
                                </div>
                                {detail}
                            </div>"##,
        title = escape_html_text(&outcome.workflow_title),
        binding = short_id(outcome.binding_id),
    )
}

fn admitted_run_detail(run: &AdmittedRun) -> String {
    let steps: String = run
        .executions
        .iter()
        .map(|execution| {
            format!(
                r##"<tr><td class="font-mono">{step} #{activation}</td><td class="font-mono">{route}</td><td>{started}</td><td>{completed}</td></tr>"##,
                step = escape_html_text(&execution.step_id),
                activation = execution.activation,
                route = execution
                    .route
                    .as_deref()
                    .map(escape_html_text)
                    .unwrap_or_default(),
                started = execution
                    .activated_at
                    .map(format_time)
                    .unwrap_or_else(|| "pending".to_string()),
                completed = execution.completed_at.map(format_time).unwrap_or_default(),
            )
        })
        .collect();
    let steps = if steps.is_empty() {
        r##"<div class="text-xs text-base-content/70">No step has been scheduled yet.</div>"##
            .to_string()
    } else {
        let latest = if run.executions.len() == RUN_EXECUTION_LIMIT {
            format!(" (latest {RUN_EXECUTION_LIMIT})")
        } else {
            String::new()
        };
        format!(
            r##"<div class="overflow-x-auto"><table class="table table-xs">
                                    <caption class="text-left text-[11px] uppercase text-base-content/70">Executions{latest}</caption>
                                    <thead><tr><th>Step</th><th>Route</th><th>Started</th><th>Completed</th></tr></thead>
                                    <tbody>{steps}</tbody>
                                </table></div>"##
        )
    };
    let usage: String = run
        .model_usage
        .iter()
        .map(|usage| {
            format!(
                r##"<li class="font-mono">{provider} / {model} · {calls} call{plural} · {input} in / {output} out tokens</li>"##,
                provider = escape_html_text(&usage.provider),
                model = escape_html_text(&usage.model),
                calls = usage.calls,
                plural = if usage.calls == 1 { "" } else { "s" },
                input = usage.input_tokens,
                output = usage.output_tokens,
            )
        })
        .collect();
    let usage = if usage.is_empty() {
        String::new()
    } else {
        format!(r##"<ul class="text-xs space-y-1">{usage}</ul>"##)
    };
    format!(
        r##"<div class="text-xs font-mono text-base-content/70">Run <span class="text-primary">{run_id}</span></div>
                                {restricted}{steps}
                                {usage}"##,
        restricted = if run.payload_restricted {
            "<p>Restricted run content</p>"
        } else {
            ""
        },
        run_id = run.run_id,
    )
}

fn run_state_label(state: RunState) -> String {
    match state {
        RunState::Queued => "Queued".to_string(),
        RunState::Running => "Running".to_string(),
        RunState::Waiting(reason) => format!(
            "Waiting on {}",
            match reason {
                WaitingReason::Decision => "a decision",
                WaitingReason::Event => "an event",
                WaitingReason::Timer => "a timer",
                WaitingReason::ChildRun => "a child run",
                WaitingReason::Effect => "an effect",
                WaitingReason::Reconciliation => "reconciliation",
                WaitingReason::AgentTurn => "an agent turn",
            }
        ),
        RunState::Succeeded => "Succeeded".to_string(),
        RunState::Failed => "Failed".to_string(),
        RunState::Cancelled => "Cancelled".to_string(),
    }
}

fn run_state_style(state: RunState) -> &'static str {
    match state {
        RunState::Queued => "badge-warning",
        RunState::Running => "badge-info animate-pulse",
        RunState::Waiting(WaitingReason::Decision | WaitingReason::Reconciliation) => {
            "badge-accent"
        }
        RunState::Waiting(_) => "badge-info badge-outline",
        RunState::Succeeded => "badge-success",
        RunState::Failed => "badge-error",
        RunState::Cancelled => "badge-ghost",
    }
}

/// One message in the thread history: an indigo agent card or a slate inbound card.
pub(crate) fn message_bubble(
    msg: &ThreadMessageView,
    outcomes: &[&MessageBindingOutcome],
) -> String {
    if msg.restricted {
        return crate::adapters::http::pages::message_visibility::restricted_message(msg);
    }
    let is_agent = msg.is_agent();
    let created_at = super::format_date_time(msg.created_at);
    let params_html = message_workflow_html(msg, outcomes);

    if is_agent {
        let body = if msg.response_contract.is_some() {
            format!(
                "<p>Validated JSON</p><pre class=\"whitespace-pre-wrap\">{}</pre>",
                super::escape_html_text(&msg.body)
            )
        } else {
            render_markdown(&msg.body)
        };
        format!(
            r##"
                    <div class="bg-primary/10 border border-primary/40 rounded-xl p-4 space-y-2 shadow-sm">
                        <div class="flex items-center justify-between border-b border-primary/40 pb-2 text-xs">
                            <div class="flex items-center gap-2 font-semibold text-primary">
                                {author_glyph}
                                <span>AI Agent Response</span>
                                <span class="px-1.5 py-0.5 rounded bg-primary/10 text-primary text-[10px] uppercase font-mono">Outbound</span>
                            </div>
                            <span class="text-base-content/70 font-mono text-[11px]">{created_at}</span>
                        </div>
                        <div class="text-xs font-mono text-base-content/70">
                            <span>Message ID: </span><span class="text-primary">{msg_id}</span>
                        </div>
                        <div class="bg-base-200 p-3 rounded-lg text-base-content border border-base-300 text-xs font-sans max-h-60 overflow-y-auto {markdown_styles}">
                            {body}
                        </div>
                        {params_html}
                    </div>
                    "##,
            author_glyph = icon(Icon::Hubot, BUTTON_ICON),
            created_at = created_at,
            msg_id = super::escape_html_text(&msg.canonical_id.to_string()),
            body = body,
            markdown_styles = MARKDOWN_CONTENT_STYLES,
            params_html = params_html,
        )
    } else {
        format!(
            r##"
                    <div class="bg-base-100 border border-base-300 rounded-xl p-4 space-y-2 shadow-sm">
                        <div class="flex items-center justify-between border-b border-base-300 pb-2 text-xs">
                            <div class="flex items-center gap-2 font-semibold text-base-content">
                                {author_glyph}
                                <span>Inbound Email</span>
                                <span class="px-1.5 py-0.5 rounded bg-base-200 text-base-content/80 text-[10px] uppercase font-mono">Inbound</span>
                            </div>
                            <span class="text-base-content/70 font-mono text-[11px]">{created_at}</span>
                        </div>
                        <div class="grid grid-cols-1 md:grid-cols-2 gap-2 text-xs font-mono text-base-content/70">
                            <div>From: <span class="text-primary">{sender}</span></div>
                            <div>Message ID: <span class="text-base-content/80">{msg_id}</span></div>
                        </div>
                        <div class="text-xs font-medium text-base-content pt-1">
                            Subject: <span class="font-normal text-base-content/80">{subject}</span>
                        </div>
                        <div class="bg-base-300 p-3 rounded-lg text-base-content/80 whitespace-pre-wrap border border-base-300 text-xs">
                            {body}
                        </div>
                        {params_html}
                    </div>
                    "##,
            author_glyph = icon(Icon::Person, BUTTON_ICON),
            created_at = created_at,
            sender = super::escape_html_text(msg.author.display()),
            msg_id = super::escape_html_text(&msg.canonical_id.to_string()),
            subject = super::escape_html_text(&msg.subject),
            body = super::escape_html_text(&msg.body),
            params_html = params_html,
        )
    }
}

/// The full "Thread History" card, or `empty_state` when the thread has no messages yet.
pub(crate) fn thread_history_section(
    messages: &[ThreadMessageView],
    thread_id_str: &str,
    wrapper_extra_class: &str,
    empty_state: &str,
    outcomes: &[MessageBindingOutcome],
) -> String {
    if messages.is_empty() {
        return empty_state.to_string();
    }
    let mut by_message: HashMap<Uuid, Vec<&MessageBindingOutcome>> = HashMap::new();
    for outcome in outcomes {
        by_message
            .entry(outcome.message_id)
            .or_default()
            .push(outcome);
    }
    let msgs_html: String = messages
        .iter()
        .map(|msg| {
            let outcomes = by_message
                .get(&msg.canonical_id.as_uuid())
                .map(Vec::as_slice)
                .unwrap_or_default();
            message_bubble(msg, outcomes)
        })
        .collect();
    let msg_count = messages.len();
    let label = if msg_count == 1 {
        "message"
    } else {
        "messages"
    };

    format!(
        r##"
            <div class="bg-base-100 border border-base-300 rounded-xl p-5 space-y-4 shadow-lg{wrapper_extra_class}">
                <div class="flex items-center justify-between border-b border-base-300 pb-3">
                    <h4 class="text-sm font-sans font-bold text-base-content flex items-center gap-2">
                        {history_glyph} Thread History ({msg_count} {label})
                    </h4>
                    <span class="text-xs font-mono text-base-content">Thread ID: {thread_id_str}</span>
                </div>
                <div class="space-y-3">
                    {msgs_html}
                </div>
            </div>
            "##,
        history_glyph = icon(Icon::CommentDiscussion, BUTTON_ICON),
    )
}

/// Everything that varies between the two "simulate a reply" forms.
pub(crate) struct ReplyFormFields<'a> {
    company_id: Uuid,
    channel_id: Uuid,
    thread_id_str: &'a str,
    last_msg_id: &'a str,
    to_value: &'a str,
    from_value: &'a str,
    subject: &'a str,
    run_test_checked: bool,
    run_checked: bool,
    /// The execution-result form pads its submit row; the loaded-thread form doesn't.
    submit_row_class: &'a str,
}

pub(crate) fn simulate_reply_form(fields: &ReplyFormFields<'_>) -> String {
    let ReplyFormFields {
        company_id,
        channel_id,
        thread_id_str,
        last_msg_id,
        to_value,
        from_value,
        subject,
        run_test_checked,
        run_checked,
        submit_row_class,
    } = *fields;
    let run_test_checked = if run_test_checked { " checked" } else { "" };
    let run_checked = if run_checked { " checked" } else { "" };
    let thread_id_str = escape_html_text(thread_id_str);
    let last_msg_id = escape_html_attr(last_msg_id);
    let to_value = escape_html_attr(to_value);
    let from_value = escape_html_attr(from_value);
    let subject = escape_html_attr(subject);

    format!(
        r##"
        <div class="bg-base-100 border border-primary/40 rounded-xl p-5 shadow-xl space-y-4">
            <div class="flex items-center justify-between border-b border-base-300 pb-3">
                <h3 class="text-sm font-bold text-base-content flex items-center gap-2">
                    <span class="text-primary">{reply_glyph}</span> Simulate Reply Webhook Call
                </h3>
                <span class="text-xs text-base-content/70">Simulate next message in Thread <span class="font-mono text-primary">{thread_id_str}</span></span>
            </div>

            <form hx-post="/companies/{company_id}/channels/{channel_id}/simulate"
                  hx-target="#simulation-result"
                  hx-swap="innerHTML"
                  hx-disabled-elt="find button[type='submit']"
                  class="space-y-4">
                <input type="hidden" name="in_reply_to" value="{last_msg_id}">

                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div>
                        <label for="to_reply" class="block text-xs font-medium text-base-content/80 mb-1">To (Recipient Address)</label>
                        <input type="text" id="to_reply" name="to" value="{to_value}" required
                            class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary">
                    </div>
                    <div>
                        <label for="from_reply" class="block text-xs font-medium text-base-content/80 mb-1">From (Sender Address)</label>
                        <input type="text" id="from_reply" name="from" value="{from_value}" disabled
                            class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm font-mono opacity-60 cursor-not-allowed">
                    </div>
                </div>

                <div>
                    <label for="subject_reply" class="block text-xs font-medium text-base-content/80 mb-1">Subject</label>
                    <input type="text" id="subject_reply" name="subject" value="{subject}" required
                        class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm focus:outline-none focus:ring-2 focus:ring-primary">
                </div>

                <div>
                    <label for="text_body_reply" class="block text-xs font-medium text-base-content/80 mb-1">Reply Text Body</label>
                    <textarea id="text_body_reply" name="text_body" rows="3" required placeholder="Type your reply message here..."
                        class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm focus:outline-none focus:ring-2 focus:ring-primary"></textarea>
                </div>

                <div>
                    <label class="block text-xs font-medium text-base-content/80 mb-2">Execution Mode</label>
                    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                        <label class="flex items-start p-2.5 bg-base-200 border border-base-300 rounded-lg cursor-pointer hover:border-warning transition">
                            <input type="radio" name="simulation_mode" value="run_test"{run_test_checked} class="mt-0.5 text-base-content focus:ring-warning">
                            <div class="ml-2.5">
                                <span class="block text-xs font-bold text-base-content">Run_Test</span>
                                <span class="block text-[11px] text-base-content/70 mt-0.5">Execute full channel & agent, skip email dispatch</span>
                            </div>
                        </label>
                        <label class="flex items-start p-2.5 bg-base-200 border border-base-300 rounded-lg cursor-pointer hover:border-success transition">
                            <input type="radio" name="simulation_mode" value="run"{run_checked} class="mt-0.5 text-base-content focus:ring-success">
                            <div class="ml-2.5">
                                <span class="block text-xs font-bold text-base-content">Run</span>
                                <span class="block text-[11px] text-base-content/70 mt-0.5">Live execution with full AI agent & outbound SMTP send</span>
                            </div>
                        </label>
                    </div>
                </div>

                <div class="flex justify-end{submit_row_class}">
                    <button type="submit"
                        class="px-5 py-2.5 bg-primary hover:bg-primary/90 text-primary-content text-sm font-semibold rounded-lg shadow-md shadow-primary/20 transition cursor-pointer flex items-center gap-2 [.htmx-request_&]:pointer-events-none [.htmx-request_&]:opacity-80">
                        <svg class="animate-spin h-4 w-4 text-base-content hidden [.htmx-request_&]:inline-block shrink-0" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" aria-hidden="true">
                            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                        </svg>
                        <span class="[.htmx-request_&]:hidden">Trigger Reply Webhook Simulation</span>
                        <span class="hidden [.htmx-request_&]:inline">Simulating...</span>
                        <span class="[.htmx-request_&]:hidden">{submit_glyph}</span>
                    </button>
                </div>
            </form>
        </div>
        "##,
        reply_glyph = icon(Icon::Reply, BUTTON_ICON),
        submit_glyph = icon(Icon::ArrowRight, BUTTON_ICON),
    )
}

/// The channel configuration an agent reply would have used, for messages with no recorded task.
/// Wrap a simulated thread view in its own live connection.
///
/// A simulation is queued for the worker like any other message, so this view starts out showing
/// only the message that was sent and fills in as the run progresses. The wrapper stays put and
/// only its contents are replaced -- swapping the element itself would tear down the `EventSource`
/// and reopen it on every update.
///
/// `hx-target="this"` is not decoration. htmx attributes are inherited, and this sits inside the
/// simulation form's target; without pinning it, each update swaps itself over the whole page.
pub fn simulation_live_view(
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    inner: &str,
) -> String {
    format!(
        r#"<div id="simulation-live" hx-ext="sse"
            sse-connect="/companies/{company_id}/channels/{channel_id}/simulate/events?thread_id={thread_id}"
            sse-swap="simulation" hx-target="this" hx-swap="innerHTML">{inner}</div>"#
    )
}

/// Everything the loaded-thread fragment renders from.
///
/// A struct rather than seven positional arguments: `&Company`, `&Channel` and `&str` in a row are
/// exactly what a call site can transpose without the compiler noticing.
pub struct SimulationThreadView<'a> {
    pub company: &'a Company,
    pub channel: &'a Channel,
    pub app_domain_name: &'a str,
    pub thread: &'a Thread,
    pub messages: &'a [ThreadMessageView],
    /// The headers a simulated reply threads onto, when mail carried this thread's newest turn.
    /// `None` -- a schedule prompt, a Slack turn -- starts a fresh mail conversation.
    pub reply_context: Option<&'a EmailReplyContext>,
    /// Every binding outcome of the thread's messages, as [`MessageOutcomeReader`] reads them.
    ///
    /// [`MessageOutcomeReader`]: crate::application::message_outcomes::MessageOutcomeReader
    pub outcomes: &'a [MessageBindingOutcome],
    pub include_oob: bool,
}

pub fn channel_simulation_loaded_thread_fragment(view: &SimulationThreadView<'_>) -> String {
    let SimulationThreadView {
        company,
        channel,
        app_domain_name,
        thread,
        messages,
        reply_context,
        outcomes,
        include_oob,
    } = *view;
    let thread_id_str = thread.id.to_string();
    let target_recipient = channel.inbound_address(&company.slug, app_domain_name);

    let default_sender = thread
        .participant_projection
        .subjects_for(TransportKind::Email)
        .first()
        .map(|address| EmailAddress::from(*address))
        .or_else(|| {
            channel
                .participant_emails
                .as_ref()
                .and_then(|e| e.first().cloned())
        })
        .unwrap_or_else(|| {
            crate::entities::value_objects::EmailAddress::from("sender@example.com")
        });

    let overview_card =
        loaded_thread_overview_card(thread, &thread_id_str, &target_recipient, messages.len());

    let messages_section = thread_history_section(
        messages,
        &thread_id_str,
        " mb-6",
        r#"<div class="bg-base-100 border border-base-300 rounded-xl p-5 shadow-lg text-base-content/70 text-xs text-center mb-6">No messages recorded in this thread yet.</div>"#,
        outcomes,
    );

    let reply_form = simulate_reply_form(&ReplyFormFields {
        company_id: company.id,
        channel_id: channel.id,
        thread_id_str: &thread_id_str,
        last_msg_id: &reply_context
            .and_then(|context| context.rfc_message_id.as_ref())
            .map(|id| id.to_string())
            .unwrap_or_default(),
        to_value: &target_recipient,
        from_value: &default_sender,
        subject: &if thread.subject.to_lowercase().starts_with("re:") {
            thread.subject.clone()
        } else {
            format!("Re: {}", thread.subject)
        },
        run_test_checked: true,
        run_checked: false,
        submit_row_class: "",
    });

    if include_oob {
        let banner = simulation_active_banner(
            company.id,
            channel.id,
            &thread_id_str,
            "Thread Loaded & Active",
        );
        format!("{banner}\n{overview_card}\n{messages_section}\n{reply_form}")
    } else {
        format!("{overview_card}\n{messages_section}\n{reply_form}")
    }
}

pub(crate) fn loaded_thread_overview_card(
    thread: &Thread,
    thread_id_str: &str,
    target_recipient: &str,
    msg_count: usize,
) -> String {
    let participant_addresses = thread
        .participant_projection
        .subjects_for(TransportKind::Email);
    let participants_str = if participant_addresses.is_empty() {
        "None recorded".to_string()
    } else {
        participant_addresses.join(", ")
    };

    format!(
        r##"
        <div class="bg-base-100 border border-base-300 rounded-xl p-5 space-y-3 text-xs font-mono shadow-lg mb-6">
            <h4 class="text-sm font-sans font-bold text-base-content border-b border-base-300 pb-2 flex items-center justify-between">
                <span>Loaded Thread Details</span>
                <span class="text-base-content text-xs font-mono">ID: {thread_id_str}</span>
            </h4>
            <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                <div>
                    <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Subject:</span>
                    <span class="text-base-content font-bold">{subject}</span>
                </div>
                <div>
                    <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Target Channel Address:</span>
                    <span class="text-primary">{target_recipient}</span>
                </div>
                <div>
                    <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Participants:</span>
                    <span class="text-base-content/80">{participants_str}</span>
                </div>
                <div>
                    <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Total Messages:</span>
                    <span class="text-base-content font-bold">{msg_count}</span>
                </div>
                <div>
                    <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Created At:</span>
                    <span class="text-base-content/70">{created_at_fmt}</span>
                </div>
                <div>
                    <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Last Updated:</span>
                    <span class="text-base-content/70">{updated_at_fmt}</span>
                </div>
            </div>
        </div>
        "##,
        subject = super::escape_html_text(&thread.subject),
        created_at_fmt = super::format_date_time(thread.created_at),
        updated_at_fmt = super::format_date_time(thread.updated_at),
    )
}

pub fn channel_simulation_thread_error_fragment(
    company_id: Uuid,
    channel_id: Uuid,
    thread_id_input: &str,
    error_msg: &str,
    include_oob: bool,
) -> String {
    let oob_form_swap = format!(
        r##"
        <div id="simulation-form-container" hx-swap-oob="outerHTML">
            <div class="bg-base-100 border border-base-300 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                <div class="flex items-center gap-2 text-sm text-base-content/80 font-medium">
                    <span class="inline-block w-2.5 h-2.5 rounded-full bg-error animate-ping"></span>
                    <span class="text-base-content font-semibold">Failed to Load Thread</span>
                </div>
                <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                   class="px-4 py-2 bg-primary hover:bg-primary/90 text-primary-content text-xs font-semibold rounded-lg shadow-md transition flex items-center gap-1.5 cursor-pointer">
                    <span class="inline-flex items-center gap-1.5">{sync_glyph} Simulate New Thread</span>
                </a>
            </div>
        </div>
        "##,
        company_id = company_id,
        channel_id = channel_id,
        sync_glyph = icon(Icon::Sync, BUTTON_ICON),
    );

    let error_body = format!(
        r##"
        <div class="space-y-4">
            <div class="p-4 rounded-xl bg-error/10 border border-error/40 text-base-content text-sm font-semibold flex items-center gap-2">
                {error_glyph}
                <span>Error Loading Thread ({thread_id_input}): {error_msg}</span>
            </div>
        </div>
        "##,
        error_glyph = icon(Icon::X, BUTTON_ICON),
        thread_id_input = escape_html_text(thread_id_input),
        error_msg = escape_html_text(error_msg),
    );

    if include_oob {
        format!("{oob_form_swap}\n{error_body}")
    } else {
        error_body
    }
}
