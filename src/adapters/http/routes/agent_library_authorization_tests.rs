use super::super::authorization_test_support::{Fixture, denied};
use super::*;

#[tokio::test]
async fn nonoperators_get_not_found_before_library_form_or_avatar_validation() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let payload = || serde_json::json!({"name":"Draft", "slug":"draft", "native_permissions":"full", "avatar_url":"javascript:alert(1)"});
    let caller = || Caller::user(f.owner.id);
    let skills = Arc::new(SkillUseCases::new(
        f.persistence.clone(),
        f.persistence.clone(),
        f.persistence.clone(),
    ));
    denied(
        create_json(
            State(f.agents.clone()),
            State(skills),
            caller(),
            Json(serde_json::from_value(payload()).unwrap()),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.foreign.name,
    )
    .await;
    denied(
        forms::create(
            State(f.agents.clone()),
            caller(),
            Form(serde_json::from_value(payload()).unwrap()),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.foreign.name,
    )
    .await;
    denied(
        forms::update(
            State(f.agents.clone()),
            caller(),
            Path(Uuid::new_v4()),
            Form(serde_json::from_value(payload()).unwrap()),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.foreign.name,
    )
    .await;
}
