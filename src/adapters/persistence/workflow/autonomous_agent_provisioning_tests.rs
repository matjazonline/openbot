use super::*;
use crate::application::agent::{AgentPublicationScope, AgentPublications};
use crate::application::use_cases::{
    agent::{AgentPersistence, AgentWrite},
    company::CompanyModelConnectionWrite,
    skill::{AgentCapabilityReader, SkillManagementPersistence, SkillWrite},
};
use crate::entities::agent_capability::{AgentBindingId, AgentId, AgentToolAlias};
use crate::services::test_support::{SCRIPTED_MODEL, SCRIPTED_PROVIDER};

struct PortableLibrary {
    agent: Uuid,
    skill: Uuid,
}

async fn model(fixture: &Fixture) {
    CompanyPersistence::replace_model_connections_for_user(
        &fixture.persistence,
        fixture.target.actor.user_id(),
        fixture.target.company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
}

/// A library agent with one portable skill; `requires_lookup` makes the skill need the
/// company-selected `support.lookup` tool.
async fn portable_library(p: &PostgresPersistence, requires_lookup: bool) -> PortableLibrary {
    let suffix = if requires_lookup { "" } else { "-plain" };
    let skill = SkillManagementPersistence::create_library(
        p,
        SkillWrite {
            slug: format!("portable-support{suffix}"),
            name: "Portable support".into(),
            description: "Requires a company-selected HTTP operation".into(),
            instructions: "Use support.lookup to verify the request.".into(),
            required_tools: requires_lookup
                .then(|| vec![TypeName::parse("support.lookup").unwrap()]),
            created_by: None,
        },
    )
    .await
    .unwrap();
    let source = AgentPersistence::create_library(
        p,
        AgentWrite {
            name: "Portable agent".into(),
            slug: format!("portable-agent{suffix}"),
            skill_ids: vec![skill.id],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    PortableLibrary {
        agent: source.id,
        skill: skill.id,
    }
}

async fn install(
    fixture: &Fixture,
    library: &PortableLibrary,
) -> AppResult<crate::entities::agent::Agent> {
    crate::adapters::persistence::test_support::company_agents(&fixture.persistence)
        .copy_from_library(
            fixture.target.actor,
            fixture.target.company,
            library.agent,
            AgentWrite {
                name: "Portable agent".into(),
                slug: "installed-agent".into(),
                provider: Some(SCRIPTED_PROVIDER.into()),
                model: Some(SCRIPTED_MODEL.into()),
                ..Default::default()
            },
            None,
        )
        .await
        .map(|created| created.agent)
}

/// 01DEP-P03-D1, plan 03A.2: a copy is published with its default desk, so a library skill that
/// needs a company tool no binding supplies refuses the copy with the publication's own words,
/// and nothing of it is written.
#[tokio::test]
async fn autonomous_agent_provisioning_library_missing_local_tool_refuses_the_copy() {
    let fixture = Fixture::new().await;
    model(&fixture).await;
    let library = portable_library(&fixture.persistence, true).await;
    let refused = install(&fixture, &library).await;
    assert!(
        matches!(&refused, Err(AppError::BadRequest(message))
            if message.contains("requires tool alias support.lookup")),
        "{refused:?}"
    );
    assert_eq!(
        installation_counts(&fixture).await,
        json!({"agents":0,"skills":0,"principals":0,"revisions":0})
    );
}

#[tokio::test]
async fn autonomous_agent_provisioning_library_requires_explicit_company_local_binding_before_publish()
 {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let fixture = &setup.fixture;
    let p = &fixture.persistence;
    // A copy whose skill needs the company tool cannot be published, so it is refused (03A.2)...
    let library = portable_library(p, true).await;
    assert!(matches!(
        install(fixture, &library).await,
        Err(AppError::BadRequest(_))
    ));
    // ...and the tool is bound explicitly on a published copy's draft, then published as its
    // next revision.
    let library = portable_library(p, false).await;
    let copied = install(fixture, &library).await.unwrap();
    let scope = AgentPublicationScope {
        company: fixture.target.company,
        actor: fixture.target.actor,
        agent: AgentId::new(copied.id),
    };
    let mut bindings = crate::adapters::persistence::agent_binding_selection::resolve_bindings_on(
        &mut p.pool().acquire().await.unwrap(),
        fixture.target.company.as_uuid(),
        None,
        &[TypeName::parse("registered.request").unwrap()],
    )
    .await
    .unwrap();
    bindings[0].alias = AgentToolAlias::parse("support.lookup").unwrap();
    let capabilities = p
        .load_capabilities(fixture.target.company.as_uuid(), copied.id)
        .await
        .unwrap()
        .unwrap();
    AgentPersistence::update(
        p,
        copied.id,
        AgentWrite {
            name: copied.name.clone(),
            slug: copied.slug.clone(),
            provider: copied.provider.clone(),
            model: copied.model.clone(),
            skill_ids: capabilities.skills.iter().map(|skill| skill.id).collect(),
            tool_bindings: Some(bindings.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let published = fixture.publish_agent(copied.id).await;
    assert_eq!(published.definition.tools.len(), 1);
    assert_eq!(published.definition.tools[0].binding, bindings[0]);
    assert_eq!(published.definition.skills.len(), 1);

    let foreign = Fixture::in_database(fixture._db.clone(), "-foreign-library").await;
    bindings[0].id = AgentBindingId::new(Uuid::new_v4());
    let rejected = AgentPersistence::create(
        &foreign.persistence,
        foreign.target.company.as_uuid(),
        AgentWrite {
            name: "Foreign attempt".into(),
            slug: "foreign-attempt".into(),
            tool_bindings: Some(bindings),
            ..Default::default()
        },
    )
    .await;
    assert!(
        rejected.is_err(),
        "an actual other company cannot select the foreign connection, even with a new binding identity"
    );
    assert!(
        AgentPersistence::list_by_company_id(p, foreign.target.company.as_uuid())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        p.active_agent_revision(scope).await.unwrap().unwrap().id,
        published.id
    );
}

#[tokio::test]
async fn autonomous_agent_provisioning_library_install_failure_rolls_back_portable_skills_and_principal()
 {
    let fixture = Fixture::new().await;
    model(&fixture).await;
    let p = &fixture.persistence;
    let library = portable_library(p, false).await;
    sqlx::raw_sql("CREATE FUNCTION library_install_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected principal failure'; END $$; CREATE TRIGGER library_install_fault BEFORE INSERT ON principals FOR EACH ROW WHEN (NEW.kind='agent') EXECUTE FUNCTION library_install_fault();")
        .execute(p.pool()).await.unwrap();
    assert!(install(&fixture, &library).await.is_err());
    let counts = installation_counts(&fixture).await;
    assert_eq!(
        counts,
        json!({"agents":0,"skills":0,"principals":0,"revisions":0})
    );
    sqlx::raw_sql(
        "DROP TRIGGER library_install_fault ON principals; DROP FUNCTION library_install_fault();",
    )
    .execute(p.pool())
    .await
    .unwrap();
    let copied = install(&fixture, &library).await.unwrap();
    assert_eq!(
        installation_counts(&fixture).await,
        json!({"agents":1,"skills":1,"principals":1,"revisions":1})
    );
    assert_eq!(copied.slug, "installed-agent");
    // The copy carries a company copy of the portable skill, not the library's own.
    let capabilities = p
        .load_capabilities(fixture.target.company.as_uuid(), copied.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        capabilities.skills[0].company_id,
        Some(fixture.target.company.as_uuid())
    );
    assert_ne!(capabilities.skills[0].id, library.skill);
}

async fn installation_counts(fixture: &Fixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('agents',(SELECT count(*) FROM agents WHERE company_id=$1),'skills',(SELECT count(*) FROM skills WHERE company_id=$1),'principals',(SELECT count(*) FROM principals WHERE company_id=$1 AND kind='agent'),'revisions',(SELECT count(*) FROM agent_revisions WHERE company_id=$1))")
        .bind(fixture.target.company.as_uuid()).fetch_one(fixture.persistence.pool()).await.unwrap()
}
