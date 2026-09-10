//! MCP settings share the UI shell and use native POST forms as well as the JSON API.
use super::{escape_html_attr as esc, escape_html_text};
use crate::{entities::mcp::*, use_cases::mcp::McpSelectingAgent};
use uuid::Uuid;

#[derive(Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionDraft {
    #[serde(default)]
    pub expected_revision: i64,
    pub slug: String,
    pub endpoint_url: String,
    pub auth_type: String,
    pub enabled: Option<String>,
    #[serde(default)]
    pub grants: Vec<String>,
}
impl DefinitionDraft {
    pub fn stored(c: &CompanyMcpConnection) -> Self {
        Self {
            expected_revision: c.revision,
            slug: c.slug.to_string(),
            endpoint_url: c.endpoint.as_str().into(),
            auth_type: c.auth.as_str().into(),
            enabled: c.enabled.then(|| "on".into()),
            grants: c.tool_grants.iter().map(|n| n.as_str().into()).collect(),
        }
    }
}
pub fn shell(title: &str, company: Uuid, body: &str, message: Option<&str>) -> String {
    let message = message
        .map(|m| {
            format!(
                "<p role=\"status\" class=\"alert mb-4\">{}</p>",
                escape_html_text(m)
            )
        })
        .unwrap_or_default();
    super::mailbox::ui_layout(
        title,
        &format!(
            r#"<main class="h-full overflow-auto p-6 mx-auto max-w-4xl space-y-4"><a class="link" href="/ui/companies?company_id={company}">Company settings</a><h1 class="text-2xl font-bold">{title}</h1>{message}{body}</main>"#
        ),
    )
}
pub fn catalog(
    company: Uuid,
    connections: &[CompanyMcpConnection],
    draft: &DefinitionDraft,
) -> String {
    let items = connections.iter().map(|c| format!(r#"<li><a class="link" href="/ui/companies/{company}/mcp-connections/{}">{}</a> · {}</li>"#,c.id,escape_html_text(&c.slug),if c.enabled {"Enabled"} else {"Disabled"})).collect::<String>();
    format!(
        "<p>Streamable HTTP · no authentication or bearer token. Saving validates settings locally. Test connection checks the server without calling tools.</p><ul>{items}</ul><h2 class=\"text-xl\">Add MCP server</h2>{}",
        definition_form(company, None, draft)
    )
}
pub fn detail(
    company: Uuid,
    connection: &CompanyMcpConnection,
    agents: &[McpSelectingAgent],
    draft: &DefinitionDraft,
) -> String {
    let id = connection.id;
    let revision = connection.revision;
    let used = agents.iter().map(|a| format!("<li><a class=\"link\" href=\"/ui/agents?company_id={company}&amp;agent_id={}\">{}</a></li>",a.id,escape_html_text(&a.name))).collect::<String>();
    let fields = definition_form(company, Some(connection), draft);
    let credentials = if connection.auth == McpAuth::Bearer {
        format!(
            r#"<h2 class="text-xl">Bearer token</h2><p>Secret {secret}. Tokens are write-only.</p><form data-submit="busy-once" data-pending-label="Working…" method="post" action="/ui/companies/{company}/mcp-connections/{id}/credential" class="space-y-3"><input type="hidden" name="expected_revision" value="{revision}"><input class="input w-full" type="password" name="token" maxlength="16384" autocomplete="new-password" aria-label="Bearer token"><p>Leave blank to remove the stored token.</p>{button}</form>"#,
            secret = if connection.secret_set {
                "set"
            } else {
                "not set"
            },
            button = button("Update token")
        )
    } else {
        String::new()
    };
    format!(
        r#"<a class="link" href="/ui/companies/{company}/mcp-connections">All MCP servers</a><p>Changes apply to every selecting agent. Endpoint changes clear the token and discovery.</p><h2 class="text-xl">Selected by</h2><ul>{used}</ul>{fields}{credentials}<form data-submit="busy-once" data-pending-label="Working…" method="post" action="/ui/companies/{company}/mcp-connections/{id}/refresh"><input type="hidden" name="expected_revision" value="{revision}">{refresh}</form><p>Refresh removes grants for missing or changed schemas. Review tools before granting them again.</p><form data-submit="busy-once" data-pending-label="Working…" method="post" action="/ui/companies/{company}/mcp-connections/{id}/delete"><input type="hidden" name="expected_revision" value="{revision}">{delete}</form>"#,
        refresh = button("Test connection / Refresh tools"),
        delete = button("Delete server")
    )
}
fn definition_form(
    company: Uuid,
    connection: Option<&CompanyMcpConnection>,
    draft: &DefinitionDraft,
) -> String {
    let action = connection
        .map(|c| format!("/ui/companies/{company}/mcp-connections/{}", c.id))
        .unwrap_or_else(|| format!("/ui/companies/{company}/mcp-connections"));
    let grants = connection.map(|c| c.discovered_tools.iter().map(|tool| {
        let checked = if draft.grants.iter().any(|g| g == tool.name.as_str()) {" checked"}else{""};
        format!("<label class=\"flex gap-2\"><input class=\"checkbox\" type=\"checkbox\" name=\"grants\" value=\"{}\"{checked}>{}</label><p>{}</p><details><summary>Input schema</summary><pre class=\"overflow-auto\">{}</pre></details>",esc(tool.name.as_str()),escape_html_text(tool.name.as_str()),escape_html_text(&tool.description),escape_html_text(&tool.input_schema.to_string()))
    }).collect::<String>()).unwrap_or_default();
    format!(
        r#"<form data-submit="busy-once" data-pending-label="Working…" method="post" action="{action}" class="space-y-3"><input type="hidden" name="expected_revision" value="{revision}"><label class="block">Name<input class="input w-full" name="slug" required maxlength="100" value="{slug}"></label><label class="block">Endpoint URL<input class="input w-full" type="url" name="endpoint_url" required maxlength="2048" value="{endpoint}"></label><label class="block">Authentication<select class="select" name="auth_type"><option value="none">None</option><option value="bearer"{bearer}>Bearer token</option></select></label><label class="flex gap-2"><input class="checkbox" type="checkbox" name="enabled"{enabled}>Enabled</label><fieldset class="space-y-2"><legend>Granted tools</legend>{grants}</fieldset>{button}</form>"#,
        revision = draft.expected_revision,
        slug = esc(&draft.slug),
        endpoint = esc(&draft.endpoint_url),
        bearer = if draft.auth_type == "bearer" {
            " selected"
        } else {
            ""
        },
        enabled = if draft.enabled.is_some() {
            " checked"
        } else {
            ""
        },
        button = button("Save definition")
    )
}
fn button(label: &str) -> String {
    format!(
        "<button type=\"submit\" class=\"btn btn-primary\"><span data-label>{label}</span></button>"
    )
}
pub fn selection(
    company: Uuid,
    selection: &AgentMcpSelection,
    connections: &[CompanyMcpConnection],
    draft: Option<&[Uuid]>,
) -> String {
    let selected = draft.unwrap_or(&selection.connection_ids);
    let mut options = connections
        .iter()
        .map(|c| {
            format!(
                "<option value=\"{}\"{}{}>{} {}</option>",
                c.id,
                if selected.contains(&c.id) {
                    " selected"
                } else {
                    ""
                },
                if !c.enabled && !selection.connection_ids.contains(&c.id) {
                    " disabled"
                } else {
                    ""
                },
                escape_html_text(&c.slug),
                if c.enabled { "" } else { "(unavailable)" }
            )
        })
        .collect::<String>();
    for id in selected
        .iter()
        .filter(|id| !connections.iter().any(|c| &c.id == *id))
    {
        options.push_str(&format!(
            "<option selected value=\"{id}\">Deleted server ({id}) — remove this selection</option>"
        ));
    }
    format!(
        r#"<a class="link" href="/ui/agents?company_id={company}&amp;agent_id={agent}">Agent settings</a><p>Select up to eight company MCP servers. Enabled grants require Rig. Disabled selections remain visible so you can remove them.</p><form data-submit="busy-once" data-pending-label="Working…" method="post" class="space-y-3"><input type="hidden" name="expected_revision" value="{revision}"><select class="select w-full h-64" name="mcp_connection_ids" multiple aria-label="MCP servers">{options}</select>{button}</form>"#,
        agent = selection.agent_id,
        revision = selection.revision,
        button = button("Save selections")
    )
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
