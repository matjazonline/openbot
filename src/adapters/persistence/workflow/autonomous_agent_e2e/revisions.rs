use super::*;

#[tokio::test]
async fn autonomous_agent_e2e_mixed_tools_skills_two_workflows_and_queued_revisions() {
    let http = scripted_scenario(
        (0..3)
            .map(|_| {
                ScriptedExchange::new(
                    |request| {
                        if request.method == "POST" && request.path == "/record" {
                            Ok(())
                        } else {
                            Err("mixed HTTP method/path")
                        }
                    },
                    ScriptedResponse::json(json!({"ok":true})),
                )
            })
            .collect(),
    )
    .await;
    let mcp = mcp_server(Call::Json).await;
    let j = Box::pin(Journey::new(&mcp, &http.base_url)).await;
    let v1 = j.publish().await;
    let first = j.workflow().await;
    let unrelated = j.workflow().await;
    assert_ne!(first.version, unrelated.version);
    let a = j.admit(&first).await;
    j.edit("Unpublished draft must not reach queued models.")
        .await;
    j.edit_skill(SKILL_V2).await;
    let still_v1 = j.admit(&unrelated).await;
    j.edit(V2).await;
    let v2 = j.publish().await;
    assert_ne!(v1.id, v2.id);
    let b = j.admit(&first).await;
    j.edit("Later unpublished draft must not reach any model.")
        .await;
    let mut llm = scripted_llm(turns(3)).await;
    register_scripted_agent_base_url(j.scope.agent.as_uuid(), &llm.base_url);
    j.drive(&mcp, &[a, still_v1, b]).await;
    j.assert_selection(a, &first, &v1).await;
    j.assert_selection(still_v1, &unrelated, &v1).await;
    j.assert_selection(b, &first, &v2).await;
    let observed = llm.observed();
    assert_eq!(observed.len(), 9);
    for (instructions, skill, count) in [(V1, SKILL_V1, 6), (V2, SKILL_V2, 3)] {
        assert_eq!(
            observed
                .iter()
                .filter(|r| {
                    let messages = r["messages"].to_string();
                    r["model"] == SCRIPTED_MODEL
                        && messages.contains(instructions)
                        && messages.contains(skill)
                })
                .count(),
            count
        );
    }
    let serialized = serde_json::to_string(&observed).unwrap();
    assert!(!serialized.contains("Unpublished") && !serialized.contains("unpublished"));
    assert!(!serialized.contains(TOKEN) && !serialized.contains("fixture-http-secret"));
    assert_eq!(v1.definition.skills[0].skill.instructions, SKILL_V1);
    assert_eq!(v2.definition.skills[0].skill.instructions, SKILL_V2);
    assert_eq!(mcp.calls(), 3);
    assert_eq!(http.finish().await, Ok(3));
    assert_eq!(llm.finish().await.unwrap(), 9);
}
