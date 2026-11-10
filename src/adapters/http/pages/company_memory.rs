//! The company memory opt-in (U1): status, enable/disable, and the lifecycle refusals operators
//! must see (E1). Owner/admin only; nothing secret is ever rendered.
use super::escape_html_text;
use crate::application::memory::{
    MemoryAgentCapture, MemoryCaptureKind, MemoryCapturePolicy, MemoryLifecycleSummary,
    MemorySettingsView, MemoryTenantState, MemoryTenantView,
};
use uuid::Uuid;

pub fn loader(company: Uuid) -> String {
    format!(
        r##"<section class="mt-4 rounded-box border border-base-300 p-3" id="company-memory"
        hx-get="/ui/companies/{company}/memory" hx-trigger="load" hx-swap="outerHTML">
        <h3 class="font-semibold">Memory</h3><p role="status">Loading memory settings…</p>
    </section>"##
    )
}

pub fn denied() -> String {
    r#"<section id="company-memory" class="mt-4 rounded-box border border-base-300 p-3"><h3 class="font-semibold">Memory</h3><p role="status">Memory settings require company owner or admin permission.</p></section>"#.into()
}

/// The fixed explanation shown with the switch (E3 declassification risk, U1 disable).
const EXPLANATION: &str = "Agents can recall and save notes across conversations through the \
     memory service. Saved notes keep the access level of what produced them, but text an owner \
     or admin writes into agent instructions, skills, workflow literals or schedule parameters is \
     treated as public (level 0): do not type classified facts there. Disabling stops new recall, \
     saves and capture and disables the service tenant; saved data is retained, and enabling \
     again revives nothing that was pending.";

pub fn section(company: Uuid, view: &MemorySettingsView, message: Option<&str>) -> String {
    let message = message
        .map(|message| {
            format!(
                r#"<p role="status" class="text-sm">{}</p>"#,
                escape_html_text(message)
            )
        })
        .unwrap_or_default();
    let body = match (&view.tenant, view.configured) {
        (None, None) => {
            "<p class=\"text-sm\">The memory service is not configured for this deployment.</p>"
                .to_owned()
        }
        (None, Some(_)) => format!(
            "<p class=\"text-sm\">Memory is off.</p>{}",
            command(company, "enable", 0, "Enable memory", "Enabling…")
        ),
        (Some(tenant), configured) => tenant_body(company, tenant, configured),
    };
    format!(
        r##"<section id="company-memory" class="mt-4 space-y-3 rounded-box border border-base-300 p-3" aria-live="polite">
        <h3 class="font-semibold">Memory</h3>
        <p class="text-sm">{explanation}</p>
        {message}{body}
    </section>"##,
        explanation = escape_html_text(EXPLANATION),
    )
}

fn tenant_body(company: Uuid, tenant: &MemoryTenantView, configured: Option<Uuid>) -> String {
    let (state, action) = match tenant.state {
        MemoryTenantState::Enabled => (
            "enabled",
            command(
                company,
                "disable",
                tenant.revision,
                "Disable memory",
                "Disabling…",
            ),
        ),
        MemoryTenantState::Disabled if configured.is_some() => (
            "disabled",
            command(
                company,
                "enable",
                tenant.revision,
                "Enable memory",
                "Enabling…",
            ),
        ),
        MemoryTenantState::Disabled => ("disabled", String::new()),
    };
    // A namespace refused because of its tenant is a tenant problem (review p7b r3 m1).
    let tenant_refused = if tenant.problems.iter().any(|problem| {
        problem.state == "failed" && problem.outcome_code.as_deref() == Some("tenant_conflict")
    }) {
        "<p class=\"text-sm text-error\">The memory service reports this company's tenant as \
         deleted or managed elsewhere; namespaces cannot be provisioned. The operator must \
         resolve it.</p>"
    } else {
        ""
    };
    // The owner's re-drive (review p7b r2 m3), offered only when something failed.
    let failed = tenant
        .tenant
        .iter()
        .chain(&tenant.problems)
        .any(|summary| summary.state == "failed" && summary.desired != "absent");
    let retry = if failed && tenant.state == MemoryTenantState::Enabled && configured.is_some() {
        command(
            company,
            "retry",
            tenant.revision,
            "Retry failed provisioning",
            "Retrying…",
        )
    } else {
        String::new()
    };
    let managed = match configured {
        Some(id) if id != tenant.provisioner_credential_id => {
            "<p class=\"text-sm text-error\">Managed by another provisioner credential: the \
             lifecycle worker refuses this company until the operator resolves it.</p>"
        }
        None => {
            "<p class=\"text-sm\">The memory service is not configured: changes are recorded \
                 and reconciled once it is.</p>"
        }
        Some(_) => "",
    };
    let generation = match tenant.generation_cap {
        Some(0) => "generation off (operator cap 0)".to_owned(),
        Some(cap) => format!("generation cap {cap} tokens per day"),
        None => "generation cap not yet known".to_owned(),
    };
    let remote = tenant
        .tenant
        .as_ref()
        .map(|summary| format!("<li>Service tenant: {}</li>", summary_text(summary)))
        .unwrap_or_default();
    let problems: String = tenant
        .problems
        .iter()
        .map(|problem| format!("<li>Namespace: {}</li>", summary_text(problem)))
        .collect();
    let more = if tenant.more_problems {
        "<li>More namespace problems are not listed.</li>"
    } else {
        ""
    };
    format!(
        r#"<p class="text-sm">Memory is {state} (revision {revision}, changed {changed}).</p>{managed}{tenant_refused}
        <ul class="text-sm list-disc pl-5">{remote}<li>Namespaces: {ready} ready, {provisioning} provisioning, {removing} removing</li><li>{generation}</li>{problems}{more}</ul>
        {action}{retry}{capture}"#,
        capture = capture_body(company, tenant),
        revision = tenant.revision,
        changed = escape_html_text(&tenant.changed_at.format("%Y-%m-%d %H:%M UTC").to_string()),
        ready = tenant.namespaces.ready,
        provisioning = tenant.namespaces.provisioning,
        removing = tenant.namespaces.removing,
    )
}

/// Shown on the page and when `threads` capture is turned on (E8, U3).
pub const THREAD_CAPTURE_TEXT: &str = "Thread capture turns every completed exchange of the agent \
     in a channel thread into the agent's own memory, whatever started the run, customer \
     messages included. Captured thread memories are labelled at the \
     thread's lowest-reader level; any reader cleared to that level, in any channel the agent \
     serves, can recall them; and a thread with a public or external reader is captured at \
     level 0.";

const PRIVATE_CAPTURE_TEXT: &str = "Private-session capture turns a person's own manual runs \
     in a private session that is not attached to a channel or thread into that person's \
     private memory; scheduled, message-triggered and child runs never reach private memory. \
     Child branches, failed or cancelled runs and samples are never captured.";

/// Per-agent capture policies (E8): off by default, owner/admin only, each change with a reason.
fn capture_body(company: Uuid, tenant: &MemoryTenantView) -> String {
    let idle = match tenant.generation_cap {
        Some(cap) if cap > 0 => "",
        _ => {
            "<p class=\"text-sm\">Capture stays idle while generation is off for this tenant \
              (the operator's cap is 0 or not yet known).</p>"
        }
    };
    let agents: String = tenant
        .capture
        .iter()
        .map(|agent| capture_agent(company, agent))
        .collect();
    let agents = if agents.is_empty() {
        "<p class=\"text-sm\">No agents yet.</p>".to_owned()
    } else {
        agents
    };
    let more = if tenant.more_agents {
        "<p class=\"text-sm\">More agents are not listed.</p>"
    } else {
        ""
    };
    format!(
        r#"<div class="space-y-2" id="company-memory-capture"><h4 class="font-semibold">Capture</h4>
        <p class="text-sm">{private}</p><p class="text-sm">{thread}</p>{idle}{agents}{more}</div>"#,
        private = escape_html_text(PRIVATE_CAPTURE_TEXT),
        thread = escape_html_text(THREAD_CAPTURE_TEXT),
    )
}

fn capture_agent(company: Uuid, agent: &MemoryAgentCapture) -> String {
    let forms: String = agent
        .policies
        .iter()
        .map(|policy| capture_form(company, agent, policy))
        .collect();
    format!(
        r#"<fieldset class="rounded-box border border-base-300 p-2"><legend class="text-sm font-semibold">{name}</legend>{forms}</fieldset>"#,
        name = escape_html_text(&agent.name),
    )
}

fn capture_form(company: Uuid, agent: &MemoryAgentCapture, policy: &MemoryCapturePolicy) -> String {
    let (label, kind) = match policy.kind {
        MemoryCaptureKind::PrivateSessions => ("Private sessions", "private_sessions"),
        MemoryCaptureKind::Threads => ("Threads", "threads"),
    };
    let id = format!("capture-{}-{kind}", agent.agent.simple());
    let state = if policy.enabled { "on" } else { "off" };
    let changed = policy
        .changed
        .as_ref()
        .map(|changed| {
            escape_html_text(&format!(
                " Last changed {} for: {}",
                changed.at.format("%Y-%m-%d %H:%M UTC"),
                changed.reason
            ))
        })
        .unwrap_or_default();
    let (next, button) = if policy.enabled {
        ("false", "Turn off")
    } else {
        ("true", "Turn on")
    };
    let limits = policy.limits;
    format!(
        r##"<form class="mt-1 space-y-1" hx-post="/ui/companies/{company}/memory/capture" hx-target="#company-memory" hx-swap="outerHTML" hx-disabled-elt="find button">
            <p class="text-sm">{label}: capture is {state} (revision {revision}).{changed}</p>
            <input type="hidden" name="agent" value="{agent}"><input type="hidden" name="kind" value="{kind}">
            <input type="hidden" name="enabled" value="{next}"><input type="hidden" name="expected_revision" value="{revision}">
            <div class="grid grid-cols-2 gap-2 max-w-2xl">
                <div class="flex flex-col gap-1"><label class="text-sm" for="{id}-allowance">Allowance tokens per window</label>
                <input class="input input-sm w-full" id="{id}-allowance" name="allowance_tokens" type="number" min="1" max="1000000" value="{allowance}" required></div>
                <div class="flex flex-col gap-1"><label class="text-sm" for="{id}-output">Maximum output tokens</label>
                <input class="input input-sm w-full" id="{id}-output" name="max_output_tokens" type="number" min="1" max="8192" value="{output}" required></div>
                <div class="flex flex-col gap-1"><label class="text-sm" for="{id}-expiry">Request expiry (hours)</label>
                <input class="input input-sm w-full" id="{id}-expiry" name="expiry_hours" type="number" min="1" max="168" value="{expiry}" required></div>
                <div class="flex flex-col gap-1"><label class="text-sm" for="{id}-reason">Reason</label>
                <input class="input input-sm w-full" id="{id}-reason" name="reason" maxlength="500" required></div>
            </div>
            <button type="submit" class="btn btn-sm"><span class="[.htmx-request_&]:hidden">{button}</span><span class="hidden [.htmx-request_&]:inline" role="status">Saving…</span></button>
        </form>"##,
        agent = agent.agent,
        revision = policy.revision,
        allowance = limits.allowance_tokens,
        output = limits.max_output_tokens,
        expiry = limits.expiry_hours,
    )
}

fn summary_text(summary: &MemoryLifecycleSummary) -> String {
    let outcome = summary
        .outcome_code
        .as_deref()
        .map(|code| format!(", {code}"))
        .unwrap_or_default();
    escape_html_text(&format!(
        "{} (wanted {}, seen {}, attempts {}{outcome})",
        summary.state, summary.desired, summary.observed, summary.attempts
    ))
}

fn command(company: Uuid, action: &str, revision: i64, label: &str, busy: &str) -> String {
    format!(
        r##"<form hx-post="/ui/companies/{company}/memory" hx-target="#company-memory" hx-swap="outerHTML" hx-disabled-elt="find button">
            <input type="hidden" name="action" value="{action}"><input type="hidden" name="expected_revision" value="{revision}">
            <button type="submit" class="btn btn-primary btn-sm"><span class="[.htmx-request_&]:hidden">{label}</span><span class="hidden [.htmx-request_&]:inline" role="status">{busy}</span></button>
        </form>"##
    )
}

#[cfg(test)]
mod browser_fixtures {
    use super::*;
    use crate::application::memory::{
        CaptureLimits, MemoryCaptureChangeRecord, MemoryNamespaceCounts, MemoryTenantState,
    };

    fn view(threads: Option<&str>) -> MemorySettingsView {
        let policy = |kind, changed: Option<&str>| MemoryCapturePolicy {
            kind,
            enabled: changed.is_some(),
            limits: CaptureLimits::DEFAULT,
            revision: i64::from(changed.is_some()),
            changed: changed.map(|reason| MemoryCaptureChangeRecord {
                by: Uuid::from_u128(5),
                reason: reason.into(),
                at: chrono::Utc::now(),
            }),
        };
        MemorySettingsView {
            configured: Some(Uuid::from_u128(4)),
            tenant: Some(MemoryTenantView {
                state: MemoryTenantState::Enabled,
                revision: 1,
                provisioner_credential_id: Uuid::from_u128(4),
                generation_cap: Some(100_000),
                changed_by: Uuid::from_u128(5),
                changed_at: chrono::Utc::now(),
                tenant: None,
                namespaces: MemoryNamespaceCounts {
                    ready: 2,
                    provisioning: 0,
                    removing: 0,
                },
                problems: Vec::new(),
                more_problems: false,
                capture: vec![MemoryAgentCapture {
                    agent: Uuid::from_u128(6),
                    name: "Support <agent>".into(),
                    policies: vec![
                        policy(MemoryCaptureKind::PrivateSessions, None),
                        policy(MemoryCaptureKind::Threads, threads),
                    ],
                }],
                more_agents: false,
            }),
        }
    }

    /// Writes the company page with the memory section and the section a thread-capture save
    /// returns, plus the production scripts, for `scripts/tests/memory-settings-browser.mjs`.
    #[test]
    #[ignore = "browser fixture: the memory settings section and its capture forms"]
    fn dump_memory_settings_browser_fixtures() {
        let root = std::path::PathBuf::from(
            std::env::var("MEMORY_SETTINGS_BROWSER_FIXTURES")
                .expect("MEMORY_SETTINGS_BROWSER_FIXTURES"),
        );
        std::fs::create_dir_all(&root).unwrap();
        let company = Uuid::from_u128(2);
        let page = super::super::layout::base_layout(
            "Company settings",
            &format!("<main>{}</main>", section(company, &view(None), None)),
        );
        std::fs::write(root.join("memory.html"), page).unwrap();
        let saved = section(
            company,
            &view(Some("Thread pilot")),
            Some(&format!("Capture settings saved. {THREAD_CAPTURE_TEXT}")),
        );
        std::fs::write(root.join("saved.html"), saved).unwrap();
        std::fs::write(
            root.join("app.js"),
            super::super::mailbox::application_javascript(),
        )
        .unwrap();
        std::fs::write(
            root.join("theme-init.js"),
            super::super::mailbox::theme_init_javascript(),
        )
        .unwrap();
    }
}
