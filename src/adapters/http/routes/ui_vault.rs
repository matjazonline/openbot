use crate::{
    adapters::{
        http::{app_state::AppState, auth::AuthenticatedUser, pages},
        integrations::owner::verified_owner,
    },
    app_error::AppResult,
    application::integrations::*,
    domain::integrations::*,
    entities::value_objects::EmailAddress,
    infra::config::AppConfig,
    use_cases::user::UserUseCases,
};
use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    response::Html,
    routing::{get, post},
};
use secrecy::SecretString;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;
pub fn router() -> Router<AppState> {
    mutation_router()
        .merge(super::ui_vault_bindings::router())
        .merge(super::ui_vault_connect::router())
        .route("/ui/profile/vault", get(page))
        .route("/ui/profile/vault/{entry_id}", get(detail))
}
pub(super) fn mutation_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<dyn IntegrationGateway>: axum::extract::FromRef<S>,
{
    Router::new()
        .route("/ui/profile/vault", post(create))
        .route(
            "/ui/profile/vault/{entry_id}",
            axum::routing::put(replace).delete(remove),
        )
        .route("/ui/profile/vault/{entry_id}/disconnect", post(disconnect))
        .layer(DefaultBodyLimit::max(64 * 1024))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StaticForm {
    key: String,
    app: String,
    label: String,
    value: SecretString,
    expected_revision: Option<i64>,
}
impl StaticForm {
    fn input(self) -> IntegrationResult<StaticAccountInput> {
        Ok(StaticAccountInput {
            key: AccountKey::parse(self.key)?,
            app: AppId::parse(self.app)?,
            metadata: AccountMetadata::new(self.label, None)?,
            value: self.value,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RevisionInput {
    expected_revision: i64,
}
#[derive(Deserialize)]
struct ListQuery {
    after: Option<Uuid>,
    connect_notice: Option<String>,
}
async fn fragment(
    gateway: &dyn IntegrationGateway,
    owner: &VerifiedOwner,
    message: Option<&str>,
) -> AppResult<Html<String>> {
    let page = gateway
        .list_accounts(owner, PageRequest::new(None, 50)?)
        .await?;
    let html = pages::vault::vault_content(&page, message, &gateway.connectable_apps().await?);
    Ok(Html(html))
}
async fn page(
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Query(query): Query<ListQuery>,
) -> AppResult<Html<String>> {
    let owner = verified_owner(&user);
    let entries = gateway
        .list_accounts(
            &owner,
            PageRequest::new(query.after.map(AccountId::new), 50)?,
        )
        .await?;
    let account = super::ui::load_account(&users, user.id).await?;
    let email = EmailAddress::from(account.email.as_str());
    let viewer = super::ui::workspace_user(&account, &email, &config);
    let notice=query.connect_notice.as_deref().and_then(|n|match n {
        "invalid_state"=>Some("Authorization could not be completed. Check your current session and start a new attempt."),
        "account_changed"=>Some("The account changed during authorization. Reload and start a new reconnect attempt."),
        "unavailable"=>Some("Vault authorization is temporarily unavailable. Try again or contact the operator."),
        _=>None,
    });
    let content = pages::vault::vault_content(&entries, notice, &gateway.connectable_apps().await?);
    Ok(Html(pages::profile_page(&pages::ProfilePage {
        user: &viewer,
        company: None,
        pane_html: &content,
    })))
}
#[derive(Deserialize)]
struct GrantQuery {
    after: Option<Uuid>,
}
async fn detail(
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Query(query): Query<GrantQuery>,
) -> AppResult<Html<String>> {
    let owner = verified_owner(&user);
    let grants = gateway
        .inspect_grants(&owner, AccountId::new(id), query.after.map(UseGrantId::new))
        .await?;
    let account = gateway.get_account(&owner, AccountId::new(id)).await?;
    let mut html = pages::vault::vault_content(
        &AccountPage {
            accounts: vec![account],
            next: None,
        },
        None,
        &gateway.connectable_apps().await?,
    );
    html.push_str(&super::ui_vault_bindings::grant_content(
        AccountId::new(id),
        &grants,
    ));
    Ok(Html(pages::base_layout("Vault account", &html)))
}
pub(super) async fn create(
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    user: AuthenticatedUser,
    Form(form): Form<StaticForm>,
) -> AppResult<Html<String>> {
    let owner = verified_owner(&user);
    gateway.create_static_account(&owner, form.input()?).await?;
    fragment(gateway.as_ref(), &owner, Some("Credential saved")).await
}
pub(super) async fn replace(
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Form(form): Form<StaticForm>,
) -> AppResult<Html<String>> {
    let owner = verified_owner(&user);
    let revision = AccountRevision::new(form.expected_revision.ok_or(IntegrationError::Invalid)?)?;
    gateway
        .replace_static_account(&owner, AccountId::new(id), revision, form.input()?)
        .await?;
    fragment(gateway.as_ref(), &owner, Some("Credential replaced")).await
}
pub(super) async fn remove(
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Query(form): Query<RevisionInput>,
) -> AppResult<Html<String>> {
    let owner = verified_owner(&user);
    gateway
        .delete_account(
            &owner,
            AccountId::new(id),
            AuthorizationRevision::new(form.expected_revision)?,
        )
        .await?;
    fragment(gateway.as_ref(), &owner, Some("Account removed")).await
}
async fn disconnect(
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Form(form): Form<RevisionInput>,
) -> AppResult<Html<String>> {
    let owner = verified_owner(&user);
    gateway
        .disconnect_account(
            &owner,
            AccountId::new(id),
            AuthorizationRevision::new(form.expected_revision)?,
        )
        .await?;
    fragment(gateway.as_ref(), &owner, Some("Account disconnected")).await
}
#[cfg(test)]
mod tests;
