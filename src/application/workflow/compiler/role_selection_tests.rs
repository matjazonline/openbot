use super::*;
use crate::adapters::workflow_source::WorkflowSourceDecoder;
use crate::application::agent::AgentKey;
use crate::application::workflow::registry;
use crate::application::workflow::reviewers::{ReviewerSelector, parse_reviewers};
use crate::domain::workflow::{StepId, VersionId};
use crate::entities::department::DepartmentRole;
use crate::entities::transport::PrincipalId;
use serde_json::json;
use uuid::Uuid;

#[test]
fn workflow_role_picker_model_selection_preserves_other_inputs_and_enforces_xor() {
    for yaml in [false, true] {
        let original = registry::example("agent.run").unwrap().source;
        let value: serde_json::Value = serde_json::from_str(&original).unwrap();
        let source = if yaml {
            serde_yaml::to_string(&value).unwrap()
        } else {
            original
        };
        let source = format!("{source}\n# unsaved comment\n");
        let step = StepId::parse("start").unwrap();
        let selected =
            select_role_reference(&WorkflowSourceDecoder, &source, &step, DepartmentRole::Lead)
                .unwrap();
        assert!(selected.ends_with("# unsaved comment\n"));
        let parsed = parse_workflow(
            &WorkflowSourceDecoder.decode(&selected).unwrap(),
            VersionId::new(Uuid::new_v4()),
        )
        .unwrap();
        let inputs = &parsed.definition.steps[&step].inputs;
        assert!(!inputs.contains_key("agent"));
        assert_eq!(
            inputs.get("role"),
            Some(&crate::domain::workflow::Binding::Literal(json!("lead")))
        );
        let pinned = select_agent_reference(
            &WorkflowSourceDecoder,
            &selected,
            &step,
            &AgentKey::parse("new-agent").unwrap(),
        )
        .unwrap();
        let parsed = parse_workflow(
            &WorkflowSourceDecoder.decode(&pinned).unwrap(),
            VersionId::new(Uuid::new_v4()),
        )
        .unwrap();
        assert!(!parsed.definition.steps[&step].inputs.contains_key("role"));
        assert_eq!(
            parsed.definition.steps[&step].inputs.get("agent"),
            Some(&crate::domain::workflow::Binding::Literal(json!(
                "new-agent"
            )))
        );
    }
}

#[test]
fn workflow_role_picker_reviewers_roundtrip_and_principal_selector_deduplication() {
    let source = registry::example("decision.human").unwrap().source;
    let principal = PrincipalId::new(Uuid::new_v4());
    let selectors = [
        ReviewerSelector::Principal(principal),
        ReviewerSelector::Role(DepartmentRole::Reviewer),
        ReviewerSelector::Role(DepartmentRole::Lead),
    ];
    let selected = select_reviewer_references(
        &WorkflowSourceDecoder,
        &source,
        &StepId::parse("start").unwrap(),
        &selectors,
    )
    .unwrap();
    let steps = reviewer_reference_steps(&WorkflowSourceDecoder, &selected).unwrap();
    assert_eq!(steps[0].selectors.as_deref(), Some(selectors.as_slice()));
    assert_eq!(parse_reviewers(&json!([{"kind":"principal","id":principal.as_uuid()},{"kind":"principal","id":principal.as_uuid()},"reviewer","reviewer"])).unwrap().len(),2);
    assert!(parse_reviewers(&json!([{"kind":"user","id":principal.as_uuid()}])).is_err());
    assert!(parse_reviewers(&json!([{"kind":"principal","id":Uuid::nil()}])).is_err());
    assert!(
        parse_reviewers(&json!(
            (0..17)
                .map(|_| json!({"kind":"principal","id":Uuid::new_v4()}))
                .collect::<Vec<_>>()
        ))
        .is_err()
    );
}
