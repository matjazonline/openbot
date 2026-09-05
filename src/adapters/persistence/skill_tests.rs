use super::*;
use std::sync::Arc;

use crate::{
    adapters::persistence::test_support::test_pool,
    entities::{
        creation::CreationProvenance, harness::SubAgentScope, skill::SkillInstruction,
        value_objects::ToolId,
    },
    use_cases::{
        agent::{AgentPersistence, AgentWrite, OwnedAgentChannelPersistence},
        channel::ChannelWrite,
        company::{CompanyPersistence, CompanyWrite},
        skill::AgentCapabilityReader,
        user::UserPersistence,
    },
};

#[test]
fn copied_slugs_stay_bounded() {
    let original = "a".repeat(SkillSlug::MAX_CHARS);
    assert_eq!(copied_slug(&original, 1).len(), SkillSlug::MAX_CHARS);
    assert_eq!(copied_slug(&original, 100).len(), SkillSlug::MAX_CHARS);
    assert!(copied_slug(&original, 100).ends_with("-100"));
}

async fn company(persistence: &PostgresPersistence, label: &str) -> Uuid {
    let suffix = Uuid::new_v4().simple().to_string();
    let user = persistence
        .create_user(
            &format!("skill_{label}_{suffix}"),
            &format!("skill_{label}_{suffix}@example.com"),
            "hash",
        )
        .await
        .unwrap();
    CompanyPersistence::create(
        persistence,
        user.id,
        CompanyWrite {
            name: format!("Skill {label}"),
            slug: format!("skill-{label}-{suffix}"),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

fn skill_write(slug: impl Into<String>) -> SkillWrite {
    let mut write = SkillWrite {
        slug: slug.into(),
        name: "Review invoice".into(),
        description: "Review an invoice and report anomalies.".into(),
        trigger: "An invoice is attached or mentioned.".into(),
        instructions: vec![SkillInstruction::Prompt {
            text: "Review the invoice and reply with the findings.".into(),
        }],
        created_by: Some(CreationProvenance::system()),
    };
    write.normalize().unwrap();
    write
}

fn agent_write(slug: impl Into<String>) -> AgentWrite {
    AgentWrite {
        name: "Capability agent".into(),
        slug: slug.into(),
        created_by: Some(CreationProvenance::system()),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_company_skill_round_trips() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let company_id = company(&persistence, "roundtrip").await;
    let created = persistence
        .create_company(company_id, skill_write("invoice-review"))
        .await
        .unwrap();
    let loaded = persistence
        .get_company(company_id, created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded, created);
    assert_eq!(loaded.company_id, Some(company_id));
}

#[tokio::test]
async fn copying_a_library_skill_produces_a_company_owned_row_and_preserves_the_source() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let company_id = company(&persistence, "copy").await;
    let slug = format!("library-{}", Uuid::new_v4().simple());
    let source = SkillManagementPersistence::create_library(&persistence, skill_write(&slug))
        .await
        .unwrap();
    let copied = persistence
        .copy_library_to_company(company_id, source.id, CreationProvenance::system())
        .await
        .unwrap();
    assert_eq!(copied.company_id, Some(company_id));
    assert_eq!(copied.slug, source.slug);
    assert_eq!(
        persistence.get_library(source.id).await.unwrap().unwrap(),
        source
    );
}

#[tokio::test]
async fn two_competing_library_skill_copies_receive_distinct_slugs() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let company_id = company(&persistence, "concurrent-copy").await;
    let source = SkillManagementPersistence::create_library(
        &persistence,
        skill_write(format!("library-{}", Uuid::new_v4().simple())),
    )
    .await
    .unwrap();

    let first =
        persistence.copy_library_to_company(company_id, source.id, CreationProvenance::system());
    let second =
        persistence.copy_library_to_company(company_id, source.id, CreationProvenance::system());
    let (first, second) = tokio::join!(first, second);
    let first = first.unwrap();
    let second = second.unwrap();
    assert_ne!(first.id, second.id);
    assert_ne!(first.slug, second.slug);
    assert!([first.slug.as_str(), second.slug.as_str()].contains(&source.slug.as_str()));
}

#[tokio::test]
async fn another_companys_skill_is_not_readable_updatable_or_deletable() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let owner = company(&persistence, "owner").await;
    let intruder = company(&persistence, "intruder").await;
    let skill = persistence
        .create_company(owner, skill_write("private-skill"))
        .await
        .unwrap();

    assert!(
        persistence
            .get_company(intruder, skill.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        persistence
            .update_company(intruder, skill.id, skill_write("stolen"))
            .await
            .is_err()
    );
    assert!(
        persistence
            .delete_company(intruder, skill.id)
            .await
            .is_err()
    );
    assert!(
        persistence
            .get_company(owner, skill.id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn capability_selection_is_scoped_and_updates_atomically() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let company_id = company(&persistence, "atomic").await;
    let other_company_id = company(&persistence, "atomic-other").await;
    let skill = persistence
        .create_company(company_id, skill_write("owned-skill"))
        .await
        .unwrap();
    let foreign = persistence
        .create_company(other_company_id, skill_write("foreign-skill"))
        .await
        .unwrap();
    let agent = AgentPersistence::create(&persistence, company_id, agent_write("atomic-agent"))
        .await
        .unwrap();

    let mut selected = agent_write("atomic-agent");
    selected.skill_ids = vec![skill.id];
    AgentPersistence::update(&persistence, agent.id, selected.clone())
        .await
        .unwrap();
    selected.skill_ids = vec![foreign.id];
    assert!(
        AgentPersistence::update(&persistence, agent.id, selected)
            .await
            .is_err()
    );
    let snapshot = persistence
        .load_for_execution(company_id, agent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot
            .skills
            .iter()
            .map(|skill| skill.id)
            .collect::<Vec<_>>(),
        [skill.id]
    );
}

#[tokio::test]
async fn a_capability_snapshot_never_mixes_an_agent_row_with_a_different_relationship_version() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = Arc::new(PostgresPersistence::new(pool));
    let company_id = company(&persistence, "snapshot-version").await;
    let first = persistence
        .create_company(company_id, skill_write("snapshot-first"))
        .await
        .unwrap();
    let second = persistence
        .create_company(company_id, skill_write("snapshot-second"))
        .await
        .unwrap();
    let agent = AgentPersistence::create(
        persistence.as_ref(),
        company_id,
        agent_write("snapshot-agent"),
    )
    .await
    .unwrap();

    let mut version_a = agent_write("snapshot-agent");
    version_a.name = "Snapshot version A".into();
    version_a.skill_ids = vec![first.id];
    let mut version_b = agent_write("snapshot-agent");
    version_b.name = "Snapshot version B".into();
    version_b.skill_ids = vec![second.id];
    AgentPersistence::update(persistence.as_ref(), agent.id, version_a.clone())
        .await
        .unwrap();

    let writer = {
        let persistence = persistence.clone();
        tokio::spawn(async move {
            for index in 0..100 {
                let write = if index % 2 == 0 {
                    version_b.clone()
                } else {
                    version_a.clone()
                };
                AgentPersistence::update(persistence.as_ref(), agent.id, write)
                    .await
                    .unwrap();
                tokio::task::yield_now().await;
            }
        })
    };

    for _ in 0..200 {
        let snapshot = persistence
            .load_for_execution(company_id, agent.id)
            .await
            .unwrap()
            .unwrap();
        let skill_ids = snapshot
            .skills
            .iter()
            .map(|skill| skill.id)
            .collect::<Vec<_>>();
        match snapshot.agent.name.as_str() {
            "Snapshot version A" => assert_eq!(skill_ids, [first.id]),
            "Snapshot version B" => assert_eq!(skill_ids, [second.id]),
            name => panic!("unexpected agent version {name}"),
        }
        tokio::task::yield_now().await;
    }
    writer.await.unwrap();
}

#[tokio::test]
async fn deleting_either_side_cascades_a_company_agent_skill_selection() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let company_id = company(&persistence, "cascade").await;
    let skill = persistence
        .create_company(company_id, skill_write("cascade-skill"))
        .await
        .unwrap();
    let mut write = agent_write("cascade-agent");
    write.skill_ids = vec![skill.id];
    let agent = AgentPersistence::create(&persistence, company_id, write)
        .await
        .unwrap();
    AgentPersistence::delete(&persistence, agent.id)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_skills WHERE agent_id = $1")
        .bind(agent.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);

    let second = AgentPersistence::create(&persistence, company_id, agent_write("cascade-agent-2"))
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_skills (company_id, agent_id, skill_id, position) VALUES ($1, $2, $3, 0)")
        .bind(company_id)
        .bind(second.id)
        .bind(skill.id)
        .execute(&pool)
        .await
        .unwrap();
    persistence
        .delete_company(company_id, skill.id)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_skills WHERE skill_id = $1")
        .bind(skill.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn deleting_an_in_use_library_skill_is_refused() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let skill = SkillManagementPersistence::create_library(
        &persistence,
        skill_write(format!("guarded-{}", Uuid::new_v4().simple())),
    )
    .await
    .unwrap();
    let mut write = agent_write(format!("library-agent-{}", Uuid::new_v4().simple()));
    write.skill_ids = vec![skill.id];
    AgentPersistence::create_library(&persistence, write)
        .await
        .unwrap();
    let error = persistence.delete_library(skill.id).await.unwrap_err();
    assert!(error.to_string().contains("still used"), "{error}");
}

#[tokio::test]
async fn a_direct_library_agent_resolves_to_all_company_siblings() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let company_id = company(&persistence, "library-scope").await;
    let agent = AgentPersistence::create_library(
        &persistence,
        agent_write(format!("global-{}", Uuid::new_v4().simple())),
    )
    .await
    .unwrap();
    let snapshot = persistence
        .load_for_execution(company_id, agent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.sub_agent_scope, SubAgentScope::AllCompanySiblings);
}

#[tokio::test]
async fn picking_a_library_agent_copies_its_skills_with_the_agent_and_channel() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let company_id = company(&persistence, "agent-copy").await;
    let source_skill = SkillManagementPersistence::create_library(
        &persistence,
        skill_write(format!("agent-skill-{}", Uuid::new_v4().simple())),
    )
    .await
    .unwrap();
    let mut source_write = agent_write(format!("source-{}", Uuid::new_v4().simple()));
    source_write.skill_ids = vec![source_skill.id];
    let source_agent = AgentPersistence::create_library(&persistence, source_write)
        .await
        .unwrap();

    let company_write = AgentWrite {
        name: source_agent.name.clone(),
        slug: format!("picked-{}", Uuid::new_v4().simple()),
        created_by: Some(CreationProvenance::system()),
        ..Default::default()
    };
    let channel_write = ChannelWrite {
        name: "Picked agent".into(),
        slug: company_write.slug.clone(),
        created_by: Some(CreationProvenance::system()),
        ..Default::default()
    };
    let (created, _channel) = persistence
        .create_owned_agent_channel_from_library(
            company_id,
            source_agent.id,
            company_write,
            channel_write,
        )
        .await
        .unwrap();
    let snapshot = persistence
        .load_for_execution(company_id, created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.skills.len(), 1);
    assert_eq!(snapshot.skills[0].company_id, Some(company_id));
    assert_ne!(snapshot.skills[0].id, source_skill.id);
    assert!(
        persistence
            .get_library(source_skill.id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn database_bounds_skill_positions_and_tool_array_shape() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let company_id = company(&persistence, "bounds").await;
    let skill = persistence
        .create_company(company_id, skill_write("bounded-skill"))
        .await
        .unwrap();
    let agent = AgentPersistence::create(&persistence, company_id, agent_write("bounded-agent"))
        .await
        .unwrap();
    assert!(
        sqlx::query("INSERT INTO agent_skills (company_id, agent_id, skill_id, position) VALUES ($1, $2, $3, 16)")
            .bind(company_id)
            .bind(agent.id)
            .bind(skill.id)
            .execute(&pool)
            .await
            .is_err()
    );

    let sibling =
        AgentPersistence::create(&persistence, company_id, agent_write("bounded-sibling"))
            .await
            .unwrap();
    assert!(
        sqlx::query(
            "INSERT INTO agent_sub_agents (company_id, agent_id, sub_agent_id, position) \
             VALUES ($1, $2, $3, 64)",
        )
        .bind(company_id)
        .bind(agent.id)
        .bind(sibling.id)
        .execute(&pool)
        .await
        .is_err()
    );
    let oversized_grant = (0..33)
        .map(|index| format!("tool-{index}"))
        .collect::<Vec<_>>();
    assert!(
        sqlx::query("UPDATE agents SET granted_tool_ids = $1 WHERE id = $2")
            .bind(oversized_grant)
            .bind(agent.id)
            .execute(&pool)
            .await
            .is_err()
    );

    let long = "x".repeat(121);
    let matrix = [
        (Vec::<Option<&str>>::new(), true),
        (vec![Some("datetime")], true),
        (vec![Some("")], false),
        (vec![None], false),
        (vec![Some(long.as_str())], false),
    ];
    for (values, expected) in matrix {
        let actual: bool = sqlx::query_scalar("SELECT valid_tool_id_array($1::text[])")
            .bind(values)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn database_scope_trigger_refuses_live_cross_library_skill_links() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let company_id = company(&persistence, "scope-trigger").await;
    let company_skill = persistence
        .create_company(company_id, skill_write("company-only"))
        .await
        .unwrap();
    let library_skill = SkillManagementPersistence::create_library(
        &persistence,
        skill_write(format!("global-{}", Uuid::new_v4().simple())),
    )
    .await
    .unwrap();
    let company_agent =
        AgentPersistence::create(&persistence, company_id, agent_write("company-agent"))
            .await
            .unwrap();
    let library_agent = AgentPersistence::create_library(
        &persistence,
        agent_write(format!("library-agent-{}", Uuid::new_v4().simple())),
    )
    .await
    .unwrap();

    assert!(
        sqlx::query(
            "INSERT INTO agent_skills (company_id, agent_id, skill_id, position) \
             VALUES ($1, $2, $3, 0)",
        )
        .bind(company_id)
        .bind(company_agent.id)
        .bind(library_skill.id)
        .execute(&pool)
        .await
        .is_err()
    );
    assert!(
        sqlx::query(
            "INSERT INTO agent_skills (company_id, agent_id, skill_id, position) \
             VALUES (NULL, $1, $2, 0)",
        )
        .bind(library_agent.id)
        .bind(company_skill.id)
        .execute(&pool)
        .await
        .is_err()
    );
}

#[tokio::test]
async fn malformed_but_deserializable_skill_json_is_a_contextual_conversion_error() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let company_id = company(&persistence, "malformed").await;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO skills (id, company_id, slug, name, description, trigger, instructions, created_by) \
         VALUES ($1, $2, 'malformed', 'Malformed', 'Malformed row.', 'Always.', $3, $4)",
    )
    .bind(id)
    .bind(company_id)
    .bind(serde_json::json!([{"prompt": ""}]))
    .bind(serde_json::to_value(CreationProvenance::system()).unwrap())
    .execute(&pool)
    .await
    .unwrap();
    let error = persistence.get_company(company_id, id).await.unwrap_err();
    assert!(error.to_string().contains(&id.to_string()), "{error}");
}

#[tokio::test]
async fn direct_sql_agent_config_cannot_reach_runtime_authority_paths() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let company_id = company(&persistence, "unsafe-config").await;
    let agent =
        AgentPersistence::create(&persistence, company_id, agent_write("unsafe-config-agent"))
            .await
            .unwrap();
    sqlx::query("UPDATE agents SET config_json = $1 WHERE id = $2")
        .bind(serde_json::json!({
            "version": 1,
            "spawner": {"management_tools": ["command"]},
            "persona": {"evolution": {"allow_llm_evolve": true}},
            "tool_security": {"enabled": false},
            "observability": {"include_prompts": true}
        }))
        .bind(agent.id)
        .execute(&pool)
        .await
        .unwrap();

    let error = AgentPersistence::get_by_id(&persistence, agent.id)
        .await
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains(&agent.id.to_string()), "{error}");
    assert!(message.contains("not accepted"), "{error}");
    assert!(
        ["spawner", "persona", "tool_security", "observability"]
            .iter()
            .any(|path| message.contains(path)),
        "{error}"
    );
}

#[test]
fn catalogue_size_remains_within_the_persisted_grant_bound() {
    let grantable = crate::entities::tool_catalogue::CatalogueTool::grantable()
        .map(|tool| ToolId::from(tool.id))
        .collect::<Vec<_>>();
    assert!(grantable.len() <= crate::entities::agent::MAX_GRANTED_TOOLS);
}
