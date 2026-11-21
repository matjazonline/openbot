//! Required desk ownership in the JSON create/edit boundary.
use super::super::authorization_test_support::Fixture;
use super::*;
use crate::application::ports::department::DepartmentWrite;
use crate::entities::channel::ChannelAccessMode;
use crate::use_cases::department::DepartmentUseCases;

async fn put(f: &Fixture, body: serde_json::Value) -> Channel {
    let payload: ChannelJsonPayload = serde_json::from_value(body).expect("a valid payload");
    let response = update_channel_json(
        State(f.channels.clone()),
        Fixture::auth(&f.owner),
        Path((f.company.id, f.channel)),
        Json(payload),
    )
    .await
    .expect("the owner saves the desk")
    .into_response();
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // 01-departments 04C.4: the desk response carries its ownership fields, always present.
    assert!(saved["channel"].get("department_id").is_some(), "{saved}");
    assert!(saved["channel"]["department_id"].is_string(), "{saved}");
    assert!(saved["channel"].get("members_only").is_none(), "{saved}");
    serde_json::from_value(saved["channel"].clone()).unwrap()
}

#[tokio::test]
async fn access_mode_json_owner_is_preserved_and_can_be_reassigned_but_not_cleared() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let departments = DepartmentUseCases::new(
        f.persistence.clone(),
        Arc::new(
            crate::adapters::persistence::department::PostgresDepartments::new(
                f.persistence.as_ref().clone(),
                crate::adapters::persistence::test_support::provisioning_templates(),
            ),
        ),
    );
    let department = departments
        .create_department(
            f.owner.id,
            f.company.id,
            DepartmentWrite {
                name: "Routing".into(),
                slug: "routing".into(),
                ..DepartmentWrite::default()
            },
            None,
        )
        .await
        .unwrap();
    let base = serde_json::json!({"name": "Inbox", "slug": "inbox"});
    let with = |extra: serde_json::Value| {
        let mut body = base.clone();
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };

    let owned = put(
        &f,
        with(serde_json::json!({"department_id": department.id})),
    )
    .await;
    assert_eq!(
        (owned.department_id, owned.access_mode),
        (department.id, ChannelAccessMode::Members)
    );
    let kept = put(&f, base.clone()).await;
    assert_eq!(kept.department_id, department.id);
    let public = put(
        &f,
        with(serde_json::json!({"participant_emails": ["@public"], "confirm_spam_disabled": true})),
    )
    .await;
    assert_eq!(
        (public.department_id, public.access_mode),
        (department.id, ChannelAccessMode::Public)
    );
    assert!(
        serde_json::from_value::<ChannelJsonPayload>(with(
            serde_json::json!({"department_id": null})
        ))
        .is_err()
    );
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(f.company.id)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    let reassigned = put(&f, with(serde_json::json!({"department_id": root}))).await;
    assert_eq!(reassigned.department_id.as_uuid(), root);
}

#[test]
fn access_mode_json_department_rejects_explicit_null_and_invalid_ids() {
    for department in [serde_json::Value::Null, serde_json::json!("invalid")] {
        assert!(
            serde_json::from_value::<ChannelJsonPayload>(
                serde_json::json!({"name":"Inbox", "department_id": department})
            )
            .is_err()
        );
    }
    let omitted: ChannelJsonPayload =
        serde_json::from_value(serde_json::json!({"name":"Inbox"})).unwrap();
    assert_eq!(omitted.department_id, None);
}
