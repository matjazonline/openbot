//! Canonical namespace for native authoring fixtures that intentionally start unpublished.
use crate::adapters::persistence::PostgresPersistence;
use crate::entities::{agent::Agent, transport::PrincipalId};

/// This does not stand in for production creation/default-workflow proof. Authoring tests need
/// an unpublished draft while their UI/application consumers require its canonical address.
pub(crate) async fn register_native_agent_address(p: &PostgresPersistence, agent: &Agent) {
    let company = agent.company_id.expect("a native company-agent fixture");
    let mut tx = p.pool().begin().await.unwrap();
    let principal: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2 FOR UPDATE",
    )
    .bind(company)
    .bind(agent.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO addresses(company_id,slug,target_kind,principal_id,is_primary) \
         VALUES($1,$2,'principal',$3,true)",
    )
    .bind(company)
    .bind(&agent.slug)
    .bind(principal)
    .execute(&mut *tx)
    .await
    .unwrap();
    crate::adapters::persistence::integration::email_binding::sync_principal_email_binding_on(
        &mut tx,
        company,
        PrincipalId::new(principal),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// Rename through the actual authoring writer, preserving the fixture's saved capabilities.
pub(crate) async fn rename_native_agent(
    p: &PostgresPersistence,
    scope: crate::application::agent::AgentPublicationScope,
    slug: &str,
) {
    use crate::application::agent::{AgentAuthoringReads, AgentDraftPersistence, SaveCompanyAgent};
    let state = p.authoring_state(scope).await.unwrap();
    let agent = state.agent;
    p.save_company_agent(SaveCompanyAgent {
        scope,
        expected_revision: agent.draft_revision,
        write: crate::use_cases::agent::AgentWrite {
            name: agent.name,
            slug: slug.into(),
            open_to_public: agent.open_to_public,
            provider: agent.provider,
            model: agent.model,
            system_prompt: agent.system_prompt,
            description: agent.description,
            avatar_url: agent.avatar_url,
            skill_ids: state.skills.iter().map(|skill| skill.id).collect(),
            ..Default::default()
        },
        tools: None,
    })
    .await
    .unwrap();
}
