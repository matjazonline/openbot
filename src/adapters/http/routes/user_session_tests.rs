use super::*;

#[tokio::test]
async fn security_auth_limits_hash_overload_reaches_html_and_json_routes() {
    use std::sync::{
        Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    struct HeldHasher {
        started: AtomicUsize,
        release: (Mutex<bool>, Condvar),
    }
    impl crate::use_cases::user::UserCredentialsHasher for HeldHasher {
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
    struct Release(Arc<HeldHasher>);
    impl Drop for Release {
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
    let release = Release(hasher.clone());
    let users = Arc::new(with_unverified_bypass(UserUseCases::new(
        hasher.clone(),
        Arc::new(StubUserPersistence {
            id: uuid::Uuid::new_v4(),
            create_error: None,
        }),
        "test-attempt-secret",
    )));
    let jobs: Vec<_> = (0..4)
        .map(|_| {
            let users = users.clone();
            tokio::spawn(async move {
                users
                    .add("newcomer", "newcomer@example.com", &"pw".into(), None)
                    .await
            })
        })
        .collect();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while hasher.started.load(Ordering::SeqCst) < 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let sessions = Arc::new(SessionAuthority::for_test(
        &crate::infra::config::AppConfig::for_test(),
    ));
    let login = LoginForm {
        email_or_username: "missing".into(),
        password: "pw".into(),
    };
    let html_login = login_form(
        State(users.clone()),
        State(sessions.clone()),
        None,
        test_work(),
        CookieJar::new(),
        Form(login.clone()),
    )
    .await
    .into_response();
    assert_eq!(html_login.status(), StatusCode::SERVICE_UNAVAILABLE);
    let login_body = axum::body::to_bytes(html_login.into_body(), 4096)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&login_body).contains(AUTH_BUSY_MESSAGE));

    let html_registration = register_form(
        State(users.clone()),
        State(sessions.clone()),
        test_work(),
        CookieJar::new(),
        Form(registration("pw", Some("pw"))),
    )
    .await
    .into_response();
    assert_eq!(html_registration.status(), StatusCode::SERVICE_UNAVAILABLE);
    let registration_body = axum::body::to_bytes(html_registration.into_body(), 4096)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&registration_body).contains(AUTH_BUSY_MESSAGE));

    let json_login = match login_json(
        State(users.clone()),
        State(sessions.clone()),
        None,
        test_work(),
        CookieJar::new(),
        Json(login),
    )
    .await
    {
        Ok(_) => panic!("busy login accepted"),
        Err(error) => error.into_response(),
    };
    assert_eq!(json_login.status(), StatusCode::SERVICE_UNAVAILABLE);
    let json_registration = match register_json(
        State(users),
        State(sessions),
        test_work(),
        CookieJar::new(),
        Json(RegisterPayload {
            username: "newcomer".into(),
            email: "newcomer@example.com".into(),
            password: "pw".into(),
        }),
    )
    .await
    {
        Ok(_) => panic!("busy registration accepted"),
        Err(error) => error.into_response(),
    };
    assert_eq!(json_registration.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(release);
    for job in jobs {
        job.await.unwrap().unwrap();
    }
}

/// Registering is signing in: the response carries a session this deployment will believe,
/// naming the account that was just created.
#[tokio::test]
async fn registering_signs_the_new_account_in() {
    let id = uuid::Uuid::new_v4();
    let sessions = Arc::new(SessionAuthority::for_test(
        &crate::infra::config::AppConfig::for_test(),
    ));

    let response = register_form(
        State(registering(id)),
        State(sessions.clone()),
        test_work(),
        CookieJar::new(),
        Form(registration("hunter2", Some("hunter2"))),
    )
    .await
    .into_response();

    let session = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().expect("a printable cookie"))
        .find_map(|value| Cookie::parse(value.to_string()).ok())
        .filter(|cookie| cookie.name() == crate::adapters::http::session::SESSION_COOKIE)
        .expect("registration issues a session cookie");

    assert_eq!(
        sessions
            .verify(session.value())
            .await
            .unwrap()
            .map(|s| s.user_id),
        Some(id)
    );
}

#[tokio::test]
async fn registering_lands_on_the_post_authentication_gateway() {
    let sessions = Arc::new(SessionAuthority::for_test(
        &crate::infra::config::AppConfig::for_test(),
    ));

    let response = register_form(
        State(registering(uuid::Uuid::new_v4())),
        State(sessions),
        test_work(),
        CookieJar::new(),
        Form(registration("hunter2", Some("hunter2"))),
    )
    .await
    .into_response();

    assert_eq!(response.headers().get("HX-Redirect").unwrap(), "/");
}

/// A rejected registration must not hand out a session for an account that was never created.
#[tokio::test]
async fn mismatched_passwords_issue_no_session() {
    let sessions = Arc::new(SessionAuthority::for_test(
        &crate::infra::config::AppConfig::for_test(),
    ));

    let response = register_form(
        State(registering(uuid::Uuid::new_v4())),
        State(sessions),
        test_work(),
        CookieJar::new(),
        Form(registration("hunter2", Some("hunter3"))),
    )
    .await
    .into_response();

    assert!(response.headers().get("HX-Redirect").is_none());
    assert!(
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .next()
            .is_none()
    );
}

#[tokio::test]
async fn logout_clears_both_cookies_and_redirects_to_login() {
    let sessions = Arc::new(SessionAuthority::for_test(
        &crate::infra::config::AppConfig::for_test(),
    ));

    // A browser part-way through the change carries both: the session it signed in with, and
    // the plaintext cookie left over from before.
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::COOKIE,
        format!(
            "session={}; user_id={}",
            sessions.cookie_for_test(uuid::Uuid::nil()).await.value(),
            uuid::Uuid::nil()
        )
        .parse()
        .unwrap(),
    );
    let jar = CookieJar::from_headers(&headers);

    let response = logout(
        Extension(crate::application::session::VerifiedSession {
            user_id: uuid::Uuid::nil(),
            session_id: crate::application::session::SessionId(uuid::Uuid::new_v4()),
        }),
        State(sessions),
        jar,
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get("location").unwrap(), "/login");

    let cleared: Vec<&str> = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().expect("a printable cookie"))
        .collect();

    // The session goes, and so does the plaintext cookie it replaced -- a browser still
    // holding one of those should stop offering it.
    assert!(
        cleared.iter().any(|cookie| cookie.starts_with("session=")),
        "{cleared:?}"
    );
    assert!(
        cleared.iter().any(|cookie| cookie.starts_with("user_id=")),
        "{cleared:?}"
    );
    assert!(
        cleared
            .iter()
            .all(|cookie| cookie.contains("Max-Age=0") && cookie.contains("Path=/")),
        "{cleared:?}"
    );
}

#[tokio::test]
async fn security_session_registration_store_outage_emits_no_cookie() {
    use crate::{adapters::persistence::PostgresPersistence, infra::config::AppConfig};
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@localhost/unused")
        .unwrap();
    pool.close().await;
    let sessions = Arc::new(SessionAuthority::new(
        &AppConfig::for_test(),
        Arc::new(PostgresPersistence::new(pool)),
    ));
    let response = register_form(
        State(registering(uuid::Uuid::new_v4())),
        State(sessions.clone()),
        test_work(),
        CookieJar::new(),
        Form(registration("hunter2", Some("hunter2"))),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(response.headers().get("set-cookie").is_none());
    let response = register_json(
        State(registering(uuid::Uuid::new_v4())),
        State(sessions),
        test_work(),
        CookieJar::new(),
        Json(RegisterPayload {
            username: "newcomer".into(),
            email: "new@example.test".into(),
            password: "hunter2".into(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(response.headers().get("set-cookie").is_none());
}
