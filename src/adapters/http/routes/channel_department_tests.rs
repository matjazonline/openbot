//! The JSON channel payload's ownership fields (01-departments p02, DEP-P02-D2): unlike the rest
//! of the PUT they are kept when absent, `null` detaches the desk, and either field alone changes
//! only itself.
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
    serde_json::from_value(saved["channel"].clone()).unwrap()
}

#[tokio::test]
async fn department_desk_ownership_json_fields_are_kept_when_absent() {
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
        with(serde_json::json!({"department_id": department.id, "members_only": true})),
    )
    .await;
    assert_eq!(
        (owned.department_id, owned.members_only, owned.access_mode),
        (Some(department.id), true, ChannelAccessMode::Allowlist)
    );

    let kept = put(&f, base.clone()).await;
    assert_eq!(
        (kept.department_id, kept.members_only),
        (Some(department.id), true)
    );

    let opened = put(&f, with(serde_json::json!({"members_only": false}))).await;
    assert_eq!(
        (
            opened.department_id,
            opened.members_only,
            opened.access_mode
        ),
        (Some(department.id), false, ChannelAccessMode::Team),
        "members_only alone leaves the department"
    );

    let detached = put(&f, with(serde_json::json!({"department_id": null}))).await;
    assert_eq!(
        (detached.department_id, detached.members_only),
        (None, false)
    );
}
