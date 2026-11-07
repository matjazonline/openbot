use super::*;
#[tokio::test]
async fn google_oauth_registered_url_state_pkce_and_private_metadata() {
    use base64::Engine;
    let f = Box::pin(Fixture::new()).await;
    let start = f.begin(None).await;
    let url = url::Url::parse(start.authorization_url.as_str()).unwrap();
    assert_eq!(
        url.origin().ascii_serialization(),
        "https://accounts.google.com"
    );
    let pairs: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(pairs["access_type"], "offline");
    assert_eq!(pairs["code_challenge_method"], "S256");
    assert!(pairs["scope"].contains("gmail.readonly") && pairs["scope"].contains("gmail.send"));
    for key in [
        "client_secret",
        "refresh_token",
        "access_token",
        "code_verifier",
    ] {
        assert!(!pairs.contains_key(key));
    }
    let facts = f.service.codec.verify(&pairs["state"]).unwrap();
    let envelope: String =
        sqlx::query_scalar("SELECT envelope FROM integration_connect_attempts WHERE id=$1")
            .bind(start.attempt.id.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap();
    let verifier=f.vault.persistence.credential_cipher().unwrap().open_envelope(&crate::adapters::persistence::credentials::envelope::CredentialContext::integration_connect(&facts),&envelope).unwrap();
    assert_eq!(
        pairs["code_challenge"],
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.expose_secret().as_bytes()))
    );
    let metadata = serde_json::to_string(
        &f.service
            .get_connect_attempt(&f.owner, start.attempt.id)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(!metadata.contains(verifier.expose_secret()) && !metadata.contains("envelope"));
    assert_eq!(f.finish(&start).await.status, ConnectStatus::Connected);
    let observation = f.script.observed.lock().unwrap();
    let observation = observation.as_ref().unwrap();
    assert!(observation.client_valid);
    assert_eq!(observation.grant_type, "authorization_code");
    assert_eq!(observation.challenge, pairs["code_challenge"]);
    assert_eq!(
        observation.code_digest,
        <[u8; 32]>::from(Sha256::digest(b"authorization-code-sentinel"))
    );
}
#[tokio::test]
async fn google_vault_callback_verified_mailbox_differs_from_login_and_replay_denies() {
    let f = Box::pin(Fixture::new()).await;
    let start = f.begin(None).await;
    let done = f.finish(&start).await;
    assert_eq!(done.status, ConnectStatus::Connected);
    let account = f
        .vault
        .get(f.owner.owner(), done.account.unwrap())
        .await
        .unwrap();
    assert_eq!(account.metadata.label(), "different-mailbox@gmail.com");
    assert_eq!(
        account.external_identity.as_ref().unwrap().subject.as_str(),
        "provider-subject"
    );
    let metadata = serde_json::to_string(&account).unwrap();
    for secret in [
        "access-sentinel",
        "refresh-sentinel",
        "authorization-code-sentinel",
        "client-secret-sentinel",
    ] {
        assert!(!metadata.contains(secret));
    }
    let material: Option<String> =
        sqlx::query_scalar("SELECT envelope FROM integration_connect_attempts WHERE id=$1")
            .bind(start.attempt.id.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert!(material.is_none());
    assert!(
        f.service
            .finish_connect(&f.owner, f.callback(&start), work(CancellationToken::new()))
            .await
            .is_err()
    );
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 1);
}
#[derive(Clone, Copy)]
enum Refusal {
    Partial,
    MissingRefresh,
    Unverified,
    Type,
    Lifetime,
    Malformed,
    Oversized,
    Admin,
    Client,
    Denied,
}
async fn refusal(case: Refusal, expected: ConnectStatus) {
    let f = Box::pin(Fixture::new()).await;
    match case {
        Refusal::Partial => {
            f.script.token.lock().unwrap()["scope"] =
                json!("openid email https://www.googleapis.com/auth/gmail.readonly")
        }
        Refusal::MissingRefresh => {
            f.script
                .token
                .lock()
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove("refresh_token");
        }
        Refusal::Unverified => f.script.profile.lock().unwrap()["email_verified"] = json!(false),
        Refusal::Type => f.script.token.lock().unwrap()["token_type"] = json!("Basic"),
        Refusal::Lifetime => f.script.token.lock().unwrap()["expires_in"] = json!(60),
        Refusal::Malformed => {
            *f.script.token.lock().unwrap() = json!("raw-provider-error-sentinel")
        }
        Refusal::Oversized => {
            f.script.token.lock().unwrap()["access_token"] =
                json!("x".repeat(MAX_CREDENTIAL_BYTES + 1))
        }
        Refusal::Admin => {
            *f.script.status.lock().unwrap() = 403;
            *f.script.token.lock().unwrap() = json!({"error":"admin_policy_enforced","error_description":"raw-provider-error-sentinel"});
        }
        Refusal::Client => {
            *f.script.status.lock().unwrap() = 401;
            *f.script.token.lock().unwrap() =
                json!({"error":"invalid_client","error_description":"client-secret-sentinel"});
        }
        Refusal::Denied => {}
    }
    let start = f.begin(None).await;
    let mut callback = f.callback(&start);
    if matches!(case, Refusal::Denied) {
        callback.code = None;
        callback.error = Some("access_denied".into());
    }
    let done = f
        .service
        .finish_connect(&f.owner, callback, work(CancellationToken::new()))
        .await
        .unwrap();
    assert_eq!(done.status, expected);
    assert!(
        f.vault
            .list(f.owner.owner(), PageRequest::new(None, 50).unwrap())
            .await
            .unwrap()
            .accounts
            .is_empty()
    );
    let encoded = format!("{:?} {}", done, serde_json::to_string(&done).unwrap());
    for value in [
        "raw-provider-error-sentinel",
        "client-secret-sentinel",
        "access-sentinel",
        "refresh-sentinel",
    ] {
        assert!(!encoded.contains(value));
    }
}
macro_rules! refusal_test {
    ($name:ident,$case:ident,$status:ident) => {
        #[tokio::test]
        async fn $name() {
            Box::pin(refusal(Refusal::$case, ConnectStatus::$status)).await;
        }
    };
}
refusal_test!(
    google_oauth_partial_permissions_refused,
    Partial,
    MissingPermissions
);
refusal_test!(
    google_oauth_missing_refresh_refused,
    MissingRefresh,
    MissingRefresh
);
refusal_test!(
    google_oauth_unverified_mailbox_refused,
    Unverified,
    IdentityMismatch
);
refusal_test!(google_oauth_wrong_token_type_refused, Type, ProviderError);
refusal_test!(google_oauth_short_lifetime_refused, Lifetime, ProviderError);
refusal_test!(
    google_oauth_malformed_response_sanitized,
    Malformed,
    ProviderError
);
refusal_test!(
    google_oauth_oversized_response_refused,
    Oversized,
    ProviderError
);
refusal_test!(
    google_oauth_workspace_admin_block_distinct,
    Admin,
    AdminBlocked
);
refusal_test!(google_oauth_invalid_client_distinct, Client, Configuration);
refusal_test!(google_oauth_user_denial_distinct, Denied, Denied);
#[tokio::test]
async fn google_oauth_personal_and_workspace_identities_supported() {
    for email in ["mailbox@gmail.com", "mailbox@workspace.example"] {
        let f = Box::pin(Fixture::new()).await;
        f.script.profile.lock().unwrap()["email"] = json!(email);
        let done = f.finish(&f.begin(None).await).await;
        assert_eq!(
            f.vault
                .get(f.owner.owner(), done.account.unwrap())
                .await
                .unwrap()
                .metadata
                .label(),
            email
        );
    }
}
#[tokio::test]
async fn google_oauth_cancellation_reaches_active_request_and_never_writes_account() {
    let f = Box::pin(Fixture::new()).await;
    let start = f.begin(None).await;
    let permit = f.script.gate.clone().acquire_owned().await.unwrap();
    let cancel = CancellationToken::new();
    let worker = f.spawn_finish(&start, cancel.clone());
    f.script.wait_calls(1).await;
    cancel.cancel();
    let done = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(done.status, ConnectStatus::Cancelled);
    drop(permit);
    assert!(
        f.vault
            .list(f.owner.owner(), PageRequest::new(None, 50).unwrap())
            .await
            .unwrap()
            .accounts
            .is_empty()
    );
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 1);
}
