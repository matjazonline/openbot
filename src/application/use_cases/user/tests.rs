use async_trait::async_trait;
use uuid::Uuid;

use super::*;

#[tokio::test]
async fn security_auth_limits_unknown_and_oauth_only_use_dummy_verification() {
    struct RecordingHasher(std::sync::Mutex<Vec<String>>);
    impl UserCredentialsHasher for RecordingHasher {
        fn hash_password(&self, _: &str) -> AppResult<String> {
            unreachable!()
        }
        fn verify_password(&self, _: &str, hash: &str) -> AppResult<bool> {
            self.0.lock().unwrap().push(hash.to_owned());
            Ok(false)
        }
    }
    let hasher = Arc::new(RecordingHasher(std::sync::Mutex::new(Vec::new())));
    let persistence = Arc::new(MockUserPersistence::new());
    let users = UserUseCases::new(
        hasher.clone(),
        persistence.clone(),
        ("test-attempt-secret").into(),
    );
    assert!(matches!(
        users.login("testuser", &"wrong".into(), None, None).await,
        Err(AppError::InvalidCredentials)
    ));
    assert!(matches!(
        users.login("missing", &"wrong".into(), None, None).await,
        Err(AppError::InvalidCredentials)
    ));
    persistence
        .password_enabled
        .store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        users.login("testuser", &"wrong".into(), None, None).await,
        Err(AppError::InvalidCredentials)
    ));
    assert_eq!(
        *hasher.0.lock().unwrap(),
        ["secret_hash", DUMMY_PASSWORD_HASH, DUMMY_PASSWORD_HASH]
    );
}

#[tokio::test]
async fn security_auth_limits_dummy_hash_is_valid_argon2() {
    let users = UserUseCases::new(
        Arc::new(crate::infra::argon2_password_hasher()),
        Arc::new(MockUserPersistence::new()),
        ("test-attempt-secret").into(),
    );
    assert!(matches!(
        users.login("missing", &"wrong".into(), None, None).await,
        Err(AppError::InvalidCredentials)
    ));
}

#[tokio::test]
async fn security_auth_limits_hash_capacity_fails_before_fifth_job() {
    use std::sync::{
        Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct HeldHasher {
        started: AtomicUsize,
        release: (Mutex<bool>, Condvar),
    }
    impl UserCredentialsHasher for HeldHasher {
        fn hash_password(&self, _: &str) -> AppResult<String> {
            self.started.fetch_add(1, Ordering::SeqCst);
            let (lock, condition) = &self.release;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
            Ok("hash".into())
        }
        fn verify_password(&self, _: &str, _: &str) -> AppResult<bool> {
            unreachable!()
        }
    }
    struct ReleaseOnDrop(Arc<HeldHasher>);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            let (lock, condition) = &self.0.release;
            *lock.lock().unwrap() = true;
            condition.notify_all();
        }
    }
    let hasher = Arc::new(HeldHasher {
        started: AtomicUsize::new(0),
        release: (Mutex::new(false), Condvar::new()),
    });
    let release_guard = ReleaseOnDrop(hasher.clone());
    let users = Arc::new(UserUseCases::new(
        hasher.clone(),
        Arc::new(MockUserPersistence::new()),
        ("test-attempt-secret").into(),
    ));
    let jobs: Vec<_> = (0..MAX_PASSWORD_HASH_JOBS)
        .map(|_| {
            let users = users.clone();
            tokio::spawn(async move {
                users
                    .add("testuser", "testuser@gmail.com", &"secret".into(), None)
                    .await
            })
        })
        .collect();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while hasher.started.load(Ordering::SeqCst) < MAX_PASSWORD_HASH_JOBS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        users
            .add("testuser", "testuser@gmail.com", &"secret".into(), None)
            .await,
        Err(AppError::ServiceUnavailable(_))
    ));
    assert_eq!(
        hasher.started.load(Ordering::SeqCst),
        MAX_PASSWORD_HASH_JOBS
    );
    drop(release_guard);
    for job in jobs {
        job.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn security_auth_limits_cancelled_hash_keeps_source_capacity() {
    use std::{
        net::SocketAddr,
        sync::{
            Condvar, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use axum::{
        Extension, Router,
        body::Body,
        extract::connect_info::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        routing::post,
    };
    use tower::ServiceExt;

    use crate::adapters::http::auth_admission::{self, AuthAdmission};

    struct HeldHasher {
        started: AtomicUsize,
        release: (Mutex<bool>, Condvar),
    }
    impl UserCredentialsHasher for HeldHasher {
        fn hash_password(&self, _: &str) -> AppResult<String> {
            self.started.fetch_add(1, Ordering::SeqCst);
            let (lock, condition) = &self.release;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
            Ok("secret_hash".into())
        }
        fn verify_password(&self, _: &str, _: &str) -> AppResult<bool> {
            unreachable!()
        }
    }
    struct ReleaseOnDrop(Arc<HeldHasher>);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            let (lock, condition) = &self.0.release;
            *lock.lock().unwrap() = true;
            condition.notify_all();
        }
    }

    fn request(peer: &str) -> Request<Body> {
        let mut request = Request::post("/api/json/user/register")
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        request
    }

    let hasher = Arc::new(HeldHasher {
        started: AtomicUsize::new(0),
        release: (Mutex::new(false), Condvar::new()),
    });
    let release = ReleaseOnDrop(hasher.clone());
    let users = Arc::new(UserUseCases::new(
        hasher.clone(),
        Arc::new(MockUserPersistence::new()),
        ("test-attempt-secret").into(),
    ));
    let app = Router::new()
        .route(
            "/api/json/user/register",
            post(move |Extension(work): Extension<AuthWorkLifetime>| {
                let users = users.clone();
                async move {
                    users
                        .add(
                            "testuser",
                            "testuser@gmail.com",
                            &"secret".into(),
                            Some(work),
                        )
                        .await
                        .map(|_| StatusCode::OK)
                        .unwrap_or(StatusCode::SERVICE_UNAVAILABLE)
                }
            }),
        )
        .layer(middleware::from_fn_with_state(
            Arc::new(AuthAdmission::new(vec![])),
            auth_admission::admit,
        ));

    let first = tokio::spawn(app.clone().oneshot(request("192.0.2.10:1001")));
    let second = tokio::spawn(app.clone().oneshot(request("192.0.2.10:1002")));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while hasher.started.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("two hash jobs started");

    first.abort();
    second.abort();
    let _ = first.await;
    let _ = second.await;
    assert_eq!(
        app.clone()
            .oneshot(request("192.0.2.10:1003"))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS,
        "cancelled requests still own source capacity while their hashes run"
    );

    let other = tokio::spawn(app.clone().oneshot(request("192.0.2.11:1001")));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while hasher.started.load(Ordering::SeqCst) < 3 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("another source reaches a free global hash slot");
    drop(release);
    assert_eq!(other.await.unwrap().unwrap().status(), StatusCode::OK);
    assert_eq!(
        app.oneshot(request("192.0.2.10:1004"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK,
        "source capacity recovers when held workers finish"
    );
}

/// The stored account, so a write is observable by the read that follows it.
struct MockUserPersistence {
    stored: std::sync::Mutex<User>,
    password_enabled: std::sync::atomic::AtomicBool,
    confirmed_address_after_read: std::sync::Mutex<Option<String>>,
}

impl MockUserPersistence {
    fn new() -> Self {
        Self {
            stored: std::sync::Mutex::new(User {
                auth_revision: 0,
                id: Uuid::new_v4(),
                username: "testuser".to_string(),
                email: "testuser@gmail.com".to_string(),
                email_verified_at: None,
                password_hash: Some("secret_hash".to_string()),
                avatar_url: None,
                created_at: chrono::Utc::now(),
            }),
            password_enabled: std::sync::atomic::AtomicBool::new(true),
            confirmed_address_after_read: std::sync::Mutex::new(None),
        }
    }

    fn id(&self) -> Uuid {
        self.stored.lock().unwrap().id
    }
}

#[async_trait]
impl UserPersistence for MockUserPersistence {
    async fn login_external_identity(&self, _: ExternalIdentity<'_>) -> AppResult<Option<User>> {
        Err(AppError::Internal(
            "External login proof is not supported by this test persistence".into(),
        ))
    }

    async fn canonical_auth_identifier(&self, value: &str) -> AppResult<String> {
        Ok(value.to_lowercase())
    }
    async fn reserve_login_attempt(&self, _: &AuthAttemptKey) -> AppResult<bool> {
        Ok(true)
    }

    async fn clear_login_attempt(&self, _: &AuthAttemptKey) -> AppResult<()> {
        Ok(())
    }

    async fn create_user(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
    ) -> AppResult<User> {
        Ok(User {
            auth_revision: 0,
            id: Uuid::new_v4(),
            username: username.to_string(),
            email: email.to_string(),
            email_verified_at: None,
            password_hash: Some(password_hash.to_string()),
            avatar_url: None,
            created_at: chrono::Utc::now(),
        })
    }

    async fn get_by_email(&self, email: &str) -> AppResult<Option<User>> {
        let stored = self.stored.lock().unwrap();
        Ok((email == stored.email).then(|| stored.clone()))
    }

    async fn get_by_username(&self, username: &str) -> AppResult<Option<User>> {
        let stored = self.stored.lock().unwrap();
        Ok((username == stored.username).then(|| stored.clone()))
    }

    async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<User>> {
        let mut stored = self.stored.lock().unwrap();
        let observed = stored.clone();
        if let Some(confirmed) = self.confirmed_address_after_read.lock().unwrap().take() {
            stored.email = confirmed;
            stored.email_verified_at = Some(chrono::Utc::now());
        }
        Ok(Some(observed))
    }

    async fn notification_preferences(&self, _id: Uuid) -> AppResult<NotificationPreferences> {
        Ok(NotificationPreferences::default())
    }

    async fn set_notification_preferences(
        &self,
        _id: Uuid,
        _preferences: NotificationPreferences,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_avatar_url(
        &self,
        _id: Uuid,
        avatar_url: Option<&AvatarUrl>,
    ) -> AppResult<Option<User>> {
        let mut stored = self.stored.lock().unwrap();
        stored.avatar_url = avatar_url.cloned();
        Ok(Some(stored.clone()))
    }

    async fn update_profile(
        &self,
        _id: Uuid,
        profile: ProfileUpdate<'_>,
    ) -> AppResult<Option<User>> {
        let mut stored = self.stored.lock().unwrap();
        if !stored.email.eq_ignore_ascii_case(profile.email.as_str()) {
            stored.email_verified_at = None;
        }
        stored.username = profile.username.to_string();
        stored.email = profile.email.to_string();
        stored.avatar_url = profile.avatar_url.cloned();
        Ok(Some(stored.clone()))
    }

    async fn update_profile_display(
        &self,
        _id: Uuid,
        username: &str,
        avatar_url: Option<&AvatarUrl>,
    ) -> AppResult<Option<User>> {
        let mut stored = self.stored.lock().unwrap();
        stored.username = username.to_string();
        stored.avatar_url = avatar_url.cloned();
        Ok(Some(stored.clone()))
    }

    async fn update_password_hash(
        &self,
        _id: Uuid,
        password_hash: &str,
    ) -> AppResult<Option<User>> {
        let mut stored = self.stored.lock().unwrap();
        stored.password_hash = Some(password_hash.to_string());
        Ok(Some(stored.clone()))
    }

    async fn activate_password(&self, id: Uuid, password_hash: &str) -> AppResult<Option<User>> {
        let updated = self.update_password_hash(id, password_hash).await?;
        self.password_enabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(updated)
    }

    async fn has_login_method(&self, _id: Uuid, method: LoginMethod) -> AppResult<bool> {
        Ok(match method {
            LoginMethod::Password => self
                .password_enabled
                .load(std::sync::atomic::Ordering::SeqCst),
            LoginMethod::Google | LoginMethod::Apple => false,
        })
    }
}

struct MockUserCredentialsHasher;

impl UserCredentialsHasher for MockUserCredentialsHasher {
    fn hash_password(&self, password: &str) -> AppResult<String> {
        Ok(format!("{}_hash", password))
    }

    fn verify_password(&self, password: &str, hash: &str) -> AppResult<bool> {
        Ok(hash == format!("{}_hash", password) || hash == "secret_hash" && password == "secret")
    }
}

#[tokio::test]
async fn add_user_works() {
    let user_use_cases = UserUseCases::new(
        Arc::new(MockUserCredentialsHasher),
        Arc::new(MockUserPersistence::new()),
        ("test-attempt-secret").into(),
    );

    let user = user_use_cases
        .add(
            "testuser",
            "testuser@gmail.com",
            &"testuser_pw".into(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(user.username, "testuser");
    assert_eq!(user.email, "testuser@gmail.com");
}

fn use_cases(persistence: Arc<MockUserPersistence>) -> UserUseCases {
    UserUseCases::new(
        Arc::new(MockUserCredentialsHasher),
        persistence,
        ("test-attempt-secret").into(),
    )
}

enum NoSmtpPolicy {
    Deny,
    AllowUnverified,
}

fn no_smtp_use_cases(persistence: Arc<MockUserPersistence>, policy: NoSmtpPolicy) -> UserUseCases {
    use_cases(persistence).with_email_confirmation(EmailConfirmation {
        registrations: Arc::new(MockRegistrations),
        account_changes: Arc::new(MockAccountChanges::default()),
        codes: Arc::new(RecordingSender::default()),

        transport_available: false,
        allow_unverified_without_smtp: matches!(policy, NoSmtpPolicy::AllowUnverified),
    })
}

/// The pending changes an account has asked for, in memory, plus the codes that were mailed.
///
/// It records the *hash* rather than the code, exactly as Postgres does, so the tests below
/// confirm the same way a browser does: by presenting a code that has to hash to what was
/// stored.
struct StoredAccountChange {
    user_id: Uuid,
    kind: AccountChangeKind,
    payload: String,
    confirmation_code_hash: String,
    live: bool,
}

#[derive(Default)]
struct MockAccountChanges {
    stored: std::sync::Mutex<Vec<StoredAccountChange>>,
}

#[async_trait]
impl AccountChangePersistence for MockAccountChanges {
    async fn save_pending_account_change(
        &self,
        pending: PendingAccountChange<'_>,
    ) -> AppResult<()> {
        let kind = pending.change.kind();
        let payload = match pending.change {
            AccountChange::Email(email) => email.to_string(),
            AccountChange::Password(hash) => hash.to_string(),
        };
        let mut stored = self.stored.lock().unwrap();
        stored.retain(|change| !(change.user_id == pending.user_id && change.kind == kind));
        stored.push(StoredAccountChange {
            user_id: pending.user_id,
            kind,
            payload,
            confirmation_code_hash: pending.confirmation_code_hash.to_string(),
            live: pending.expires_at > chrono::Utc::now(),
        });
        Ok(())
    }

    async fn confirm_pending_account_change(
        &self,
        user_id: Uuid,
        kind: AccountChangeKind,
        confirmation_code_hash: &str,
        _: &AuthAttemptKey,
    ) -> AppResult<Option<User>> {
        let mut stored = self.stored.lock().unwrap();
        let found = stored.iter().position(|change| {
            change.user_id == user_id
                && change.kind == kind
                && change.confirmation_code_hash == confirmation_code_hash
                && change.live
        });
        let Some(found) = found else { return Ok(None) };
        let payload = stored.remove(found).payload;

        Ok(Some(User {
            auth_revision: 0,
            id: user_id,
            username: "testuser".to_string(),
            email: match kind {
                AccountChangeKind::Email => payload.clone(),
                AccountChangeKind::Password => "testuser@gmail.com".to_string(),
            },
            email_verified_at: (kind == AccountChangeKind::Email).then(chrono::Utc::now),
            password_hash: Some(match kind {
                AccountChangeKind::Email => "secret_hash".to_string(),
                AccountChangeKind::Password => payload,
            }),
            avatar_url: None,
            created_at: chrono::Utc::now(),
        }))
    }

    async fn list_pending_account_changes(&self, user_id: Uuid) -> AppResult<Vec<PendingChange>> {
        Ok(self
            .stored
            .lock()
            .unwrap()
            .iter()
            .filter(|change| change.user_id == user_id && change.live)
            .map(|change| match change.kind {
                AccountChangeKind::Email => PendingChange::Email {
                    new_email: EmailAddress::from(change.payload.as_str()),
                    expires_at: chrono::Utc::now() + chrono::Duration::minutes(15),
                },
                AccountChangeKind::Password => PendingChange::Password {
                    expires_at: chrono::Utc::now() + chrono::Duration::minutes(15),
                },
            })
            .collect())
    }

    async fn discard_pending_account_change(
        &self,
        user_id: Uuid,
        kind: AccountChangeKind,
    ) -> AppResult<()> {
        self.stored
            .lock()
            .unwrap()
            .retain(|change| !(change.user_id == user_id && change.kind == kind));
        Ok(())
    }
}

/// Keeps every code instead of mailing it, so a test can present one back.
#[derive(Default)]
struct RecordingSender {
    sent: std::sync::Mutex<Vec<(EmailAddress, String, ConfirmationPurpose)>>,
}

impl RecordingSender {
    /// The code from the most recent mail, and where it went.
    fn last(&self) -> (EmailAddress, String, ConfirmationPurpose) {
        self.sent
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a code was mailed")
    }
}

#[async_trait]
impl ConfirmationCodeSender for RecordingSender {
    async fn send_code(
        &self,
        recipient: &EmailAddress,
        code: &str,
        purpose: ConfirmationPurpose,
    ) -> AppResult<()> {
        self.sent
            .lock()
            .unwrap()
            .push((recipient.clone(), code.to_string(), purpose));
        Ok(())
    }
}

struct MockRegistrations;

#[async_trait]
impl RegistrationPersistence for MockRegistrations {
    async fn save_pending_registration(&self, _: PendingRegistration<'_>) -> AppResult<()> {
        unimplemented!()
    }
    async fn confirm_pending_registration(
        &self,
        _: &EmailAddress,
        _: &str,
        _: &AuthAttemptKey,
    ) -> AppResult<Option<User>> {
        unimplemented!()
    }
}

/// Use cases wired the way a real deployment is: codes are mailed and have to come back.
///
/// Explicitly configured to model a deployment with an outbound confirmation transport.
fn confirming_use_cases(
    persistence: Arc<MockUserPersistence>,
) -> (UserUseCases, Arc<MockAccountChanges>, Arc<RecordingSender>) {
    let changes = Arc::new(MockAccountChanges::default());
    let sender = Arc::new(RecordingSender::default());
    let use_cases = UserUseCases::new(
        Arc::new(MockUserCredentialsHasher),
        persistence,
        ("test-attempt-secret").into(),
    )
    .with_email_confirmation(EmailConfirmation {
        registrations: Arc::new(MockRegistrations),
        account_changes: changes.clone(),
        codes: sender.clone(),
        transport_available: true,
        allow_unverified_without_smtp: false,
    });

    (use_cases, changes, sender)
}

#[tokio::test]
async fn a_new_address_is_mailed_a_code_and_is_not_the_account_s_until_it_comes_back() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();
    let (use_cases, _, sender) = confirming_use_cases(persistence.clone());

    let saved = use_cases
        .update_profile(
            id,
            ProfileUpdate {
                username: "renamed",
                email: &EmailAddress::from("new@example.com"),
                avatar_url: None,
            },
        )
        .await
        .unwrap();

    // The rename lands now; the address does not.
    assert_eq!(saved.user.username, "renamed");
    assert_eq!(
        persistence.stored.lock().unwrap().email,
        "testuser@gmail.com"
    );

    // The code goes to the *new* address -- proving it is reachable is the whole point.
    let (sent_to, code, purpose) = sender.last();
    assert_eq!(sent_to.as_str(), "new@example.com");
    assert_eq!(purpose, ConfirmationPurpose::EmailChange);
    assert!(matches!(
        saved.address,
        AddressSaveOutcome::ConfirmationPending(PendingChange::Email { .. })
    ));

    let confirmed = use_cases
        .confirm_account_change(id, AccountChangeKind::Email, &code)
        .await
        .unwrap();
    assert_eq!(confirmed.email, "new@example.com");
    assert!(
        use_cases
            .pending_account_changes(id)
            .await
            .unwrap()
            .is_empty(),
        "a spent code leaves nothing to confirm again"
    );
}

#[tokio::test]
async fn a_new_password_is_mailed_a_code_and_the_old_one_stands_until_it_comes_back() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();
    let (use_cases, _, sender) = confirming_use_cases(persistence.clone());

    let outcome = use_cases
        .change_password(
            id,
            PasswordChange {
                current: &"secret".into(),
                new: &"a-longer-secret".into(),
                confirmation: &"a-longer-secret".into(),
            },
            None,
        )
        .await
        .unwrap();

    assert!(matches!(outcome, PasswordOutcome::ConfirmationSent(_)));
    assert_eq!(
        persistence.stored.lock().unwrap().password_hash,
        Some("secret_hash".to_owned()),
        "the stored password must still be the old one"
    );

    // This code goes to the address the account already has: the mailbox is what a stolen
    // session does not have.
    let (sent_to, code, purpose) = sender.last();
    assert_eq!(sent_to.as_str(), "testuser@gmail.com");
    assert_eq!(purpose, ConfirmationPurpose::PasswordChange);

    let confirmed = use_cases
        .confirm_account_change(id, AccountChangeKind::Password, &code)
        .await
        .unwrap();
    assert_eq!(
        confirmed.password_hash,
        Some("a-longer-secret_hash".to_owned())
    );
}

#[tokio::test]
async fn a_code_confirms_only_the_change_it_was_mailed_for() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();
    let (use_cases, _, sender) = confirming_use_cases(persistence.clone());

    use_cases
        .update_profile(
            id,
            ProfileUpdate {
                username: "testuser",
                email: &EmailAddress::from("new@example.com"),
                avatar_url: None,
            },
        )
        .await
        .unwrap();
    let (_, email_code, _) = sender.last();

    use_cases
        .change_password(
            id,
            PasswordChange {
                current: &"secret".into(),
                new: &"a-longer-secret".into(),
                confirmation: &"a-longer-secret".into(),
            },
            None,
        )
        .await
        .unwrap();

    // Both are outstanding at once, and the address code must not spend the password request.
    assert_eq!(
        use_cases.pending_account_changes(id).await.unwrap().len(),
        2
    );
    let crossed = use_cases
        .confirm_account_change(id, AccountChangeKind::Password, &email_code)
        .await;
    assert!(matches!(crossed, Err(AppError::BadRequest(_))));

    for wrong in ["000000", "12345", "abcdef"] {
        assert!(
            use_cases
                .confirm_account_change(id, AccountChangeKind::Email, wrong)
                .await
                .is_err(),
            "{wrong} should not confirm anything"
        );
    }
}

#[tokio::test]
async fn a_cancelled_request_gives_the_form_back_and_voids_its_code() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();
    let (use_cases, _, sender) = confirming_use_cases(persistence);

    use_cases
        .update_profile(
            id,
            ProfileUpdate {
                username: "testuser",
                email: &EmailAddress::from("new@example.com"),
                avatar_url: None,
            },
        )
        .await
        .unwrap();
    let (_, code, _) = sender.last();

    use_cases
        .discard_account_change(id, AccountChangeKind::Email)
        .await
        .unwrap();

    assert!(
        use_cases
            .pending_account_changes(id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        use_cases
            .confirm_account_change(id, AccountChangeKind::Email, &code)
            .await
            .is_err(),
        "a cancelled request's code must not still work"
    );
}

#[tokio::test]
async fn updating_a_profile_trims_and_stores_every_field() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();

    let saved = no_smtp_use_cases(persistence, NoSmtpPolicy::AllowUnverified)
        .update_profile(
            id,
            ProfileUpdate {
                username: "  renamed  ",
                email: &EmailAddress::from("  new@example.com  "),
                avatar_url: Some(&AvatarUrl::from("https://cdn.example.com/me.png")),
            },
        )
        .await
        .unwrap();

    assert_eq!(saved.user.username, "renamed");
    assert_eq!(saved.user.email, "new@example.com");
    assert_eq!(
        saved.user.avatar_url.as_deref(),
        Some("https://cdn.example.com/me.png")
    );
    assert!(matches!(
        saved.address,
        AddressSaveOutcome::InstalledUnverified
    ));
}

#[tokio::test]
async fn a_profile_needs_a_name_and_an_address_shaped_like_one() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();
    let use_cases = use_cases(persistence);

    for (username, email) in [("   ", "fine@example.com"), ("named", "not-an-address")] {
        let rejected = use_cases
            .update_profile(
                id,
                ProfileUpdate {
                    username,
                    email: &EmailAddress::from(email),
                    avatar_url: None,
                },
            )
            .await;

        assert!(
            matches!(rejected, Err(AppError::BadRequest(_))),
            "{username}/{email} should have been refused, got {rejected:?}"
        );
    }
}

#[tokio::test]
async fn changing_a_password_rehashes_it_only_for_whoever_knows_the_current_one() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();
    let use_cases = use_cases(persistence.clone());

    let wrong_current = use_cases
        .change_password(
            id,
            PasswordChange {
                current: &"not-the-password".into(),
                new: &"a-longer-secret".into(),
                confirmation: &"a-longer-secret".into(),
            },
            None,
        )
        .await;
    assert!(matches!(wrong_current, Err(AppError::InvalidCredentials)));

    use_cases
        .change_password(
            id,
            PasswordChange {
                current: &"secret".into(),
                new: &"a-longer-secret".into(),
                confirmation: &"a-longer-secret".into(),
            },
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        persistence.stored.lock().unwrap().password_hash,
        Some("a-longer-secret_hash".to_owned())
    );
}

#[tokio::test]
async fn a_new_password_must_be_long_enough_and_confirmed() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();
    let use_cases = use_cases(persistence.clone());

    for (new, confirmation) in [("short", "short"), ("a-longer-secret", "a-longer-secrat")] {
        let rejected = use_cases
            .change_password(
                id,
                PasswordChange {
                    current: &"secret".into(),
                    new: &new.into(),
                    confirmation: &confirmation.into(),
                },
                None,
            )
            .await;

        assert!(
            matches!(rejected, Err(AppError::BadRequest(_))),
            "{new}/{confirmation} should have been refused, got {rejected:?}"
        );
    }

    assert_eq!(
        persistence.stored.lock().unwrap().password_hash,
        Some("secret_hash".to_owned()),
        "a refused change must leave the stored password alone"
    );
}

#[tokio::test]
async fn login_user_works() {
    let user_use_cases = UserUseCases::new(
        Arc::new(MockUserCredentialsHasher),
        Arc::new(MockUserPersistence::new()),
        ("test-attempt-secret").into(),
    );

    let user = user_use_cases
        .login("testuser@gmail.com", &"secret".into(), None, None)
        .await
        .unwrap();

    assert_eq!(user.username, "testuser");
}

#[tokio::test]
async fn linking_an_external_provider_requires_the_accounts_email() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();
    let user_use_cases = use_cases(persistence);
    let other_email = EmailAddress::from("someone-else@example.com");

    let result = user_use_cases
        .link_external(
            id,
            ExternalIdentity {
                provider: LoginMethod::Apple,
                subject: "apple-subject",
                email: &other_email,
                email_verified: true,
                display_name: None,
            },
        )
        .await;

    assert!(matches!(result, Err(AppError::BadRequest(message)) if message.contains("must match")));
}

#[tokio::test]
async fn security_email_identity_application_rejects_unverified_oauth_claims() {
    let persistence = Arc::new(MockUserPersistence::new());
    let user_id = persistence.id();
    let users = use_cases(persistence);
    let email = EmailAddress::from("testuser@gmail.com");
    let identity = ExternalIdentity {
        provider: LoginMethod::Google,
        subject: "unverified-subject",
        email: &email,
        email_verified: false,
        display_name: None,
    };
    assert!(matches!(
        users.register_external(identity).await,
        Err(AppError::BadRequest(_))
    ));
    assert!(matches!(
        users.login_external(identity).await,
        Err(AppError::BadRequest(_))
    ));
    assert!(matches!(
        users.link_external(user_id, identity).await,
        Err(AppError::BadRequest(_))
    ));
}

#[tokio::test]
async fn security_email_identity_no_smtp_registration_has_no_proof() {
    let users = no_smtp_use_cases(
        Arc::new(MockUserPersistence::new()),
        NoSmtpPolicy::AllowUnverified,
    );
    let created = users
        .register("newuser", "newuser@example.com", &"password".into(), None)
        .await
        .unwrap();
    let RegistrationOutcome::CreatedUnverified(account) = created else {
        panic!("explicit bypass creates only an unverified account");
    };
    assert!(account.email_verified_at.is_none());
}

#[tokio::test]
async fn security_email_identity_default_no_smtp_refuses_registration_and_address_change() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();
    persistence.stored.lock().unwrap().email_verified_at = Some(chrono::Utc::now());
    let users = no_smtp_use_cases(persistence, NoSmtpPolicy::Deny);

    assert!(matches!(
        users
            .register("newuser", "newuser@example.com", &"password".into(), None)
            .await,
        Err(AppError::BadRequest(message)) if message.contains("SMTP")
    ));

    let saved = users
        .update_profile(
            id,
            ProfileUpdate {
                username: "renamed",
                email: &EmailAddress::from("new@example.com"),
                avatar_url: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(saved.address, AddressSaveOutcome::Unavailable));
    assert_eq!(saved.user.username, "renamed");
    assert_eq!(saved.user.email, "testuser@gmail.com");
    assert!(saved.user.email_verified_at.is_some());
}

#[tokio::test]
async fn security_email_identity_stale_profile_read_preserves_concurrent_confirmation() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();
    *persistence.confirmed_address_after_read.lock().unwrap() =
        Some("confirmed@example.com".into());
    let users = no_smtp_use_cases(persistence, NoSmtpPolicy::Deny);
    let saved = users
        .update_profile(
            id,
            ProfileUpdate {
                username: "renamed",
                email: &EmailAddress::from("testuser@gmail.com"),
                avatar_url: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        saved.address,
        AddressSaveOutcome::CurrentAddressChanged
    ));
    assert_eq!(saved.user.username, "renamed");
    assert_eq!(saved.user.email, "confirmed@example.com");
    assert!(saved.user.email_verified_at.is_some());
}

#[tokio::test]
async fn security_email_identity_bypass_address_change_clears_old_proof() {
    let persistence = Arc::new(MockUserPersistence::new());
    let id = persistence.id();
    persistence.stored.lock().unwrap().email_verified_at = Some(chrono::Utc::now());
    let users = no_smtp_use_cases(persistence, NoSmtpPolicy::AllowUnverified);
    let saved = users
        .update_profile(
            id,
            ProfileUpdate {
                username: "renamed",
                email: &EmailAddress::from("new@example.com"),
                avatar_url: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        saved.address,
        AddressSaveOutcome::InstalledUnverified
    ));
    assert_eq!(saved.user.email, "new@example.com");
    assert!(saved.user.email_verified_at.is_none());
}

#[tokio::test]
async fn an_oauth_only_account_can_add_password_login() {
    let persistence = Arc::new(MockUserPersistence::new());
    persistence
        .password_enabled
        .store(false, std::sync::atomic::Ordering::SeqCst);
    persistence.stored.lock().unwrap().password_hash = None;
    let id = persistence.id();
    let user_use_cases = use_cases(persistence.clone());

    let result = user_use_cases
        .set_password(
            id,
            PasswordSetup {
                new: &"a-new-password".into(),
                confirmation: &"a-new-password".into(),
            },
            None,
        )
        .await
        .expect("set password");

    assert!(matches!(result, PasswordOutcome::Changed));
    assert!(
        persistence
            .has_login_method(id, LoginMethod::Password)
            .await
            .unwrap()
    );
    assert_eq!(
        persistence.stored.lock().unwrap().password_hash,
        Some("a-new-password_hash".to_owned())
    );
}

#[tokio::test]
async fn passwordless_login_refuses_even_if_dummy_verification_returns_true() {
    struct AcceptingHasher(std::sync::atomic::AtomicUsize);
    impl UserCredentialsHasher for AcceptingHasher {
        fn hash_password(&self, _: &str) -> AppResult<String> {
            unreachable!()
        }
        fn verify_password(&self, _: &str, hash: &str) -> AppResult<bool> {
            assert_eq!(hash, DUMMY_PASSWORD_HASH);
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(true)
        }
    }
    let persistence = Arc::new(MockUserPersistence::new());
    persistence.stored.lock().unwrap().password_hash = None;
    let hasher = Arc::new(AcceptingHasher(std::sync::atomic::AtomicUsize::new(0)));
    let users = UserUseCases::new(hasher.clone(), persistence, ("test-attempt-secret").into());
    assert!(matches!(
        users
            .login("testuser", &"candidate".into(), None, None)
            .await,
        Err(AppError::InvalidCredentials)
    ));
    assert_eq!(hasher.0.load(std::sync::atomic::Ordering::SeqCst), 1);
}
