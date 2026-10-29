use super::*;
use crate::{
    adapters::{
        http::session::test_support::TestHasher,
        persistence::{
            PostgresPersistence,
            test_support::{OwnDatabase, own_database},
        },
    },
    use_cases::user::{
        ConfirmationCodeSender, ConfirmationPurpose, EmailConfirmation, ExternalIdentity,
        LoginMethod, UserPersistence,
    },
};
use axum::http::StatusCode;
use std::sync::Mutex;

#[derive(Clone, Copy)]
enum AccountKind {
    Password,
    Google,
}
#[derive(Clone, Copy)]
enum Confirmation {
    Immediate,
    Code,
}
#[derive(Default)]
struct Codes(Mutex<Vec<String>>);
#[async_trait::async_trait]
impl ConfirmationCodeSender for Codes {
    async fn send_code(
        &self,
        _: &EmailAddress,
        code: &str,
        _: ConfirmationPurpose,
    ) -> AppResult<()> {
        self.0.lock().unwrap().push(code.into());
        Ok(())
    }
}
struct Fixture {
    _database: OwnDatabase,
    users: Arc<UserUseCases>,
    sessions: Arc<SessionAuthority>,
    config: Arc<AppConfig>,
    codes: Arc<Codes>,
    user: User,
    cookies: Vec<String>,
}
impl Fixture {
    async fn new(kind: AccountKind, confirmation: Confirmation) -> Option<Self> {
        let database = own_database().await?;
        let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
        let id = Uuid::new_v4().simple().to_string();
        let email = EmailAddress::from(format!("{id}@example.test"));
        let user = match kind {
            AccountKind::Password => persistence
                .create_user(&id, &email, "test:old-password")
                .await
                .unwrap(),
            AccountKind::Google => persistence
                .create_external_user(ExternalIdentity {
                    provider: LoginMethod::Google,
                    subject: &id,
                    email: &email,
                    email_verified: true,
                    display_name: None,
                })
                .await
                .unwrap(),
        };
        let config = Arc::new(AppConfig::for_test());
        let codes = Arc::new(Codes::default());
        let users = Arc::new(
            UserUseCases::new(Arc::new(TestHasher), persistence.clone(), "attempt-key")
                .with_email_confirmation(EmailConfirmation {
                    registrations: persistence.clone(),
                    account_changes: persistence.clone(),
                    codes: codes.clone(),
                    config: config.clone(),
                    transport_available: matches!(confirmation, Confirmation::Code),
                    allow_unverified_without_smtp: matches!(confirmation, Confirmation::Immediate),
                }),
        );
        let sessions = Arc::new(SessionAuthority::new(&config, persistence));
        let mut cookies = Vec::new();
        for _ in 0..2 {
            cookies.push(sessions.cookie(&user).await.unwrap().value().to_string());
        }
        Some(Self {
            _database: database,
            users,
            sessions,
            config,
            codes,
            user,
            cookies,
        })
    }
    fn jar(&self) -> CookieJar {
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            format!("session={}", self.cookies[0]).parse().unwrap(),
        );
        CookieJar::from_headers(&headers)
    }
    fn actor(&self) -> AuthenticatedUser {
        AuthenticatedUser { id: self.user.id }
    }
    async fn assert_signed_out(&self, response: &Response) {
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["HX-Redirect"], "/login");
        assert!(
            response
                .headers()
                .get_all("set-cookie")
                .iter()
                .any(|h| h.to_str().unwrap().contains("session=;"))
        );
        for token in &self.cookies {
            assert!(self.sessions.verify(token).await.unwrap().is_none());
        }
    }
}
fn htmx() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("HX-Request", "true".parse().unwrap());
    headers
}
fn work() -> Extension<AuthWorkLifetime> {
    Extension(AuthWorkLifetime::new(()))
}
fn password_form() -> PasswordForm {
    PasswordForm {
        current_password: "old-password".into(),
        new_password: "new-password".into(),
        confirm_password: "new-password".into(),
    }
}

#[tokio::test]
async fn security_session_profile_immediate_security_changes_sign_in_again() {
    let Some(f) = Fixture::new(AccountKind::Password, Confirmation::Immediate).await else {
        return;
    };
    let response = change_password(
        State(f.users.clone()),
        State(f.sessions.clone()),
        htmx(),
        f.jar(),
        f.actor(),
        work(),
        Form(password_form()),
    )
    .await
    .unwrap();
    f.assert_signed_out(&response).await;
    let Some(f) = Fixture::new(AccountKind::Google, Confirmation::Immediate).await else {
        return;
    };
    let response = set_password(
        State(f.users.clone()),
        State(f.sessions.clone()),
        htmx(),
        f.jar(),
        f.actor(),
        work(),
        Form(PasswordSetupForm {
            new_password: "new-password".into(),
            confirm_password: "new-password".into(),
        }),
    )
    .await
    .unwrap();
    f.assert_signed_out(&response).await;
    let Some(f) = Fixture::new(AccountKind::Password, Confirmation::Immediate).await else {
        return;
    };
    let response = update_profile(
        State(f.users.clone()),
        State(f.config.clone()),
        State(f.sessions.clone()),
        htmx(),
        f.jar(),
        f.actor(),
        Form(IdentityForm {
            username: f.user.username.clone(),
            email: format!("{}@next.test", f.user.id),
            avatar_url: String::new(),
        }),
    )
    .await
    .unwrap();
    f.assert_signed_out(&response).await;
}

#[tokio::test]
async fn security_session_profile_code_confirmed_password_setup_and_email_revoke() {
    for (kind, account) in [
        (AccountChangeKind::Password, AccountKind::Password),
        (AccountChangeKind::Password, AccountKind::Google),
        (AccountChangeKind::Email, AccountKind::Password),
    ] {
        let Some(f) = Fixture::new(account, Confirmation::Code).await else {
            return;
        };
        let pending = match kind {
            AccountChangeKind::Password if matches!(account, AccountKind::Google) => set_password(
                State(f.users.clone()),
                State(f.sessions.clone()),
                htmx(),
                f.jar(),
                f.actor(),
                work(),
                Form(PasswordSetupForm {
                    new_password: "new-password".into(),
                    confirm_password: "new-password".into(),
                }),
            )
            .await
            .unwrap(),
            AccountChangeKind::Password => change_password(
                State(f.users.clone()),
                State(f.sessions.clone()),
                htmx(),
                f.jar(),
                f.actor(),
                work(),
                Form(password_form()),
            )
            .await
            .unwrap(),
            AccountChangeKind::Email => update_profile(
                State(f.users.clone()),
                State(f.config.clone()),
                State(f.sessions.clone()),
                htmx(),
                f.jar(),
                f.actor(),
                Form(IdentityForm {
                    username: f.user.username.clone(),
                    email: format!("{}@next.test", f.user.id),
                    avatar_url: String::new(),
                }),
            )
            .await
            .unwrap(),
        };
        assert!(pending.headers().get("set-cookie").is_none());
        assert!(pending.headers().get("HX-Redirect").is_none());
        for token in &f.cookies {
            assert!(f.sessions.verify(token).await.unwrap().is_some());
        }
        let code = f.codes.0.lock().unwrap().last().unwrap().clone();
        let response = confirm_change(
            State(f.users.clone()),
            State(f.sessions.clone()),
            htmx(),
            f.jar(),
            f.actor(),
            Path(kind.as_str().into()),
            Form(CodeForm { code }),
        )
        .await
        .unwrap();
        f.assert_signed_out(&response).await;
    }
}

#[tokio::test]
async fn security_session_profile_failed_change_keeps_session_and_document_redirects() {
    let Some(f) = Fixture::new(AccountKind::Password, Confirmation::Immediate).await else {
        return;
    };
    let mut form = password_form();
    form.current_password = "wrong-password".into();
    let failed = change_password(
        State(f.users.clone()),
        State(f.sessions.clone()),
        htmx(),
        f.jar(),
        f.actor(),
        work(),
        Form(form),
    )
    .await
    .unwrap();
    assert!(failed.headers().get("set-cookie").is_none());
    assert!(failed.headers().get("HX-Redirect").is_none());
    assert!(f.sessions.verify(&f.cookies[0]).await.unwrap().is_some());
    let success = change_password(
        State(f.users.clone()),
        State(f.sessions.clone()),
        HeaderMap::new(),
        f.jar(),
        f.actor(),
        work(),
        Form(password_form()),
    )
    .await
    .unwrap();
    assert_eq!(success.status(), StatusCode::SEE_OTHER);
    assert_eq!(success.headers()["location"], "/login");
    assert!(f.sessions.verify(&f.cookies[0]).await.unwrap().is_none());
}

#[tokio::test]
#[ignore = "writes actual profile responses for the browser protocol check"]
async fn dump_session_revocation_browser_fixtures() {
    let directory = std::path::PathBuf::from(
        std::env::var("SESSION_REVOCATION_BROWSER_FIXTURES").expect("fixture directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let f = Fixture::new(AccountKind::Password, Confirmation::Immediate)
        .await
        .unwrap();
    let account = Account::load(&f.users, f.user.id).await.unwrap();
    std::fs::write(
        directory.join("profile.html"),
        account.pane(None, pages::ProfileOutcome::Untouched),
    )
    .unwrap();
    let mut wrong = password_form();
    wrong.current_password = "wrong-password".into();
    let failed = change_password(
        State(f.users.clone()),
        State(f.sessions.clone()),
        htmx(),
        f.jar(),
        f.actor(),
        work(),
        Form(wrong),
    )
    .await
    .unwrap();
    let success = change_password(
        State(f.users.clone()),
        State(f.sessions.clone()),
        htmx(),
        f.jar(),
        f.actor(),
        work(),
        Form(password_form()),
    )
    .await
    .unwrap();
    for (name, response) in [("failed", failed), ("success", success)] {
        let status = response.status().as_u16();
        let headers: Vec<_> = response
            .headers()
            .iter()
            .map(|(key, value)| {
                (
                    key.as_str().to_string(),
                    value.to_str().unwrap().to_string(),
                )
            })
            .collect();
        let body = axum::body::to_bytes(response.into_body(), 128 * 1024)
            .await
            .unwrap();
        let fixture = serde_json::json!({"status":status, "headers":headers,"body": String::from_utf8(body.to_vec()).unwrap()});
        std::fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec(&fixture).unwrap(),
        )
        .unwrap();
    }
}
