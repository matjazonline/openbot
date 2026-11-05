use super::*;

#[tokio::test]
async fn autonomous_agent_storage_revision_round_trip_seals_members_and_active_owner() {
    let f = AdmissionFixture::new().await;
    let first = AgentToolBinding::builtin(AgentBuiltinTool::SearchKnowledge);
    let extra = AgentToolBinding::builtin(AgentBuiltinTool::MessageReply);
    let agent = new_agent(&f, vec![first.clone(), extra.clone()]).await;
    let value = definition(&f, agent.id, vec![first]);
    let revision = save_revision(&f, &value).await;
    let content: String = sqlx::query_scalar(
        "SELECT content FROM agent_revisions WHERE company_id=$1 AND agent_id=$2 AND id=$3",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(agent.id)
    .bind(revision)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(
        ResolvedAgentDefinition::decode(content.as_bytes())
            .unwrap()
            .content_hash()
            .unwrap(),
        value.content_hash().unwrap()
    );
    for change in [
        "UPDATE agent_revisions SET content=content WHERE id=$1",
        "DELETE FROM agent_revisions WHERE id=$1",
    ] {
        native_rejection(
            sqlx::query(change)
                .bind(revision)
                .execute(f.persistence().pool())
                .await
                .unwrap_err(),
        );
    }
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO agent_revision_bindings(company_id,agent_id,revision_id,binding_id,position) VALUES($1,$2,$3,$4,1)")
        .bind(f.binding.target.company.as_uuid()).bind(agent.id).bind(revision).bind(extra.id.as_uuid()).execute(&mut *tx).await.unwrap();
    native_rejection(tx.commit().await.unwrap_err());
    let other = new_agent(&f, Vec::new()).await;
    native_rejection(
        sqlx::query(
            "INSERT INTO agent_active_revisions(company_id,agent_id,revision_id) VALUES($1,$2,$3)",
        )
        .bind(f.binding.target.company.as_uuid())
        .bind(other.id)
        .bind(revision)
        .execute(f.persistence().pool())
        .await
        .unwrap_err(),
    );
    sqlx::query(
        "INSERT INTO agent_active_revisions(company_id,agent_id,revision_id) VALUES($1,$2,$3)",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(agent.id)
    .bind(revision)
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let mut incomplete = f.persistence().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO agent_revisions(company_id,agent_id,id,draft_revision,content,digest) VALUES($1,$2,$3,1,$4,$5)")
        .bind(f.binding.target.company.as_uuid()).bind(agent.id).bind(Uuid::new_v4()).bind(content).bind(value.content_hash().unwrap().to_vec())
        .execute(&mut *incomplete).await.unwrap();
    native_rejection(incomplete.commit().await.unwrap_err());
}

#[tokio::test]
async fn autonomous_agent_storage_skill_incarnations_and_sealed_revision_assignments() {
    let f = AdmissionFixture::new().await;
    let skill = add_skill(&f).await;
    let extra = add_skill(&f).await;
    let agent = new_agent(&f, Vec::new()).await;
    let write = AgentWrite {
        name: agent.name.clone(),
        slug: agent.slug.clone(),
        skill_ids: vec![skill.id, extra.id],
        ..Default::default()
    };
    AgentPersistence::update(f.persistence(), agent.id, write.clone())
        .await
        .unwrap();
    let assigned: Vec<Uuid> = sqlx::query_scalar(
        "SELECT assignment_id FROM agent_skills WHERE agent_id=$1 ORDER BY position",
    )
    .bind(agent.id)
    .fetch_all(f.persistence().pool())
    .await
    .unwrap();
    AgentPersistence::update(f.persistence(), agent.id, write.clone())
        .await
        .unwrap();
    let retained: Vec<Uuid> = sqlx::query_scalar(
        "SELECT assignment_id FROM agent_skills WHERE agent_id=$1 ORDER BY position",
    )
    .bind(agent.id)
    .fetch_all(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(assigned, retained);
    let mut value = definition(&f, agent.id, Vec::new());
    value.skills = vec![ResolvedAgentSkill {
        assignment_id: AgentSkillAssignmentId::new(assigned[0]),
        skill: SkillSnapshot {
            company_id: f.binding.target.company,
            skill_id: skill.id,
            name: TypeName::parse(skill.slug.as_str()).unwrap(),
            instructions: skill.instructions,
            required_tools: Vec::new(),
        },
    }];
    let revision = save_revision(&f, &value).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO agent_revision_skills(company_id,agent_id,revision_id,assignment_id,skill_id,position) VALUES($1,$2,$3,$4,$5,1)")
        .bind(f.binding.target.company.as_uuid()).bind(agent.id).bind(revision).bind(assigned[1]).bind(extra.id).execute(&mut *tx).await.unwrap();
    native_rejection(tx.commit().await.unwrap_err());
    sqlx::query("INSERT INTO agent_skill_grants(company_id,agent_id,assignment_id,skill_id) VALUES($1,$2,$3,$4)")
        .bind(f.binding.target.company.as_uuid()).bind(agent.id).bind(assigned[0]).bind(skill.id).execute(f.persistence().pool()).await.unwrap();
    let mut detached = write.clone();
    detached.skill_ids.clear();
    AgentPersistence::update(f.persistence(), agent.id, detached)
        .await
        .unwrap();
    let live: bool =
        sqlx::query_scalar("SELECT revoked_at IS NULL FROM agent_skill_grants WHERE agent_id=$1")
            .bind(agent.id)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert!(live);
    AgentPersistence::update(f.persistence(), agent.id, write)
        .await
        .unwrap();
    let replaced: Vec<Uuid> = sqlx::query_scalar(
        "SELECT assignment_id FROM agent_skills WHERE agent_id=$1 ORDER BY position",
    )
    .bind(agent.id)
    .fetch_all(f.persistence().pool())
    .await
    .unwrap();
    assert_ne!(assigned, replaced);
}
