use super::*;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn integration_connect_admission_exhaustion_precedes_attempt_and_provider_work() {
    let f = Box::pin(Fixture::new()).await;
    let router = app(&f);
    for n in 0..30 {
        let method = if n % 2 == 0 { "HEAD" } else { "GET" };
        let response = router
            .clone()
            .oneshot(request(
                method,
                "/auth/integrations/%67oogle/callback?state=invalid&code=sentinel",
                Some(&f.cookie),
                "",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }
    for (method, path, origin) in [
        (
            "POST",
            "/ui/profile/vault/apps/g%6dail/connect",
            Some("http://localhost:3000"),
        ),
        ("GET", "/auth/integrations/%67oogle/callback", None),
        ("HEAD", "/auth/integrations/google/callback", None),
    ] {
        let response = router
            .clone()
            .oneshot(request(method, path, Some(&f.cookie), "", origin))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }
    assert_eq!(f.attempt_count().await, 0);
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 0);
}

fn callback_path(f: &Fixture, start: &ConnectStart) -> String {
    let callback = f.callback(start);
    let mut url = url::Url::parse("http://localhost/auth/integrations/%67oogle/callback").unwrap();
    url.query_pairs_mut()
        .append_pair("state", &callback.state)
        .append_pair("code", "sentinel");
    format!("{}?{}", url.path(), url.query().unwrap())
}

#[tokio::test]
async fn integration_connect_admission_concurrent_callbacks_hold_the_real_active_budget() {
    let f = Box::pin(Fixture::new()).await;
    let router = app(&f);
    let starts = [f.begin(None).await, f.begin(None).await];
    let held = f.script.gate.clone().acquire_owned().await.unwrap();
    let mut workers = Vec::new();
    for start in &starts {
        let request = request("GET", &callback_path(&f, start), Some(&f.cookie), "", None);
        let router = router.clone();
        workers.push(tokio::spawn(async move {
            router.oneshot(request).await.unwrap()
        }));
    }
    f.script.wait_calls(2).await;
    for (method, path, origin) in [
        (
            "POST",
            "/ui/profile/vault/apps/g%6dail/connect",
            Some("http://localhost:3000"),
        ),
        ("HEAD", "/auth/integrations/%67oogle/callback", None),
    ] {
        let response = router
            .clone()
            .oneshot(request(method, path, Some(&f.cookie), "", origin))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }
    assert_eq!(f.attempt_count().await, 2);
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 2);
    drop(held);
    for worker in workers {
        assert_eq!(worker.await.unwrap().status(), StatusCode::SEE_OTHER);
    }
    let response = router
        .oneshot(request(
            "POST",
            "/ui/profile/vault/apps/gmail/connect",
            Some(&f.cookie),
            "",
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(f.attempt_count().await, 3);
}
