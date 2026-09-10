//! Reuse the pinned standalone implementations, never an ai-agents agent or unrestricted registry.
use std::sync::Arc;

use ai_agents::tools::*;

use crate::entities::value_objects::ToolId;

/// Standalone tools do not run the upstream executor's policy checks. Reject allocation
/// amplification before polling them: a Tokio timeout cannot interrupt synchronous loops.
pub(super) fn validate_work(id: &ToolId, args: &serde_json::Value) -> Result<(), &'static str> {
    let operation = args["operation"].as_str().unwrap_or("").to_lowercase();
    let number = |key: &str, default: f64| args[key].as_f64().unwrap_or(default);
    let too_large = |key: &str| args[key].as_u64().is_some_and(|n| n > 10_000);
    if ["length", "count", "width"]
        .iter()
        .any(|key| too_large(key))
    {
        return Err("Requested expansion exceeds the tool work budget");
    }
    match id.as_str() {
        "template" if operation != "render" => {
            return Err("Only inline template rendering is allowed");
        }
        "template" => {
            // Fuel meters instructions, not multiplication/filter allocations inside one instruction.
            let text = args["template"].as_str().unwrap_or("");
            if args.to_string().len() > 8192 || text.len() > 4096 || text.contains(['*', '+', '~'])
            {
                return Err("Template exceeds the bounded inline expression policy");
            }
            for block in text.split("{%").skip(1) {
                let keyword = block
                    .trim_start_matches(['-', '+'])
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                if ![
                    "if", "elif", "else", "endif", "for", "endfor", "break", "continue",
                ]
                .contains(&keyword)
                {
                    return Err("Template permits only bounded conditional and loop blocks");
                }
            }
        }
        "math" if operation == "range" => {
            let min = number("min", 0.0);
            let max = number("max", 0.0);
            let step = number("step", 1.0);
            let count = ((max - min) / step).abs();
            if !count.is_finite() || count > 10_000.0 || min + step == min || max + step == max {
                return Err("Range exceeds the tool work budget or makes no numeric progress");
            }
        }
        "random" if operation == "number" => {
            if !(number("max", 1.0) - number("min", 0.0)).is_finite() {
                return Err("Random number interval is not finite");
            }
        }
        "json" => {
            if args["path"].as_str().is_some_and(|path| {
                path.split('.')
                    .any(|part| part.parse::<u64>().is_ok_and(|index| index > 10_000))
            }) {
                return Err("JSON array index exceeds the tool work budget");
            }
        }
        "text" => {
            let bytes = args["text"].as_str().map_or(0, str::len);
            if bytes.saturating_mul(args["count"].as_u64().unwrap_or(1) as usize) > 65_536 {
                return Err("Text expansion exceeds the tool work budget");
            }
            let replacement = args["replacement"].as_str().map_or(1, str::len);
            if operation == "replace"
                && bytes.saturating_add(1).saturating_mul(replacement) > 65_536
            {
                return Err("Text replacement exceeds the tool work budget");
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn create(id: &ToolId) -> Option<Arc<dyn Tool>> {
    Some(match id.as_str() {
        "calculator" => Arc::new(CalculatorTool::new()),
        "datetime" => Arc::new(DateTimeTool::new()),
        "echo" => Arc::new(EchoTool::new()),
        "json" => Arc::new(JsonTool::new()),
        "math" => Arc::new(MathTool::new()),
        "random" => Arc::new(RandomTool::new()),
        "template" => Arc::new(TemplateTool::new()),
        "text" => Arc::new(TextTool::new()),
        "todo" => Arc::new(TodoTool::new(TodoStore::default())),
        "web_fetch" => Arc::new(WebFetchTool::new()),
        _ => return None,
    })
}

pub(super) fn context(
    tool: &dyn Tool,
    call_id: &str,
    timeout: std::time::Duration,
) -> ToolExecutionContext {
    let safety = tool.safety_metadata();
    ToolExecutionContext {
        requested_name: tool.id().into(),
        canonical_id: tool.id().into(),
        display_name: tool.name().into(),
        provider_id: None,
        registry_version: 1,
        policy_version: 1,
        runtime_control_version: 1,
        call_id: call_id.into(),
        source: ToolCallSource::Manual,
        actor: ToolActorContext::default(),
        cancellation: ToolCancellationToken::default(),
        started_at: chrono::Utc::now(),
        deadline: Some(
            chrono::Utc::now() + chrono::Duration::from_std(timeout).unwrap_or_default(),
        ),
        permission: ToolPolicyDecisionRecord::allow(),
        approval: None,
        classification: ToolCallClassification::from_metadata(&safety),
        limits: ToolExecutionLimits {
            timeout_ms: Some(timeout.as_millis() as u64),
            max_output_chars: Some(safety.max_output_chars.unwrap_or(16_384).min(16_384)),
            max_result_chars: Some(safety.max_result_size_chars.unwrap_or(65_536).min(65_536)),
            max_results: Some(100),
            max_response_bytes: Some(1_048_576),
            max_redirects: Some(5),
            ..Default::default()
        },
        safety,
        policy_snapshot: serde_json::Value::Null,
        custom_config: serde_json::Value::Null,
    }
}
