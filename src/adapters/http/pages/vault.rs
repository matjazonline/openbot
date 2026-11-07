use super::{escape_html_attr, escape_html_text};
use crate::domain::integrations::*;

pub fn vault_content(
    page: &AccountPage,
    message: Option<&str>,
    apps: &[crate::application::integrations::ConnectApp],
) -> String {
    let banner = message
        .map(|text| {
            format!(
                "<p role=\"status\" class=\"alert mb-4\">{}</p>",
                escape_html_text(text)
            )
        })
        .unwrap_or_default();
    let accounts = page
        .accounts
        .iter()
        .map(|a| account_card(a, apps))
        .collect::<String>();
    let next = page
        .next
        .map(|id| {
            format!(
                "<a class=\"link\" href=\"/ui/profile/vault?after={}\">Next page</a>",
                id.as_uuid()
            )
        })
        .unwrap_or_default();
    format!(
        r##"<div id="vault-content" class="mx-auto max-w-3xl p-6 lg:p-10">
        <a class="link" href="/ui/profile">Back to profile</a><h1 class="mb-4 text-2xl font-bold">Vault / Connected apps</h1>
        <p class="mb-4 text-sm">Private, server-encrypted storage. Saved secrets can be replaced and cannot be read back. Company connections require a separate owner confirmation.</p>
        {banner}{connect}{create}<div class="space-y-4">{accounts}</div>{next}</div>"##,
        connect = connect_apps(apps),
        create = create_form()
    )
}
fn create_form() -> String {
    r##"<section class="mb-6 rounded-box border border-base-300 p-5"><h2 class="text-lg font-semibold">Save a credential</h2>
        <form hx-post="/ui/profile/vault" hx-target="#vault-content" hx-swap="outerHTML" hx-sync="this:drop" class="space-y-3">
        <label class="block">Key<input class="input w-full" name="key" maxlength="128" required autocomplete="off"></label>
        <label class="block">Label<input class="input w-full" name="label" maxlength="256" required></label>
        <label class="block">Authentication<select class="select w-full" name="app"><option value="static-bearer">Bearer token</option><option value="static-api-key">API key (X-API-Key)</option></select></label>
        <label class="block">Secret<textarea class="textarea w-full" name="value" maxlength="16384" required autocomplete="off" spellcheck="false"></textarea></label>
        <p class="text-xs opacity-70">Enter a token or the adapter's typed JSON: token for bearer, api_key for API key.</p><button type="submit" class="btn btn-primary">Save credential</button></form></section>"##.to_string()
}
fn account_card(
    account: &ConnectedAccount,
    apps: &[crate::application::integrations::ConnectApp],
) -> String {
    let id = account.id.as_uuid();
    let revision = account.authorization_revision.get();
    let replace = if account.auth_method == AuthMethod::StaticSecret {
        replacement_form(account)
    } else {
        apps.iter().filter(|app|app.config==account.auth_config_id&&app.provider==account.provider_id).map(|app|format!("<form method=\"post\" action=\"/ui/profile/vault/apps/{}/connect\" hx-post=\"/ui/profile/vault/apps/{}/connect\" hx-sync=\"this:drop\" hx-disabled-elt=\"find button\"><input type=\"hidden\" name=\"account\" value=\"{id}\"><input type=\"hidden\" name=\"authorization\" value=\"{}\"><button class=\"btn btn-sm\" type=\"submit\">Reconnect {}</button><p>Company grants require fresh confirmation after reconnecting.</p></form>",escape_html_attr(app.id.as_str()),escape_html_attr(app.id.as_str()),account.authorization_revision.get(),escape_html_text(app.id.as_str()))).collect()
    };
    let configured = match account.status {
        AccountStatus::Ready => "Configured",
        AccountStatus::ReconnectRequired => "Reconnect required",
        AccountStatus::Disconnected => "Not configured",
    };
    format!(
        r##"<section class="rounded-box border border-base-300 p-5"><h2 class="text-lg font-semibold">{label}</h2><p class="font-mono text-xs">{key}</p><p>{configured}</p><p>{description}</p>
        <p class="text-xs opacity-70"><a class="link" href="/ui/profile/vault/{id}">Inspect company grants</a></p>{replace}
        <form hx-post="/ui/profile/vault/{id}/disconnect" hx-target="#vault-content" hx-swap="outerHTML" hx-sync="this:drop">
        <input type="hidden" name="expected_revision" value="{revision}"><button class="btn btn-sm" type="submit">Disconnect locally</button></form>
        <form hx-delete="/ui/profile/vault/{id}" hx-target="#vault-content" hx-swap="outerHTML" hx-sync="this:drop">
        <input type="hidden" name="expected_revision" value="{revision}"><button class="btn btn-sm" type="submit">Remove account</button></form></section>"##,
        label = escape_html_text(account.metadata.label()),
        key = escape_html_text(account.key.as_str()),
        description = escape_html_text(account.metadata.description().unwrap_or(""))
    )
}
fn replacement_form(account: &ConnectedAccount) -> String {
    let app = match account.provider_id.as_str() {
        "static-bearer" => "static-bearer",
        "static-api-key" => "static-api-key",
        _ => return String::new(),
    };
    format!(
        r##"<form hx-put="/ui/profile/vault/{id}" hx-target="#vault-content" hx-swap="outerHTML" hx-sync="this:drop" class="my-4 space-y-3">
        <input type="hidden" name="expected_revision" value="{revision}"><input type="hidden" name="key" value="{key}"><input type="hidden" name="app" value="{app}">
        <label class="block">Label<input class="input w-full" name="label" value="{label}" maxlength="256" required></label>
        <label class="block">Replacement secret<textarea class="textarea w-full" name="value" maxlength="16384" required autocomplete="off" spellcheck="false"></textarea></label><button type="submit" class="btn btn-primary">Replace credential</button></form>"##,
        id = account.id.as_uuid(),
        revision = account.revision.get(),
        key = escape_html_attr(account.key.as_str()),
        label = escape_html_attr(account.metadata.label())
    )
}

fn connect_apps(apps: &[crate::application::integrations::ConnectApp]) -> String {
    let forms=apps.iter().map(|app|format!("<form method=\"post\" action=\"/ui/profile/vault/apps/{}/connect\" hx-post=\"/ui/profile/vault/apps/{}/connect\" hx-sync=\"this:drop\" hx-disabled-elt=\"find button\"><button class=\"btn btn-primary\" type=\"submit\">Connect {}</button></form>",escape_html_attr(app.id.as_str()),escape_html_attr(app.id.as_str()),escape_html_text(app.id.as_str()))).collect::<String>();
    format!(
        "<section class=\"mx-auto max-w-3xl p-6\"><h2>Connect an app</h2><p>App consent saves a private account. Company access requires a separate owner confirmation.</p>{forms}</section>"
    )
}
