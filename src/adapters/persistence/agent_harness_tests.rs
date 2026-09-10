use super::{PostgresPersistence, test_support::test_pool};
use crate::{
    entities::creation::CreationProvenance,
    use_cases::{
        agent::{AgentWrite, OwnedAgentChannelPersistence},
        channel::ChannelWrite,
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};
use std::time::Duration;
use uuid::Uuid;

#[path = "agent_harness_query_tests.rs"]
mod query_scopes;

struct Fixture {
    company: Uuid,
    agent: Uuid,
    channel: Uuid,
}
async fn fixture(pool: &sqlx::PgPool) -> Fixture {
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let user = persistence
        .create_user(
            &format!("harness-{suffix}"),
            &format!("harness-{suffix}@example.com"),
            "hash",
        )
        .await
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        user.id,
        CompanyWrite {
            name: "Harness fencing".into(),
            slug: format!("harness-{suffix}"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let write = AgentWrite {
        harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
        name: "Harness fixture".into(),
        slug: "harness-fixture".into(),
        created_by: Some(CreationProvenance::system()),
        ..Default::default()
    };
    let (agent, channel) = persistence
        .create_owned_agent_channel(
            company.id,
            write.clone(),
            ChannelWrite {
                name: "Harness fixture".into(),
                slug: "harness-fixture".into(),
                created_by: Some(CreationProvenance::system()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    Fixture {
        company: company.id,
        agent: agent.id,
        channel: channel.id,
    }
}

#[tokio::test]
async fn harness_change_competes_with_task_creation_and_waits_for_settlement() {
    let _queue_guard = super::test_support::UNSCOPED_CLAIM.lock().await;
    let Some(pool) = test_pool().await else {
        return;
    };
    let Fixture {
        company,
        agent,
        channel,
    } = fixture(&pool).await;
    let task = Uuid::new_v4();
    let mut creating = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO background_tasks (id, correlation_id, company_id, channel_id, task_type, payload) VALUES ($1, $1, $2, $3, 'email_agent_dispatch', '{}')")
        .bind(task).bind(company).bind(channel).execute(&mut *creating).await.unwrap();
    let update = change_harness(&pool, agent);
    tokio::pin!(update);
    // The insert owns a SHARE lock until commit; the competing update cannot pass it.
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut update)
            .await
            .is_err()
    );
    creating.commit().await.unwrap();
    assert_fenced(
        tokio::time::timeout(Duration::from_secs(5), update)
            .await
            .unwrap(),
    );
    let persistence = PostgresPersistence::new(pool.clone());
    let error = persistence
        .update_agent_and_owned_address(
            agent,
            AgentWrite {
                name: "Harness fixture".into(),
                slug: "harness-fixture".into(),
                harness_kind: Some(crate::entities::harness::HarnessKind::Rig),
                config_json: Some(serde_json::json!({"version":1})),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, crate::app_error::AppError::Conflict(_)));
    for status in [
        "processing",
        "pending_approval",
        "waiting_for_third_party_reply",
        "stopped",
        "failed",
        "dead_letter",
    ] {
        sqlx::query("UPDATE background_tasks SET status = $1,
            worker_id = CASE WHEN $1 = 'processing' THEN gen_random_uuid() END,
            execution_generation = CASE WHEN $1 = 'processing' THEN gen_random_uuid() END,
            locked_at = CASE WHEN $1 = 'processing' THEN CURRENT_TIMESTAMP END,
            lock_expires_at = CASE WHEN $1 = 'processing' THEN CURRENT_TIMESTAMP + INTERVAL '1 minute' END
            WHERE id = $2")
            .bind(status)
            .bind(task)
            .execute(&pool)
            .await
            .unwrap();
        assert_fenced(change_harness(&pool, agent).await);
    }
    sqlx::query("UPDATE background_tasks SET status = 'completed' WHERE id = $1")
        .bind(task)
        .execute(&pool)
        .await
        .unwrap();
    change_harness(&pool, agent).await.unwrap();
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(company)
        .execute(&pool)
        .await
        .unwrap();
}

async fn change_harness(
    pool: &sqlx::PgPool,
    agent: Uuid,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query("UPDATE agents SET harness_kind = 'rig' WHERE id = $1")
        .bind(agent)
        .execute(pool)
        .await
}
fn assert_fenced(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) {
    let error = result.unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|db| db.constraint()),
        Some("agent_harness_has_unsettled_tasks")
    );
}

#[tokio::test]
async fn baseline_defaults_to_rig_and_accepts_only_supported_harnesses() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let Fixture { company, agent, .. } = fixture(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    let explicit: String = sqlx::query_scalar("SELECT harness_kind FROM agents WHERE id = $1")
        .bind(agent)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(explicit, "ai_agents");
    let default: String = sqlx::query_scalar(
        "UPDATE agents SET harness_kind = DEFAULT WHERE id = $1 RETURNING harness_kind",
    )
    .bind(agent)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(default, "rig");
    let invalid = sqlx::query("UPDATE agents SET harness_kind = 'unknown' WHERE id = $1")
        .bind(agent)
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        invalid.as_database_error().unwrap().constraint(),
        Some("agents_harness_kind_check")
    );
    tx.rollback().await.unwrap();
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(company)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn persistence_binds_deployment_policy_and_preserves_omitted_updates() {
    use crate::{entities::harness::HarnessKind, use_cases::agent::AgentPersistence};
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let legacy =
        PostgresPersistence::new(pool.clone()).with_default_agent_harness(HarnessKind::AiAgents);
    let rig = PostgresPersistence::new(pool.clone()).with_default_agent_harness(HarnessKind::Rig);
    for kind in HarnessKind::ALL {
        let created = AgentPersistence::create(
            &legacy,
            fixture.company,
            AgentWrite {
                name: kind.as_str().into(),
                slug: kind.as_str().replace('_', "-"),
                harness_kind: Some(kind),
                config_json: Some(serde_json::Value::Null),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(created.harness_kind, kind);
        assert!(created.config_json.is_none());
        let updated = AgentPersistence::update(
            &rig,
            created.id,
            AgentWrite {
                name: "Updated".into(),
                slug: created.slug.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.harness_kind, kind);
        assert_eq!(
            AgentPersistence::get_by_id(&rig, created.id)
                .await
                .unwrap()
                .unwrap()
                .harness_kind,
            kind
        );
    }
    let inherited = AgentPersistence::create(
        &legacy,
        fixture.company,
        AgentWrite {
            name: "Inherited".into(),
            slug: "inherited".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(inherited.harness_kind, HarnessKind::AiAgents);
    let invalid = sqlx::query("UPDATE agents SET harness_kind='unknown' WHERE id=$1")
        .bind(inherited.id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert_eq!(
        invalid.as_database_error().unwrap().constraint(),
        Some("agents_harness_kind_check")
    );
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(fixture.company)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn switching_to_an_explicit_empty_configuration_round_trips_through_the_use_case() {
    use crate::{
        entities::harness::HarnessKind,
        use_cases::agent::{AgentPersistence, AgentUseCases, SpamScanning},
    };
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let p = std::sync::Arc::new(PostgresPersistence::new(pool.clone()));
    let owner = CompanyPersistence::get_by_id(p.as_ref(), fixture.company)
        .await
        .unwrap()
        .unwrap()
        .user_id;
    let legacy = AgentWrite {
        name: "Harness fixture".into(),
        slug: "harness-fixture".into(),
        harness_kind: Some(HarnessKind::AiAgents),
        config_json: Some(
            serde_json::json!({"version":1,"reasoning":{"mode":"react","max_iterations":2}}),
        ),
        ..Default::default()
    };
    AgentPersistence::update(p.as_ref(), fixture.agent, legacy)
        .await
        .unwrap();
    let use_cases = AgentUseCases::new(p.clone(), p.clone(), p.clone(), SpamScanning::Available);
    let target = AgentWrite {
        name: "Harness fixture".into(),
        slug: "harness-fixture".into(),
        harness_kind: Some(HarnessKind::Rig),
        config_json: Some(serde_json::json!({"version":1})),
        ..Default::default()
    };
    let updated = use_cases
        .update_agent(owner, fixture.company, fixture.agent, target)
        .await
        .unwrap();
    assert_eq!(updated.harness_kind, HarnessKind::Rig);
    assert!(updated.config_json.is_none());
    assert_eq!(
        AgentPersistence::get_by_id(p.as_ref(), fixture.agent)
            .await
            .unwrap()
            .unwrap()
            .harness_kind,
        HarnessKind::Rig
    );
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(fixture.company)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn response_contract_round_trip_patch_and_admission_fence() {
    use crate::entities::{
        harness::HarnessKind,
        response_contract::{ContractUpdate, ResponseContract},
    };
    use crate::use_cases::agent::AgentPersistence;
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = super::test_support::UNSCOPED_CLAIM.lock().await;
    let fixture = fixture(&pool).await;
    change_harness(&pool, fixture.agent).await.unwrap();
    let persistence = PostgresPersistence::new(pool.clone());
    let contract: ResponseContract = serde_json::from_value(
        serde_json::json!({"version":1,"format":"json_schema","schema":{"type":"object"}}),
    )
    .unwrap();
    let write = || AgentWrite {
        name: "Harness fixture".into(),
        slug: "harness-fixture".into(),
        harness_kind: Some(HarnessKind::Rig),
        ..Default::default()
    };
    let mut configured = write();
    configured.response_contract = ContractUpdate(Some(Some(contract.clone())));
    let saved = AgentPersistence::update(&persistence, fixture.agent, configured)
        .await
        .unwrap();
    assert_eq!(saved.response_contract, Some(contract.clone()));
    let unrelated = AgentPersistence::update(&persistence, fixture.agent, write())
        .await
        .unwrap();
    assert_eq!(unrelated.response_contract, Some(contract.clone()));
    let mut unsupported = write();
    unsupported.harness_kind = Some(HarnessKind::AiAgents);
    assert!(
        AgentPersistence::update(&persistence, fixture.agent, unsupported)
            .await
            .is_err()
    );
    let task = Uuid::new_v4();
    let mut creating = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO background_tasks (id,correlation_id,company_id,channel_id,task_type,payload) VALUES ($1,$1,$2,$3,'email_agent_dispatch','{}')")
        .bind(task).bind(fixture.company).bind(fixture.channel).execute(&mut *creating).await.unwrap();
    let clearing = sqlx::query("UPDATE agents SET response_contract=NULL WHERE id=$1")
        .bind(fixture.agent)
        .execute(&pool);
    tokio::pin!(clearing);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut clearing)
            .await
            .is_err()
    );
    creating.commit().await.unwrap();
    assert_fenced(clearing.await);
    sqlx::query("DELETE FROM background_tasks WHERE id=$1")
        .bind(task)
        .execute(&pool)
        .await
        .unwrap();
    let mut clear = write();
    clear.response_contract = ContractUpdate(Some(None));
    assert!(
        AgentPersistence::update(&persistence, fixture.agent, clear)
            .await
            .unwrap()
            .response_contract
            .is_none()
    );
    assert!(
        AgentPersistence::get_by_id(&persistence, fixture.agent)
            .await
            .unwrap()
            .unwrap()
            .response_contract
            .is_none()
    );
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(fixture.company)
        .execute(&pool)
        .await
        .unwrap();
}
