//! Department JSON routes refuse a plain member, a stranger and a foreign company alike with 404,
//! and never name the company they hide (mirrors `channel_authorization_tests.rs`).
use super::super::authorization_test_support::{Fixture, denied};
use super::*;
use axum::http::StatusCode;

fn use_cases(f: &Fixture) -> Arc<DepartmentUseCases> {
    Arc::new(DepartmentUseCases::new(
        f.persistence.clone(),
        Arc::new(
            crate::adapters::persistence::department::PostgresDepartments::new(
                f.persistence.as_ref().clone(),
                crate::adapters::persistence::test_support::provisioning_templates(),
            ),
        ),
    ))
}

fn create_payload(name: &str) -> DepartmentCreatePayload {
    DepartmentCreatePayload {
        kind: Default::default(),
        sponsors: Vec::new(),
        target_date: None,
        escalation: None,
        name: name.into(),
        slug: None,
        description: None,
        charter: None,
        parent_id: None,
        lead: None,
    }
}

#[path = "root_department_route_tests.rs"]
mod root;

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
                Json(RoleAssignmentPayload { decider: false }),
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
                axum::extract::Query(RoleRemovalQuery { successor: None }),
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
                Query(ListQuery::default()),
            )
            .await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
    }
    assert_department_lookup_errors(&f).await;
    // The member can list, and sees none of the departments they are not in.
    let listed = list_departments_json(
        State(departments.clone()),
        Fixture::auth(&f.member),
        Path(f.company.id),
        Query(ListQuery::default()),
    )
    .await
    .unwrap()
    .into_response();
    assert_eq!(listed.status(), StatusCode::OK);
    let body = axum::body::to_bytes(listed.into_body(), 65536)
        .await
        .unwrap();
    let listed: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["is_root"], true);
    assert_eq!(listed[0]["company_id"], serde_json::json!(f.company.id));
    assert_eq!(listed[0]["slug"], serde_json::json!(f.company.slug));
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
        Json(RoleAssignmentPayload { decider: false }),
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

// Both current unit authority and the company read port propagate infrastructure errors.
async fn assert_department_lookup_errors(f: &Fixture) {
    let unavailable = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://localhost/unavailable")
        .unwrap();
    unavailable.close().await;
    let unavailable = crate::adapters::persistence::PostgresPersistence::new(unavailable);
    let failing = Arc::new(DepartmentUseCases::new(
        Arc::new(crate::use_cases::company_test_support::MockCompanyPersistence::failing()),
        Arc::new(
            crate::adapters::persistence::department::PostgresDepartments::new(
                unavailable,
                crate::adapters::persistence::test_support::provisioning_templates(),
            ),
        ),
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
        list_departments_json(
            State(failing),
            Fixture::auth(&f.owner),
            Path(f.company.id),
            Query(ListQuery::default()),
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR,
        &f.foreign.name,
    )
    .await;
}

#[tokio::test]
async fn department_sponsor_json_carries_decider_and_round_trips_escalation_patch() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let departments = use_cases(&f);
    let created = departments
        .create_department(
            f.owner.id,
            f.company.id,
            crate::application::ports::department::DepartmentWrite {
                name: "API sponsors".into(),
                slug: "api-sponsors".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    let person: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(f.company.id)
            .bind(f.owner.id)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    assign_role_json(
        State(departments.clone()),
        Fixture::auth(&f.owner),
        Path((f.company.id, created.id.as_uuid(), "sponsor".into(), person)),
        Json(RoleAssignmentPayload { decider: false }),
    )
    .await
    .unwrap();
    let response = get_department_json(
        State(departments.clone()),
        Fixture::auth(&f.owner),
        Path((f.company.id, created.id.as_uuid())),
    )
    .await
    .unwrap()
    .into_response();
    let bytes = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let detail: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(detail["roles"][0]["role"], "sponsor");
    assert_eq!(detail["roles"][0]["decider"], true);
    let payload: DepartmentUpdatePayload =
        serde_json::from_value(serde_json::json!({"escalation":{"notify":"sponsor","hops":2}}))
            .unwrap();
    update_department_json(
        State(departments.clone()),
        Fixture::auth(&f.owner),
        Path((f.company.id, created.id.as_uuid())),
        Json(payload),
    )
    .await
    .unwrap();
    let stored = departments
        .visible_department(f.owner.id, f.company.id, created.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(stored.department.escalation).unwrap(),
        serde_json::json!({"notify":"sponsor","hops":2})
    );
    let payload: DepartmentUpdatePayload =
        serde_json::from_value(serde_json::json!({"description":"keep policy"})).unwrap();
    update_department_json(
        State(departments.clone()),
        Fixture::auth(&f.owner),
        Path((f.company.id, created.id.as_uuid())),
        Json(payload),
    )
    .await
    .unwrap();
    assert!(
        departments
            .visible_department(f.owner.id, f.company.id, created.id)
            .await
            .unwrap()
            .department
            .escalation
            .is_some()
    );
    let payload: DepartmentUpdatePayload =
        serde_json::from_value(serde_json::json!({"escalation":null})).unwrap();
    update_department_json(
        State(departments.clone()),
        Fixture::auth(&f.owner),
        Path((f.company.id, created.id.as_uuid())),
        Json(payload),
    )
    .await
    .unwrap();
    assert!(
        departments
            .visible_department(f.owner.id, f.company.id, created.id)
            .await
            .unwrap()
            .department
            .escalation
            .is_none()
    );
    assert!(
        serde_json::from_value::<DepartmentUpdatePayload>(
            serde_json::json!({"escalation":{"notify":"sponsor","hops":3}})
        )
        .is_err()
    );
}

/// Plan08 03A: the JSON create takes a goal's kind, target date and sponsors (the first decides),
/// the response names them, an edit moves the date, and a standing create refuses goal fields.
#[tokio::test]
async fn goal_unit_json_create_and_edit_carry_kind_target_date_and_sponsors() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let departments = use_cases(&f);
    let person: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(f.company.id)
            .bind(f.owner.id)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    let payload: DepartmentCreatePayload = serde_json::from_value(serde_json::json!({
        "name": "Launch X", "kind": "goal", "target_date": "2027-03-31", "sponsors": [person]
    }))
    .unwrap();
    let response = create_department_json(
        State(departments.clone()),
        Fixture::auth(&f.owner),
        Path(f.company.id),
        Json(payload),
    )
    .await
    .unwrap()
    .into_response();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["department"]["kind"], "goal");
    assert_eq!(body["department"]["target_date"], "2027-03-31");
    let id = DepartmentId::new(body["department"]["id"].as_str().unwrap().parse().unwrap());
    let detail = departments
        .visible_department(f.owner.id, f.company.id, id)
        .await
        .unwrap();
    assert!(
        detail
            .roles
            .iter()
            .any(|role| role.role == DepartmentRole::Sponsor && role.decider),
        "{:?}",
        detail.roles
    );
    let payload: DepartmentUpdatePayload =
        serde_json::from_value(serde_json::json!({"target_date": "2027-06-30"})).unwrap();
    update_department_json(
        State(departments.clone()),
        Fixture::auth(&f.owner),
        Path((f.company.id, id.as_uuid())),
        Json(payload),
    )
    .await
    .unwrap();
    let stored = departments
        .visible_department(f.owner.id, f.company.id, id)
        .await
        .unwrap();
    assert_eq!(
        stored.department.target_date.unwrap().to_string(),
        "2027-06-30"
    );
    let payload: DepartmentCreatePayload = serde_json::from_value(serde_json::json!({
        "name": "Plain", "target_date": "2027-03-31"
    }))
    .unwrap();
    let refused = create_department_json(
        State(departments.clone()),
        Fixture::auth(&f.owner),
        Path(f.company.id),
        Json(payload),
    )
    .await;
    assert!(
        matches!(&refused, Err(AppError::BadRequest(message)) if message.starts_with("department.target_date_goal_only")),
        "a standing create refuses goal fields"
    );
}

/// A goal unit under `parent`, sponsored by the owner, with one desk (`members` access).
async fn goal_with_desk(
    f: &Fixture,
    departments: &DepartmentUseCases,
    name: &str,
    parent: Option<DepartmentId>,
) -> (DepartmentId, Uuid) {
    let owner: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(f.company.id)
    .bind(f.owner.id)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    let goal = departments
        .create_department(
            f.owner.id,
            f.company.id,
            DepartmentWrite {
                name: name.into(),
                slug: name.to_lowercase(),
                parent_id: parent,
                goal: Some(crate::application::ports::department::GoalWrite {
                    target_date: None,
                    sponsors: vec![crate::entities::transport::PrincipalId::new(owner)],
                }),
                ..DepartmentWrite::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    let desk = crate::adapters::persistence::test_support::create_channel(
        f.persistence.as_ref(),
        f.company.id,
        crate::application::ports::channel::ChannelWrite {
            name: format!("{name} desk"),
            slug: format!("{}-desk", name.to_lowercase()),
            department_id: Some(goal),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    (goal, desk)
}

async fn rollup_status(
    departments: &Arc<DepartmentUseCases>,
    user: &crate::entities::user::User,
    company: Uuid,
    goal: DepartmentId,
) -> AppResult<serde_json::Value> {
    let response = goal_rollup_json(
        State(departments.clone()),
        Fixture::auth(user),
        Path((company, goal.as_uuid())),
    )
    .await?
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    Ok(serde_json::from_slice(&bytes).unwrap())
}

fn goal_not_found(answer: &AppResult<serde_json::Value>) -> bool {
    matches!(answer, Err(AppError::NotFound(message)) if message == "Goal not found")
}

/// 03D.5: a member who may view one of the goal's desks reads its rollup — counts, states, unit
/// names and spend, no run content — and the goals list names it; once the desk is no longer
/// visible to them, the goal is not found and not listed. Runs themselves stay behind the run
/// board's own rule (`other_tenants_and_plain_members_reach_no_run…`).
#[tokio::test]
async fn goal_authorization_member_sees_counts_not_runs() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let departments = use_cases(&f);
    let (goal, desk) = goal_with_desk(&f, &departments, "Launch", None).await;
    assert!(goal_not_found(
        &rollup_status(&departments, &f.member, f.company.id, goal).await
    ));
    sqlx::query("UPDATE channels SET access_mode='public' WHERE id=$1")
        .bind(desk)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    let body = rollup_status(&departments, &f.member, f.company.id, goal)
        .await
        .unwrap();
    assert_eq!(body["name"], "Launch");
    let mut keys: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "archived",
            "calls",
            "escalations_open",
            "first_run_at",
            "goal",
            "last_activity_at",
            "last_finished_at",
            "name",
            "runs_counted",
            "spend",
            "target_date",
            "truncated",
            "units",
            "waiting",
            "waiting_on_people_seconds",
            "waiting_total",
        ],
        "counts, states, names and times only"
    );
    let listed = departments
        .visible_goals(f.member.id, f.company.id, false)
        .await
        .unwrap();
    assert!(!listed.cut);
    assert_eq!(
        listed
            .goals
            .iter()
            .map(|goal| goal.name.as_str())
            .collect::<Vec<_>>(),
        ["Launch"]
    );
    sqlx::query("UPDATE channels SET access_mode='members' WHERE id=$1")
        .bind(desk)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    assert!(goal_not_found(
        &rollup_status(&departments, &f.member, f.company.id, goal).await
    ));
    assert!(
        departments
            .visible_goals(f.member.id, f.company.id, false)
            .await
            .unwrap()
            .goals
            .is_empty()
    );
}

/// 03D.5: a stranger, another company's owner and a member who neither sees a desk nor manages
/// the goal get "not found" alike; a standing unit is no goal; the lead of the goal's parent sees
/// it as its manager.
#[tokio::test]
async fn goal_authorization_stranger_and_manager() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let departments = use_cases(&f);
    let parent = departments
        .create_department(
            f.owner.id,
            f.company.id,
            DepartmentWrite {
                name: "Programs".into(),
                slug: "programs".into(),
                ..DepartmentWrite::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    let (goal, _) = goal_with_desk(&f, &departments, "Launch", Some(parent)).await;
    rollup_status(&departments, &f.owner, f.company.id, goal)
        .await
        .expect("the owner manages every unit");
    for (user, company) in [
        (&f.stranger, f.company.id),
        (&f.owner, f.foreign.id),
        (&f.member, f.company.id),
    ] {
        let answer = rollup_status(&departments, user, company, goal).await;
        assert!(
            matches!(&answer, Err(AppError::NotFound(_))),
            "{}: {answer:?}",
            user.username
        );
        if let Err(AppError::NotFound(message)) = &answer {
            assert!(!message.contains(&f.foreign.name));
        }
    }
    assert!(goal_not_found(
        &rollup_status(&departments, &f.owner, f.company.id, parent).await
    ));
    let member: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(f.company.id)
    .bind(f.member.id)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO department_roles (company_id, department_id, role, principal_id) \
         VALUES ($1, $2, 'lead', $3)",
    )
    .bind(f.company.id)
    .bind(parent.as_uuid())
    .bind(member)
    .execute(f.persistence.pool())
    .await
    .unwrap();
    let body = rollup_status(&departments, &f.member, f.company.id, goal)
        .await
        .expect("the parent's lead manages the goal");
    assert_eq!(body["goal"], goal.as_uuid().to_string());
}
