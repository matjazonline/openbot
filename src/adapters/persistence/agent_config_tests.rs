//! Agent settings persistence after 06C: one runtime (Rig), no tool grants or native policy,
//! and edits that omit a setting keep the stored value.
use super::{PostgresPersistence, test_support::test_pool};
use crate::{
    entities::{
        creation::CreationProvenance,
        response_contract::{ContractUpdate, ResponseContract},
    },
    use_cases::{
        agent::{AgentPersistence, AgentWrite, OwnedAgentChannelPersistence},
        channel::ChannelWrite,
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};
use uuid::Uuid;

struct Fixture {
    company: Uuid,
    agent: Uuid,
}

async fn fixture(persistence: &PostgresPersistence) -> Fixture {
    let suffix = Uuid::new_v4().simple().to_string();
    let user = persistence
        .create_user(
            &format!("agent-config-{suffix}"),
            &format!("agent-config-{suffix}@example.com"),
            "hash",
        )
        .await
        .unwrap();
    let company = CompanyPersistence::create(
        persistence,
        user.id,
        CompanyWrite {
            name: "Agent config".into(),
            slug: format!("agent-config-{suffix}"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (agent, _) = persistence
        .create_owned_agent_channel(
            company.id,
            write(),
            ChannelWrite {
                name: "Agent config".into(),
                slug: "agent-config".into(),
                created_by: Some(CreationProvenance::system()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    Fixture {
        company: company.id,
        agent: agent.id,
    }
}

fn write() -> AgentWrite {
    AgentWrite {
        name: "Agent config".into(),
        slug: "agent-config".into(),
        created_by: Some(CreationProvenance::system()),
        ..Default::default()
    }
}

#[tokio::test]
async fn retired_selection_columns_and_tables_are_gone() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'agents' \
           AND column_name IN ('harness_kind', 'granted_tool_ids', 'native_tool_policy')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(columns.is_empty(), "{columns:?}");
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables WHERE table_schema = 'public' \
           AND table_name IN ('agent_mcp_selections', 'agent_mcp_selection_revisions', \
                              'agent_channel_provisions')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(tables.is_empty(), "{tables:?}");
}

#[tokio::test]
async fn rig_config_is_canonical_and_omitted_updates_keep_it() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = fixture(&persistence).await;
    let mut configured = write();
    configured.config_json = Some(serde_json::json!({"version": 1, "max_turns": 3}));
    let saved = AgentPersistence::update(&persistence, fixture.agent, configured)
        .await
        .unwrap();
    assert_eq!(
        saved.config_json,
        Some(serde_json::json!({"version": 1, "max_turns": 3}))
    );
    let renamed = AgentPersistence::update(&persistence, fixture.agent, write())
        .await
        .unwrap();
    assert_eq!(
        renamed.config_json, saved.config_json,
        "omission keeps the config"
    );
    let mut reset = write();
    reset.config_json = Some(serde_json::json!({"version": 1}));
    let reset = AgentPersistence::update(&persistence, fixture.agent, reset)
        .await
        .unwrap();
    assert!(
        reset.config_json.is_none(),
        "the canonical default is stored as absent"
    );
    let mut retired = write();
    retired.config_json = Some(
        serde_json::json!({"version": 1, "reasoning": {"mode": "react", "max_iterations": 2}}),
    );
    assert!(
        AgentPersistence::update(&persistence, fixture.agent, retired)
            .await
            .is_err(),
        "ai-agents settings are no longer accepted"
    );
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(fixture.company)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn response_contract_round_trips_survives_omission_and_clears() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let fixture = fixture(&persistence).await;
    let contract: ResponseContract = serde_json::from_value(
        serde_json::json!({"version":1,"format":"json_schema","schema":{"type":"object"}}),
    )
    .unwrap();
    let mut configured = write();
    configured.response_contract = ContractUpdate(Some(Some(contract.clone())));
    let saved = AgentPersistence::update(&persistence, fixture.agent, configured)
        .await
        .unwrap();
    assert_eq!(saved.response_contract, Some(contract.clone()));
    let unrelated = AgentPersistence::update(&persistence, fixture.agent, write())
        .await
        .unwrap();
    assert_eq!(unrelated.response_contract, Some(contract));
    let mut clear = write();
    clear.response_contract = ContractUpdate(Some(None));
    AgentPersistence::update(&persistence, fixture.agent, clear)
        .await
        .unwrap();
    assert!(
        AgentPersistence::get_by_id(&persistence, fixture.agent)
            .await
            .unwrap()
            .unwrap()
            .response_contract
            .is_none()
    );
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(fixture.company)
        .execute(&pool)
        .await
        .unwrap();
}
