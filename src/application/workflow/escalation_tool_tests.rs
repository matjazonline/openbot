use super::*;
use crate::application::agent::ActionEffect;
use uuid::Uuid;

#[test]
fn workflow_escalation_tools_trust_only_exact_company_local_snapshots() {
    let company = CompanyId::new(Uuid::new_v4());
    for kind in EscalationTool::ALL {
        let valid = snapshot(company, kind).unwrap();
        assert!(trusted(&valid).unwrap());
        assert_eq!(
            valid.kind(),
            crate::application::workflow::publication::ToolKind::Escalation
        );
        for edit in 0..4 {
            let mut forged = valid.clone();
            match edit {
                0 => {
                    forged.policy.effect = match valid.policy.effect {
                        ActionEffect::Read => ActionEffect::Write,
                        _ => ActionEffect::Read,
                    }
                }
                1 => forged.policy.policy_revision += 1,
                2 => forged.contract.input_schema = json!({"type":"object"}),
                _ => forged.contract.output_schema = json!({"type":"object"}),
            }
            assert!(!trusted(&forged).unwrap());
        }
        assert!(crate::application::workflow::provisioning::is_reserved_tool_name(kind.name()));
    }
}

#[test]
fn workflow_escalation_tools_bound_revisions_and_reject_conditional_choice_forgery() {
    let id = Uuid::new_v4();
    for choice in [
        json!({"kind":"acknowledge"}),
        json!({"kind":"reassign","principal":id}),
        json!({"kind":"cancel","expected_run_revision":1}),
    ] {
        assert!(
            serde_json::from_value::<RespondInput>(json!({"decision_id":id,"choice":choice}))
                .is_ok()
        );
    }
    for choice in [
        json!({"kind":"reassign"}),
        json!({"kind":"cancel"}),
        json!({"kind":"acknowledge","principal":id}),
        json!({"kind":"cancel","expected_run_revision":-1}),
        json!({"kind":"cancel","expected_run_revision":"1"}),
        json!({"kind":"acknowledge","actor":id}),
    ] {
        assert!(
            serde_json::from_value::<RespondInput>(json!({"decision_id":id,"choice":choice}))
                .is_err()
        );
    }
    let span = SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    };
    let schema = Schema::compile(schemas(EscalationTool::Respond).0, "/input", span).unwrap();
    assert!(schema.validate(&json!({"decision_id":id,"choice":{"kind":"cancel","expected_run_revision":u64::MAX}}),"/input",span).is_err());
    assert!(
        serde_json::from_value::<ReadInput>(json!({"decision_id":id,"company_id":id})).is_err()
    );
}
