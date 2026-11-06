use super::*;
use crate::application::agent::{
    AgentDraftPersistence, AgentToolAssignment, AgentToolChoice, SaveCompanyAgent,
};
use crate::use_cases::skill::AgentCapabilityReader;

async fn request(f: &Fixture) -> SaveCompanyAgent {
    let stored =
        f.p.load_capabilities(f.scope.company.as_uuid(), f.scope.agent.as_uuid())
            .await
            .unwrap()
            .unwrap();
    SaveCompanyAgent {
        scope: f.scope,
        expected_revision: stored.agent.draft_revision,
        write: AgentWrite {
            name: stored.agent.name,
            slug: stored.agent.slug,
            provider: stored.agent.provider,
            model: stored.agent.model,
            system_prompt: stored.agent.system_prompt,
            description: stored.agent.description,
            avatar_url: stored.agent.avatar_url,
            skill_ids: stored.skills.into_iter().map(|skill| skill.id).collect(),
            ..Default::default()
        },
        tools: None,
    }
}

fn tool(alias: &str, kind: AgentBuiltinTool) -> AgentToolAssignment {
    AgentToolAssignment {
        binding: None,
        alias: AgentToolAlias::parse(alias).unwrap(),
        choice: AgentToolChoice::Builtin { tool: kind },
    }
}

async fn bindings(f: &Fixture) -> Vec<AgentToolBinding> {
    crate::adapters::persistence::agent_capability::bindings_on(
        &mut f.p.pool().acquire().await.unwrap(),
        Some(f.scope.company.as_uuid()),
        f.scope.agent.as_uuid(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn autonomous_agent_authoring_competing_draft_writers_commit_one_complete_assignment_set() {
    let f = Fixture::new().await;
    let alpha = f.skill(vec![]).await;
    let beta = f.skill(vec![]).await;
    let mut left = request(&f).await;
    let mut right = request(&f).await;
    let expected = left.expected_revision;
    left.write.name = "Alpha".into();
    left.write.system_prompt = Some("Alpha instructions".into());
    left.write.skill_ids = vec![alpha.id];
    left.tools = Some(vec![tool("alpha.reply", AgentBuiltinTool::MessageReply)]);
    right.write.name = "Beta".into();
    right.write.system_prompt = Some("Beta instructions".into());
    right.write.skill_ids = vec![beta.id];
    right.tools = Some(vec![tool("beta.send", AgentBuiltinTool::MessageSend)]);
    let (left, right) = tokio::join!(f.p.save_company_agent(left), f.p.save_company_agent(right));
    let winner = match (left, right) {
        (Ok(saved), Err(AppError::Conflict(_))) | (Err(AppError::Conflict(_)), Ok(saved)) => saved,
        outcomes => panic!("one atomic winner required: {outcomes:?}"),
    };
    assert_eq!(winner.draft_revision.get(), expected.get() + 1);
    let stored =
        f.p.load_capabilities(f.scope.company.as_uuid(), f.scope.agent.as_uuid())
            .await
            .unwrap()
            .unwrap();
    assert_eq!(stored.agent, winner);
    let tools = bindings(&f).await;
    assert_eq!(tools.len(), 1);
    if winner.name == "Alpha" {
        assert_eq!(stored.skills[0].id, alpha.id);
        assert_eq!(tools[0].alias.as_str(), "alpha.reply");
        assert_eq!(winner.system_prompt.as_deref(), Some("Alpha instructions"));
    } else {
        assert_eq!(winner.name, "Beta");
        assert_eq!(stored.skills[0].id, beta.id);
        assert_eq!(tools[0].alias.as_str(), "beta.send");
        assert_eq!(winner.system_prompt.as_deref(), Some("Beta instructions"));
    }
    assert!(f.p.active_agent_revision(f.scope).await.unwrap().is_none());
    let identities: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_capability_bindings WHERE company_id=$1 AND agent_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.agent.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(
        identities, 2,
        "initial identity plus only the winner; loser inserts nothing"
    );
}

#[tokio::test]
async fn autonomous_agent_authoring_save_competing_with_publication_never_activates_unsaved_content()
 {
    let f = Fixture::new().await;
    let original =
        f.p.publish_agent(f.request("original").await)
            .await
            .unwrap();
    let mut save = request(&f).await;
    save.write.system_prompt = Some("Later draft instructions".into());
    save.tools = Some(vec![]);
    let publish = f.request("race-with-save").await;
    let (saved, published) = tokio::join!(f.p.save_company_agent(save), f.p.publish_agent(publish));
    let saved = saved.unwrap();
    let active = f.p.active_agent_revision(f.scope).await.unwrap().unwrap();
    match published {
        Ok(published) => assert_eq!(active.id, published.id),
        Err(AppError::Conflict(_)) => assert_eq!(active.id, original.id),
        Err(error) => panic!("Unexpected publication failure: {error}"),
    }
    assert_eq!(
        active.definition.instructions,
        original.definition.instructions
    );
    assert_eq!(active.draft_revision.get() + 1, saved.draft_revision.get());
    assert!(bindings(&f).await.is_empty());
    let live:bool=sqlx::query_scalar("SELECT revoked_at IS NULL FROM agent_binding_grants WHERE company_id=$1 AND agent_id=$2 AND binding_id=$3")
        .bind(f.scope.company.as_uuid()).bind(f.scope.agent.as_uuid()).bind(f.binding.id.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert!(live, "a draft save cannot revoke published access");
}

#[tokio::test]
async fn autonomous_agent_authoring_final_metadata_conflict_rolls_back_skill_and_binding_changes() {
    let f = Fixture::new().await;
    let skill = f.skill(vec![]).await;
    let published =
        f.p.publish_agent(f.request("before-failure").await)
            .await
            .unwrap();
    AgentPersistence::create(
        &f.p,
        f.scope.company.as_uuid(),
        AgentWrite {
            name: "Taken".into(),
            slug: "taken".into(),
            ..write()
        },
    )
    .await
    .unwrap();
    let before = request(&f).await;
    let before_bindings = bindings(&f).await;
    let mut rejected = request(&f).await;
    rejected.write.slug = "taken".into();
    rejected.write.name = "Must roll back".into();
    rejected.write.skill_ids = vec![];
    rejected.tools = Some(vec![tool("changed.send", AgentBuiltinTool::MessageSend)]);
    assert!(matches!(
        f.p.save_company_agent(rejected).await,
        Err(AppError::Conflict(_))
    ));
    let after = request(&f).await;
    assert_eq!(after.expected_revision, before.expected_revision);
    assert_eq!(after.write.name, before.write.name);
    assert_eq!(after.write.skill_ids, vec![skill.id]);
    assert_eq!(bindings(&f).await, before_bindings);
    assert_eq!(
        f.p.active_agent_revision(f.scope)
            .await
            .unwrap()
            .unwrap()
            .id,
        published.id
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_capability_bindings WHERE company_id=$1 AND agent_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.agent.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(count, 1, "failed source insert was rolled back too");
}

#[tokio::test]
async fn autonomous_agent_authoring_refuses_builder_spoofing_reserved_aliases_and_foreign_scope() {
    let f = Fixture::new().await;
    let other = Fixture::new().await;
    let before = request(&f).await.expected_revision;
    let mut forged = request(&f).await;
    forged.write.name = "Agent Builder".into();
    forged.write.slug = "agent-builder".into();
    forged.tools = Some(vec![tool(
        "create_agent_channel",
        AgentBuiltinTool::CreateAgentChannel,
    )]);
    assert!(matches!(
        f.p.save_company_agent(forged).await,
        Err(AppError::BadRequest(_))
    ));
    let mut reserved = request(&f).await;
    reserved.tools = Some(vec![tool("message.reply", AgentBuiltinTool::MessageSend)]);
    assert!(matches!(
        f.p.save_company_agent(reserved).await,
        Err(AppError::BadRequest(_))
    ));
    let mut foreign = request(&f).await;
    foreign.scope.agent = other.scope.agent;
    assert!(matches!(
        f.p.save_company_agent(foreign).await,
        Err(AppError::NotFound(_))
    ));
    let mut outsider = request(&f).await;
    outsider.scope.actor = other.scope.actor;
    assert!(matches!(
        f.p.save_company_agent(outsider).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(request(&f).await.expected_revision, before);
    assert_eq!(bindings(&f).await, vec![f.binding]);
}
