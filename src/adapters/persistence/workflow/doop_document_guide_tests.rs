use super::*;

#[test]
fn doop_agent_pinned_guide_skills_preserve_the_full_capture_with_existing_bounds() {
    let guide = captured("guide");
    let text = guide["content"][0]["text"].as_str().unwrap();
    assert_eq!(GUIDES.concat(), text);
    assert!(text.chars().count() > crate::application::workflow::registry::MAX_TEXT);
    assert!(
        guide.to_string().len() < 65_536,
        "per-block bound, not aggregate"
    );
    for part in GUIDES {
        assert!(part.len() <= crate::entities::skill::MAX_SKILL_INSTRUCTIONS_BYTES);
    }
    assert_eq!(editor_tools().len(), 5);
}

#[tokio::test]
async fn doop_agent_actual_guide_text_bound_preserves_unknown_without_reissue() {
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let mut guide = captured("guide");
    guide["isError"] = json!(false);
    setup.server.set_result(guide);
    let claim = claimed(&setup).await;
    let runtime = setup.runtime();
    for _ in 0..2 {
        let failure = runtime
            .run(&kind(), &claim)
            .await
            .completed_result()
            .unwrap_err();
        assert_eq!(code(&failure), "mcp.unresolved");
    }
    assert_eq!(setup.server.calls(), 1);
    assert_eq!(
        action_counts(&setup.fixture.persistence).await,
        counts(1, 1, 0)
    );
}
