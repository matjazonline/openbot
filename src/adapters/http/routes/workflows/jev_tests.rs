use super::*;
use crate::adapters::persistence::credentials::CredentialCipher;

#[tokio::test]
async fn jev_connection_routes_owner_admin_scope_revision_and_secret_free_views() {
    let mut f = Fixture::new().await.unwrap();
    f.persistence = PostgresPersistence::with_credential_cipher(
        f.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    f.engine = Arc::new(WorkflowEngine::for_tests(f.persistence.clone()));
    let create = |user| {
        jev::create(
            f.state(),
            by(user),
            Path(f.company),
            body(json!({"enabled":true,"models":["jev-1.14.0"],"api_key":"super-secret-key"})),
        )
    };
    assert_eq!(
        answer(create(f.member).await).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        answer(create(f.stranger).await).await.0,
        StatusCode::NOT_FOUND
    );
    let (status, c) = answer(create(f.owner).await).await;
    assert_eq!(status, StatusCode::CREATED, "{c}");
    let id: Uuid = serde_json::from_value(c["id"].clone()).unwrap();
    assert_eq!(answer(create(f.admin).await).await.0, StatusCode::CREATED);
    assert!(
        !c.to_string().contains("secret")
            && !c.to_string().contains("api_key")
            && !c.to_string().contains("envelope")
    );
    for (company, user) in [
        (f.other, f.stranger),
        (f.company, f.stranger),
        (f.company, f.member),
    ] {
        assert_eq!(
            answer(jev::read(f.state(), by(user), Path((company, id))).await)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            answer(
                jev::update(
                    f.state(),
                    by(user),
                    Path((company, id)),
                    body(json!({"expected_revision":1,"enabled":false,"models":["jev-1.14.0"]}))
                )
                .await
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            answer(
                jev::rotate(
                    f.state(),
                    by(user),
                    Path((company, id)),
                    body(json!({"expected_revision":1,"api_key":"new-secret"}))
                )
                .await
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    let (status, rotated) = answer(
        jev::rotate(
            f.state(),
            by(f.admin),
            Path((f.company, id)),
            body(json!({"expected_revision":1,"api_key":"new-secret"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    assert_eq!(rotated["revision"], 2);
    assert_eq!(
        answer(
            jev::update(
                f.state(),
                by(f.owner),
                Path((f.company, id)),
                body(json!({"expected_revision":1,"enabled":false,"models":["jev-1.14.0"]}))
            )
            .await
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, disabled) = answer(
        jev::update(
            f.state(),
            by(f.owner),
            Path((f.company, id)),
            body(json!({"expected_revision":2,"enabled":false,"models":["jev-1.13.0"]})),
        )
        .await,
    )
    .await;
    assert_eq!(disabled["ready"], false);
    let (_, read) = answer(jev::read(f.state(), by(f.owner), Path((f.company, id))).await).await;
    assert_eq!(read, disabled);
    let (_, list) = answer(jev::list(f.state(), by(f.owner), Path(f.company)).await).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert!(!list.to_string().contains("secret"));
}
#[test]
fn jev_connection_request_rejects_origin_aliases_and_secret_boundaries() {
    assert!(
        serde_json::from_value::<jev::CreateBody>(
            json!({"enabled":true,"models":["jev-latest"],"api_key":"key"})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<jev::CreateBody>(json!({"enabled":true,"models":["jev-1.14.0"],"api_key":"key","origin":"https://attacker.test"})).is_err());
    use crate::application::workflow::jev::{validate_models, validate_secret};
    for key in ["", "line\nbreak", "with space"] {
        assert!(validate_secret(&secrecy::SecretString::from(key)).is_err());
    }
    assert!(validate_secret(&secrecy::SecretString::from("a".repeat(4097))).is_err());
    assert!(validate_models(&Default::default()).is_err());
}
