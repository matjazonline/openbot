use super::*;

pub fn agent_tool_catalog(
    company: Uuid,
    agent: Uuid,
    kind: AgentToolKind,
    connection: Option<Uuid>,
    connections: &AgentConnectionPage,
    tools: &AgentToolPage,
) -> String {
    let form = format!("agent-catalog-form-{agent}");
    let kinds = [
        AgentToolKind::Builtin,
        AgentToolKind::Http,
        AgentToolKind::Mcp,
        AgentToolKind::Workflow,
    ]
    .into_iter()
    .map(|item| {
        format!(
            r#"<option value="{value}"{selected}>{label}</option>"#,
            value = kind_name(item),
            selected = if item == kind { " selected" } else { "" },
            label = match item {
                AgentToolKind::Builtin => "Built-in",
                AgentToolKind::Http => "Company HTTP",
                AgentToolKind::Mcp => "Reviewed MCP",
                AgentToolKind::Workflow => "Registered workflow",
            }
        )
    })
    .collect::<String>();
    let mut options = format!(
        r#"<option value="" disabled{}>Select a connection</option>"#,
        if connection.is_none() {
            " selected"
        } else {
            ""
        }
    );
    if let Some(id) = connection.filter(|id| !connections.items.iter().any(|item| item.id == *id)) {
        options.push_str(&format!(
            r#"<option value="{id}" selected>Selected connection {id} (outside this page)</option>"#
        ));
    }
    for item in &connections.items {
        options.push_str(&format!(
            r#"<option value="{id}"{selected}>{name} · {id}</option>"#,
            id = item.id,
            name = escape_html_text(&item.name),
            selected = if connection == Some(item.id) {
                " selected"
            } else {
                ""
            }
        ));
    }
    let connection_picker = if matches!(kind, AgentToolKind::Http | AgentToolKind::Mcp) {
        format!(
            r##"<label class="form-control">Connection<select name="connection" form="{form}" class="select select-sm" hx-get="/ui/agents/{agent}/tool-catalog?company_id={company}" hx-include="[form='{form}']" hx-params="kind,connection" hx-target="#agent-tool-catalog-{agent}" hx-swap="outerHTML" hx-sync="#agent-tool-catalog-{agent}:replace">{options}</select></label>"##
        )
    } else {
        String::new()
    };
    let rows = tools.items.iter().map(tool_option).collect::<String>();
    let connection_more = connections
        .next
        .map(|after| {
            catalog_more(
                company,
                agent,
                kind,
                connection,
                Some(after),
                None,
                "More connections",
            )
        })
        .unwrap_or_default();
    let operation_more = tools
        .next
        .as_ref()
        .map(|after| {
            catalog_more(
                company,
                agent,
                kind,
                connection,
                None,
                Some(after.as_str()),
                "More operations",
            )
        })
        .unwrap_or_default();
    format!(
        r##"<section id="agent-tool-catalog-{agent}" class="space-y-3 border-t border-base-300 pt-3" aria-label="Add an agent tool">
        <label class="form-control">Tool source<select name="kind" form="{form}" class="select select-sm" hx-get="/ui/agents/{agent}/tool-catalog?company_id={company}" hx-include="[form='{form}']" hx-params="kind" hx-target="#agent-tool-catalog-{agent}" hx-swap="outerHTML" hx-sync="#agent-tool-catalog-{agent}:replace">{kinds}</select></label>
        {connection_picker}{connection_more}<div class="space-y-2">{rows}</div>{operation_more}
        <p class="text-xs opacity-60">At most 50 results per page. Connections and reviewed operations are managed separately. <a class="link" href="/ui/mcp?company_id={company}">Manage MCP connections</a></p>
        <p class="text-xs opacity-70 [.htmx-request_&]:block hidden" role="status">Loading tool choices…</p></section>"##
    )
}

fn tool_option(tool: &AgentToolOption) -> String {
    let available = tool.availability == AgentToolAvailability::Configured;
    format!(
        r##"<div class="rounded border border-base-300 p-2"><span class="font-medium">{name}</span>
        <p class="break-all text-xs">{target}</p><p class="text-xs opacity-70">{availability}</p>
        <button type="button" class="btn btn-outline btn-xs" data-agent-tool-add data-choice="{choice}" data-alias="{alias}"{disabled}>Add to draft</button></div>"##,
        name = escape_html_text(tool.name.as_str()),
        target = escape_html_text(&tool.choice.target_label()),
        availability = availability_label(tool.availability),
        alias = escape_html_attr(tool.name.as_str()),
        choice =
            escape_html_attr(&serde_json::to_string(&tool.choice).expect("tool choice serializes")),
        disabled = if available { "" } else { " disabled" }
    )
}

fn catalog_more(
    company: Uuid,
    agent: Uuid,
    kind: AgentToolKind,
    connection: Option<Uuid>,
    after_connection: Option<Uuid>,
    after_operation: Option<&str>,
    label: &str,
) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("company_id", &company.to_string())
        .append_pair("kind", kind_name(kind));
    if let Some(connection) = connection {
        query.append_pair("connection", &connection.to_string());
    }
    if let Some(after) = after_connection {
        query.append_pair("after_connection", &after.to_string());
    }
    if let Some(after) = after_operation {
        query.append_pair("after_operation", after);
    }
    let query = escape_html_attr(&query.finish());
    format!(
        r##"<button type="button" class="btn btn-ghost btn-xs" hx-get="/ui/agents/{agent}/tool-catalog?{query}" hx-params="none" hx-target="#agent-tool-catalog-{agent}" hx-swap="outerHTML" hx-sync="#agent-tool-catalog-{agent}:replace">{label} · results continue</button>"##
    )
}
