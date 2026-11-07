use crate::{
    adapters::{
        http::{
            auth::AuthenticatedUser,
            pages::{escape_html_attr, escape_html_text},
        },
        integrations::owner::verified_owner,
    },
    app_error::AppResult,
    application::{integrations::*, use_cases::integration_binding::*},
    domain::integrations::*,
};
use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;
pub(super) fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<dyn IntegrationGateway>: axum::extract::FromRef<S>,
    Arc<dyn IntegrationBindings>: axum::extract::FromRef<S>,
{
    Router::new()
        .route("/ui/profile/vault/binding-proposals", post(propose))
        .route(
            "/ui/profile/vault/binding-proposals/{proposal}",
            get(view).post(confirm),
        )
        .route("/ui/profile/vault/grants/{grant}/revoke", post(revoke))
        .layer(DefaultBodyLimit::max(8192))
}
async fn propose(
    State(bindings): State<Arc<dyn IntegrationBindings>>,
    user: AuthenticatedUser,
    Form(command): Form<ProposeBinding>,
) -> AppResult<Redirect> {
    let proposal = bindings.propose(user.id, command).await?;
    Ok(Redirect::to(&format!(
        "/ui/profile/vault/binding-proposals/{}",
        proposal.id
    )))
}
#[derive(Deserialize)]
struct AccountQuery {
    after: Option<Uuid>,
}
async fn view(
    State(bindings): State<Arc<dyn IntegrationBindings>>,
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Query(query): Query<AccountQuery>,
) -> AppResult<Html<String>> {
    let owner = verified_owner(&user);
    let proposal = bindings.proposal(&owner, id).await?;
    let page = gateway
        .list_accounts(
            &owner,
            PageRequest::new(query.after.map(AccountId::new), 50)?,
        )
        .await?;
    let options = page
        .accounts
        .iter()
        .filter(|a| a.status == AccountStatus::Ready)
        .map(|a| {
            format!(
                "<option value=\"{}:{}\">{}</option>",
                a.id.as_uuid(),
                a.authorization_revision.get(),
                escape_html_text(a.metadata.label())
            )
        })
        .collect::<String>();
    let more=page.next.map(|account|format!("<a class=\"link\" href=\"/ui/profile/vault/binding-proposals/{id}?after={}\">More accounts</a>",account.as_uuid())).unwrap_or_default();
    let content = format!(
        r##"<main id="vault-binding-consent" class="mx-auto max-w-3xl p-6"><h1 class="text-2xl">Confirm company credential use</h1><p>Company: {company}</p><p>Connection: {connection} ({kind})</p><p>App: {app}</p><p>Exact destination: {destination}</p><p>This connection may use the selected account for all its registered deterministic workflow operations. Company administrators cannot read the credential. Generic model tools cannot use it.</p><form method="post" action="/ui/profile/vault/binding-proposals/{id}" hx-post="/ui/profile/vault/binding-proposals/{id}" hx-target="#vault-binding-consent" hx-swap="outerHTML" hx-sync="this:drop"><label>Private account<select class="select" name="account_revision" required>{options}</select></label><button class="btn btn-primary" type="submit">Confirm connection use</button></form>{more}</main>"##,
        company = escape_html_text(&proposal.company_name),
        connection = proposal.connection,
        kind = proposal.kind.as_str(),
        app = escape_html_text(proposal.app.as_str()),
        destination = escape_html_text(proposal.destination.as_str())
    );
    Ok(Html(crate::adapters::http::pages::base_layout(
        "Confirm connection use",
        &content,
    )))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmationForm {
    account_revision: String,
}
async fn confirm(
    State(bindings): State<Arc<dyn IntegrationBindings>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Form(form): Form<ConfirmationForm>,
) -> AppResult<Response> {
    let (account, revision) = form
        .account_revision
        .split_once(':')
        .ok_or(IntegrationError::Invalid)?;
    let account = AccountId::new(Uuid::parse_str(account).map_err(|_| IntegrationError::Invalid)?);
    let revision =
        AuthorizationRevision::new(revision.parse().map_err(|_| IntegrationError::Invalid)?)?;
    bindings
        .confirm(verified_owner(&user), id, account, revision)
        .await?;
    Ok(navigate(&format!(
        "/ui/profile/vault/{}",
        account.as_uuid()
    )))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeForm {
    expected_revision: i64,
}
async fn revoke(
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Form(form): Form<RevokeForm>,
) -> AppResult<Response> {
    gateway
        .revoke_use_grant(
            &verified_owner(&user),
            UseGrantId::new(id),
            GrantRevision::new(form.expected_revision)?,
        )
        .await?;
    Ok(navigate("/ui/profile/vault"))
}
pub(super) fn grant_content(account: AccountId, page: &GrantInspection) -> String {
    let entries=page.grants.iter().map(|grant|format!(r##"<li><span>{app}: {destination} — {status}</span><p>{binding}</p><form method="post" action="/ui/profile/vault/grants/{id}/revoke" hx-post="/ui/profile/vault/grants/{id}/revoke" hx-sync="this:drop"><input type="hidden" name="expected_revision" value="{revision}"><button class="btn btn-sm" type="submit">Unbind connection</button></form></li>"##,
        binding=escape_html_text(grant.binding.as_str()),app=escape_html_text(grant.app.as_str()),destination=escape_html_text(grant.destination.as_str()),status=if grant.enabled{"Owner consent recorded; current membership and account authorization still apply"}else{"Revoked"},id=grant.id.as_uuid(),revision=grant.revision.get())).collect::<String>();
    let next = page
        .next
        .map(|id| {
            format!(
                "<a class=\"link\" href=\"/ui/profile/vault/{}?after={}\">More grants</a>",
                account.as_uuid(),
                id.as_uuid()
            )
        })
        .unwrap_or_default();
    format!(
        "<section class=\"p-6\"><h2>Company grants</h2><ul>{entries}</ul>{next}<a class=\"link\" href=\"/ui/profile/vault/{}\">Account details</a></section>",
        escape_html_attr(&account.as_uuid().to_string())
    )
}

fn navigate(target: &str) -> Response {
    let mut response = Redirect::to(target).into_response();
    if let Ok(value) = target.parse() {
        response.headers_mut().insert("HX-Redirect", value);
    }
    response
}

#[cfg(test)]
#[path = "vault_binding_route_tests.rs"]
mod tests;
