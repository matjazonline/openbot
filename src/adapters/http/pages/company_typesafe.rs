//! Secret-free company connection forms; no credential is ever echoed.
use super::{escape_html_attr, escape_html_text};
use crate::application::workflow::jev::JevConnection;
use uuid::Uuid;

pub fn loader(company: Uuid) -> String {
    format!(
        r##"<section class="mt-4 rounded-box border border-base-300 p-3" id="typesafe-connections"
        hx-get="/ui/companies/{company}/typesafe-connections" hx-trigger="load" hx-swap="outerHTML">
        <h3 class="font-semibold">TypeSafe Jev connections</h3><p role="status">Loading connections…</p>
    </section>"##
    )
}

pub fn connections(company: Uuid, connections: &[JevConnection], message: Option<&str>) -> String {
    let rows: String = connections
        .iter()
        .map(|connection| row(company, connection))
        .collect();
    let message = message
        .map(|message| format!(r#"<p role="status">{}</p>"#, escape_html_text(message)))
        .unwrap_or_default();
    format!(
        r##"<section id="typesafe-connections" class="mt-4 space-y-3 rounded-box border border-base-300 p-3">
        <h3 class="font-semibold">TypeSafe Jev connections</h3>
        <p class="text-sm">For workflow Jev evaluations. Select exact model versions in source and allow them here. Ready means the connection is enabled. Credential decryption and authorization are checked when a step uses it; this list does not verify the key with TypeSafe. Images are experimental on Jev 1.13/1.14; Box requires 1.14.</p>
        {message}{rows}
        <form hx-post="/ui/companies/{company}/typesafe-connections" hx-target="#typesafe-connections" hx-swap="outerHTML" hx-disabled-elt="find button" class="space-y-2">
            <h4 class="font-semibold">New connection</h4>
            {fields}{key}{save}
        </form>
    </section>"##,
        fields = fields("jev-1.14.0", true),
        key = key(),
        save = save("Create connection")
    )
}

fn row(company: Uuid, connection: &JevConnection) -> String {
    let id = connection.id;
    let revision = connection.revision;
    let models = connection
        .models
        .iter()
        .map(|model| model.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r##"<div class="space-y-2 border-t border-base-300 pt-3">
        <p class="text-sm"><span class="font-mono">{id}</span> · {readiness} · revision {revision}</p>
        <form hx-post="/ui/companies/{company}/typesafe-connections/{id}" hx-target="#typesafe-connections" hx-swap="outerHTML" hx-disabled-elt="find button" class="space-y-2">
            <input type="hidden" name="expected_revision" value="{revision}">{fields}{save}
        </form>
        <form hx-post="/ui/companies/{company}/typesafe-connections/{id}/rotate" hx-target="#typesafe-connections" hx-swap="outerHTML" hx-disabled-elt="find button" class="space-y-2">
            <input type="hidden" name="expected_revision" value="{revision}">{key}{rotate}
        </form>
    </div>"##,
        readiness = if connection.ready {
            "ready"
        } else {
            "not ready"
        },
        fields = fields(&models, connection.enabled),
        save = save("Save models and availability"),
        key = key(),
        rotate = save("Rotate API key")
    )
}

fn fields(models: &str, enabled: bool) -> String {
    format!(
        r#"<label class="form-control"><span class="text-xs">Allowed versioned models (comma separated)</span><input class="input w-full font-mono" name="models" value="{}" required maxlength="4096"></label>
        <label class="label"><input type="checkbox" class="checkbox checkbox-sm" name="enabled" value="true"{}>Enabled (uncheck to revoke provider use)</label>"#,
        escape_html_attr(models),
        if enabled { " checked" } else { "" }
    )
}

fn key() -> &'static str {
    r#"<label class="form-control"><span class="text-xs">TypeSafe API key</span><input class="input w-full" type="password" name="api_key" value="" required maxlength="4096" autocomplete="new-password"></label>"#
}

fn save(label: &str) -> String {
    format!(
        r#"<button type="submit" class="btn btn-primary btn-sm"><span class="[.htmx-request_&]:hidden">{label}</span><span class="hidden [.htmx-request_&]:inline" role="status">Saving…</span></button>"#
    )
}
