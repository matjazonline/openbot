use super::*;
use crate::test_support::workflow::decode;
use serde_json::json;
use uuid::Uuid;

pub(super) const ID: &str = "00000000-0000-0000-0000-000000000001";

pub(super) fn single(source_binding: &str) -> String {
    format!(
        r#"format_version: 1
workflow_id: {ID}
input_schema: {{type: object, properties: {{message: {{type: string}}}}, required: [message]}}
parameter_schema: true
output_schema: true
resources: []
entry: start
steps:
  start:
    type: data.map
    with:
      message: {source_binding}
    routes: {{success: $end}}
limits: {{max_steps: 20, max_context_bytes: 4096}}
"#
    )
}

pub(super) fn descriptor() -> StepDescriptor {
    StepDescriptor {
        type_name: TypeName::parse("data.map").unwrap(),
        input_schema: json!({"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false}),
        output_schema: json!({"type":"object","properties":{"result":{"type":"string"}},"required":["result"],"additionalProperties":false}),
        routes: RouteContract::Success,
        control: ControlKind::None,
        ordered_rule: false,
    }
}

pub(super) fn compile_one(source: &str) -> Result<CompiledWorkflow, Diagnostic> {
    let registry = BTreeMap::from([(TypeName::parse("data.map").unwrap(), descriptor())]);
    let deps = BTreeMap::from([(
        WorkflowId::new(Uuid::parse_str(ID).unwrap()),
        BTreeSet::new(),
    )]);
    compile(
        decode(source)?,
        VersionId::new(Uuid::nil()),
        CompileFacts {
            descriptors: &registry,
            dependencies: &deps,
        },
    )
}

pub(super) fn compile_custom(
    source: &str,
    descriptors: Vec<StepDescriptor>,
    dependencies: BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
) -> Result<CompiledWorkflow, Diagnostic> {
    let registry = descriptors
        .into_iter()
        .map(|descriptor| (descriptor.type_name.clone(), descriptor))
        .collect();
    compile(
        decode(source)?,
        VersionId::new(Uuid::nil()),
        CompileFacts {
            descriptors: &registry,
            dependencies: &dependencies,
        },
    )
}

pub(super) fn empty_deps() -> BTreeMap<WorkflowId, BTreeSet<WorkflowId>> {
    BTreeMap::from([(
        WorkflowId::new(Uuid::parse_str(ID).unwrap()),
        BTreeSet::new(),
    )])
}
