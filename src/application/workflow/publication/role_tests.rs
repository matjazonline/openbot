use super::*;
use crate::entities::department::DepartmentRole;

fn role_source(kind: &str, role: &str) -> Value {
    let mut value = source(kind);
    let inputs = value["steps"]["start"]["with"].as_object_mut().unwrap();
    inputs.remove("agent");
    inputs.insert("role".into(), json!({"literal":role}));
    value
}

fn compile_source(source: &Value) -> Result<CompiledWorkflow, Diagnostic> {
    let workflow =
        WorkflowId::new(Uuid::parse_str(source["workflow_id"].as_str().unwrap()).unwrap());
    registry::compile(
        decode(&source.to_string())?,
        VersionId::new(Uuid::nil()),
        &Default::default(),
        &BTreeMap::from([(workflow, std::collections::BTreeSet::new())]),
    )
}

#[test]
fn workflow_role_source_selectors_are_exclusive_static_and_fixed_for_every_model_kind() {
    for kind in ["agent.run", "ai.classify", "decision.agent"] {
        let valid = role_source(kind, "lead");
        let compiled = compile_source(&valid).unwrap();
        assert_eq!(compiled.role_references()[0].role, DepartmentRole::Lead);
        assert_eq!(
            compiled.role_references()[0].mode,
            if kind == "agent.run" {
                RoleUseMode::Tools
            } else {
                RoleUseMode::ModelOnly
            }
        );
        let mut both = valid.clone();
        both["steps"]["start"]["with"]["agent"] = json!({"literal":"assistant"});
        assert_eq!(
            compile_source(&both).err().unwrap().code,
            "registry.agent_or_role"
        );
        let mut neither = valid.clone();
        neither["steps"]["start"]["with"]
            .as_object_mut()
            .unwrap()
            .remove("role");
        assert_eq!(
            compile_source(&neither).err().unwrap().code,
            "registry.agent_or_role"
        );
        for selector in [
            json!({"literal":"boss"}),
            json!({"ref":"/params/role"}),
            json!({"literal":7}),
        ] {
            let mut invalid = valid.clone();
            invalid["steps"]["start"]["with"]["role"] = selector;
            assert_eq!(
                compile_source(&invalid).err().unwrap().code,
                "registry.role"
            );
        }
        let reviewer = compile_source(&role_source(kind, "reviewer")).unwrap();
        assert_ne!(compiled.content_hash(), reviewer.content_hash());
        assert_eq!(reviewer.representation()["roles"][0]["role"], "reviewer");
    }
    let mut not_model = source("data.map");
    not_model["steps"]["start"]["with"]["role"] = json!({"literal":"lead"});
    assert!(compile_source(&not_model).is_err());
}

#[test]
fn workflow_role_publication_has_requirements_without_agent_capture_and_restores_exactly() {
    for kind in ["agent.run", "ai.classify", "decision.agent"] {
        let source = role_source(kind, "reviewer");
        assert!(
            agent_universe(&decode(&source.to_string()).unwrap())
                .unwrap()
                .is_empty()
        );
        let bundle = build(&source, DependencySnapshots::default()).unwrap();
        assert!(bundle.snapshots().agents.is_empty());
        assert_eq!(bundle.manifest()["roles"][0]["role"], "reviewer");
        let restored = restore_bundle(
            &store_bundle(&bundle).unwrap(),
            &crate::adapters::workflow_source::WorkflowSourceDecoder,
        )
        .unwrap();
        assert_eq!(restored.content_hash(), bundle.content_hash());
        assert_eq!(
            restored.compiled().role_references(),
            bundle.compiled().role_references()
        );
        assert_eq!(
            build(&source, snapshots()).err().unwrap().code,
            "publication.agent",
            "a forged extra agent is refused"
        );
    }
}

#[test]
fn workflow_role_publication_mixed_steps_keep_only_the_pinned_agent() {
    let mut mixed = source("agent.run");
    mixed["steps"]["start"]["routes"]["success"] = json!("role_step");
    mixed["steps"]["role_step"] = role_source("agent.run", "lead")["steps"]["start"].clone();
    let bundle = build(&mixed, snapshots()).unwrap();
    assert_eq!(bundle.snapshots().agents.len(), 1);
    assert_eq!(bundle.snapshots().agents[0].uses.len(), 1);
    assert_eq!(bundle.snapshots().agents[0].uses[0].step.as_str(), "start");
    assert_eq!(
        bundle.compiled().role_references()[0].step.as_str(),
        "role_step"
    );
}

#[test]
fn workflow_role_source_sample_and_help_explain_the_same_requirement() {
    use crate::application::workflow::fixtures::Fixture;
    let sample = Fixture::RoleLeadResponse;
    let bundle = freeze(
        decode(sample.source()).unwrap(),
        company(),
        sample.version_id(),
        sample.snapshots(company()),
        vec![],
    )
    .unwrap();
    assert_eq!(
        bundle.compiled().role_references()[0].role,
        DepartmentRole::Lead
    );
    assert!(bundle.snapshots().agents.is_empty());
    for kind in ["agent.run", "ai.classify", "decision.agent"] {
        let example = registry::example(kind).unwrap();
        let help = registry::authoring_help(
            decode(&example.source).unwrap(),
            VersionId::new(Uuid::nil()),
            &StepId::parse("start").unwrap(),
            &example.facts,
            &example.dependencies,
        )
        .unwrap();
        assert!(help.notes.contains("role"));
        assert!(help.notes.contains("reviewer"));
        assert!(help.notes.contains("nearest ancestor"));
    }
}

#[test]
fn workflow_role_model_and_people_requirements_coexist_on_one_step_without_selector_confusion() {
    let mut value = role_source("agent.run", "lead");
    value["steps"]["start"]["with"]["approval"] =
        json!({"literal":{"reviewers":["reviewer"],"deadline":"2099-01-01T00:00:00Z"}});
    let bundle = build(&value, DependencySnapshots::default()).unwrap();
    let references = bundle.compiled().role_references();
    assert_eq!(references.len(), 2);
    assert!(
        references
            .iter()
            .any(|reference| reference.role == DepartmentRole::Lead
                && reference.mode == RoleUseMode::Tools)
    );
    assert!(
        references
            .iter()
            .any(|reference| reference.role == DepartmentRole::Reviewer
                && reference.mode == RoleUseMode::People)
    );
    let restored = restore_bundle(
        &store_bundle(&bundle).unwrap(),
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )
    .unwrap();
    assert_eq!(restored.compiled().role_references(), references);
    value["steps"]["start"]["with"]["approval"]["literal"]["reviewers"] = json!(["lead"]);
    assert_eq!(compile_source(&value).err().unwrap().code, "registry.role");
}

fn approval_default_source(fallback: Value) -> Value {
    let mut value = source("http.request");
    value["parameter_schema"] = json!({"type":"object","properties":{
        "approval":{"type":"object"},"backup":{"type":"object"}}});
    value["steps"]["start"]["with"]["approval"] =
        json!({"default":{"ref":"/params/approval","value":fallback}});
    value
}

#[test]
fn workflow_role_publication_rejects_whole_approval_role_defaults_including_nested_containers() {
    let approval = json!({"reviewers":["reviewer"],"deadline":"2099-01-01T00:00:00Z"});
    for fallback in [
        json!({"literal":approval}),
        json!({"object":{"reviewers":{"literal":["reviewer"]},
            "deadline":{"literal":"2099-01-01T00:00:00Z"}}}),
        json!({"default":{"ref":"/params/backup","value":{"literal":approval}}}),
    ] {
        let value = approval_default_source(fallback);
        for diagnostic in [
            compile_source(&value)
                .err()
                .expect("compiler must reject the hidden role"),
            build(&value, DependencySnapshots::default())
                .err()
                .expect("publication must reject the hidden role"),
        ] {
            assert_eq!(diagnostic.code, "registry.role");
            assert_eq!(diagnostic.field_path.as_ref(), "/steps/start/with/approval");
            assert!(diagnostic.message.contains("unconditional literal"));
        }
    }
}

#[test]
fn workflow_role_publication_preserves_whole_approval_pinned_principal_defaults_and_params() {
    use crate::domain::workflow::{Context, RunId, RunMetadata};
    let principal = Uuid::from_u128(6);
    let approval = json!({"reviewers":[{"kind":"principal","id":principal}],
        "deadline":"2099-01-01T00:00:00Z"});
    let value = approval_default_source(json!({"literal":approval}));
    let bundle = build(&value, DependencySnapshots::default()).unwrap();
    assert!(bundle.compiled().role_references().is_empty());
    let input = json!({});
    let outputs = BTreeMap::new();
    let mut override_approval = approval.clone();
    override_approval["reviewers"][0]["id"] = json!(Uuid::from_u128(8));
    for params in [json!({}), json!({"approval":override_approval})] {
        let expected = params.get("approval").unwrap_or(&approval);
        let context = Context {
            input: &input,
            params: &params,
            step_outputs: &outputs,
            run: RunMetadata {
                run_id: RunId::new(Uuid::from_u128(7)),
                parent_run_id: None,
                deadline: None,
            },
        };
        let inputs = bundle
            .compiled()
            .prepare_step_inputs(&StepId::parse("start").unwrap(), &context)
            .unwrap();
        assert_eq!(&inputs.value()["approval"], expected);
    }
}
