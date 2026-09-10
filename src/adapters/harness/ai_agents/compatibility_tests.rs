//! Deterministic characterization of the pinned executable skill semantics.
use super::*;
use crate::entities::{
    creation::CreationProvenance,
    harness::{AgentCapabilitySpec, HarnessConfig, SubAgentScope},
    skill::{Skill, SkillInstruction},
};
use crate::services::test_support::{
    LlmTurn, ScriptedExchange, ScriptedResponse, scripted_scenario,
};
use serde_json::json;
use uuid::Uuid;

fn skill() -> Skill {
    Skill {
        id: Uuid::new_v4(), company_id: None, slug: "ordered-proof".into(),
        name: "Ordered proof".into(), description: "Execute two dependent echo steps".into(),
        trigger: "Review a record".into(), created_by: CreationProvenance::system(),
        created_at: chrono::Utc::now(), updated_at: chrono::Utc::now(),
        instructions: vec![
            SkillInstruction::Tool { tool: "echo".into(), args: Some(json!({"message":"first"})), output_as: Some("named".into()) },
            SkillInstruction::Tool { tool: "echo".into(), args: Some(json!({"message":"{{ steps[0].result.message }}-second"})), output_as: None },
            SkillInstruction::Prompt { text: "Ordered={{ steps[1].result.message }}; argument={{ steps[1].args.message }}; alias={{ named|default('missing') }}".into() },
        ],
    }
}

#[tokio::test]
async fn ordered_skill_steps_render_prior_results_and_characterize_ignored_output_alias() {
    let llm = scripted_scenario(vec![
        ScriptedExchange::new(
            |request| {
                if !request.body.to_string().contains("ordered-proof") {
                    return Err("router must see attached skill");
                }
                Ok(())
            },
            ScriptedResponse::turn(LlmTurn::text("ordered-proof"), 0),
        ),
        ScriptedExchange::new(
            |request| {
                if !request
                    .body
                    .to_string()
                    .contains("Ordered=first-second; argument=first-second; alias=missing")
                {
                    return Err("ordered skill results and pinned alias behavior");
                }
                Ok(())
            },
            ScriptedResponse::turn(LlmTurn::text("Ordered steps verified."), 1),
        ),
    ])
    .await;
    let spec = AgentCapabilitySpec {
        response_contract: None,
        harness: HarnessKind::AiAgents,
        name: "Compatibility proof".into(),
        system_prompt: "Follow the selected skill.".into(),
        provider: "openai".into(),
        model: "gpt-4o-mini".into(),
        provider_base_url: Some(llm.base_url.clone()),
        skills: vec![skill()],
        granted_tools: vec![],
        sub_agents: SubAgentScope::AllCompanySiblings,
        harness_config: HarnessConfig::empty(HarnessKind::AiAgents),
    };
    // The executable skill contributes its own tool grant; the model only chooses the skill.
    let result = Box::pin(AiAgentsHarness::new().run(AgentRun {
        company_id: None,
        execution: None,
        deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(300),
        spec: Box::new(spec),
        agent_id: Uuid::new_v4(),
        api_key: "fixture-key",
        full_prompt: "Review the record.",
        history_message_count: 0,
        recipient_role: None,
        approvals: None,
        tool_host: None,
        trace: None,
    }))
    .await;
    assert_eq!(llm.finish().await, Ok(2));
    assert_eq!(result.unwrap().content, "Ordered steps verified.");
}
