use super::*;
use crate::adapters::{
    persistence::{PostgresPersistence, test_support::test_pool},
    response_schema::JsonResponseValidator,
};
use crate::use_cases::{
    agent::{AgentPersistence, SpamScanning},
    company::{CompanyPersistence, CompanyWrite},
    user::UserPersistence,
};
use serde_json::{Value, json};

async fn response_json(response: impl IntoResponse) -> Value {
    let response = response.into_response();
    assert!(response.status().is_success());
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

struct Fixture {
    persistence: Arc<PostgresPersistence>,
    agents: Arc<AgentUseCases>,
    skills: Arc<SkillUseCases>,
    user: Uuid,
    company: Uuid,
}
impl Fixture {
    async fn new() -> Option<Self> {
        let persistence = Arc::new(PostgresPersistence::new(test_pool().await?));
        let suffix = Uuid::new_v4().simple().to_string();
        let user = persistence
            .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
            .await
            .unwrap()
            .id;
        let company = CompanyPersistence::create(
            persistence.as_ref(),
            user,
            CompanyWrite {
                name: "Runtime API".into(),
                slug: suffix,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let skills = Arc::new(SkillUseCases::new(
            persistence.clone(),
            persistence.clone(),
            persistence.clone(),
        ));
        let agents = Arc::new(
            AgentUseCases::new(
                persistence.clone(),
                persistence.clone(),
                persistence.clone(),
                SpamScanning::Unavailable,
            )
            .with_response_validator(Arc::new(JsonResponseValidator)),
        );
        Some(Self {
            persistence,
            agents,
            skills,
            user,
            company,
        })
    }
    async fn create(&self, body: Value) -> Value {
        response_json(
            create_agent_json(
                State(self.agents.clone()),
                AuthenticatedUser { id: self.user },
                Path(self.company),
                Json(serde_json::from_value(body).unwrap()),
            )
            .await
            .unwrap(),
        )
        .await
    }
    async fn update(&self, id: Uuid, patch: Value) -> AppResult<Value> {
        let mut body =
            json!({"name":"Structured", "slug":"structured", "description":"Updated description"});
        body.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        Ok(response_json(
            update_agent_json(
                State(self.agents.clone()),
                State(self.skills.clone()),
                AuthenticatedUser { id: self.user },
                Path((self.company, id)),
                Json(serde_json::from_value(body).unwrap()),
            )
            .await?,
        )
        .await)
    }
    async fn cleanup(&self) {
        CompanyPersistence::delete(self.persistence.as_ref(), self.company)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn api_config_contracts_and_unrelated_edits_round_trip() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let saved = f
        .create(json!({"name":"Default", "slug":"default-agent"}))
        .await;
    for retired in ["harness_kind", "granted_tool_ids", "native_tool_policy"] {
        assert!(saved["agent"].get(retired).is_none(), "{retired}");
    }
    let saved = f
        .create(json!({"name":"Structured", "slug":"structured",
            "config_json":{"version":1,"max_turns":3},
            "response_contract":{"version":1,"format":"json_schema","schema":{"type":"object"}}}))
        .await;
    let id = serde_json::from_value(saved["agent"]["id"].clone()).unwrap();
    let edited = f.update(id, json!({})).await.unwrap();
    for field in ["config_json", "response_contract"] {
        assert_eq!(edited["agent"][field], saved["agent"][field], "{field}");
    }
    let defaults = f
        .update(id, json!({"config_json":{"version":1}}))
        .await
        .unwrap();
    assert!(defaults["agent"]["config_json"].is_null());
    assert!(f.update(id,json!({"response_contract":{"version":1,"format":"json_schema","schema":{"$ref":"https://example.test/schema"}}})).await.is_err());
    assert_eq!(
        AgentPersistence::get_by_id(f.persistence.as_ref(), id)
            .await
            .unwrap()
            .unwrap()
            .response_contract
            .unwrap()
            .schema(),
        &json!({"type":"object"})
    );
    let cleared = f
        .update(id, json!({"response_contract":null}))
        .await
        .unwrap();
    assert!(cleared["agent"]["response_contract"].is_null());
    f.cleanup().await;
}

#[tokio::test]
async fn library_copy_keeps_its_contract_and_config() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let contract = serde_json::from_value(
        json!({"version":1,"format":"json_schema","schema":{"type":"object"}}),
    )
    .unwrap();
    let definition = f
        .agents
        .create_library_agent(AgentWrite {
            name: "Structured library".into(),
            slug: format!("library-{}", Uuid::new_v4().simple()),
            config_json: Some(json!({"version":1,"max_turns":4})),
            response_contract: crate::entities::response_contract::ContractUpdate(Some(Some(
                contract,
            ))),
            ..Default::default()
        })
        .await
        .unwrap();
    let copy = f
        .agents
        .create_agent_from_library(f.user, f.company, definition.id)
        .await
        .unwrap()
        .agent;
    assert_eq!(copy.response_contract, definition.response_contract);
    assert_eq!(copy.config_json, definition.config_json);
    assert_ne!(copy.id, definition.id);
    f.agents.delete_library_agent(definition.id).await.unwrap();
    f.cleanup().await;
}

#[test]
fn native_forms_distinguish_omission_clear_and_invalid_schema() {
    let parse = |extra: Value| {
        let mut value = json!({"name":"Form"});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value::<AgentForm>(value).unwrap()
    };
    assert!(parse(json!({})).response_contract().unwrap().0.is_none());
    assert_eq!(
        parse(json!({"response_format":"text"}))
            .response_contract()
            .unwrap()
            .0,
        Some(None)
    );
    let form = parse(json!({"response_format":"json_schema", "response_schema":"{broken"}));
    assert!(
        form.response_contract()
            .unwrap_err()
            .contains("response_schema")
    );
    let form =
        parse(json!({"response_format":"json_schema", "response_schema":"{\"type\":\"object\"}"}));
    assert_eq!(
        form.response_contract()
            .unwrap()
            .0
            .unwrap()
            .unwrap()
            .schema(),
        &json!({"type":"object"})
    );
}
