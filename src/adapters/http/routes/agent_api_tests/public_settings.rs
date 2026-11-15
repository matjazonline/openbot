use super::*;
use axum::{body::Body, extract::FromRequest, http::Request};

#[tokio::test]
async fn direct_public_settings_json_round_trips_and_preserves_desk_policy() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let created = f.create(json!({"name":"Private", "slug":"private"})).await;
    let id: Uuid = serde_json::from_value(created["agent"]["id"].clone()).unwrap();
    assert_eq!(created["agent"]["open_to_public"], false);
    let before: Vec<(Uuid, String, i64)> = sqlx::query_as(
        "SELECT id, access_mode, settings_revision FROM channels WHERE company_id=$1 ORDER BY id",
    )
    .bind(f.company)
    .fetch_all(f.persistence.pool())
    .await
    .unwrap();

    let error = f
        .update(id, json!({"open_to_public":true}))
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::BadRequest(ref reason) if reason.contains("confirm_spam_disabled"))
    );
    let unchanged = f
        .agents
        .get_company_agent(f.user, f.company, id)
        .await
        .unwrap()
        .unwrap();
    assert!(!unchanged.open_to_public);
    assert_eq!(
        serde_json::to_value(unchanged.draft_revision).unwrap(),
        created["agent"]["draft_revision"]
    );

    let opened = f
        .update(
            id,
            json!({"open_to_public":true,"confirm_spam_disabled":true}),
        )
        .await
        .unwrap();
    assert_eq!(opened["agent"]["open_to_public"], true);
    let stored = f
        .agents
        .get_company_agent(f.user, f.company, id)
        .await
        .unwrap()
        .unwrap();
    assert!(stored.open_to_public);
    let open_revision: i64 = sqlx::query_scalar(
        "SELECT settings_revision FROM principals WHERE company_id=$1 AND agent_id=$2",
    )
    .bind(f.company)
    .bind(id)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    let closed = f.update(id, json!({"open_to_public":false})).await.unwrap();
    assert_eq!(closed["agent"]["open_to_public"], false);
    let closed_revision: i64 = sqlx::query_scalar(
        "SELECT settings_revision FROM principals WHERE company_id=$1 AND agent_id=$2",
    )
    .bind(f.company)
    .bind(id)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(closed_revision, open_revision + 1);
    let after: Vec<(Uuid, String, i64)> = sqlx::query_as(
        "SELECT id, access_mode, settings_revision FROM channels WHERE company_id=$1 ORDER BY id",
    )
    .bind(f.company)
    .fetch_all(f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(
        after, before,
        "opening/revoking direct mail does not open or reconfigure a desk"
    );
    f.cleanup().await;
}

#[tokio::test]
async fn direct_public_settings_unconfirmed_create_has_no_partial_agent_or_desk() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let result = create_agent_json(
        State(f.agents.clone()),
        State(crate::adapters::persistence::test_support::department_use_cases(&f.persistence)),
        AuthenticatedUser { id: f.user },
        Path(f.company),
        Json(json!({"name":"Public", "slug":"public", "open_to_public":true})),
    )
    .await;
    assert!(
        matches!(result, Err(AppError::BadRequest(ref reason)) if reason.contains("confirm_spam_disabled"))
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM agents WHERE company_id=$1")
        .bind(f.company)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM channels WHERE company_id=$1")
        .bind(f.company)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
    let saved = f
        .create(
            json!({"name":"Public", "slug":"public", "open_to_public":true,
        "confirm_spam_disabled":true}),
        )
        .await;
    assert_eq!(saved["agent"]["open_to_public"], true);
    f.cleanup().await;
}

#[tokio::test]
async fn direct_public_settings_scanning_available_does_not_require_acknowledgement() {
    let Some(mut f) = Fixture::new().await else {
        return;
    };
    f.agents = Arc::new(
        AgentUseCases::new(
            f.persistence.clone(),
            f.persistence.clone(),
            f.persistence.clone(),
            crate::use_cases::agent::SpamScanning::Available,
        )
        .with_company_agents(crate::adapters::persistence::test_support::company_agents(
            &f.persistence,
        )),
    );
    let saved = f
        .create(json!({"name":"Scanned", "slug":"scanned", "open_to_public":true}))
        .await;
    assert_eq!(saved["agent"]["open_to_public"], true);
    f.cleanup().await;
}

async fn submit_form(f: &Fixture, id: Uuid, public: bool) {
    let stored = f
        .agents
        .get_company_agent(f.user, f.company, id)
        .await
        .unwrap()
        .unwrap();
    let fields = if public {
        "&open_to_public=true&confirm_spam_disabled=true"
    } else {
        ""
    };
    let request = Request::builder()
        .method("PUT")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "name=Form&slug=form&expected_revision={}{fields}",
            stored.draft_revision.get()
        )))
        .unwrap();
    let Form(form) = Form::<AgentForm>::from_request(request, &()).await.unwrap();
    let Html(html) = update_agent_handler(
        State(Arc::new(CompanyUseCases::new(f.persistence.clone()))),
        State(f.agents.clone()),
        State(f.skills.clone()),
        AuthenticatedUser { id: f.user },
        Path((f.company, id)),
        Form(form),
    )
    .await
    .unwrap();
    assert!(!html.contains("Update failed:"), "{html}");
}

#[tokio::test]
async fn direct_public_settings_form_saves_and_renders_the_current_principal_flag() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let created = f.create(json!({"name":"Form", "slug":"form"})).await;
    let id: Uuid = serde_json::from_value(created["agent"]["id"].clone()).unwrap();
    for public in [true, false] {
        submit_form(&f, id, public).await;
        let stored = f
            .agents
            .get_company_agent(f.user, f.company, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.open_to_public, public);
        let Html(form) = edit_agent_form(
            State(Arc::new(CompanyUseCases::new(f.persistence.clone()))),
            State(f.agents.clone()),
            AuthenticatedUser { id: f.user },
            Path((f.company, id)),
        )
        .await
        .unwrap();
        assert!(form.contains("Allow external messages to this agent's direct address"));
        let checkbox = form
            .split("name=\"open_to_public\"")
            .nth(1)
            .unwrap()
            .split('>')
            .next()
            .unwrap();
        assert_eq!(checkbox.contains(" checked"), public);
    }
    f.cleanup().await;
}

async fn fixed_revision_write(
    f: &Fixture,
    id: Uuid,
    expected: crate::entities::agent_capability::AgentDraftRevision,
    public: bool,
) -> AppResult<Value> {
    let result = update_agent_json(
        State(f.agents.clone()),State(f.skills.clone()),
        State(crate::adapters::persistence::test_support::department_use_cases(&f.persistence)),
        AuthenticatedUser{id:f.user},Path((f.company,id)),
        Json(serde_json::from_value(json!({"expected_revision":expected,
            "agent":{"name":"Contended","slug":"contended","open_to_public":public,"confirm_spam_disabled":true}})).unwrap()),
    ).await?;
    Ok(response_json(result).await)
}

#[tokio::test]
async fn direct_public_settings_competing_draft_writers_change_policy_once() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let saved = f
        .create(json!({"name":"Contended","slug":"contended"}))
        .await;
    let id: Uuid = serde_json::from_value(saved["agent"]["id"].clone()).unwrap();
    let current = f
        .agents
        .get_company_agent(f.user, f.company, id)
        .await
        .unwrap()
        .unwrap();
    let (opened, closed) = tokio::join!(
        fixed_revision_write(&f, id, current.draft_revision, true),
        fixed_revision_write(&f, id, current.draft_revision, false),
    );
    let expected_public = match (opened, closed) {
        (Ok(_), Err(AppError::Conflict(_))) => true,
        (Err(AppError::Conflict(_)), Ok(_)) => false,
        other => panic!("one fenced policy writer must win: {other:?}"),
    };
    let stored = f
        .agents
        .get_company_agent(f.user, f.company, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.open_to_public, expected_public);
    assert_eq!(
        stored.draft_revision.get(),
        current.draft_revision.get() + 1
    );
    f.cleanup().await;
}
