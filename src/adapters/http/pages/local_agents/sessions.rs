use super::*;

pub(super) fn start(view: &Page<'_>) -> String {
    let base = base(view.company.id);
    let open = format!(
        r#"<form method="get" action="{base}" class="flex flex-wrap gap-3"><label>Open a local session reference<input class="input" name="session" required maxlength="36" aria-label="Local session ID"></label><button class="btn" type="submit">Open session</button></form>"#
    );
    let Some(conversation) = view.conversation else {
        return format!(
            r#"<section class="space-y-3"><h2 class="text-xl font-semibold">Local sessions</h2><p>Open a conversation in Mailbox and choose “Run locally” to start a session associated with it. Workflows can use the explicit local_agent.run step.</p><a class="link" href="/ui?company_id={}">Choose a conversation in Mailbox</a>{open}</section>"#,
            view.company.id
        );
    };
    let profiles = view
        .profiles
        .entries
        .iter()
        .chain(view.profile.filter(|profile| {
            !view
                .profiles
                .entries
                .iter()
                .any(|entry| entry.id == profile.id)
        }))
        .map(|profile| {
            option(
                &profile.id.to_string(),
                &profile.name,
                view.profile
                    .is_some_and(|selected| selected.id == profile.id),
            )
        })
        .collect::<String>();
    let fields = format!(
        r#"<input type="hidden" name="request_id" value="{request}"><input type="hidden" name="conversation_id" value="{conversation}"><label class="block">Local profile<select name="profile_id" class="select w-full" required>{profiles}</select></label><label class="block">Prompt<textarea name="prompt" class="textarea w-full" required maxlength="32768" rows="4"></textarea></label><p>This starts a native CLI agent with the profile's local files and permissions. Only this bounded prompt is sent; Openbot conversation history and tool authority are not added.</p>"#,
        request = request_id(),
        conversation = conversation.id
    );
    let sessions=view.sessions.map_or_else(String::new,|page| {
        let rows=page.entries.iter().map(|session|format!(r#"<li><a class="link font-mono" href="{base}?session={id}">{id}</a> · created {created}</li>"#,id=session.id,created=text(&session.created_at.to_rfc3339()))).collect::<String>();
        format!("<ul>{rows}</ul>{}",if page.truncated{"<p>Showing the newest 100 sessions. Open a known session reference for older work; Cloudroom owns the full transcript.</p>"}else{""})
    });
    format!(
        r#"<section class="space-y-4"><h2 class="text-xl font-semibold">Start native work for {subject}</h2>{start}{sessions}{open}</section>"#,
        subject = text(&conversation.subject),
        start = form(&format!("{base}/sessions"), &fields, "Start native agent")
    )
}

pub(super) fn detail(company: Uuid, details: &SessionDetails) -> String {
    let session = &details.session;
    let base = base(company);
    let thread=session.thread_id.as_ref().map_or_else(||"<p>Native thread identity is not known yet. Do not repeat uncertain submissions.</p>".into(),|id|format!(r#"<label class="block">Cloudroom thread ID<input class="input w-full font-mono" readonly value="{}"></label><p>Copy this ID and open the thread in Cloudroom on the machine. A remote browser cannot open that machine's localhost UI.</p>"#,attr(id.as_str())));
    let commands = if session.thread_id.is_some() {
        let fields = format!(
            r#"<input type="hidden" name="request_id" value="{}"><input type="hidden" name="kind" value="send"><label class="block">Follow-up prompt<textarea class="textarea w-full" name="prompt" required maxlength="32768" rows="3"></textarea></label>"#,
            request_id()
        );
        let follow = form(
            &format!("{base}/sessions/{}/commands", session.id),
            &fields,
            "Send explicit follow-up",
        );
        let controls=[("inspect","Inspect thread"),("stop","Stop entire thread"),("archive","Archive entire thread")].into_iter().map(|(kind,label)|form(&format!("{base}/sessions/{}/commands",session.id),&format!(r#"<input type="hidden" name="request_id" value="{}"><input type="hidden" name="kind" value="{kind}">"#,request_id()),label)).collect::<String>();
        format!(
            "{follow}<p>Stop and archive are thread-wide: they can affect work started in the local UI. They never complete or restart an Openbot workflow.</p><div class=\"flex flex-wrap gap-3\">{controls}</div>"
        )
    } else {
        String::new()
    };
    let operations = details
        .recent
        .entries
        .iter()
        .map(operation)
        .collect::<String>();
    format!(
        r#"<section class="space-y-4"><h2 class="text-xl font-semibold">Session <span class="font-mono select-all">{id}</span></h2><p>Original profile: <code class="select-all">{profile}</code> · {provider} / {model}</p>{thread}{commands}<p>Native permission requests must be answered in Cloudroom. A waiting or disconnected native agent is not a completed operation.</p><div class="space-y-4">{operations}</div>{older}</section>"#,
        id = session.id,
        profile = session.profile_id,
        provider = text(&enum_value(&session.selection.provider)),
        model = text(session.selection.model.as_str()),
        older = if details.recent.truncated {
            "<p>Earlier operations omitted from this bounded view. Cloudroom retains the canonical transcript.</p>"
        } else {
            ""
        }
    )
}

fn operation(view: &LocalOperationView) -> String {
    let operation = &view.operation;
    let prompt = match operation.command() {
        LocalCommand::Create { prompt, .. } | LocalCommand::Send { prompt, .. } => format!(
            "<h4 class=\"font-semibold\">Prompt</h4><pre class=\"whitespace-pre-wrap break-words\">{}</pre>",
            text(prompt.as_str())
        ),
        _ => String::new(),
    };
    let status = operation.outcome().map_or_else(
        || {
            view.observed_turn_state.map_or_else(
                || "Queued / no terminal evidence".into(),
                |state| enum_value(&state),
            )
        },
        |outcome| enum_value(&outcome),
    );
    let usage = view.measured_usage.as_ref().map_or_else(
        || "Unavailable".into(),
        |usage| {
            format!(
                "{} input tokens · {} output tokens",
                usage.input_tokens, usage.output_tokens
            )
        },
    );
    let output=view.output.as_ref().map_or_else(String::new,|output|format!("<h4 class=\"font-semibold\">Bounded output</h4><pre class=\"whitespace-pre-wrap break-words\">{}</pre>",text(output.as_str())));
    let blocked = if view.observed_turn_state == Some(TurnState::WaitingForInput) {
        "<p>Waiting for input or permission. Respond in Cloudroom on the machine.</p>"
    } else {
        ""
    };
    format!(
        r#"<article class="card card-border p-4 space-y-2"><h3 class="font-mono select-all">{id}</h3><p>{origin} · {status} · deadline {deadline}</p><p>Measured usage: {usage}</p>{blocked}{prompt}{output}{truncated}</article>"#,
        id = operation.id(),
        origin = if view.origin == OperationOrigin::Workflow {
            "Workflow request"
        } else {
            "Direct request"
        },
        status = text(&status),
        deadline = text(&operation.deadline().to_rfc3339()),
        usage = text(&usage),
        truncated = if view.output_truncated {
            "<p>Output shortened by the operation or page bound; use Cloudroom for the full transcript.</p>"
        } else {
            ""
        }
    )
}
