use super::*;
use crate::application::agent::AgentGrantIdentity;

#[tokio::test]
async fn autonomous_agent_e2e_draft_skill_detach_keeps_selection_until_exact_revocation() {
    let http = scripted_scenario(vec![ScriptedExchange::new(
        |_| Ok(()),
        ScriptedResponse::json(json!({"ok":true})),
    )])
    .await;
    let mcp = mcp_server(Call::Json).await;
    let j = Box::pin(Journey::new(&mcp, &http.base_url)).await;
    let published = j.publish().await;
    let workflow = j.workflow().await;
    let first = j.admit(&workflow).await;
    j.save_draft(|request| request.write.skill_ids.clear())
        .await;
    let mut llm = scripted_llm(turns(1)).await;
    register_scripted_agent_base_url(j.scope.agent.as_uuid(), &llm.base_url);
    j.drive(&mcp, &[first]).await;
    j.assert_selection(first, &workflow, &published).await;
    let second = j.admit(&workflow).await;
    let grant = AgentGrantIdentity::Skill(published.definition.skills[0].assignment_id);
    j.f.persistence
        .revoke_agent_grant(j.scope, grant)
        .await
        .unwrap();
    j.process_pending(&mcp, &[second]).await;
    assert_eq!(j.run_state(second).await, "failed");
    let used: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM model_usage_calls WHERE company_id=$1 AND run_id=$2",
    )
    .bind(j.scope.company.as_uuid())
    .bind(second.as_uuid())
    .fetch_one(j.f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(
        used, 0,
        "the exact revoked assignment refuses a fresh model entry"
    );
    let observed = llm.observed();
    assert_eq!(observed.len(), 3);
    assert!(
        observed
            .iter()
            .all(|r| r["messages"].to_string().contains(SKILL_V1))
    );
    assert_eq!(mcp.calls(), 1);
    assert_eq!(http.finish().await, Ok(1));
    assert_eq!(llm.finish().await.unwrap(), 3);
}

#[tokio::test]
async fn autonomous_agent_e2e_model_removal_after_admission_refuses_before_all_peers() {
    let http = scripted_scenario(vec![]).await;
    let mcp = mcp_server(Call::Json).await;
    let j = Box::pin(Journey::new(&mcp, &http.base_url)).await;
    let published = j.publish().await;
    let workflow = j.workflow().await;
    let run = j.admit(&workflow).await;
    remove_selected_model(&j).await;
    let llm = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(j.scope.agent.as_uuid(), &llm.base_url);
    j.process_pending(&mcp, &[run]).await;
    assert_eq!(j.run_state(run).await, "failed");
    let view =
        j.f.persistence
            .run_view(RunReadScope {
                company: j.scope.company,
                run,
                viewer: j.scope.actor,
            })
            .await
            .unwrap()
            .unwrap();
    assert_eq!(
        view.dependencies.unwrap().agents[0].revision_id,
        published.id
    );
    assert!(view.calls.items.is_empty());
    assert_eq!(mcp.calls(), 0);
    assert!(mcp.log().is_empty());
    assert_eq!(http.finish().await, Ok(0));
    assert_eq!(llm.finish().await.unwrap(), 0);
}

#[tokio::test]
async fn autonomous_agent_e2e_invalid_required_alias_draft_cannot_replace_active_capabilities() {
    let http = scripted_scenario(
        (0..2)
            .map(|_| ScriptedExchange::new(|_| Ok(()), ScriptedResponse::json(json!({"ok":true}))))
            .collect(),
    )
    .await;
    let mcp = mcp_server(Call::Json).await;
    let j = Box::pin(Journey::new(&mcp, &http.base_url)).await;
    let published = j.publish().await;
    let workflow = j.workflow().await;
    let first = j.admit(&workflow).await;
    j.save_draft(|request| {
        request
            .tools
            .as_mut()
            .unwrap()
            .retain(|t| matches!(t.choice, AgentToolChoice::Mcp { .. }))
    })
    .await;
    let error =
        j.f.persistence
            .publish_agent(j.publication_request("invalid-missing-alias").await)
            .await
            .err()
            .expect("missing required HTTP alias");
    assert!(matches!(error, AppError::BadRequest(_)));
    assert!(error.to_string().contains("registered.request"));
    let second = j.admit(&workflow).await;
    let llm = scripted_llm(turns(2)).await;
    register_scripted_agent_base_url(j.scope.agent.as_uuid(), &llm.base_url);
    j.drive(&mcp, &[first, second]).await;
    j.assert_selection(first, &workflow, &published).await;
    j.assert_selection(second, &workflow, &published).await;
    assert_eq!(
        j.f.persistence
            .authoring_state(j.scope)
            .await
            .unwrap()
            .active
            .unwrap()
            .id,
        published.id
    );
    assert_eq!(mcp.calls(), 2);
    assert_eq!(http.finish().await, Ok(2));
    assert_eq!(llm.finish().await.unwrap(), 6);
}

async fn remove_selected_model(j: &Journey) {
    let p = &j.f.persistence;
    let configuration = |models: Vec<crate::entities::value_objects::ModelName>| {
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: None,
            models,
            is_default: true,
        }]
    };
    CompanyPersistence::replace_model_connections_for_user(
        p,
        j.scope.actor.user_id(),
        j.scope.company.as_uuid(),
        configuration(vec![SCRIPTED_MODEL.into(), "other-model".into()]),
    )
    .await
    .unwrap();
    j.save_draft(|request| request.write.model = Some("other-model".into()))
        .await;
    CompanyPersistence::replace_model_connections_for_user(
        p,
        j.scope.actor.user_id(),
        j.scope.company.as_uuid(),
        configuration(vec!["other-model".into()]),
    )
    .await
    .unwrap();
}
