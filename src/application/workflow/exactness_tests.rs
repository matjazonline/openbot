use super::*;
use crate::application::workflow::registry::TYPES;
use crate::domain::workflow::TypeName;
use crate::entities::agent_capability::AgentBuiltinTool;
use serde_json::json;
use uuid::Uuid;

/// Every built-in tool, through an exhaustive match so a new variant fails to compile here.
fn every_builtin() -> Vec<AgentBuiltinTool> {
    let all = [
        AgentBuiltinTool::MessageSend,
        AgentBuiltinTool::MessageReply,
        AgentBuiltinTool::ListCompanyAgents,
        AgentBuiltinTool::CreateAgentChannel,
        AgentBuiltinTool::SearchKnowledge,
        AgentBuiltinTool::SearchMemory,
        AgentBuiltinTool::MemoryTimeline,
        AgentBuiltinTool::MemoryDetails,
        AgentBuiltinTool::SaveMemory,
        AgentBuiltinTool::ReadEscalation,
        AgentBuiltinTool::RespondToEscalation,
    ];
    for tool in all {
        match tool {
            AgentBuiltinTool::MessageSend
            | AgentBuiltinTool::MessageReply
            | AgentBuiltinTool::ListCompanyAgents
            | AgentBuiltinTool::CreateAgentChannel
            | AgentBuiltinTool::SearchKnowledge
            | AgentBuiltinTool::SearchMemory
            | AgentBuiltinTool::MemoryTimeline
            | AgentBuiltinTool::MemoryDetails
            | AgentBuiltinTool::SaveMemory
            | AgentBuiltinTool::ReadEscalation
            | AgentBuiltinTool::RespondToEscalation => (),
        }
    }
    all.to_vec()
}

/// One value of every tool target, with its expected accounting.
fn every_target() -> Vec<(AgentToolTarget, ToolAccounting)> {
    let name = |text: &str| TypeName::parse(text).unwrap();
    let pin = serde_json::from_value(json!({
        "workflow_id": Uuid::nil(), "version_id": Uuid::nil(), "params": {}
    }));
    let mut targets = vec![
        (
            AgentToolTarget::Http {
                connection: Uuid::nil(),
            },
            ToolAccounting::Unaccounted,
        ),
        (
            AgentToolTarget::Mcp {
                connection: Uuid::nil(),
            },
            ToolAccounting::Unaccounted,
        ),
        (
            AgentToolTarget::Message {
                name: name("message.send"),
            },
            ToolAccounting::Receipt,
        ),
        (
            AgentToolTarget::Provisioning {
                name: name("list_company_agents"),
            },
            ToolAccounting::Configuration,
        ),
        (AgentToolTarget::Knowledge, ToolAccounting::RaisingRead),
        (
            AgentToolTarget::Escalation {
                tool: crate::application::workflow::escalation::tools::EscalationTool::Read,
            },
            ToolAccounting::Configuration,
        ),
        (
            AgentToolTarget::Escalation {
                tool: crate::application::workflow::escalation::tools::EscalationTool::Respond,
            },
            ToolAccounting::Configuration,
        ),
    ];
    for tool in crate::application::workflow::memory_tools::MemoryTool::ALL {
        let accounting = match tool {
            crate::application::workflow::memory_tools::MemoryTool::Save => ToolAccounting::Receipt,
            _ => ToolAccounting::RaisingRead,
        };
        targets.push((AgentToolTarget::Memory { tool }, accounting));
    }
    if let Ok(pin) = pin {
        targets.push((
            AgentToolTarget::Workflow { pin },
            ToolAccounting::ChildControl,
        ));
    }
    for (target, _) in &targets {
        match target {
            AgentToolTarget::Http { .. }
            | AgentToolTarget::Mcp { .. }
            | AgentToolTarget::Message { .. }
            | AgentToolTarget::Escalation { .. }
            | AgentToolTarget::Provisioning { .. }
            | AgentToolTarget::Knowledge
            | AgentToolTarget::Memory { .. }
            | AgentToolTarget::Workflow { .. } => (),
        }
    }
    targets
}

#[test]
fn memory_access_level_every_step_kind_and_tool_target_is_classified() {
    let expected = [
        ("context.history", StepAccounting::RaisingRead),
        ("memory.load", StepAccounting::RaisingRead),
        ("memory.save", StepAccounting::Receipt),
        ("ai.classify", StepAccounting::ModelStep),
        ("jev.evaluate", StepAccounting::Unaccounted),
        ("agent.run", StepAccounting::ModelStep),
        ("local_agent.run", StepAccounting::Unaccounted),
        ("decision.rule", StepAccounting::Pure),
        ("decision.human", StepAccounting::HumanDecision),
        ("decision.agent", StepAccounting::ModelStep),
        ("data.map", StepAccounting::Pure),
        ("data.url", StepAccounting::Pure),
        ("channel.no_reply", StepAccounting::Pure),
        ("http.request", StepAccounting::Unaccounted),
        ("tool.call", StepAccounting::Unaccounted),
        ("mcp.call", StepAccounting::Unaccounted),
        ("message.send", StepAccounting::Receipt),
        ("message.reply", StepAccounting::Receipt),
        ("message.ask", StepAccounting::HumanReply),
        ("workflow.call", StepAccounting::ChildControl),
        ("flow.repeat", StepAccounting::ChildControl),
        ("wait.event", StepAccounting::Unaccounted),
        ("wait.timer", StepAccounting::Pure),
    ];
    // Every registered kind is listed exactly once, so a new kind fails here until classified.
    assert_eq!(expected.len(), TYPES.len());
    for kind in TYPES {
        let (_, accounting) = expected
            .iter()
            .find(|(listed, _)| *listed == kind)
            .unwrap_or_else(|| panic!("{kind} is not classified"));
        assert_eq!(step_accounting(kind), *accounting, "{kind}");
        let lowers = matches!(
            accounting,
            StepAccounting::Unaccounted | StepAccounting::HumanReply
        );
        assert_eq!(step_output_exact(kind, &json!({})), !lowers, "{kind}");
    }
    assert!(!step_output_exact("future.kind", &json!({})));

    let targets = every_target();
    // Six single-sample variants, four memory tools, and both escalation operations.
    assert_eq!(
        targets.len(),
        12,
        "every AgentToolTarget variant has a sample"
    );
    for (target, accounting) in &targets {
        assert_eq!(tool_accounting(target), *accounting, "{target:?}");
        assert_eq!(
            tool_result_exact(Some(target)),
            *accounting != ToolAccounting::Unaccounted,
            "{target:?}"
        );
    }
    assert!(
        !tool_result_exact(None),
        "an unprepared tool is not trusted"
    );
    for tool in every_builtin() {
        let target = AgentToolTarget::builtin(tool).unwrap();
        assert!(tool_result_exact(Some(&target)), "{tool:?}");
    }
}

#[test]
fn memory_access_level_human_decision_choice_is_exact_and_free_text_is_not() {
    let exact = |output| step_output_exact("decision.human", &output);
    assert!(exact(json!({"choice":"approve","data":null,"feedback":""})));
    assert!(exact(json!({"choice":"approve","data":{},"feedback":"  "})));
    assert!(exact(json!({"choice":"timeout"})));
    assert!(!exact(
        json!({"choice":"approve","data":null,"feedback":"the code is 7731"})
    ));
    assert!(!exact(
        json!({"choice":"approve","data":{"note":"x"},"feedback":""})
    ));
    assert!(!exact(json!({"choice":"approve","data":[1],"feedback":""})));
    assert!(!exact(json!({"choice":"approve","feedback":3})));
    assert!(!step_output_exact("message.ask", &json!({"reply":"ok"})));
}
