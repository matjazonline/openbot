//! A scripted `channel.call` (gate AWC-P05-D2 r2, D3): live's launch checks over the frozen
//! target, then the author's fixture. V6 and V7.
use super::*;
use crate::application::agent::{
    ActionEffect, ActionRecovery, ApprovedActionPolicy, ModelSettings, ResolvedAgentDefinition,
    ResolvedAgentTool,
};
use crate::application::workflow::channel_call::{
    AUDIENCE_REFUSED, DESK_DISABLED, INTAKE_INVALID, INTAKE_NOT_DECLARED, INVALID_INPUT,
    NO_RESPONDER, REVIEW_REJECTED, TARGET_UNAVAILABLE, builtin_channel_call_snapshot,
};
use crate::entities::agent_capability::{
    AgentBindingId, AgentBindingSource, AgentBuiltinTool, AgentToolAlias, AgentToolBinding,
};

const DESK: Uuid = Uuid::from_u128(900);
const RESPONDER: Uuid = Uuid::from_u128(901);

fn call_tool() -> ResolvedAgentTool {
    let snapshot = builtin_channel_call_snapshot(company()).unwrap();
    ResolvedAgentTool {
        unit_id: None,
        binding: AgentToolBinding {
            id: AgentBindingId::new(Uuid::from_u128(990)),
            alias: AgentToolAlias::parse("channel.call").unwrap(),
            source: AgentBindingSource::Builtin {
                tool: AgentBuiltinTool::ChannelCall,
            },
        },
        input_schema: snapshot.contract.input_schema,
        output_schema: snapshot.contract.output_schema,
        policy: snapshot.policy,
        approval_required: false,
    }
}

fn http_tool() -> ResolvedAgentTool {
    ResolvedAgentTool {
        unit_id: Some(Uuid::nil()),
        binding: AgentToolBinding {
            id: AgentBindingId::new(Uuid::from_u128(991)),
            alias: AgentToolAlias::parse("crm.read").unwrap(),
            source: AgentBindingSource::Http {
                connection: Uuid::from_u128(7),
                operation: name("crm.read"),
            },
        },
        input_schema: json!({"type":"object"}),
        output_schema: json!({"type":"object"}),
        policy: ApprovedActionPolicy {
            capability: name("http.request"),
            policy_revision: 1,
            effect: ActionEffect::Read,
            recovery: ActionRecovery::SafeRepeat,
        },
        approval_required: false,
    }
}

struct Case {
    bundle: Arc<PublishedBundle>,
    request: SampleRequest,
    target: FrozenCallTarget,
}

impl Case {
    fn new(call: ScriptedToolCall) -> Self {
        let bundle = build(
            &source("agent.run"),
            Fixture::AutonomousResponse.snapshots(company()),
            vec![],
        );
        let agent = bundle.snapshots().agents[0].clone();
        let mut request = request(vec![script(1, "start", vec![json!("done")])]);
        request.scripts[0].responses[0].tools = vec![call];
        request.call_targets = vec![DESK];
        request.agents = vec![ResolvedAgentDefinition {
            native_permissions: Default::default(),
            company_id: bundle.company_id(),
            agent_id: agent.agent_id,
            key: agent.key.clone(),
            instructions: "Ask the desk".into(),
            model: Some(ModelSettings {
                provider: name("test"),
                model: "sample".into(),
                max_output_tokens: 1000,
            }),
            tools: vec![call_tool(), http_tool()],
            skills: vec![],
        }];
        let target = FrozenCallTarget {
            desk: DESK,
            enabled: true,
            unit_archived: false,
            responder: Some(FrozenResponder {
                binding: RESPONDER,
                revision: 3,
            }),
            writable_agents: [agent.agent_id].into(),
            review_required: false,
            intake: Some(json!({"type":"object","additionalProperties":false,
                "required":["order"],"properties":{"order":{"type":"string","maxLength":16}}})),
            result_fields: Some(json!({"type":"object","additionalProperties":false,
                "required":["status"],"properties":{"status":{"type":"string","maxLength":16}}})),
        };
        Self {
            bundle,
            request,
            target,
        }
    }

    fn call(&mut self) -> &mut ScriptedToolCall {
        &mut self.request.scripts[0].responses[0].tools[0]
    }

    fn run(&self) -> SampleReport {
        super::super::execute(
            PreparedSample::new_with_targets(
                self.bundle.clone(),
                self.request.clone(),
                vec![],
                vec![self.target.clone()],
            )
            .unwrap(),
        )
        .unwrap()
    }
}

fn arguments() -> Value {
    json!({"channel_id":DESK,"content":{"subject":"Order","body":"Where is order 7?"},
        "fields":{"order":"7"}})
}

fn answer() -> Value {
    json!({"closure":"done","child_run_id":Uuid::from_u128(902),
        "content":{"subject":"Order","body":"It shipped."},"fields":{"status":"shipped"}})
}

fn scripted(input: Value, output: Option<Value>, approval: Option<bool>) -> ScriptedToolCall {
    ScriptedToolCall {
        name: name("channel.call"),
        input,
        output,
        approval,
    }
}

/// The one simulated effect and its label.
fn simulated(report: &SampleReport) -> (&Value, &'static str) {
    complete(report);
    let calls: Vec<_> = report
        .effects
        .iter()
        .filter(|effect| effect.kind == SampleEffectKind::SimulatedCall)
        .collect();
    assert_eq!(calls.len(), 1, "{report:?}");
    assert_eq!(calls[0].operation.as_str(), "channel.call");
    (&calls[0].output, calls[0].label.unwrap())
}

fn error_code(output: &Value) -> &str {
    output
        .pointer("/error/code")
        .and_then(Value::as_str)
        .unwrap()
}

fn stopped(case: &Case, expected: &str) {
    let report = case.run();
    assert_eq!(report.status, SampleStatus::Stopped, "{report:?}");
    let diagnostic = report.diagnostic.clone().unwrap();
    assert!(diagnostic.contains(expected), "{diagnostic}");
    assert!(
        report
            .effects
            .iter()
            .all(|effect| effect.kind != SampleEffectKind::SimulatedCall),
        "a stopped call records no simulated effect"
    );
}

#[test]
fn channel_call_sample_simulated_result() {
    // The fixture is the answer, labelled; the report never prints the frozen responder.
    let case = Case::new(scripted(arguments(), Some(answer()), None));
    let report = case.run();
    let (output, label) = simulated(&report);
    assert_eq!(output, &answer());
    assert_eq!(label, "Simulated call result (author fixture)");
    let printed = serde_json::to_string(&report).unwrap();
    assert!(!printed.contains(&RESPONDER.to_string()));
    assert!(!printed.contains("\"revision\":3"));
    // Every success closure is accepted.
    for closure in [
        json!({"closure":"handed_off","child_run_id":Uuid::from_u128(902),"run_id":Uuid::from_u128(903),
            "content":{"body":"Passed on."}}),
        json!({"closure":"denied","code":"channel.denied","reason":"Not ours"}),
        json!({"closure":"no_reply","child_run_id":Uuid::from_u128(902)}),
    ] {
        let mut case = Case::new(scripted(arguments(), Some(closure.clone()), None));
        assert_eq!(simulated(&case.run()).0, &closure);
        // F9: without a declared result sub-schema any fields object passes, as live.
        case.target.result_fields = None;
        case.call().output = Some(answer());
        assert_eq!(simulated(&case.run()).0, &answer());
    }
    // A reviewed desk: approval true gives the fixture, false the review_rejected error.
    let mut case = Case::new(scripted(arguments(), Some(answer()), Some(true)));
    case.target.review_required = true;
    assert_eq!(simulated(&case.run()).0, &answer());
    case.call().approval = Some(false);
    case.call().output = None;
    let report = case.run();
    let (output, label) = simulated(&report);
    assert_eq!(error_code(output), REVIEW_REJECTED);
    assert_eq!(label, "Simulated call: review rejected");
    // Launch answers come from the frozen facts, as live gives them, with no fixture.
    let launch = |edit: &dyn Fn(&mut Case)| {
        let mut case = Case::new(scripted(arguments(), None, None));
        edit(&mut case);
        let report = case.run();
        let (output, label) = simulated(&report);
        assert_eq!(label, "Simulated call: answered at launch");
        output.clone()
    };
    let output = launch(&|case| case.target.writable_agents.clear());
    assert_eq!(error_code(&output), TARGET_UNAVAILABLE);
    let output = launch(&|case| case.target.unit_archived = true);
    assert_eq!(output["closure"], "denied");
    assert_eq!(output["code"], crate::app_error::AppError::UNIT_ARCHIVED);
    let output = launch(&|case| case.target.enabled = false);
    assert_eq!(output["code"], DESK_DISABLED);
    let output = launch(&|case| case.target.responder = None);
    assert_eq!(output["code"], NO_RESPONDER);
    let output = launch(&|case| case.call().input = json!({"channel_id":DESK}));
    assert_eq!(error_code(&output), INVALID_INPUT);
    let output = launch(&|case| case.call().input["fields"] = json!({"order":7}));
    assert_eq!(error_code(&output), INTAKE_INVALID);
    let output = launch(&|case| case.target.intake = None);
    assert_eq!(error_code(&output), INTAKE_NOT_DECLARED);
    // Not simulated: destination admission, level release, tree limit, unit spend.
    assert_ne!(error_code(&output), AUDIENCE_REFUSED);
}

#[test]
fn channel_call_sample_incomplete() {
    // A desk not declared among the targets.
    let mut case = Case::new(scripted(arguments(), Some(answer()), None));
    case.call().input["channel_id"] = json!(Uuid::from_u128(999));
    stopped(&case, "channel_call.sample_target_undeclared");
    // A reviewed desk with no scripted review.
    let mut case = Case::new(scripted(arguments(), Some(answer()), None));
    case.target.review_required = true;
    stopped(&case, "channel_call.sample_review_undeclared");
    // No scripted answer.
    let case = Case::new(scripted(arguments(), None, None));
    stopped(&case, "channel_call.sample_result_undeclared");
    // A review scripted for a desk that has none.
    let case = Case::new(scripted(arguments(), Some(answer()), Some(true)));
    stopped(&case, "does not review");
    // A review scripted on any other tool.
    let mut case = Case::new(scripted(arguments(), Some(answer()), None));
    *case.call() = ScriptedToolCall {
        name: name("crm.read"),
        input: json!({}),
        output: Some(json!({})),
        approval: Some(true),
    };
    stopped(&case, "Only a channel.call");
    // An answer that is not a success closure, or outside the output contract.
    for output in [
        json!({"error":{"code":"channel_call.revoked","message":"gone"}}),
        json!({"closure":"done"}),
    ] {
        let case = Case::new(scripted(arguments(), Some(output), None));
        assert_eq!(case.run().status, SampleStatus::Stopped);
    }
    // Fields outside the frozen result sub-schema (F6, kept by F9 when one is declared).
    let mut bad = answer();
    bad["fields"] = json!({"status":"shipped","extra":true});
    let case = Case::new(scripted(arguments(), Some(bad), None));
    assert_eq!(case.run().status, SampleStatus::Stopped);
    // An answer scripted for a call live answers at launch.
    let mut case = Case::new(scripted(arguments(), Some(answer()), None));
    case.target.enabled = false;
    stopped(&case, "without the desk's answer");
}
