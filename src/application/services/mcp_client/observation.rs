//! Purpose-limited workflow/model data. Authoritative MCP receipts stay intact.
use crate::app_error::{AppError, AppResult, ExecutionFailure};
use serde_json::{Value, json};

// Existing MCP response/JSON input bounds cap the source; also bound this walk
// independently so a stored value cannot consume unbounded work or stack.
const MAX_DEPTH: usize = 128;
const MAX_NODES: usize = 1_048_576;

pub(crate) fn omit_binary_content(mut value: Value) -> AppResult<Value> {
    let mut remaining = MAX_NODES;
    project(&mut value, 0, &mut remaining)?;
    Ok(value)
}

pub(crate) fn model_data(value: &Value) -> AppResult<String> {
    serde_json::to_string(&omit_binary_content(value.clone())?)
        .map_err(|_| AppError::Execution(ExecutionFailure::Protocol))
}

fn binary_block(block: &Value) -> bool {
    matches!(block["type"].as_str(), Some("image" | "audio"))
        || (block["type"] == "resource" && block["resource"].get("blob").is_some())
}

fn project(value: &mut Value, depth: usize, remaining: &mut usize) -> AppResult<()> {
    if depth > MAX_DEPTH || *remaining == 0 {
        return Err(AppError::Execution(ExecutionFailure::Budget));
    }
    *remaining -= 1;
    if value
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|blocks| blocks.iter().any(binary_block))
    {
        let is_error = value
            .get("isError")
            .and_then(Value::as_bool)
            .ok_or(AppError::Execution(ExecutionFailure::Protocol))?;
        // Discard the whole carrier, including possible binary copies in text,
        // structuredContent or metadata. These flags are application-owned.
        *value = json!({
            "content": [{"type":"text", "text":"Binary content omitted. Visual inspection was not performed."}],
            "isError": is_error,
            "_meta": {"binaryContentOmitted":true, "visualInspectionPerformed":false}
        });
        return Ok(());
    }
    match value {
        Value::Object(fields) => {
            for child in fields.values_mut() {
                project(child, depth + 1, remaining)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                project(child, depth + 1, remaining)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(name: &str) -> Value {
        let source = match name {
            "png" => include_str!("../../../adapters/mcp/fixtures/doop/screenshot.json"),
            "pdf" => include_str!("../../../adapters/mcp/fixtures/doop/pdf.json"),
            _ => unreachable!(),
        };
        let mut result: Value = serde_json::from_str(source).unwrap();
        // McpCallResult normalizes the protocol's optional status at ingress.
        result["isError"] = json!(false);
        result
    }

    #[test]
    fn doop_binary_observation_omits_genuine_png_pdf_and_all_sibling_copies() {
        for kind in ["png", "pdf"] {
            let mut source = captured(kind);
            source["structuredContent"] = json!({"echo":"private-binary-copy"});
            source["content"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"text","text":"private-binary-copy"}));
            let original = source.clone();
            let projected = omit_binary_content(source).unwrap();
            assert_eq!(projected["isError"], false);
            assert_eq!(projected["_meta"]["binaryContentOmitted"], true);
            assert_eq!(projected["_meta"]["visualInspectionPerformed"], false);
            assert!(!projected.to_string().contains("private-binary-copy"));
            assert!(projected.to_string().len() < 512);
            assert_eq!(omit_binary_content(projected.clone()).unwrap(), projected);
            assert_ne!(original, projected);
        }
    }

    #[test]
    fn doop_binary_observation_nested_errors_keep_status_and_refuse_shape_corruption() {
        let source = json!({"child":[{"content":[{"type":"audio","mimeType":"audio/wav","data":"private-binary"}],"isError":true}]});
        let result = omit_binary_content(source).unwrap();
        assert_eq!(result["child"][0]["isError"], true);
        assert!(!result.to_string().contains("private-binary"));
        assert!(omit_binary_content(json!({"content":[{"type":"image","data":"x"}]})).is_err());
        let deep = (0..MAX_DEPTH + 2).fold(json!(null), |value, _| json!([value]));
        assert!(omit_binary_content(deep).is_err());
    }

    #[test]
    fn doop_binary_observation_keeps_nonbinary_values_and_inert_resource_links() {
        let source = json!({"child":{"content":[{"type":"text","text":"ordinary text"},{"type":"resource_link","uri":"https://never-fetched.invalid","name":"item"}],"structuredContent":{"id":"frame"},"isError":false},"user":"arbitrary text stays text"});
        assert_eq!(omit_binary_content(source.clone()).unwrap(), source);
    }
}
