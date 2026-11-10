use super::*;

pub(super) async fn page(
    State(bindings): State<Arc<dyn IntegrationBindings>>,
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    user: AuthenticatedUser,
    Path(target): Path<BindingTarget>,
) -> AppResult<Html<String>> {
    let current = bindings.source_info(user.id, target).await?;
    let apps = gateway.binding_apps().await?;
    let choices = apps
        .iter()
        .filter(|app| {
            app.purposes.contains(&target.kind.purpose())
                && (app.destinations.is_empty() || app.destinations.contains(&current.destination))
        })
        .map(|app| {
            format!(
                "<option value=\"{}\">{}</option>",
                escape_html_attr(app.id.as_str()),
                escape_html_text(app.id.as_str())
            )
        })
        .collect::<String>();
    let configured = match &current.source {
        CurrentCredentialSource::Owned => {
            "<p>This connection uses its own authentication settings.</p>".to_owned()
        }
        CurrentCredentialSource::Vault {
            account,
            status: None,
        } => format!(
            "<p>Configured vault account: {account}. Account or consent is unavailable.</p>",
            account = account.as_uuid()
        ),
        CurrentCredentialSource::Vault {
            status: Some(status),
            ..
        } => {
            let account_status = match status.status {
                AccountStatus::Ready => "Ready",
                AccountStatus::ReconnectRequired => "Reconnect required",
                AccountStatus::Disconnected => "Disconnected",
            };
            format!(
                "<p>Configured account: {} · {} · {}. {}</p>",
                escape_html_text(&status.label),
                escape_html_text(status.app.as_str()),
                account_status,
                if status.authorization_current && status.grant_enabled {
                    "Owner consent recorded."
                } else {
                    "Fresh owner confirmation is required."
                }
            )
        }
    };
    let form = if choices.is_empty() {
        "<p>No registered app supports this connection type.</p>".to_owned()
    } else {
        format!(
            r#"<form data-submit="busy-once" data-pending-label="Requesting confirmation…" method="post" action="/ui/profile/vault/binding-proposals" class="space-y-3"><input type="hidden" name="company" value="{company}"><input type="hidden" name="connection" value="{connection}"><input type="hidden" name="kind" value="{kind}"><input type="hidden" name="expected_revision" value="{revision}"><label class="block">App<select class="select w-full" name="app" required>{choices}</select></label><p>The next page lists your compatible accounts and asks you to confirm this company's use. Selecting an app does not grant access.</p><button class="btn btn-primary" type="submit"><span data-label>Choose private account</span><span data-progress class="loading loading-spinner loading-xs hidden" aria-hidden="true"></span></button></form>"#,
            company = target.company,
            connection = target.connection,
            kind = target.kind.as_str(),
            revision = current.revision
        )
    };
    let content = format!(
        r#"<main class="mx-auto max-w-3xl p-6 space-y-4"><h1 class="text-2xl">Credential source</h1><p>Exact destination: {}</p>{configured}{form}<a class="link" href="/ui/profile/vault">Manage your private accounts</a></main>"#,
        escape_html_text(current.destination.as_str())
    );
    Ok(Html(crate::adapters::http::pages::base_layout(
        "Credential source",
        &content,
    )))
}
