use super::*;
use crate::adapters::workflow_source::decode;
use crate::domain::workflow::Context;
use uuid::Uuid;
pub(super) fn compile_example(example: &AuthoringExample) -> Result<CompiledWorkflow, Diagnostic> {
    compile(
        decode(&example.source)?,
        VersionId::new(Uuid::nil()),
        &example.facts,
        &example.dependencies,
    )
}
pub(super) fn changed(example: &mut AuthoringExample, edit: impl FnOnce(&mut Value)) {
    let mut source: Value = serde_json::from_str(&example.source).unwrap();
    edit(&mut source);
    example.source = source.to_string();
}
pub(super) fn step() -> StepId {
    StepId::parse("start").unwrap()
}
pub(super) fn context<'a>(
    input: &'a Value,
    params: &'a Value,
    outputs: &'a BTreeMap<StepId, Value>,
    _run: &'a Value,
) -> Context<'a> {
    Context {
        input,
        params,
        step_outputs: outputs,
        run: crate::domain::workflow::RunMetadata {
            run_id: crate::domain::workflow::RunId::new(Uuid::nil()),
            parent_run_id: None,
        },
    }
}
#[test]
fn workflow_registry_exact_catalogue_and_help_examples_compile() {
    let expected = "agent.run ai.classify context.load data.map decision.agent decision.human decision.rule flow.repeat http.request mcp.call memory.load memory.save message.reply message.send tool.call wait.event wait.timer workflow.call";
    assert_eq!(
        TYPES.iter().copied().collect::<BTreeSet<_>>(),
        expected.split_whitespace().collect()
    );
    assert_eq!(TYPES.len(), 18);
    assert_eq!(TYPES.iter().collect::<BTreeSet<_>>().len(), 18);
    for name in TYPES {
        let example = example(name).unwrap();
        let compiled = compile_example(&example).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let help = authoring_help(
            decode(&example.source).unwrap(),
            VersionId::new(Uuid::nil()),
            &step(),
            &example.facts,
            &example.dependencies,
        )
        .unwrap();
        assert_eq!(help.type_name.as_str(), name);
        assert!(help.notes.contains("Execution unavailable"));
        assert!(help.input_schema.is_object());
        assert!(help.constraints["recovery"].is_string());
        assert!(
            help.constraints["frozen_inputs_required"]
                .as_bool()
                .unwrap()
        );
        assert_eq!(help.example, example.source);
        assert!(
            compiled.representation()["descriptors"]["start"]["registration"]["revision"]
                .is_string()
        );
        let input = json!({});
        let params = json!({});
        let run = json!({"id":"run","parent_id":null});
        let outputs = BTreeMap::new();
        compiled
            .prepare_step_inputs(&step(), &context(&input, &params, &outputs, &run))
            .unwrap();
        assert!(
            compiled
                .validate_step_output(&step(), &Value::Null)
                .is_err(),
            "{name}"
        );
    }
}
#[test]
fn workflow_registry_every_type_rejects_missing_invalid_and_unknown_fields() {
    for name in TYPES {
        let original = example(name).unwrap();
        let help = authoring_help(
            decode(&original.source).unwrap(),
            VersionId::new(Uuid::nil()),
            &step(),
            &original.facts,
            &original.dependencies,
        )
        .unwrap();
        let required = help.input_schema["required"].as_array().unwrap();
        for field in required {
            let field = field.as_str().unwrap();
            let mut example = example(name).unwrap();
            changed(&mut example, |v| {
                v["steps"]["start"]["with"]
                    .as_object_mut()
                    .unwrap()
                    .remove(field);
            });
            assert!(compile_example(&example).is_err(), "missing {name}.{field}");
        }
        for edit in 0..3 {
            let mut example = example(name).unwrap();
            changed(&mut example, |v| match edit {
                0 => {
                    v["steps"]["start"]["unknown"] = json!(true);
                }
                1 => {
                    v["steps"]["start"]["with"]["unknown"] = json!({"literal":true});
                }
                _ => {
                    let fields = v["steps"]["start"]["with"].as_object_mut().unwrap();
                    for binding in fields.values_mut() {
                        *binding = json!({"literal":null});
                    }
                }
            });
            assert!(
                compile_example(&example).is_err(),
                "invalid {name} edit {edit}"
            );
        }
    }
}
