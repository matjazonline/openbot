use super::*;
use crate::entities::{agent::Agent, skill::Skill};
use crate::use_cases::skill::SkillManagementPersistence;

struct SkillHistory {
    f: AdmissionFixture,
    agent: Agent,
    skill: Skill,
    assignment: Uuid,
    revision: Uuid,
    content: String,
}
impl SkillHistory {
    async fn new() -> Self {
        let f = AdmissionFixture::new().await;
        let skill = add_skill(&f).await;
        let agent = new_agent(&f, Vec::new()).await;
        AgentPersistence::update(
            f.persistence(),
            agent.id,
            AgentWrite {
                name: agent.name.clone(),
                slug: agent.slug.clone(),
                provider: agent.provider.clone(),
                model: agent.model.clone(),
                system_prompt: agent.system_prompt.clone(),
                skill_ids: vec![skill.id],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let assignment: Uuid =
            sqlx::query_scalar("SELECT assignment_id FROM agent_skills WHERE agent_id=$1")
                .bind(agent.id)
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
        let mut value = definition(&f, agent.id, Vec::new());
        value.skills.push(ResolvedAgentSkill {
            assignment_id: AgentSkillAssignmentId::new(assignment),
            skill: SkillSnapshot {
                description: "Skill instructions".into(),
                files: Vec::new(),
                company_id: f.binding.target.company,
                skill_id: skill.id,
                name: TypeName::parse(skill.slug.as_str()).unwrap(),
                instructions: skill.instructions.clone(),
                required_tools: Vec::new(),
            },
        });
        let revision = save_revision(&f, &value).await;
        let content = String::from_utf8(value.canonical_content().unwrap()).unwrap();
        sqlx::query("INSERT INTO agent_skill_grants(company_id,agent_id,assignment_id,skill_id) VALUES($1,$2,$3,$4)")
            .bind(f.binding.target.company.as_uuid()).bind(agent.id).bind(assignment).bind(skill.id)
            .execute(f.persistence().pool()).await.unwrap();
        Self {
            f,
            agent,
            skill,
            assignment,
            revision,
            content,
        }
    }
    async fn current(&self, db: &mut sqlx::PgConnection) -> Option<Uuid> {
        sqlx::query_scalar("SELECT live_skill_id FROM agent_skill_assignments WHERE company_id=$1 AND agent_id=$2 AND id=$3 FOR SHARE")
            .bind(self.f.binding.target.company.as_uuid()).bind(self.agent.id).bind(self.assignment).fetch_one(db).await.unwrap()
    }
    async fn assert_retired(&self) {
        let p = self.f.persistence();
        assert_eq!(
            self.current(&mut p.pool().acquire().await.unwrap()).await,
            None
        );
        let live: bool = sqlx::query_scalar(
            "SELECT revoked_at IS NULL FROM agent_skill_grants WHERE assignment_id=$1",
        )
        .bind(self.assignment)
        .fetch_one(p.pool())
        .await
        .unwrap();
        assert!(!live);
        let saved: String = sqlx::query_scalar("SELECT content FROM agent_revisions WHERE id=$1")
            .bind(self.revision)
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert_eq!(saved, self.content);
        let restored = ResolvedAgentDefinition::decode(saved.as_bytes()).unwrap();
        assert_eq!(
            restored.skills[0].skill.instructions,
            self.skill.instructions
        );
        assert_eq!(restored.skills[0].assignment_id.as_uuid(), self.assignment);
        let remaining: i64 =
            sqlx::query_scalar("SELECT count(*) FROM agent_skills WHERE agent_id=$1")
                .bind(self.agent.id)
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert_eq!(remaining, 0);
    }
    async fn assert_retired_update_rejected(&self, fresh: Uuid) {
        let p = self.f.persistence();
        let mut update = p.pool().begin().await.unwrap();
        sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
            .execute(&mut *update)
            .await
            .unwrap();
        let error = sqlx::query("UPDATE agent_skills SET assignment_id=$2 WHERE agent_id=$1")
            .bind(self.agent.id)
            .bind(self.assignment)
            .execute(&mut *update)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        update.rollback().await.unwrap();
        let retained: Uuid =
            sqlx::query_scalar("SELECT assignment_id FROM agent_skills WHERE agent_id=$1")
                .bind(self.agent.id)
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert_eq!(
            retained, fresh,
            "failed UPDATE must preserve the current live incarnation"
        );
        sqlx::query("UPDATE agent_skills SET position=1 WHERE agent_id=$1")
            .bind(self.agent.id)
            .execute(p.pool())
            .await
            .unwrap();
        let reordered: Uuid = sqlx::query_scalar(
            "SELECT assignment_id FROM agent_skills WHERE agent_id=$1 AND position=1",
        )
        .bind(self.agent.id)
        .fetch_one(p.pool())
        .await
        .unwrap();
        assert_eq!(
            reordered, fresh,
            "valid ordering updates retain their live identity"
        );
    }
}

#[tokio::test]
async fn autonomous_agent_storage_skill_delete_preserves_history_and_cannot_revive_assignment() {
    let saved = SkillHistory::new().await;
    let p = saved.f.persistence();
    p.delete_company(saved.f.binding.target.company.as_uuid(), saved.skill.id)
        .await
        .unwrap();
    saved.assert_retired().await;
    let replacement = add_skill(&saved.f).await;
    // Reusing a deleted resource UUID cannot reconnect the old immutable assignment.
    sqlx::query("UPDATE skills SET id=$2,instructions='Replacement shared text.' WHERE id=$1")
        .bind(replacement.id)
        .bind(saved.skill.id)
        .execute(p.pool())
        .await
        .unwrap();
    native_rejection(
        sqlx::query("UPDATE agent_skill_assignments SET live_skill_id=skill_id WHERE id=$1")
            .bind(saved.assignment)
            .execute(p.pool())
            .await
            .unwrap_err(),
    );
    native_rejection(sqlx::query("INSERT INTO agent_skills(company_id,agent_id,skill_id,assignment_id,position) VALUES($1,$2,$3,$4,0)")
        .bind(saved.f.binding.target.company.as_uuid()).bind(saved.agent.id).bind(saved.skill.id).bind(saved.assignment)
        .execute(p.pool()).await.unwrap_err());
    AgentPersistence::update(
        p,
        saved.agent.id,
        AgentWrite {
            name: saved.agent.name.clone(),
            slug: saved.agent.slug.clone(),
            skill_ids: vec![saved.skill.id],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let fresh: Uuid =
        sqlx::query_scalar("SELECT assignment_id FROM agent_skills WHERE agent_id=$1")
            .bind(saved.agent.id)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_ne!(fresh, saved.assignment);
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT live_skill_id FROM agent_skill_assignments WHERE id=$1")
            .bind(fresh)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(current, Some(saved.skill.id));
    saved.assert_retired_update_rejected(fresh).await;
    native_rejection(sqlx::query("INSERT INTO agent_skill_assignments(company_id,agent_id,id,skill_id) VALUES($1,$2,$3,$4)")
        .bind(saved.f.binding.target.company.as_uuid()).bind(saved.agent.id).bind(Uuid::new_v4()).bind(saved.skill.id)
        .execute(p.pool()).await.unwrap_err());
}

#[tokio::test]
async fn autonomous_agent_storage_skill_authority_reader_serializes_before_source_delete() {
    let saved = SkillHistory::new().await;
    let p = saved.f.persistence();
    let mut reader = p.pool().begin().await.unwrap();
    assert_eq!(saved.current(&mut reader).await, Some(saved.skill.id));
    let mut deleting = p.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout='100ms'")
        .execute(&mut *deleting)
        .await
        .unwrap();
    let error = sqlx::query("DELETE FROM skills WHERE company_id=$1 AND id=$2")
        .bind(saved.f.binding.target.company.as_uuid())
        .bind(saved.skill.id)
        .execute(&mut *deleting)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
    deleting.rollback().await.unwrap();
    reader.commit().await.unwrap();
    p.delete_company(saved.f.binding.target.company.as_uuid(), saved.skill.id)
        .await
        .unwrap();
    saved.assert_retired().await;
}

#[tokio::test]
async fn autonomous_agent_storage_skill_delete_serializes_before_authority_reader() {
    let saved = SkillHistory::new().await;
    let p = saved.f.persistence();
    let mut deleting = p.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM skills WHERE company_id=$1 AND id=$2")
        .bind(saved.f.binding.target.company.as_uuid())
        .bind(saved.skill.id)
        .execute(&mut *deleting)
        .await
        .unwrap();
    let mut reader = p.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout='100ms'")
        .execute(&mut *reader)
        .await
        .unwrap();
    let error = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT live_skill_id FROM agent_skill_assignments WHERE id=$1 FOR SHARE",
    )
    .bind(saved.assignment)
    .fetch_one(&mut *reader)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
    reader.rollback().await.unwrap();
    deleting.commit().await.unwrap();
    saved.assert_retired().await;
}
