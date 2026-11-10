use super::*;
use crate::entities::mcp::McpDiscoveredTool;
use serde_json::{Value, json};

#[test]
fn doop_catalog_unapproved_discovery_exposes_current_output_and_separate_fingerprints() {
    let mut tool = McpDiscoveredTool {
        name: "get_canvas".to_owned().try_into().unwrap(),
        description: "Untrusted metadata".into(),
        input_schema: json!({"type":"object","properties":{"canvas_id":{"type":"string"}}}),
        output_schema: Some(
            json!({"type":"object","properties":{"workspace_id":{"type":"string"}}}),
        ),
    };
    let initial = tool_projection(&tool);
    assert_eq!(
        initial["output_schema"],
        tool.output_schema.clone().unwrap()
    );
    assert_eq!(initial["input_schema"], tool.input_schema);
    assert!(initial.get("schema_fingerprint").is_none());
    tool.output_schema = Some(json!({"type":"object","required":["workspace_id"]}));
    let changed = tool_projection(&tool);
    assert_eq!(
        changed["input_schema_fingerprint"],
        initial["input_schema_fingerprint"]
    );
    assert_ne!(
        changed["output_schema_fingerprint"],
        initial["output_schema_fingerprint"]
    );
    tool.output_schema = None;
    let undeclared = tool_projection(&tool);
    assert_eq!(undeclared["output_schema"], Value::Null);
    assert_eq!(undeclared["output_schema_fingerprint"], Value::Null);
}
