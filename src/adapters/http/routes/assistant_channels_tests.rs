use super::*;
use crate::adapters::persistence::workflow::provisioning::{
    AssistantChannelService, ProvisioningDeployment,
};
use crate::adapters::persistence::{PostgresPersistence, test_support::test_pool};
use crate::adapters::workflow_source::WorkflowSourceDecoder;
use crate::application::workflow::templates::TemplateCatalogue;
use crate::use_cases::{
    agent::{AgentPersistence, AgentWrite},
    company::{CompanyModelConnectionWrite, CompanyPersistence, CompanyWrite},
    user::UserPersistence,
};
use serde_json::json;

struct Fixture {
    persistence: PostgresPersistence,
    channels: Arc<dyn AssistantChannels>,
    owner: Uuid,
    company: Uuid,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let persistence = PostgresPersistence::new(test_pool().await?);
        let suffix = Uuid::new_v4().simple().to_string();
        let owner = persistence
            .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
            .await
            .unwrap()
            .id;
        let company = CompanyPersistence::create(
            &persistence,
            owner,
            CompanyWrite {
                name: "Assistants".into(),
                slug: suffix,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        CompanyPersistence::replace_model_connections_for_user(
            &persistence,
            owner,
            company,
            vec![CompanyModelConnectionWrite {
                provider: "openai".into(),
                api_key: Some("test-key".into()),
                models: vec!["gpt-4o-mini".into()],
                is_default: true,
            }],
        )
        .await
        .unwrap();
        for (slug, model) in [("support", Some("gpt-4o-mini")), ("inherits", None)] {
            AgentPersistence::create(
                &persistence,
                company,
                AgentWrite {
                    name: slug.into(),
                    slug: slug.into(),
                    provider: model.map(|_| "openai".into()),
                    model: model.map(Into::into),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        let templates = ProvisioningDeployment::new(
            Arc::new(TemplateCatalogue::shipped(&WorkflowSourceDecoder).unwrap()),
            &[],
            "agents.test",
        );
        Some(Self {
            channels: Arc::new(AssistantChannelService::new(persistence.clone(), templates)),
            persistence,
            owner,
            company,
        })
    }

    async fn post(&self, user: Uuid, body: Value) -> (StatusCode, Value) {
        let response = match create_assistant_channel_json(
            State(self.channels.clone()),
            AuthenticatedUser { id: user },
            Path(self.company),
            Json(serde_json::from_value(body).unwrap()),
        )
        .await
        {
            Ok(created) => created.into_response(),
            Err(error) => error.into_response(),
        };
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

fn body(key: &str, agent: &str, channel: &str) -> Value {
    json!({"agent_slug": agent, "channel_slug": channel, "channel_name": "Desk",
        "reply_mode": "autonomous", "command_key": key})
}

/// D2.4, F049.1: created is 201 and a retry with the key replays it; a changed request under the
/// key and a taken address are 409, invalid input is 400, and a member is 404.
#[tokio::test]
async fn assistant_channel_route_maps_results_and_refusals() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (status, created) = f.post(f.owner, body("k1", "support", "desk")).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["channel"]["slug"], "desk");
    let company: String = sqlx::query_scalar("SELECT slug::text FROM companies WHERE id = $1")
        .bind(f.company)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(
        created["channel"]["address"],
        format!("desk@{company}.agents.test"),
        "F051: the address a member can write to"
    );
    assert_eq!(
        f.post(f.owner, body("k1", "support", "desk")).await,
        (StatusCode::CREATED, created)
    );
    assert_eq!(
        f.post(f.owner, body("k1", "support", "elsewhere")).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.post(f.owner, body("k2", "support", "desk")).await.0,
        StatusCode::CONFLICT,
        "the address is taken"
    );
    assert_eq!(
        f.post(f.owner, body("k3", "inherits", "inherits-desk"))
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "a workflow needs the agent's explicit model"
    );
    let member = f
        .persistence
        .create_user(
            &Uuid::new_v4().simple().to_string(),
            "member@example.test",
            "hash",
        )
        .await
        .unwrap()
        .id;
    sqlx::query(
        "INSERT INTO company_members(id, company_id, user_id, role) VALUES ($1, $2, $3, 'member')",
    )
    .bind(Uuid::new_v4())
    .bind(f.company)
    .bind(member)
    .execute(f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(
        f.post(member, body("k4", "support", "member-desk")).await.0,
        StatusCode::NOT_FOUND
    );
    assert!(
        serde_json::from_value::<AssistantChannelBody>(json!({
            "agent_slug": "support", "channel_slug": "x", "channel_name": "X",
            "reply_mode": "public", "command_key": "k5"
        }))
        .is_err()
    );
    CompanyPersistence::delete(&f.persistence, f.company)
        .await
        .unwrap();
}
