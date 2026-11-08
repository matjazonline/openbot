use crate::{
    adapters::{
        http::{pages, session::SessionAuthority},
        integrations::owner::owner_for_user,
    },
    app_error::{AppError, AppResult},
    application::integrations::{connect::ConnectBrowser, *},
    domain::integrations::*,
};
use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use secrecy::SecretString;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;
pub(super) fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<dyn IntegrationGateway>: axum::extract::FromRef<S>,
    Arc<SessionAuthority>: axum::extract::FromRef<S>,
{
    Router::new()
        .route("/ui/profile/vault/apps/{app}/connect", post(start))
        .route("/auth/integrations/{provider}/callback", get(callback))
        .route("/ui/profile/vault/connect-attempts/{id}", get(status))
        .route(
            "/ui/profile/vault/connect-attempts/{id}/cancel",
            post(cancel),
        )
        .layer(DefaultBodyLimit::max(8192))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartForm {
    account: Option<Uuid>,
    authorization: Option<i64>,
}
pub(super) async fn browser(
    sessions: &SessionAuthority,
    headers: &HeaderMap,
) -> AppResult<VerifiedOwner> {
    use sha2::{Digest, Sha256};
    let session = sessions
        .user_from_headers(headers)
        .await?
        .ok_or(AppError::InvalidCredentials)?;
    let reference = session.session_id.0.to_string();
    Ok(VerifiedOwner::for_browser(
        owner_for_user(session.user_id),
        ConnectBrowser {
            session: ConnectSessionReference::parse(reference.clone())?,
            binding: BrowserBinding::parse(format!(
                "{:x}",
                Sha256::digest(format!("openbot/vault/{reference}").as_bytes())
            ))?,
        },
    ))
}
async fn start(
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    State(sessions): State<Arc<SessionAuthority>>,
    headers: HeaderMap,
    Path(app): Path<String>,
    Form(form): Form<StartForm>,
) -> AppResult<Response> {
    let owner = browser(&sessions, &headers).await?;
    let app = AppId::parse(app)?;
    let descriptor = gateway
        .connectable_apps()
        .await?
        .into_iter()
        .find(|a| a.id == app)
        .ok_or(IntegrationError::Unavailable)?;
    let reconnect = match (form.account, form.authorization) {
        (None, None) => None,
        (Some(account), Some(auth)) => Some(ReconnectTarget {
            account: AccountId::new(account),
            authorization: AuthorizationRevision::new(auth)?,
        }),
        _ => return Err(IntegrationError::Invalid.into()),
    };
    let connected = gateway
        .begin_connect(
            &owner,
            BeginConnect {
                app,
                config: descriptor.config,
                requested_capabilities: RequestedCapabilities::try_from(descriptor.capabilities)?,
                return_target: SafeReturnTarget::parse("/ui/profile/vault".into())?,
                reconnect,
            },
        )
        .await?;
    // A form redirect to another origin is blocked by form-action 'self'. HTMX
    // performs a top-level navigation; a normal form receives an ordinary link.
    let url = connected.authorization_url.as_str();
    let mut response = Html(pages::base_layout(
        "Continue to app consent",
        &format!(
            "<main><h1>Continue to app consent</h1><p>Choose an account and review the app permissions.</p><a class=\"btn btn-primary\" href=\"{}\">Continue</a><p><a class=\"link\" href=\"/ui/profile/vault\">Return to Connected apps</a></p></main>",
            pages::escape_html_attr(url)
        ),
    ))
    .into_response();
    response.headers_mut().insert(
        "HX-Redirect",
        url.parse().map_err(|_| IntegrationError::Invalid)?,
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        header::HeaderValue::from_static("no-referrer"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}
#[derive(Deserialize)]
struct Callback {
    state: Option<String>,
    code: Option<SecretString>,
    error: Option<String>,
}
async fn callback(
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    State(sessions): State<Arc<SessionAuthority>>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Query(query): Query<Callback>,
) -> Response {
    let result = async {
        let owner = browser(&sessions, &headers).await?;
        let callback = ConnectCallback {
            provider: ProviderId::parse(provider)?,
            state: query.state.ok_or(IntegrationError::Invalid)?,
            code: query.code,
            error: query.error,
        };
        let attempt = gateway
            .finish_connect(
                &owner,
                callback,
                ConnectWork {
                    deadline: chrono::Utc::now() + chrono::Duration::seconds(30),
                    cancellation: tokio_util::sync::CancellationToken::new(),
                },
            )
            .await?;
        Ok::<_, AppError>(attempt)
    }
    .await;
    match result {
        Ok(attempt) => redirect(&format!(
            "/ui/profile/vault/connect-attempts/{}",
            attempt.id.as_uuid()
        )),
        Err(AppError::Conflict(_)) => redirect("/ui/profile/vault?connect_notice=account_changed"),
        Err(AppError::ServiceUnavailable(_)) => {
            redirect("/ui/profile/vault?connect_notice=unavailable")
        }
        Err(_) => redirect("/ui/profile/vault?connect_notice=invalid_state"),
    }
}
async fn status(
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    State(sessions): State<Arc<SessionAuthority>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Html<String>> {
    let owner = browser(&sessions, &headers).await?;
    let attempt = gateway
        .get_connect_attempt(&owner, ConnectAttemptId::new(id))
        .await?;
    let message = match attempt.status {
        ConnectStatus::Pending => "Waiting for provider consent.",
        ConnectStatus::Processing => "Completing authorization.",
        ConnectStatus::Connected => {
            "Private account connected. Company access requires separate owner confirmation."
        }
        ConnectStatus::Denied => "Provider consent was denied.",
        ConnectStatus::Expired => "Authorization expired; start again.",
        ConnectStatus::Cancelled => "Authorization cancelled.",
        ConnectStatus::MissingPermissions => "Required app permissions were not granted.",
        ConnectStatus::MissingRefresh => {
            "An offline refresh grant is required. Connect again and approve consent."
        }
        ConnectStatus::IdentityMismatch => {
            "The selected provider identity cannot reconnect this entry. Connect it as a new account."
        }
        ConnectStatus::Conflict => {
            "This authorization is already connected or the account changed."
        }
        ConnectStatus::Configuration => {
            "Provider client configuration is unavailable; contact the operator."
        }
        ConnectStatus::AdminBlocked => "Your Workspace administrator blocked this app.",
        ConnectStatus::ProviderError => "Provider authorization failed. Start a new attempt.",
    };
    Ok(Html(pages::base_layout(
        "Connected app",
        &format!(
            "<main class=\"p-6\"><h1>Connected app</h1><p role=\"status\">{message}</p><a class=\"link\" href=\"{}\">Return</a></main>",
            pages::escape_html_attr(attempt.return_target.as_str())
        ),
    )))
}
async fn cancel(
    State(gateway): State<Arc<dyn IntegrationGateway>>,
    State(sessions): State<Arc<SessionAuthority>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    let owner = browser(&sessions, &headers).await?;
    gateway
        .cancel_connect(&owner, ConnectAttemptId::new(id))
        .await?;
    Ok(redirect("/ui/profile/vault"))
}
fn redirect(path: &str) -> Response {
    let mut response = Redirect::to(path).into_response();
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        header::HeaderValue::from_static("no-referrer"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

#[cfg(test)]
mod tests;
