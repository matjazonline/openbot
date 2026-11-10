use super::super::authoring;
use super::*;

#[tokio::test]
async fn vault_authoring_http_setup_scopes_connection_and_registration_and_checks_revision() {
    let f = Fixture::new().await;
    let create =
        || Json(serde_json::from_value(json!({"origin":"https://gmail.googleapis.com"})).unwrap());
    assert!(
        authoring::create(State(f.s.clone()), f.user(), Path(f.foreign), create())
            .await
            .is_err()
    );
    let connection = authoring::create(State(f.s.clone()), f.user(), Path(f.company), create())
        .await
        .unwrap()
        .0;
    let draft = || {
        Json(
            serde_json::from_value(json!({"expected_revision":connection.revision,
        "name":"gmail.reviewed-send", "input_schema":{"type":"object"}, "output_schema":true,
        "effect":"write", "approval_required":true}))
            .unwrap(),
        )
    };
    let path = || {
        Path(ConnectionPath {
            company: f.company,
            id: connection.id,
        })
    };
    assert!(
        authoring::register(
            State(f.s.clone()),
            f.user(),
            Path(ConnectionPath {
                company: f.company,
                id: f.foreign_http.id
            }),
            draft()
        )
        .await
        .is_err()
    );
    assert!(
        authoring::register(
            State(f.s.clone()),
            f.user(),
            Path(ConnectionPath {
                company: f.foreign,
                id: connection.id
            }),
            draft()
        )
        .await
        .is_err()
    );
    let updated = authoring::register(State(f.s.clone()), f.user(), path(), draft())
        .await
        .unwrap()
        .0;
    assert_eq!(updated.revision, connection.revision + 1);
    assert!(
        authoring::register(State(f.s.clone()), f.user(), path(), draft())
            .await
            .is_err()
    );
    let required: bool = sqlx::query_scalar("SELECT approval_required FROM workflow_http_operations WHERE company_id=$1 AND connection_id=$2 AND name='gmail.reviewed-send'")
        .bind(f.company).bind(connection.id).fetch_one(f.p.pool()).await.unwrap();
    assert!(required);
}

#[test]
fn vault_authoring_http_setup_rejects_credentials_and_unrecognized_fields() {
    assert!(
        serde_json::from_value::<authoring::Create>(
            json!({"origin":"https://gmail.googleapis.com","token":"never"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<authoring::Register>(json!({"expected_revision":1,
        "name":"x", "input_schema":true, "output_schema":true, "effect":"write",
        "approval_required":true,"recovery":"safe_repeat"}))
        .is_err()
    );
}
