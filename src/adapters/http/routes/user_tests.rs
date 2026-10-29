use axum_extra::extract::cookie::Cookie;

use super::*;
use crate::use_cases::user::{
    AccountChangeKind, AccountChangePersistence, AuthAttemptKey, ConfirmationCodeSender,
    ConfirmationPurpose, EmailConfirmation, PendingAccountChange, PendingChange,
    PendingRegistration, RegistrationPersistence,
};

struct DisabledEmailPorts;

fn disabled_email_error() -> AppError {
    AppError::Internal("Disabled confirmation test port was called".into())
}

#[async_trait::async_trait]
impl RegistrationPersistence for DisabledEmailPorts {
    async fn save_pending_registration(&self, _: PendingRegistration<'_>) -> AppResult<()> {
        Err(disabled_email_error())
    }

    async fn confirm_pending_registration(
        &self,
        _: &EmailAddress,
        _: &str,
        _: &AuthAttemptKey,
    ) -> AppResult<Option<crate::entities::user::User>> {
        Err(disabled_email_error())
    }
}

#[async_trait::async_trait]
impl AccountChangePersistence for DisabledEmailPorts {
    async fn save_pending_account_change(&self, _: PendingAccountChange<'_>) -> AppResult<()> {
        Err(disabled_email_error())
    }

    async fn confirm_pending_account_change(
        &self,
        _: uuid::Uuid,
        _: AccountChangeKind,
        _: &str,
        _: &AuthAttemptKey,
    ) -> AppResult<Option<crate::entities::user::User>> {
        Err(disabled_email_error())
    }

    async fn list_pending_account_changes(&self, _: uuid::Uuid) -> AppResult<Vec<PendingChange>> {
        Err(disabled_email_error())
    }

    async fn discard_pending_account_change(
        &self,
        _: uuid::Uuid,
        _: AccountChangeKind,
    ) -> AppResult<()> {
        Err(disabled_email_error())
    }
}

#[async_trait::async_trait]
impl ConfirmationCodeSender for DisabledEmailPorts {
    async fn send_code(&self, _: &EmailAddress, _: &str, _: ConfirmationPurpose) -> AppResult<()> {
        Err(disabled_email_error())
    }
}

fn with_unverified_bypass(users: UserUseCases) -> UserUseCases {
    let disabled = Arc::new(DisabledEmailPorts);
    users.with_email_confirmation(EmailConfirmation {
        registrations: disabled.clone(),
        account_changes: disabled.clone(),
        codes: disabled,
        config: Arc::new(AppConfig::for_test()),
        transport_available: false,
        allow_unverified_without_smtp: true,
    })
}

#[tokio::test]
async fn login_page_contains_htmx_attributes() {
    let html = pages::login_page(false, false);
    assert!(html.contains("/assets/htmx-2.0.4.min.js"));
    assert!(html.contains("hx-post=\"/api/user/login\""));
    assert!(html.contains("hx-target=\"#response-message\""));
    assert!(html.contains("(408|413|429|503)"));
    assert!(!html.contains(">Companies</a>"));
    assert!(!html.contains(">My Invites</a>"));
}

#[tokio::test]
async fn register_page_contains_htmx_attributes() {
    let html = pages::register_page(false, false);
    assert!(html.contains("/assets/htmx-2.0.4.min.js"));
    assert!(html.contains("hx-post=\"/api/user/register\""));
    assert!(html.contains("hx-target=\"#response-message\""));
    assert!(!html.contains(">Companies</a>"));
    assert!(!html.contains(">My Invites</a>"));
}

#[test]
fn google_buttons_are_only_rendered_when_google_is_configured() {
    assert!(!pages::login_page(false, false).contains("/auth/google/login"));
    assert!(!pages::register_page(false, false).contains("/auth/google/register"));
    assert!(pages::login_page(true, false).contains("/auth/google/login"));
    assert!(pages::register_page(true, false).contains("/auth/google/register"));
    assert!(pages::login_page(false, true).contains("/auth/apple/login"));
    assert!(pages::register_page(false, true).contains("/auth/apple/register"));
}

#[test]
fn security_email_identity_google_requires_explicit_true_claim() {
    let missing: GoogleProfile =
        serde_json::from_str(r#"{"sub":"subject","email":"a@example.com"}"#).unwrap();
    assert!(!missing.email_verified);
    let false_claim: GoogleProfile =
        serde_json::from_str(r#"{"sub":"subject","email":"a@example.com","email_verified":false}"#)
            .unwrap();
    assert!(!false_claim.email_verified);
    let true_claim: GoogleProfile =
        serde_json::from_str(r#"{"sub":"subject","email":"a@example.com","email_verified":true}"#)
            .unwrap();
    assert!(true_claim.email_verified);
    assert!(
        serde_json::from_str::<GoogleProfile>(
            r#"{"sub":"subject","email":"a@example.com","email_verified":"true"}"#
        )
        .is_err()
    );
}

#[test]
fn alerts_render_correct_html() {
    let success = pages::success_alert("Done!", Some(("/login", "Login")));
    assert!(success.contains("Done!"));
    assert!(success.contains("href=\"/login\""));

    let error = pages::error_alert("Something went wrong");
    assert!(error.contains("Something went wrong"));
}

#[test]
fn security_html_injection_alerts_encode_text_and_constrain_links() {
    let markup = r#"<img src=x onerror=alert(1)> & ' "#;
    let success = pages::success_alert(markup, Some(("/login?next=\" onfocus=alert(1)", markup)));
    assert!(success.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(success.contains("href=\"/login?next=&quot; onfocus=alert(1)\""));
    assert!(!success.contains("<img"));
    assert!(!success.contains("href=\"/login?next=\""));

    let error = pages::error_alert(markup);
    assert!(error.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(!error.contains("<img"));
    assert!(!pages::success_alert("safe", Some(("javascript:alert(1)", "bad"))).contains("<a "));
    assert!(!pages::success_alert("safe", Some(("//evil.example", "bad"))).contains("<a "));
}

#[tokio::test]
async fn security_html_injection_registration_response_keeps_username_as_text() {
    let sessions = Arc::new(SessionAuthority::for_test(
        &crate::infra::config::AppConfig::for_test(),
    ));
    let mut form = registration("hunter2", Some("hunter2"));
    form.username = "<img src=x onerror=alert(1)>".into();
    let response = register_form(
        State(registering(uuid::Uuid::new_v4())),
        State(sessions),
        test_work(),
        CookieJar::new(),
        Form(form),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("HX-Redirect").unwrap(), "/");
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(!html.contains("<img"));
}

#[tokio::test]
async fn security_html_injection_rejected_registration_keeps_error_as_text() {
    let sessions = Arc::new(SessionAuthority::for_test(
        &crate::infra::config::AppConfig::for_test(),
    ));
    let response = register_form(
        State(registering_with_error(
            uuid::Uuid::new_v4(),
            Some("<svg onload=alert(1)>".into()),
        )),
        State(sessions),
        test_work(),
        CookieJar::new(),
        Form(registration("hunter2", Some("hunter2"))),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("HX-Redirect").is_none());
    assert!(response.headers().get("set-cookie").is_none());
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("&lt;svg onload=alert(1)&gt;"));
    assert!(!html.contains("<svg onload=alert(1)"));
}

/// A persistence that hands back the account it was asked to create, so a handler that signs
/// the new user in has an id to sign in.
struct StubUserPersistence {
    id: uuid::Uuid,
    create_error: Option<String>,
}

#[async_trait::async_trait]
impl crate::use_cases::user::UserPersistence for StubUserPersistence {
    async fn login_external_identity(
        &self,
        _: ExternalIdentity<'_>,
    ) -> AppResult<Option<crate::entities::user::User>> {
        Err(AppError::Internal(
            "External login proof is not supported by this route stub".into(),
        ))
    }

    async fn canonical_auth_identifier(&self, value: &str) -> AppResult<String> {
        Ok(value.to_lowercase())
    }

    async fn reserve_login_attempt(
        &self,
        _: &crate::use_cases::user::AuthAttemptKey,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn clear_login_attempt(
        &self,
        _: &crate::use_cases::user::AuthAttemptKey,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn create_user(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
    ) -> AppResult<crate::entities::user::User> {
        if let Some(message) = &self.create_error {
            return Err(AppError::BadRequest(message.clone()));
        }
        Ok(crate::entities::user::User {
            auth_revision: 0,
            id: self.id,
            username: username.to_string(),
            email: email.to_string(),
            email_verified_at: None,
            password_hash: password_hash.to_string(),
            avatar_url: None,
            created_at: chrono::Utc::now(),
        })
    }

    async fn get_by_email(&self, _: &str) -> AppResult<Option<crate::entities::user::User>> {
        Ok(None)
    }

    async fn get_by_username(&self, _: &str) -> AppResult<Option<crate::entities::user::User>> {
        Ok(None)
    }

    async fn get_by_id(&self, _: uuid::Uuid) -> AppResult<Option<crate::entities::user::User>> {
        Ok(None)
    }

    async fn notification_preferences(
        &self,
        _: uuid::Uuid,
    ) -> AppResult<crate::entities::notification::NotificationPreferences> {
        Ok(crate::entities::notification::NotificationPreferences::default())
    }

    async fn set_notification_preferences(
        &self,
        _: uuid::Uuid,
        _: crate::entities::notification::NotificationPreferences,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_avatar_url(
        &self,
        _: uuid::Uuid,
        _: Option<&crate::entities::value_objects::AvatarUrl>,
    ) -> AppResult<Option<crate::entities::user::User>> {
        Ok(None)
    }

    async fn update_profile(
        &self,
        _: uuid::Uuid,
        _: crate::use_cases::user::ProfileUpdate<'_>,
    ) -> AppResult<Option<crate::entities::user::User>> {
        Ok(None)
    }

    async fn update_profile_display(
        &self,
        _: uuid::Uuid,
        _: &str,
        _: Option<&crate::entities::value_objects::AvatarUrl>,
    ) -> AppResult<Option<crate::entities::user::User>> {
        Ok(None)
    }

    async fn update_password_hash(
        &self,
        _: uuid::Uuid,
        _: &str,
    ) -> AppResult<Option<crate::entities::user::User>> {
        Ok(None)
    }
}

struct StubHasher;

impl crate::use_cases::user::UserCredentialsHasher for StubHasher {
    fn hash_password(&self, password: &str) -> AppResult<String> {
        Ok(format!("{password}_hash"))
    }

    fn verify_password(&self, password: &str, hash: &str) -> AppResult<bool> {
        Ok(hash == format!("{password}_hash"))
    }
}

fn registering(id: uuid::Uuid) -> Arc<UserUseCases> {
    registering_with_error(id, None)
}

fn registering_without_smtp_bypass(id: uuid::Uuid) -> Arc<UserUseCases> {
    Arc::new(UserUseCases::new(
        Arc::new(StubHasher),
        Arc::new(StubUserPersistence {
            id,
            create_error: None,
        }),
        "test-attempt-secret",
    ))
}

fn registering_with_error(id: uuid::Uuid, create_error: Option<String>) -> Arc<UserUseCases> {
    Arc::new(with_unverified_bypass(UserUseCases::new(
        Arc::new(StubHasher),
        Arc::new(StubUserPersistence { id, create_error }),
        "test-attempt-secret",
    )))
}

fn test_work() -> Extension<AuthWorkLifetime> {
    Extension(AuthWorkLifetime::new(()))
}

fn registration(password: &str, confirm: Option<&str>) -> RegisterForm {
    RegisterForm {
        username: "newcomer".to_string(),
        email: "newcomer@example.com".to_string(),
        password: password.into(),
        confirm_password: confirm.map(Into::into),
    }
}

#[tokio::test]
async fn security_email_identity_form_and_json_share_default_no_smtp_denial() {
    let users = registering_without_smtp_bypass(uuid::Uuid::new_v4());
    let sessions = Arc::new(SessionAuthority::for_test(&AppConfig::for_test()));
    let form = register_form(
        State(users.clone()),
        State(sessions.clone()),
        test_work(),
        CookieJar::new(),
        Form(registration("hunter2", Some("hunter2"))),
    )
    .await
    .into_response();
    assert!(form.headers().get("set-cookie").is_none());
    let body = axum::body::to_bytes(form.into_body(), 4096).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("outbound SMTP is not configured"));

    let json = register_json(
        State(users),
        State(sessions),
        test_work(),
        CookieJar::new(),
        Json(RegisterPayload {
            username: "newcomer".into(),
            email: "newcomer@example.com".into(),
            password: "hunter2".into(),
        }),
    )
    .await;
    assert!(matches!(json, Err(AppError::BadRequest(message)) if message.contains("SMTP")));
}

#[tokio::test]
async fn security_email_identity_bypassed_form_and_json_disclose_unverified_status() {
    let users = registering(uuid::Uuid::new_v4());
    let sessions = Arc::new(SessionAuthority::for_test(&AppConfig::for_test()));
    let form = register_form(
        State(users.clone()),
        State(sessions.clone()),
        test_work(),
        CookieJar::new(),
        Form(registration("hunter2", Some("hunter2"))),
    )
    .await
    .into_response();
    let body = axum::body::to_bytes(form.into_body(), 4096).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("unverified email"));

    let json = register_json(
        State(users),
        State(sessions),
        test_work(),
        CookieJar::new(),
        Json(RegisterPayload {
            username: "newcomer".into(),
            email: "newcomer@example.com".into(),
            password: "hunter2".into(),
        }),
    )
    .await
    .unwrap();
    let body = axum::body::to_bytes(json.into_body(), 4096).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("unverified email"));
}

#[path = "user_session_tests.rs"]
mod session_routes;

#[path = "session_revocation_tests.rs"]
mod revocation;
