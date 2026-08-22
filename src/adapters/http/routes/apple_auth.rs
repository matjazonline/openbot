use std::sync::Arc;

use axum::{
    Form, Router,
    extract::State,
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use jsonwebtoken::{
    Algorithm, EncodingKey, Header, Validation, decode, decode_header, encode, jwk::JwkSet,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    adapters::http::{
        app_state::AppState, auth::AuthenticatedUser, pages, session::SessionAuthority,
    },
    app_error::{AppError, AppResult},
    entities::value_objects::EmailAddress,
    infra::config::{AppConfig, AppleOAuthConfig},
    use_cases::user::{ExternalIdentity, LoginMethod, UserUseCases},
};

const APPLE_FLOW_COOKIE: &str = "apple_oauth_transaction";

#[derive(Clone, Copy)]
enum AppleFlow {
    Register,
    Login,
    Connect,
}

impl AppleFlow {
    fn as_str(self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::Login => "login",
            Self::Connect => "connect",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "register" => Some(Self::Register),
            "login" => Some(Self::Login),
            "connect" => Some(Self::Connect),
            _ => None,
        }
    }
}

pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/auth/apple/register", get(start_registration))
        .route("/auth/apple/login", get(start_login))
        .route("/auth/apple/callback", post(callback))
}

pub fn protected_router() -> Router<AppState> {
    Router::new().route("/auth/apple/connect", get(start_connection))
}

async fn start_registration(
    State(config): State<Arc<AppConfig>>,
    State(sessions): State<Arc<SessionAuthority>>,
    jar: CookieJar,
) -> impl IntoResponse {
    start_flow(&config, &sessions, jar, AppleFlow::Register, None)
}

async fn start_login(
    State(config): State<Arc<AppConfig>>,
    State(sessions): State<Arc<SessionAuthority>>,
    jar: CookieJar,
) -> impl IntoResponse {
    start_flow(&config, &sessions, jar, AppleFlow::Login, None)
}

async fn start_connection(
    State(config): State<Arc<AppConfig>>,
    State(sessions): State<Arc<SessionAuthority>>,
    user: AuthenticatedUser,
    jar: CookieJar,
) -> impl IntoResponse {
    start_flow(&config, &sessions, jar, AppleFlow::Connect, Some(user.id))
}

fn start_flow(
    config: &AppConfig,
    sessions: &SessionAuthority,
    jar: CookieJar,
    flow: AppleFlow,
    user_id: Option<Uuid>,
) -> axum::response::Response {
    let Some(apple) = AppleOAuthConfig::from_env() else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Html(pages::error_alert(
                "Apple authentication is not configured.",
            )),
        )
            .into_response();
    };
    let nonce = Uuid::new_v4().simple().to_string();
    let state = sessions.issue_oauth_state("apple", flow.as_str(), user_id, nonce.clone());
    let encoded_state = sessions.encode_oauth_state(&state);
    let cookie = Cookie::build((APPLE_FLOW_COOKIE, state.transaction))
        .path("/auth/apple")
        .http_only(true)
        .same_site(SameSite::None)
        .secure(true)
        .max_age(time::Duration::minutes(10))
        .build();
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &apple.client_id)
        .append_pair("redirect_uri", &apple_redirect_uri(config))
        .append_pair("response_type", "code")
        .append_pair("response_mode", "form_post")
        .append_pair("scope", "name email")
        .append_pair("state", &encoded_state)
        .append_pair("nonce", &nonce)
        .finish();
    (
        jar.add(cookie),
        Redirect::temporary(&format!("https://appleid.apple.com/auth/authorize?{query}")),
    )
        .into_response()
}

#[derive(Deserialize)]
struct AppleCallbackForm {
    code: Option<String>,
    state: Option<String>,
    user: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct AppleUserPayload {
    name: Option<AppleName>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppleName {
    first_name: Option<String>,
    last_name: Option<String>,
}

impl AppleName {
    fn display_name(&self) -> Option<String> {
        let name = [self.first_name.as_deref(), self.last_name.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        (!name.is_empty()).then_some(name)
    }
}

async fn callback(
    State(config): State<Arc<AppConfig>>,
    State(users): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
    jar: CookieJar,
    Form(form): Form<AppleCallbackForm>,
) -> impl IntoResponse {
    let cleared = jar
        .clone()
        .remove(Cookie::build(APPLE_FLOW_COOKIE).path("/auth/apple").build());
    match complete_flow(&config, &users, &sessions, &jar, form).await {
        Ok((user, AppleFlow::Connect)) => (
            cleared.add(sessions.cookie(user.id)),
            Redirect::temporary("/ui/profile?connected=apple"),
        )
            .into_response(),
        Ok((user, _)) => (
            cleared.add(sessions.cookie(user.id)),
            Redirect::temporary("/ui"),
        )
            .into_response(),
        Err(error) => (
            cleared,
            (
                axum::http::StatusCode::BAD_REQUEST,
                Html(pages::error_alert(&error.to_string())),
            ),
        )
            .into_response(),
    }
}

async fn complete_flow(
    config: &AppConfig,
    users: &UserUseCases,
    sessions: &SessionAuthority,
    jar: &CookieJar,
    form: AppleCallbackForm,
) -> AppResult<(crate::entities::user::User, AppleFlow)> {
    if form.error.is_some() {
        return Err(AppError::BadRequest(
            "Apple authentication was cancelled.".into(),
        ));
    }
    let encoded_state = form
        .state
        .ok_or_else(|| AppError::BadRequest("Missing Apple authentication state.".into()))?;
    let state = sessions
        .verify_oauth_state(&encoded_state)
        .filter(|state| state.provider == "apple")
        .ok_or_else(|| AppError::BadRequest("Invalid Apple authentication state.".into()))?;
    let transaction = jar.get(APPLE_FLOW_COOKIE).ok_or_else(|| {
        AppError::BadRequest("Apple authentication expired. Please try again.".into())
    })?;
    if transaction.value() != state.transaction {
        return Err(AppError::BadRequest(
            "Invalid Apple authentication state.".into(),
        ));
    }
    let flow = AppleFlow::parse(&state.flow)
        .ok_or_else(|| AppError::BadRequest("Invalid Apple authentication flow.".into()))?;
    let apple = AppleOAuthConfig::from_env()
        .ok_or_else(|| AppError::BadRequest("Apple authentication is not configured.".into()))?;
    let code = form.code.ok_or_else(|| {
        AppError::BadRequest("Apple did not return an authorization code.".into())
    })?;
    let identity = exchange_and_verify(config, &apple, &code, &state.nonce).await?;
    let supplied_name = form
        .user
        .as_deref()
        .and_then(|payload| serde_json::from_str::<AppleUserPayload>(payload).ok())
        .and_then(|payload| payload.name.and_then(|name| name.display_name()));
    let email = EmailAddress::from(identity.email.as_str());
    let external = ExternalIdentity {
        provider: LoginMethod::Apple,
        subject: &identity.sub,
        email: &email,
        display_name: supplied_name.as_deref(),
    };
    let user = match flow {
        AppleFlow::Register => users.register_external(external).await?,
        AppleFlow::Login => {
            users
                .login_external(LoginMethod::Apple, &identity.sub)
                .await?
        }
        AppleFlow::Connect => {
            let user_id = state.user_id.ok_or_else(|| {
                AppError::BadRequest("Apple connection is missing its account.".into())
            })?;
            users.link_external(user_id, external).await?
        }
    };
    Ok((user, flow))
}

#[derive(Serialize)]
struct ClientSecretClaims<'a> {
    iss: &'a str,
    iat: i64,
    exp: i64,
    aud: &'static str,
    sub: &'a str,
}

#[derive(Deserialize)]
struct AppleTokenResponse {
    id_token: String,
}

#[derive(Deserialize)]
struct AppleIdentityClaims {
    sub: String,
    email: String,
    email_verified: BoolClaim,
    nonce: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BoolClaim {
    Bool(bool),
    Text(String),
}

impl BoolClaim {
    fn is_true(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            Self::Text(value) => value.eq_ignore_ascii_case("true"),
        }
    }
}

async fn exchange_and_verify(
    config: &AppConfig,
    apple: &AppleOAuthConfig,
    code: &str,
    expected_nonce: &str,
) -> AppResult<AppleIdentityClaims> {
    let client_secret = apple_client_secret(apple)?;
    let redirect_uri = apple_redirect_uri(config);
    let token = reqwest::Client::new()
        .post("https://appleid.apple.com/auth/token")
        .form(&[
            ("client_id", apple.client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("code", code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri.as_str()),
        ])
        .send()
        .await
        .map_err(|_| AppError::BadRequest("Could not contact Apple.".into()))?
        .error_for_status()
        .map_err(|_| AppError::BadRequest("Apple rejected the authorization code.".into()))?
        .json::<AppleTokenResponse>()
        .await
        .map_err(|_| AppError::BadRequest("Apple returned an invalid token response.".into()))?;
    verify_identity_token(&apple.client_id, &token.id_token, expected_nonce).await
}

fn apple_client_secret(apple: &AppleOAuthConfig) -> AppResult<String> {
    let key = STANDARD
        .decode(&apple.private_key_base64)
        .map_err(|_| AppError::Internal("Apple private key is not valid base64.".into()))?;
    let encoding = EncodingKey::from_ec_pem(&key)
        .map_err(|_| AppError::Internal("Apple private key is not a valid EC key.".into()))?;
    let now = chrono::Utc::now().timestamp();
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(apple.key_id.clone());
    encode(
        &header,
        &ClientSecretClaims {
            iss: &apple.team_id,
            iat: now,
            exp: now + 300,
            aud: "https://appleid.apple.com",
            sub: &apple.client_id,
        },
        &encoding,
    )
    .map_err(|_| AppError::Internal("Could not sign the Apple client secret.".into()))
}

async fn verify_identity_token(
    client_id: &str,
    token: &str,
    expected_nonce: &str,
) -> AppResult<AppleIdentityClaims> {
    let header = decode_header(token)
        .map_err(|_| AppError::BadRequest("Apple returned an invalid identity token.".into()))?;
    if header.alg != Algorithm::RS256 {
        return Err(AppError::BadRequest(
            "Apple identity token used an unexpected signing algorithm.".into(),
        ));
    }
    let key_id = header
        .kid
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("Apple identity token has no key id.".into()))?;
    let keys = reqwest::Client::new()
        .get("https://appleid.apple.com/auth/keys")
        .send()
        .await
        .map_err(|_| AppError::BadRequest("Could not load Apple's signing keys.".into()))?
        .error_for_status()
        .map_err(|_| AppError::BadRequest("Apple's signing keys were unavailable.".into()))?
        .json::<JwkSet>()
        .await
        .map_err(|_| AppError::BadRequest("Apple returned invalid signing keys.".into()))?;
    let key = keys
        .find(key_id)
        .ok_or_else(|| AppError::BadRequest("Apple signed with an unknown key.".into()))?;
    let decoding = jsonwebtoken::DecodingKey::from_jwk(key)
        .map_err(|_| AppError::BadRequest("Apple returned an unusable signing key.".into()))?;
    let mut validation = Validation::new(header.alg);
    validation.set_issuer(&["https://appleid.apple.com"]);
    validation.set_audience(&[client_id]);
    let claims = decode::<AppleIdentityClaims>(token, &decoding, &validation)
        .map_err(|_| AppError::BadRequest("Apple identity token validation failed.".into()))?
        .claims;
    if claims.nonce != expected_nonce || !claims.email_verified.is_true() {
        return Err(AppError::BadRequest(
            "Apple did not verify this authentication request and email.".into(),
        ));
    }
    Ok(claims)
}

fn apple_redirect_uri(config: &AppConfig) -> String {
    let domain = config.app_domain_name.trim_end_matches('/');
    let host = domain
        .strip_prefix("https://")
        .or_else(|| domain.strip_prefix("http://"))
        .unwrap_or(domain);
    format!("https://{host}/auth/apple/callback")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_claim_booleans_accept_both_wire_shapes() {
        assert!(serde_json::from_str::<BoolClaim>("true").unwrap().is_true());
        assert!(
            serde_json::from_str::<BoolClaim>(r#""true""#)
                .unwrap()
                .is_true()
        );
        assert!(
            !serde_json::from_str::<BoolClaim>(r#""false""#)
                .unwrap()
                .is_true()
        );
    }

    #[test]
    fn apple_name_is_optional_and_joined_without_blank_parts() {
        let name = AppleName {
            first_name: Some(" Dana ".into()),
            last_name: Some("Scully".into()),
        };
        assert_eq!(name.display_name().as_deref(), Some("Dana Scully"));
    }

    #[test]
    fn apple_callback_is_always_https() {
        let mut config = AppConfig::for_test();
        config.app_domain_name = "accounts.example.com".into();
        assert_eq!(
            apple_redirect_uri(&config),
            "https://accounts.example.com/auth/apple/callback"
        );
    }
}
