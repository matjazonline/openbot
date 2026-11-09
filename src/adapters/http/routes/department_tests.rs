//! Department JSON routes refuse a plain member, a stranger and a foreign company alike with 404,
//! and never name the company they hide (mirrors `channel_authorization_tests.rs`).
use super::super::authorization_test_support::{Fixture, denied};
use super::*;
use axum::http::StatusCode;

fn use_cases(f: &Fixture) -> Arc<DepartmentUseCases> {
    Arc::new(DepartmentUseCases::new(
        f.persistence.clone(),
        f.persistence.clone(),
    ))
}

fn create_payload(name: &str) -> DepartmentCreatePayload {
    DepartmentCreatePayload {
        name: name.into(),
        slug: None,
        description: None,
        charter: None,
        parent_id: None,
        lead: None,
    }
}

#[tokio::test]
async fn department_routes_hide_foreign_companies_and_refuse_members_and_strangers() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let departments = use_cases(&f);
    let created = departments
        .create_department(
            f.owner.id,
            f.company.id,
            DepartmentWrite {
                name: "Routes".into(),
                slug: "routes".into(),
                ..DepartmentWrite::default()
            },
            None,
        )
        .await
        .unwrap();
    let department = created.id.as_uuid();
    let principal = Uuid::new_v4();
    let cases = [
        (&f.owner, f.foreign.id),
        (&f.member, f.company.id),
        (&f.stranger, f.company.id),
    ];
    for (user, company) in cases {
        let auth = || Fixture::auth(user);
        let state = || State(departments.clone());
        denied(
            create_department_json(state(), auth(), Path(company), Json(create_payload("New")))
                .await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
        denied(
            update_department_json(
                state(),
                auth(),
                Path((company, department)),
                Json(DepartmentUpdatePayload {
                    name: Some("Renamed".into()),
                    ..DepartmentUpdatePayload::default()
                }),
            )
            .await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
        denied(
            delete_department_json(state(), auth(), Path((company, department))).await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
        denied(
            add_member_json(state(), auth(), Path((company, department, principal))).await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
        denied(
            remove_member_json(state(), auth(), Path((company, department, principal))).await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
        denied(
            assign_role_json(
                state(),
                auth(),
                Path((company, department, "lead".into(), principal)),
            )
            .await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
        denied(
            clear_role_json(
                state(),
                auth(),
                Path((company, department, "lead".into(), principal)),
            )
            .await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
        denied(
            get_department_json(state(), auth(), Path((company, department))).await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
    }
    for (user, company) in [(&f.owner, f.foreign.id), (&f.stranger, f.company.id)] {
        denied(
            list_departments_json(
                State(departments.clone()),
                Fixture::auth(user),
                Path(company),
            )
            .await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
    }
    // A failing company lookup is a server error, never read as "not yours".
    let failing = Arc::new(DepartmentUseCases::new(
        Arc::new(crate::use_cases::company_test_support::MockCompanyPersistence::failing()),
        f.persistence.clone(),
    ));
    denied(
        create_department_json(
            State(failing.clone()),
            Fixture::auth(&f.owner),
            Path(f.company.id),
            Json(create_payload("Down")),
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR,
        &f.foreign.name,
    )
    .await;
    denied(
        list_departments_json(State(failing), Fixture::auth(&f.owner), Path(f.company.id)).await,
        StatusCode::INTERNAL_SERVER_ERROR,
        &f.foreign.name,
    )
    .await;
    // The member can list, and sees none of the departments they are not in.
    let listed = list_departments_json(
        State(departments.clone()),
        Fixture::auth(&f.member),
        Path(f.company.id),
    )
    .await
    .unwrap()
    .into_response();
    assert_eq!(listed.status(), StatusCode::OK);
    let body = axum::body::to_bytes(listed.into_body(), 65536)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), b"[]");
    // And the department survived every refused write.
    assert_eq!(
        departments
            .visible_department(f.owner.id, f.company.id, created.id)
            .await
            .unwrap()
            .department
            .name,
        "Routes"
    );
}

#[tokio::test]
async fn department_routes_round_trip_and_refuse_an_unknown_role() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let departments = use_cases(&f);
    let owner = || Fixture::auth(&f.owner);
    let state = || State(departments.clone());
    let response = create_department_json(
        state(),
        owner(),
        Path(f.company.id),
        Json(DepartmentCreatePayload {
            charter: Some("Keep the lights on.".into()),
            ..create_payload("Platform Team")
        }),
    )
    .await
    .unwrap()
    .into_response();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(created["department"]["slug"], "platform-team");
    let department: Uuid = serde_json::from_value(created["department"]["id"].clone()).unwrap();

    let cleared = update_department_json(
        state(),
        owner(),
        Path((f.company.id, department)),
        Json(serde_json::from_str::<DepartmentUpdatePayload>(r#"{"charter": null}"#).unwrap()),
    )
    .await
    .unwrap()
    .into_response();
    let body = axum::body::to_bytes(cleared.into_body(), 65536)
        .await
        .unwrap();
    let cleared: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(cleared["department"]["charter"], serde_json::Value::Null);
    assert_eq!(
        cleared["department"]["name"], "Platform Team",
        "absent stays"
    );

    let renamed = update_department_json(
        state(),
        owner(),
        Path((f.company.id, department)),
        Json(serde_json::from_str::<DepartmentUpdatePayload>(r#"{"name": "Platform"}"#).unwrap()),
    )
    .await
    .unwrap()
    .into_response();
    let body = axum::body::to_bytes(renamed.into_body(), 65536)
        .await
        .unwrap();
    let renamed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(renamed["department"]["name"], "Platform");
    assert_eq!(
        renamed["department"]["slug"], "platform-team",
        "the address stays"
    );

    let unknown = assign_role_json(
        state(),
        owner(),
        Path((f.company.id, department, "boss".into(), Uuid::new_v4())),
    )
    .await;
    denied(unknown, StatusCode::BAD_REQUEST, &f.foreign.name).await;

    let detail = get_department_json(state(), owner(), Path((f.company.id, department)))
        .await
        .unwrap()
        .into_response();
    let body = axum::body::to_bytes(detail.into_body(), 65536)
        .await
        .unwrap();
    let detail: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail["id"], serde_json::json!(department));
    assert_eq!(detail["members"], serde_json::json!([]));
    assert_eq!(detail["roles"], serde_json::json!([]));
}
