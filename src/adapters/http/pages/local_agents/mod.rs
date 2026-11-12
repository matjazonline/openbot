use super::{escape_html_attr as attr, escape_html_text as text};
use crate::application::local_agents::{management::*, *};
use crate::domain::{
    entities::{company::Company, thread::Thread},
    local_agents::*,
};
use uuid::Uuid;

mod machines;
mod profiles;
mod sessions;

pub struct Page<'a> {
    pub company: &'a Company,
    pub machines: &'a [LocalMachine],
    pub pairings: &'a [PairingAttempt],
    pub profiles: &'a RecordPage<LocalAgentProfile>,
    pub native_agents: &'a RecordPage<NativeAgentChoice>,
    pub machine: Option<&'a MachineDetails>,
    pub runtime: Option<&'a MachineRuntime>,
    pub profile: Option<&'a LocalAgentProfile>,
    pub provider: Option<LocalProvider>,
    pub model: Option<&'a ModelId>,
    pub conversation: Option<&'a Thread>,
    pub sessions: Option<&'a RecordPage<SessionSummary>>,
    pub session: Option<&'a SessionDetails>,
    pub test: Option<&'a LocalOperationView>,
    pub pairing: Option<&'a CreatedPairing>,
}

pub fn page(view: &Page<'_>) -> String {
    let company = view.company.id;
    let body = format!(
        r#"<main class="h-full overflow-auto p-6 mx-auto max-w-5xl space-y-6">
        <a class="link" href="/ui/companies?company_id={company}">Company settings</a>
        <h1 class="text-2xl font-bold">Local agents · {name}</h1>
        <p>Cloudroom runs native agents under the machine's local account. Local profiles are separate from Openbot agents and use the local account's files, tools, provider logins and permission policy.</p>
        {machines}{profiles}{start}{session}</main>"#,
        name = text(&view.company.name),
        machines = machines::render(view),
        profiles = profiles::render(view),
        start = sessions::start(view),
        session = view
            .session
            .map_or_else(String::new, |session| sessions::detail(company, session))
    );
    super::mailbox::ui_layout("Local agents", &body)
}

fn base(company: Uuid) -> String {
    format!("/ui/companies/{company}/local-agents")
}
fn navigation(view: &Page<'_>, query: &str) -> String {
    let context = view.conversation.map_or_else(String::new, |thread| {
        format!("&channel={}&conversation={}", thread.channel_id, thread.id)
    });
    format!("{}?{query}{context}", base(view.company.id))
}
fn conversation_fields(view: &Page<'_>) -> String {
    view.conversation.map_or_else(String::new, |thread| format!(
        r#"<input type="hidden" name="channel" value="{}"><input type="hidden" name="conversation" value="{}">"#,
        thread.channel_id, thread.id))
}
fn request_id() -> Uuid {
    Uuid::now_v7()
}
fn button(label: &str) -> String {
    super::onboarding::busy_submit_button(label)
}
fn form(action: &str, fields: &str, label: &str) -> String {
    format!(
        r#"<form method="post" action="{}" data-submit="busy-once" data-pending-label="Working…" class="space-y-3">{}{}</form>"#,
        attr(action),
        fields,
        button(label)
    )
}
fn enum_value(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .expect("closed enum serializes")
        .as_str()
        .expect("string enum")
        .to_owned()
}
fn option(value: &str, label: &str, selected: bool) -> String {
    format!(
        r#"<option value="{}"{}>{}</option>"#,
        attr(value),
        if selected { " selected" } else { "" },
        text(label)
    )
}

pub(crate) const PROFILE_PERMISSIONS_SCRIPT: &str = r#"
document.addEventListener('input', function (event) {
    var agent = event.target.closest('[data-local-agent-selector]');
    if (!agent) return;
    var form = agent.closest('form');
    var permissions = form.querySelector('[data-local-agent-permissions]');
    var fallback = form.querySelector('[data-local-agent-fallback]');
    if (!permissions || !fallback) return;
    if (!permissions.disabled && permissions.value) fallback.value = permissions.value;
    var usesAgent = agent.value.trim().length > 0;
    permissions.disabled = usesAgent;
    permissions.required = !usesAgent;
    fallback.disabled = !usesAgent;
});
"#;
