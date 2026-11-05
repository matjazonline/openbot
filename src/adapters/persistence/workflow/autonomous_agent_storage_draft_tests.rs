use super::*;

#[tokio::test]
async fn autonomous_agent_storage_draft_round_trip_order_and_live_grants_are_independent() {
    let f = AdmissionFixture::new().await;
    let first = AgentToolBinding::builtin(AgentBuiltinTool::SearchKnowledge);
    let second = AgentToolBinding::builtin(AgentBuiltinTool::MessageReply);
    let agent = new_agent(&f, vec![first.clone(), second.clone()]).await;
    assert_eq!(
        bindings(&f, agent.id).await,
        vec![first.clone(), second.clone()]
    );
    sqlx::query(
        "INSERT INTO agent_binding_grants(company_id,agent_id,binding_id) VALUES($1,$2,$3)",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(agent.id)
    .bind(first.id.as_uuid())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let mut write = AgentWrite {
        name: "Changed draft".into(),
        slug: agent.slug,
        system_prompt: Some("Changed instructions".into()),
        tool_bindings: Some(vec![second.clone(), first.clone()]),
        ..Default::default()
    };
    AgentPersistence::update(f.persistence(), agent.id, write.clone())
        .await
        .unwrap();
    assert_eq!(bindings(&f, agent.id).await, vec![second, first]);
    write.tool_bindings = Some(Vec::new());
    AgentPersistence::update(f.persistence(), agent.id, write)
        .await
        .unwrap();
    assert!(bindings(&f, agent.id).await.is_empty());
    let live: bool =
        sqlx::query_scalar("SELECT revoked_at IS NULL FROM agent_binding_grants WHERE agent_id=$1")
            .bind(agent.id)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert!(live, "draft removal must not change live authority");
    let revision: i64 = sqlx::query_scalar("SELECT draft_revision FROM agents WHERE id=$1")
        .bind(agent.id)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(revision, 3);
    let loaded = AgentPersistence::get_by_id(f.persistence(), agent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.draft_revision.get(), 3);
    assert_eq!(
        loaded.system_prompt.as_deref(),
        Some("Changed instructions")
    );
}

#[tokio::test]
async fn autonomous_agent_storage_competing_writers_cannot_retarget_a_binding_identity() {
    let f = AdmissionFixture::new().await;
    let agent = new_agent(&f, Vec::new()).await;
    let mut left = AgentToolBinding::builtin(AgentBuiltinTool::SearchKnowledge);
    left.alias = AgentToolAlias::parse("lookup").unwrap();
    let mut right = left.clone();
    right.source = AgentBindingSource::Builtin {
        tool: AgentBuiltinTool::MessageReply,
    };
    let write = |binding| AgentWrite {
        name: "Competing edit".into(),
        slug: agent.slug.clone(),
        tool_bindings: Some(vec![binding]),
        ..Default::default()
    };
    let (one, two) = tokio::join!(
        AgentPersistence::update(f.persistence(), agent.id, write(left.clone())),
        AgentPersistence::update(f.persistence(), agent.id, write(right.clone()))
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    assert!(matches!(
        one.as_ref().err().or(two.as_ref().err()),
        Some(AppError::Conflict(_))
    ));
    let saved = bindings(&f, agent.id).await;
    assert_eq!(saved, vec![if one.is_ok() { left } else { right }]);
    native_rejection(
        sqlx::query("UPDATE agent_capability_bindings SET alias='redirect' WHERE agent_id=$1")
            .bind(agent.id)
            .execute(f.persistence().pool())
            .await
            .unwrap_err(),
    );
}

#[tokio::test]
async fn autonomous_agent_storage_native_scope_kind_and_reserved_alias_constraints() {
    let f = AdmissionFixture::new().await;
    let foreign = f.foreign().await;
    let agent = new_agent(&f, Vec::new()).await;
    let foreign_agent = new_agent(&foreign, Vec::new()).await;
    native_rejection(sqlx::query("INSERT INTO agent_capability_bindings(company_id,agent_id,id,alias,kind,builtin) VALUES($1,$2,$3,'lookup','builtin','search_knowledge')")
        .bind(f.binding.target.company.as_uuid()).bind(foreign_agent.id).bind(Uuid::new_v4()).execute(f.persistence().pool()).await.unwrap_err());
    native_rejection(sqlx::query("INSERT INTO agent_capability_bindings(company_id,agent_id,id,alias,kind,builtin) VALUES($1,$2,$3,'message.send','builtin','search_knowledge')")
        .bind(f.binding.target.company.as_uuid()).bind(agent.id).bind(Uuid::new_v4()).execute(f.persistence().pool()).await.unwrap_err());
    let connection = Uuid::new_v4();
    native_mcp(&f, connection).await;
    for (company, owner) in [
        (f.binding.target.company, agent.id),
        (foreign.binding.target.company, foreign_agent.id),
    ] {
        native_rejection(sqlx::query("INSERT INTO agent_capability_bindings(company_id,agent_id,id,alias,kind,http_connection_id,operation_name) VALUES($1,$2,$3,'lookup','http',$4,'lookup')")
            .bind(company.as_uuid()).bind(owner).bind(Uuid::new_v4()).bind(connection).execute(f.persistence().pool()).await.unwrap_err());
    }
    // NULL in a discriminator arm must fail the native CHECK rather than pass SQL's UNKNOWN.
    native_rejection(sqlx::query("INSERT INTO agent_capability_bindings(company_id,agent_id,id,alias,kind,http_connection_id) VALUES($1,$2,$3,'lookup','builtin',$4)")
        .bind(f.binding.target.company.as_uuid()).bind(agent.id).bind(Uuid::new_v4()).bind(connection).execute(f.persistence().pool()).await.unwrap_err());
}
