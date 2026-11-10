use crate::app_error::{AppError, AppResult, ExecutionFailure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Author-selected policy, frozen with a prepared workflow agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpErrorPolicy {
    Observe,
    FailStep,
}

pub enum McpResultSource {
    Mcp,
    Unavailable,
    Other,
}

impl McpErrorPolicy {
    pub fn from_inputs(inputs: &Value) -> AppResult<Self> {
        match inputs.get("mcp_error_policy") {
            None => Ok(Self::Observe),
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|_| AppError::BadRequest("Invalid MCP error policy".into())),
        }
    }

    /// Only protocol/host error discriminators count; business text is never parsed.
    pub fn check_result(self, source: McpResultSource, result: &Value) -> AppResult<()> {
        let failed = match source {
            McpResultSource::Mcp => crate::application::workflow::mcp::mcp_result_is_error(result),
            McpResultSource::Unavailable => {
                result.pointer("/error/code").and_then(Value::as_str) == Some("unknown_tool")
            }
            McpResultSource::Other => false,
        };
        if self == Self::FailStep && failed {
            return Err(AppError::Execution(ExecutionFailure::McpToolFailed));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn doop_agent_strict_mcp_policy_does_not_classify_business_fields_or_text() {
        let strict = McpErrorPolicy::FailStep;
        let refused = json!({"isError":true,"content":[]});
        assert!(strict.check_result(McpResultSource::Mcp, &refused).is_err());
        assert!(
            strict
                .check_result(McpResultSource::Other, &refused)
                .is_ok()
        );
        assert!(
            McpErrorPolicy::Observe
                .check_result(McpResultSource::Mcp, &refused)
                .is_ok()
        );
        for result in [
            json!({"isError":false}),
            json!({"content":[]}),
            json!({"content":[{"type":"text","text":"isError: true; unknown_tool"}]}),
            json!({"error":{"code":"unknown_tool"}}),
        ] {
            assert!(strict.check_result(McpResultSource::Mcp, &result).is_ok());
        }
        let host = json!({"error":{"code":"unknown_tool"}});
        assert!(
            strict
                .check_result(McpResultSource::Unavailable, &host)
                .is_err()
        );
        assert!(strict.check_result(McpResultSource::Other, &host).is_ok());
    }

    #[test]
    fn doop_agent_mcp_policy_parsing_is_typed_and_serialization_is_required() {
        assert_eq!(
            McpErrorPolicy::from_inputs(&json!({})).unwrap(),
            McpErrorPolicy::Observe
        );
        assert_eq!(
            McpErrorPolicy::from_inputs(&json!({"mcp_error_policy":"fail_step"})).unwrap(),
            McpErrorPolicy::FailStep
        );
        for invalid in [
            json!(true),
            json!(null),
            json!("ignore"),
            json!({"ref":"/input/policy"}),
        ] {
            assert!(McpErrorPolicy::from_inputs(&json!({"mcp_error_policy":invalid})).is_err());
        }
        assert_eq!(
            serde_json::to_value(McpErrorPolicy::FailStep).unwrap(),
            "fail_step"
        );
    }
}
