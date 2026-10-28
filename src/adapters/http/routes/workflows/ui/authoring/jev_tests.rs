use super::super::super::typesafe;
use super::*;
use crate::adapters::persistence::credentials::CredentialCipher;
use crate::application::workflow::jev::JevConnections;

async fn fixture() -> Fixture {
    let mut f = Fixture::new().await.unwrap();
    f.persistence = PostgresPersistence::with_credential_cipher(
        f.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    f.engine = Arc::new(WorkflowEngine::for_tests(f.persistence.clone()));
    f
}

fn create_form(models: &str) -> axum::Form<typesafe::ConnectionForm> {
    axum::Form(typesafe::ConnectionForm {
        models: models.into(),
        enabled: Some("true".into()),
        expected_revision: None,
        api_key: Some(secrecy::SecretString::from("do-not-echo-this-key")),
    })
}

#[tokio::test]
async fn workflow_authoring_jev_company_forms_permissions_rotation_and_readiness() {
    let f = fixture().await;
    for user in [f.member, f.stranger] {
        assert!(
            typesafe::create(f.ui(user), Path(f.company), create_form("jev-1.14.0"))
                .await
                .is_err()
        );
        let denied = typesafe::list(f.ui(user), Path(f.company)).await.unwrap().0;
        assert!(
            denied.contains("require company owner or admin permission")
                && !denied.contains("<form")
        );
    }
    let html = typesafe::create(f.ui(f.admin), Path(f.company), create_form("jev-1.14.0"))
        .await
        .unwrap()
        .0;
    assert!(
        html.contains("ready") && html.contains("Rotate API key") && !html.contains("do-not-echo")
    );
    let c = f
        .persistence
        .list_jev(CompanyId::new(f.company), f.ui(f.owner).actor().unwrap())
        .await
        .unwrap()
        .remove(0);
    let html = typesafe::rotate(
        f.ui(f.owner),
        Path((f.company, c.id)),
        axum::Form(typesafe::RotateForm {
            expected_revision: c.revision,
            api_key: "rotated-secret".into(),
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("revision 2") && !html.contains("rotated-secret"));
    let html = typesafe::update(
        f.ui(f.owner),
        Path((f.company, c.id)),
        axum::Form(typesafe::ConnectionForm {
            models: "jev-1.14.0".into(),
            enabled: None,
            expected_revision: Some(2),
            api_key: None,
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("not ready"));
    let html = typesafe::rotate(
        f.ui(f.owner),
        Path((f.company, c.id)),
        axum::Form(typesafe::RotateForm {
            expected_revision: 1,
            api_key: "stale-secret".into(),
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("connection changed") && !html.contains("stale-secret"));
    let html = typesafe::create(f.ui(f.owner), Path(f.company), create_form("jev-latest"))
        .await
        .unwrap()
        .0;
    assert!(!html.contains("do-not-echo") && !html.contains("connection saved"));
    let foreign = typesafe::update(
        f.ui(f.stranger),
        Path((f.other, c.id)),
        axum::Form(typesafe::ConnectionForm {
            models: "jev-1.14.0".into(),
            enabled: None,
            expected_revision: Some(3),
            api_key: None,
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(foreign.contains("permission denied") && !foreign.contains(&c.id.to_string()));
}

#[tokio::test]
async fn workflow_authoring_jev_catalogue_binding_and_sample() {
    let f = fixture().await;
    let _ = typesafe::create(f.ui(f.owner), Path(f.company), create_form("jev-1.14.0"))
        .await
        .unwrap();
    let c = f
        .persistence
        .list_jev(CompanyId::new(f.company), f.ui(f.owner).actor().unwrap())
        .await
        .unwrap()
        .remove(0);
    let workflow = Uuid::new_v4();
    let source = source_of("jev.evaluate", workflow);
    let version = f.published(workflow, "Jev mixed", &source).await;
    let pane = f.pane(f.owner, workflow, "bindings").await.unwrap();
    assert!(pane.contains(&format!("value=\"{}\"", c.id)) && pane.contains("ready · jev-1.14.0"));
    let key = field_value(&pane, "form_key");
    let html = save_binding(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        form(&[
            ("form_key", &key),
            ("version_id", &version.to_string()),
            ("association", "company"),
            ("params_mode", "json"),
            ("params_json", "{}"),
            ("resource:service", &c.id.to_string()),
        ]),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("Saved binding"), "{html}");
    let catalogue = catalogue(f.ui(f.owner), f.query()).await.unwrap().0;
    assert!(
        catalogue.contains("jev.evaluate")
            && catalogue.contains("Noul has no separate confidence")
            && catalogue.contains("/answers/department/confidence")
    );
    let scripts = json!([{"workflow_id":workflow,"step":"start","responses":[{"output":crate::adapters::workflow_jev::tests::output()}]}]);
    let html = sample(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        form(&[
            ("expected_revision", "1"),
            ("input", "{}"),
            ("params", "{}"),
            ("scripts", &scripts.to_string()),
        ]),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("Sample completed"), "{html}");
    assert!(!html.contains("do-not-echo"));
}
