use super::*;

pub fn agent_publication_panel(
    company: Uuid,
    state: &AgentAuthoringState,
    retry: Option<&PublishAgent>,
    message: Option<&str>,
) -> String {
    let agent = state.agent.id;
    let draft = retry
        .map(|request| request.expected_draft_revision)
        .unwrap_or(state.agent.draft_revision)
        .get();
    let active = retry
        .map(|request| request.expected_active_revision)
        .unwrap_or_else(|| state.active.as_ref().map(|active| active.id));
    let active_input = active
        .map(|id| id.as_uuid().to_string())
        .unwrap_or_default();
    let key = retry
        .map(|request| request.key.as_str().to_owned())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let active_text = state
        .active
        .as_ref()
        .map(|active| {
            format!(
                "{} (from draft {})",
                active.id.as_uuid(),
                active.draft_revision.get()
            )
        })
        .unwrap_or_else(|| {
            "Inactive — publish a valid draft before using this agent in a new run".into()
        });
    let tools = state
        .active
        .as_ref()
        .map(|active| {
            active
                .tools
                .iter()
                .map(|tool| active_tool(company, agent, tool))
                .collect::<String>()
        })
        .unwrap_or_default();
    let skills = state
        .active
        .as_ref()
        .map(|active| {
            active
                .skills
                .iter()
                .map(|skill| active_skill(company, agent, skill))
                .collect::<String>()
        })
        .unwrap_or_default();
    let deactivate = state.active.as_ref().map(|active| format!(r##"<form hx-post="/ui/agents/{agent}/deactivate?company_id={company}" hx-target="#agent-publication-{agent}" hx-swap="outerHTML"><input type="hidden" name="expected_active_revision" value="{}"><button class="btn btn-warning btn-outline btn-sm">Deactivate for new runs</button></form>"##, active.id.as_uuid())).unwrap_or_default();
    let message = message
        .map(|message| {
            format!(
                r#"<p class="alert break-words" role="status">{}</p>"#,
                escape_html_text(message)
            )
        })
        .unwrap_or_default();
    let retry_notice = if retry.is_some() {
        "This action retains its original request key and expected revisions. Retry it to confirm the same publication, or reload this panel to begin a new action."
    } else {
        "Publication uses the saved draft. Save your editor changes first."
    };
    format!(
        r##"<section id="agent-publication-{agent}" class="mt-6 space-y-4 rounded-box border border-base-300 p-4" hx-sync="this:queue all" hx-disabled-elt="#agent-publication-{agent} button">
        <h3 class="font-semibold">Saved draft and publication</h3>{message}
        <p>Saved draft: <span data-agent-saved-draft>{saved}</span></p><p class="break-all">Active revision: <span data-agent-active-revision>{active_text}</span></p>
        <p class="text-sm opacity-70">Future runs use the active revision. Running work and retries keep the revision they selected. Removing or revoking access may stop running work. Changes to shared skills enter this agent when you publish again.</p>
        <p class="text-xs opacity-70">{retry_notice}</p>
        <div class="flex flex-wrap gap-2"><form hx-post="/ui/agents/{agent}/preview?company_id={company}" hx-target="#agent-publication-{agent}" hx-swap="outerHTML"><button class="btn btn-outline btn-sm">Validate saved draft</button></form>
        <form hx-post="/ui/agents/{agent}/publish?company_id={company}" hx-target="#agent-publication-{agent}" hx-swap="outerHTML"><input type="hidden" name="expected_draft_revision" value="{draft}"><input type="hidden" name="expected_active_revision" value="{active_input}"><input type="hidden" name="key" value="{key}"><button class="btn btn-primary btn-sm">Publish saved draft</button></form>{deactivate}
        <button type="button" class="btn btn-ghost btn-sm" hx-get="/ui/agents/{agent}/publication?company_id={company}" hx-params="none" hx-target="#agent-publication-{agent}" hx-swap="outerHTML">Reload publication status</button></div>
        <div class="space-y-2">{tools}{skills}</div>{history}</section>"##,
        saved = state.agent.draft_revision.get(),
        active_text = escape_html_text(&active_text),
        key = escape_html_attr(&key),
        history = agent_revision_history(company, agent, &state.history)
    )
}

fn active_tool(company: Uuid, agent: Uuid, tool: &AgentToolState) -> String {
    let binding = tool
        .assignment
        .binding
        .expect("stored active tool has a binding identity");
    let recovery = if tool.access == AgentGrantState::Revoked {
        "Remove this tool from the draft, select it again, save and publish to create a new binding. Regranting the upstream connection or registration does not revive this binding."
    } else {
        ""
    };
    let revoke = if tool.access == AgentGrantState::Live {
        format!(
            r##"<form hx-post="/ui/agents/{agent}/revoke?company_id={company}" hx-target="#agent-publication-{agent}" hx-swap="outerHTML" hx-confirm="Revoke this exact binding? Running work that uses it may stop."><input type="hidden" name="binding" value="{}"><button class="btn btn-error btn-outline btn-xs">Revoke binding access now</button></form>"##,
            binding.as_uuid()
        )
    } else {
        String::new()
    };
    format!(
        r#"<div class="rounded border border-base-300 p-2"><strong>{alias}</strong><p class="break-all text-xs">{target}</p><p class="break-all font-mono text-xs">Binding {binding}</p><p class="text-xs">{access} · {availability}</p><p class="text-xs">{recovery}</p>{revoke}</div>"#,
        alias = escape_html_text(tool.assignment.alias.as_str()),
        target = escape_html_text(&tool.assignment.choice.target_label()),
        binding = binding.as_uuid(),
        access = access_label(tool.access),
        availability = availability_label(tool.availability)
    )
}

fn active_skill(company: Uuid, agent: Uuid, skill: &AgentActiveSkill) -> String {
    let assignment = skill.assignment.as_uuid();
    let recovery = if skill.access == AgentGrantState::Revoked {
        "To create a new skill assignment, remove this skill from the draft and save, then add it again, save and publish."
    } else {
        ""
    };
    let revoke = if skill.access == AgentGrantState::Live {
        format!(
            r##"<form hx-post="/ui/agents/{agent}/revoke?company_id={company}" hx-target="#agent-publication-{agent}" hx-swap="outerHTML" hx-confirm="Revoke this skill assignment? Running work that uses it may stop."><input type="hidden" name="skill" value="{assignment}"><button class="btn btn-error btn-outline btn-xs">Revoke skill access now</button></form>"##
        )
    } else {
        String::new()
    };
    format!(
        r#"<div class="rounded border border-base-300 p-2"><strong>Skill {name}</strong><p class="break-all font-mono text-xs">Assignment {assignment}</p><p class="text-xs">{access}</p><p class="text-xs">{recovery}</p>{revoke}</div>"#,
        name = escape_html_text(skill.name.as_str()),
        access = access_label(skill.access)
    )
}

pub fn agent_revision_history(company: Uuid, agent: Uuid, page: &AgentHistoryPage) -> String {
    let rows = page.items.iter().map(|entry| format!(r#"<li class="break-all text-xs"><span class="font-mono">{id}</span> · draft {draft} · {created}<br>Digest: {digest}</li>"#,
        id = entry.id.as_uuid(), draft = entry.draft_revision.get(), created = entry.created_at,
        digest = escape_html_text(&entry.digest))).collect::<String>();
    let more = page.next.map(|cursor| {
        let query = url::form_urlencoded::Serializer::new(String::new()).append_pair("company_id", &company.to_string())
            .append_pair("before_created_at", &cursor.created_at.to_rfc3339()).append_pair("before_id", &cursor.id.as_uuid().to_string()).finish();
        format!(r##"<button type="button" class="btn btn-ghost btn-xs" hx-get="/ui/agents/{agent}/history?{}" hx-params="none" hx-target="#agent-history-{agent}" hx-swap="outerHTML" hx-sync="#agent-history-{agent}:replace">More revisions · history continues</button>"##, escape_html_attr(&query))
    }).unwrap_or_default();
    format!(
        r#"<section id="agent-history-{agent}" class="space-y-2"><h4 class="font-semibold">Revision history</h4><p class="text-xs opacity-60">Up to 20 revisions per page, newest first.</p><ol class="space-y-2">{rows}</ol>{more}</section>"#
    )
}
