//! One step of a run, loaded when its row opens: what it froze and produced, the agent it ran
//! with and that agent's frozen tools and skills, its model calls, action receipts and child
//! runs, and the worker attempts under a diagnostics disclosure.
use super::*;
use crate::application::workflow::run_views::StepView;

/// The agent a model step ran, as the run's bundle froze it.
pub struct FrozenAgentView<'a> {
    pub handle: &'a str,
    pub model: String,
    pub tools: Vec<&'a str>,
    pub skills: Vec<&'a str>,
}

pub struct StepBodyView<'a> {
    pub company_id: Uuid,
    pub step: &'a StepView,
    pub agent: Option<FrozenAgentView<'a>>,
}

pub fn run_step_body(view: &StepBodyView<'_>) -> String {
    let step = view.step;
    let company_id = view.company_id;
    let section = |title: &str, body: String| {
        format!(
            r#"<div class="space-y-1"><p class="text-xs font-semibold">{}</p>{body}</div>"#,
            escape_html_text(title)
        )
    };
    let mut parts = vec![format!(
        r#"<p class="text-xs opacity-70">Created {created}{activated}{completed}{choice}{route}</p>"#,
        created = format_date_time(step.created_at),
        activated = step
            .activated_at
            .map(|at| format!(" · started {}", format_date_time(at)))
            .unwrap_or_default(),
        completed = step
            .completed_at
            .map(|at| format!(" · finished {}", format_date_time(at)))
            .unwrap_or_default(),
        choice = step
            .frozen_choice
            .as_deref()
            .map(|choice| format!(" · choice {}", escape_html_text(choice)))
            .unwrap_or_default(),
        route = match (&step.committed_route, &step.route_target) {
            (Some(route), Some(target)) => format!(
                " · route {} → {}",
                escape_html_text(route),
                escape_html_text(target)
            ),
            (Some(route), None) => format!(" · route {}", escape_html_text(route)),
            _ => String::new(),
        },
    )];
    if let Some(agent) = &view.agent {
        let list = |names: &[&str], none: &str| {
            if names.is_empty() {
                none.to_owned()
            } else {
                names
                    .iter()
                    .map(|name| escape_html_text(name))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        };
        parts.push(section(
            "Agent",
            format!(
                r#"<p class="text-sm"><span class="font-mono">@{handle}</span> · {model} · uses the agent's tools and skills as frozen when this version was published: tools {tools}; skills {skills}.</p>"#,
                handle = escape_html_text(agent.handle),
                model = escape_html_text(&agent.model),
                tools = list(&agent.tools, "none"),
                skills = list(&agent.skills, "none"),
            ),
        ));
    }
    parts.push(section(
        "Inputs (frozen when the step started)",
        step.frozen_inputs
            .as_ref()
            .map(json_preview)
            .unwrap_or_else(|| r#"<p class="text-xs opacity-70">Not frozen yet.</p>"#.to_owned()),
    ));
    parts.push(section(
        "Output",
        step.committed_output
            .as_ref()
            .map(json_preview)
            .unwrap_or_else(|| {
                r#"<p class="text-xs opacity-70">No output committed.</p>"#.to_owned()
            }),
    ));
    if !step.model_calls.is_empty() {
        let rows: String = step
            .model_calls
            .iter()
            .map(|call| {
                format!(
                    r#"<tr><td>{provider} / {model}</td><td>{input}</td><td>{output}</td><td>{source}</td><td>{at}</td></tr>"#,
                    provider = escape_html_text(&call.provider),
                    model = escape_html_text(&call.model),
                    input = call
                        .input_tokens
                        .map(|tokens| tokens.to_string())
                        .unwrap_or_else(|| "pending".into()),
                    output = call
                        .output_tokens
                        .map(|tokens| tokens.to_string())
                        .unwrap_or_else(|| "pending".into()),
                    source = escape_html_text(call.usage_source.as_deref().unwrap_or("-")),
                    at = format_date_time(call.requested_at),
                )
            })
            .collect();
        parts.push(section(
            "Model usage",
            format!(
                r#"<div class="overflow-x-auto"><table class="table table-xs"><thead class="text-base-content"><tr><th>Model</th><th>Input tokens</th><th>Output tokens</th><th>Usage</th><th>Requested</th></tr></thead><tbody>{rows}</tbody></table></div>"#
            ),
        ));
    }
    if !step.receipts.is_empty() {
        let rows: String = step
            .receipts
            .iter()
            .map(|receipt| {
                format!(
                    r#"<li class="space-y-1"><p class="text-xs">{kind} action accepted {at} · dispatch <span class="font-mono">{dispatch}</span></p>{result}</li>"#,
                    kind = escape_html_text(&receipt.effect_kind),
                    at = format_date_time(receipt.created_at),
                    dispatch = short_id(receipt.dispatch),
                    result = json_preview(&receipt.result),
                )
            })
            .collect();
        parts.push(section(
            "Action receipts",
            format!(r#"<ul class="space-y-2">{rows}</ul>"#),
        ));
    }
    if !step.children.is_empty() {
        let rows: String = step
            .children
            .iter()
            .map(|child| {
                let id = child.run.as_uuid();
                format!(
                    r#"<li class="text-sm">{link} {state}{round}{call}</li>"#,
                    link = run_link(company_id, id),
                    state = run_state_badge(&child.state, child.waiting_reason.as_deref()),
                    round = child
                        .round
                        .map(|round| format!(" · round {round}"))
                        .unwrap_or_default(),
                    call = child
                        .tool_call
                        .map(|call| format!(
                            r#" · tool call <span class="font-mono">{}</span>"#,
                            short_id(call)
                        ))
                        .unwrap_or_default(),
                )
            })
            .collect();
        parts.push(section("Child runs", format!(r#"<ul>{rows}</ul>"#)));
    }
    if !step.attempts.is_empty() {
        let rows: String = step
            .attempts
            .iter()
            .map(|attempt| {
                format!(
                    r#"<tr><td>{number}</td><td>{status}</td><td>{stop}</td><td>{failure}</td><td>{safety}</td><td>{started}</td><td>{finished}</td></tr>"#,
                    number = attempt.number,
                    status = escape_html_text(&attempt.status),
                    stop = escape_html_text(attempt.stop_reason.as_deref().unwrap_or("-")),
                    failure = escape_html_text(&match (&attempt.failure_class, &attempt.failure_code) {
                        (Some(class), Some(code)) => format!("{class}: {code}"),
                        (Some(class), None) => class.clone(),
                        (None, Some(code)) => code.clone(),
                        (None, None) => "-".to_owned(),
                    }),
                    safety = escape_html_text(attempt.retry_safety.as_deref().unwrap_or("-")),
                    started = format_date_time(attempt.started_at),
                    finished = attempt
                        .finished_at
                        .map(format_date_time)
                        .unwrap_or_else(|| "-".into()),
                )
            })
            .collect();
        parts.push(format!(
            r#"<details><summary class="cursor-pointer text-xs font-semibold">Diagnostics: worker attempts</summary><div class="overflow-x-auto"><table class="table table-xs"><thead class="text-base-content"><tr><th>Attempt</th><th>Status</th><th>Stop</th><th>Failure</th><th>Retry safety</th><th>Started</th><th>Finished</th></tr></thead><tbody>{rows}</tbody></table></div></details>"#
        ));
    }
    if step.truncated {
        parts.push(
            r#"<p class="text-xs opacity-70">Some lists here are cut at 50 entries.</p>"#
                .to_owned(),
        );
    }
    format!(r#"<div class="space-y-3">{}</div>"#, parts.join(""))
}
