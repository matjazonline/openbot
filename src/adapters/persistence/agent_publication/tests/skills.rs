use super::*;

#[tokio::test]
async fn autonomous_agent_skills_shared_edit_does_not_change_frozen_text_or_live_assignment() {
    let f = Fixture::new().await;
    let skill = f
        .skill(vec![TypeName::parse("message.reply").unwrap()])
        .await;
    let first =
        f.p.publish_agent(f.request("original").await)
            .await
            .unwrap();
    f.p.update_company(
        f.scope.company.as_uuid(),
        skill.id,
        SkillWrite {
            slug: skill.slug.as_str().into(),
            name: skill.name,
            description: skill.description,
            instructions: "Changed shared text.".into(),
            required_tools: Some(vec![TypeName::parse("message.reply").unwrap()]),
            created_by: None,
        },
    )
    .await
    .unwrap();
    let old =
        f.p.published_agent_revision(f.scope, first.id)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(old.digest, first.digest);
    assert_eq!(
        old.definition.skills[0].skill.instructions,
        "Original shared text."
    );
    let second = f.p.publish_agent(f.request("edited").await).await.unwrap();
    assert_eq!(
        second.definition.skills[0].skill.instructions,
        "Changed shared text."
    );
    assert_eq!(
        second.definition.skills[0].assignment_id,
        first.definition.skills[0].assignment_id
    );
    assert_ne!(second.digest, first.digest);
}

#[tokio::test]
async fn autonomous_agent_skills_missing_requirement_preserves_active_without_adding_grants() {
    let f = Fixture::new().await;
    let first =
        f.p.publish_agent(f.request("original").await)
            .await
            .unwrap();
    f.skill(vec![TypeName::parse("crm.lookup").unwrap()]).await;
    assert!(f.p.preview_agent(f.scope).await.is_err());
    assert!(
        f.p.publish_agent(f.request("missing-tool").await)
            .await
            .is_err()
    );
    let bindings: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_binding_grants WHERE company_id=$1 AND agent_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.agent.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(bindings, 1);
    assert_eq!(
        f.p.active_agent_revision(f.scope)
            .await
            .unwrap()
            .unwrap()
            .id,
        first.id
    );
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 1,
            requests: 1
        }
    );
}

#[tokio::test]
async fn autonomous_agent_skills_removal_reassignment_and_explicit_revoke_preserve_incarnations() {
    let f = Fixture::new().await;
    let skill = f.skill(vec![]).await;
    let first =
        f.p.publish_agent(f.request("original").await)
            .await
            .unwrap();
    let old = first.definition.skills[0].assignment_id;
    f.update(write()).await;
    f.p.publish_agent(f.request("removed").await).await.unwrap();
    f.update(AgentWrite {
        skill_ids: vec![skill.id],
        ..write()
    })
    .await;
    let second =
        f.p.publish_agent(f.request("reassigned").await)
            .await
            .unwrap();
    let fresh = second.definition.skills[0].assignment_id;
    assert_ne!(fresh, old);
    f.p.revoke_agent_grant(f.scope, AgentGrantIdentity::Skill(fresh))
        .await
        .unwrap();
    assert!(matches!(
        f.p.publish_agent(f.request("retired").await).await,
        Err(AppError::Conflict(_))
    ));
    let revoked: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_skill_grants WHERE company_id=$1 AND agent_id=$2 AND revoked_at IS NOT NULL")
        .bind(f.scope.company.as_uuid()).bind(f.scope.agent.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(revoked, 2);
    assert_eq!(
        f.p.published_agent_revision(f.scope, first.id)
            .await
            .unwrap()
            .unwrap()
            .digest,
        first.digest
    );
}
