//! Real JSON handlers over scoped management and unchanged conversation visibility.
use super::*;
use crate::adapters::http::routes::schedule::{self, ScheduleRequest};
use crate::adapters::persistence::workflow::schedule_test_support::{
    bind, publish, schedule_use_cases, source,
};
use crate::application::ports::department::DepartmentWrite;
use crate::application::workflow::{RelatedAssociation, RelatedChannelId};
use crate::entities::company_member::CompanyAccessRole;
use crate::entities::schedule::{ScheduleTimezone, ScheduleType};
use crate::entities::user::User;
use crate::use_cases::user::UserPersistence;
use serde_json::{Value, json};

struct Scoped {
    f: Fixture,
    remote: DepartmentId,
    sibling: User,
}

impl Scoped {
    async fn new() -> Option<Self> {
        let f = Fixture::new().await?;
        let remote = crate::adapters::persistence::test_support::delegated_unit(
            f.persistence.as_ref(),
            crate::domain::workflow::CompanyId::new(f.company.id),
            crate::application::workflow::WorkflowActor::authenticated(f.owner.id).unwrap(),
            crate::application::workflow::WorkflowActor::authenticated(f.member.id).unwrap(),
        )
        .await;
        let cases = f.departments();
        let suffix = Uuid::new_v4().simple().to_string();
        let sibling = f
            .persistence
            .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
            .await
            .unwrap();
        let principal = crate::adapters::persistence::test_support::insert_root_member(
            f.persistence.pool(),
            crate::domain::workflow::CompanyId::new(f.company.id),
            sibling.id,
            CompanyAccessRole::Member,
            crate::entities::access_level::AccessLevel::PUBLIC,
        )
        .await;
        cases
            .create_department(
                f.owner.id,
                f.company.id,
                unit("Support", None),
                Some(principal),
            )
            .await
            .unwrap();
        Some(Self { f, remote, sibling })
    }

    async fn create(&self, name: &str) -> Uuid {
        let response = create_channel_json(
            State(self.f.channels.clone()),
            Fixture::auth(&self.f.member),
            Path(self.f.company.id),
            Json(payload(self.remote, name)),
        )
        .await
        .unwrap()
        .into_response();
        assert_eq!(response.status(), StatusCode::CREATED);
        let value = body(response).await;
        assert_eq!(value["success"], true);
        assert_eq!(value["channel"]["department_id"], self.remote.to_string());
        Uuid::parse_str(value["channel"]["id"].as_str().unwrap()).unwrap()
    }
}

fn unit(name: &str, parent_id: Option<DepartmentId>) -> DepartmentWrite {
    DepartmentWrite {
        name: name.into(),
        slug: name.to_lowercase(),
        parent_id,
        ..Default::default()
    }
}

fn payload(unit: DepartmentId, name: &str) -> ChannelJsonPayload {
    serde_json::from_value(json!({"name":name,"department_id":unit})).unwrap()
}

async fn body(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn delegated_admin_desk_json_routes_allow_the_unit_lead_and_hide_siblings_and_foreign_ids() {
    let Some(s) = Scoped::new().await else {
        return;
    };
    let f = &s.f;
    let desk = s.create("Delegated intake").await;
    let path = (f.company.id, desk);
    assert!(
        f.channels
            .get_readable_channel(&Fixture::viewer(&f.member), f.company.id, desk)
            .await
            .unwrap()
            .is_none()
    );
    let read = get_channel_json(
        State(f.channels.clone()),
        Fixture::auth(&f.member),
        Path(path),
    )
    .await
    .unwrap()
    .into_response();
    assert_eq!(read.status(), StatusCode::OK);
    assert_eq!(body(read).await["name"], "Delegated intake");
    let updated = update_channel_json(
        State(f.channels.clone()),
        Fixture::auth(&f.member),
        Path(path),
        Json(payload(s.remote, "Updated intake")),
    )
    .await
    .unwrap()
    .into_response();
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(body(updated).await["channel"]["name"], "Updated intake");
    assert_desk_denials(&s, desk).await;
    assert_eq!(
        delete_channel_json(
            State(f.channels.clone()),
            Fixture::auth(&f.member),
            Path(path)
        )
        .await
        .unwrap()
        .into_response()
        .status(),
        StatusCode::NO_CONTENT
    );
}

async fn assert_desk_denials(s: &Scoped, desk: Uuid) {
    let f = &s.f;
    let path = (f.company.id, desk);
    for user in [&s.sibling, &f.stranger] {
        denied(
            get_channel_json(State(f.channels.clone()), Fixture::auth(user), Path(path)).await,
            StatusCode::NOT_FOUND,
            "Updated intake",
        )
        .await;
        denied(
            update_channel_json(
                State(f.channels.clone()),
                Fixture::auth(user),
                Path(path),
                Json(payload(s.remote, "Stolen")),
            )
            .await,
            StatusCode::NOT_FOUND,
            "Updated intake",
        )
        .await;
        denied(
            delete_channel_json(State(f.channels.clone()), Fixture::auth(user), Path(path)).await,
            StatusCode::NOT_FOUND,
            "Updated intake",
        )
        .await;
    }
    for path in [(f.company.id, f.foreign_channel), (f.foreign.id, desk)] {
        denied(
            get_channel_json(
                State(f.channels.clone()),
                Fixture::auth(&f.member),
                Path(path),
            )
            .await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
    }
}

fn schedule_request(binding: Uuid, name: &str) -> ScheduleRequest {
    ScheduleRequest {
        name: name.into(),
        schedule_type: ScheduleType::Interval,
        interval_seconds: Some(3600),
        scheduled_at: None,
        timezone: ScheduleTimezone::utc(),
        binding_id: binding,
        enabled: true,
    }
}

#[tokio::test]
async fn delegated_admin_schedule_json_routes_scope_configuration_to_the_desk_unit() {
    let Some(s) = Scoped::new().await else {
        return;
    };
    let f = &s.f;
    let desk = s.create("Schedule intake").await;
    let p = f.persistence.as_ref();
    let version = publish(p, f.company.id, f.owner.id, source(None)).await;
    let binding = bind(
        p,
        f.company.id,
        f.owner.id,
        version,
        RelatedAssociation::Channel(RelatedChannelId::new(desk)),
        None,
    )
    .await;
    let cases = Arc::new(schedule_use_cases(p));
    let (status, Json(created)) = schedule::create_channel_schedule_json(
        State(cases.clone()),
        Fixture::auth(&f.member),
        Path((f.company.id, desk)),
        Json(schedule_request(binding.as_uuid(), "Initial")),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::CREATED);
    let path = (f.company.id, desk, created.id);
    let Json(updated) = schedule::update_schedule_json(
        State(cases.clone()),
        Fixture::auth(&f.member),
        Path(path),
        Json(schedule_request(binding.as_uuid(), "Edited")),
    )
    .await
    .unwrap();
    assert_eq!(updated.name, "Edited");
    let Json(list) = schedule::list_channel_schedules_json(
        State(cases.clone()),
        Fixture::auth(&f.member),
        Path((f.company.id, desk)),
    )
    .await
    .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, created.id);
    assert_schedule_denials(&s, &cases, path, binding.as_uuid()).await;
    assert_eq!(
        schedule::delete_schedule_json(State(cases), Fixture::auth(&f.member), Path(path))
            .await
            .unwrap(),
        StatusCode::NO_CONTENT
    );
}

async fn assert_schedule_denials(
    s: &Scoped,
    cases: &Arc<crate::application::use_cases::schedule::ScheduleUseCases>,
    path: (Uuid, Uuid, Uuid),
    binding: Uuid,
) {
    let f = &s.f;
    let (_, desk, id) = path;
    for user in [&s.sibling, &f.stranger] {
        denied(
            schedule::get_schedule_json(State(cases.clone()), Fixture::auth(user), Path(path))
                .await,
            StatusCode::NOT_FOUND,
            "Edited",
        )
        .await;
        denied(
            schedule::update_schedule_json(
                State(cases.clone()),
                Fixture::auth(user),
                Path(path),
                Json(schedule_request(binding, "Stolen")),
            )
            .await,
            StatusCode::NOT_FOUND,
            "Edited",
        )
        .await;
        denied(
            schedule::delete_schedule_json(State(cases.clone()), Fixture::auth(user), Path(path))
                .await,
            StatusCode::NOT_FOUND,
            "Edited",
        )
        .await;
    }
    denied(
        schedule::get_schedule_json(
            State(cases.clone()),
            Fixture::auth(&f.member),
            Path((f.foreign.id, desk, id)),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.foreign.name,
    )
    .await;
}
