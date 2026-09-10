//! Explicit native wire formats for the five supported model factories.
use super::ScriptedRequest;
use serde_json::{Value, json};

pub(crate) const MODEL: &str = "company-authorized-model";
pub(crate) const KEYS: [&str; 5] = ["google", "openai", "anthropic", "groq", "xai"];

pub(crate) enum Reply {
    Text,
    ToolCall,
}

pub(crate) fn response(provider: &str, reply: Reply) -> Value {
    let tool = matches!(reply, Reply::ToolCall);
    match provider {
        "google" => json!({"responseId":"gemini-response", "modelVersion":MODEL,
            "candidates":[{"content":{"role":"model","parts": if tool {json!([{"functionCall":{"id":"call-proof","name":"lookup","args":{"key":"record"}},"thoughtSignature":"c2NyaXB0ZWQ="}])} else {json!([{"text":"verified"}])}},"finishReason":"STOP","index":0}],
            "usageMetadata":{"promptTokenCount":2,"candidatesTokenCount":3,"totalTokenCount":5}}),
        "anthropic" => json!({"id":"msg-proof","type":"message","role":"assistant","model":MODEL,
            "content": if tool {json!([{"type":"tool_use","id":"call-proof","name":"lookup","input":{"key":"record"}}])} else {json!([{"type":"text","text":"verified"}])},
            "stop_reason":if tool {"tool_use"} else {"end_turn"},"stop_sequence":null,"usage":{"input_tokens":2,"output_tokens":3}}),
        "xai" => {
            json!({"id":"resp-proof","object":"response","created_at":0,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":MODEL,
            "usage":{"input_tokens":2,"output_tokens":3,"total_tokens":5},"tools":[],
            "output": if tool {json!([{"type":"function_call","id":"item-proof","call_id":"call-proof","name":"lookup","arguments":"{\"key\":\"record\"}","status":"completed"}])} else {json!([{"type":"message","id":"msg-proof","role":"assistant","status":"completed","content":[{"type":"output_text","text":"verified","annotations":[]}]}])}})
        }
        "openai" | "groq" => {
            json!({"id":"chat-proof","object":"chat.completion","created":0,"model":MODEL,
            "choices":[{"index":0,"finish_reason":if tool {"tool_calls"} else {"stop"},"message":if tool {json!({"role":"assistant","content":null,"tool_calls":[{"id":"call-proof","type":"function","function":{"name":"lookup","arguments":"{\"key\":\"record\"}"}}]})} else {json!({"role":"assistant","content":"verified"})}}],
            "usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}})
        }
        _ => panic!("no fixture for provider"),
    }
}

pub(crate) fn check_request(
    request: &ScriptedRequest,
    provider: &str,
    key: &str,
) -> Result<(), &'static str> {
    let path = match provider {
        "google" => format!("/v1beta/models/{MODEL}:generateContent"),
        "anthropic" => "/v1/messages".into(),
        "xai" => "/v1/responses".into(),
        "openai" | "groq" => "/chat/completions".into(),
        _ => return Err("unsupported simulator protocol"),
    };
    if request.method != "POST" || request.path != path {
        return Err("method/path");
    }
    let auth = match provider {
        "google" => request.header_matches("x-goog-api-key", key),
        "anthropic" => request.header_matches("x-api-key", key),
        _ => request.header_matches("authorization", &format!("Bearer {key}")),
    };
    if !auth {
        return Err("authentication (redacted)");
    }
    if provider != "google" && request.body["model"] != MODEL {
        return Err("company model");
    }
    Ok(())
}

pub(crate) fn check_tools(body: &Value, provider: &str) -> Result<(), &'static str> {
    let definition = match provider {
        "google" => &body["tools"][0]["functionDeclarations"][0],
        "anthropic" | "xai" => &body["tools"][0],
        "openai" | "groq" => &body["tools"][0]["function"],
        _ => return Err("unsupported simulator protocol"),
    };
    if definition["name"] != "lookup" {
        return Err("tool name");
    }
    let schema = if provider == "anthropic" {
        &definition["input_schema"]
    } else {
        &definition["parameters"]
    };
    if schema["required"] != json!(["key"])
        || schema["properties"]["key"]["type"]
            .as_str()
            .is_none_or(|kind| !kind.eq_ignore_ascii_case("string"))
    {
        return Err("tool schema");
    }
    Ok(())
}

pub(crate) fn check_result(body: &Value, provider: &str) -> Result<(), &'static str> {
    match provider {
        "google" => check_google_result(body),
        "anthropic" => check_anthropic_result(body),
        "xai" => check_xai_result(body),
        "openai" | "groq" => check_chat_result(body),
        _ => Err("unsupported simulator protocol"),
    }
}

fn check_google_result(body: &Value) -> Result<(), &'static str> {
    let contents = body["contents"].as_array().ok_or("contents")?;
    let calls: Vec<_> = contents
        .iter()
        .flat_map(|item| item["parts"].as_array().into_iter().flatten())
        .filter(|part| part.get("functionCall").is_some())
        .collect();
    if calls.len() != 1
        || calls[0]["functionCall"]["id"] != "call-proof"
        || calls[0]["functionCall"]["name"] != "lookup"
        || calls[0]["functionCall"]["args"] != json!({"key":"record"})
        || calls[0]["thoughtSignature"] != "c2NyaXB0ZWQ="
    {
        return Err("Gemini assistant identity/opaque continuation");
    }
    let results: Vec<_> = contents
        .iter()
        .flat_map(|item| item["parts"].as_array().into_iter().flatten())
        .filter_map(|part| part.get("functionResponse"))
        .collect();
    if results.len() != 1
        || results[0]["id"] != "call-proof"
        || results[0]["name"] != "lookup"
        || !results[0].to_string().contains("record-value")
    {
        return Err("Gemini result");
    }
    Ok(())
}

fn check_anthropic_result(body: &Value) -> Result<(), &'static str> {
    let calls: Vec<_> = body["messages"]
        .as_array()
        .ok_or("messages")?
        .iter()
        .flat_map(|item| item["content"].as_array().into_iter().flatten())
        .filter(|part| part["type"] == "tool_use")
        .collect();
    if calls.len() != 1
        || calls[0]["id"] != "call-proof"
        || calls[0]["name"] != "lookup"
        || calls[0]["input"] != json!({"key":"record"})
    {
        return Err("Anthropic assistant call identity");
    }
    let results: Vec<_> = body["messages"]
        .as_array()
        .ok_or("messages")?
        .iter()
        .flat_map(|item| item["content"].as_array().into_iter().flatten())
        .filter(|part| part["type"] == "tool_result")
        .collect();
    if results.len() != 1
        || results[0]["tool_use_id"] != "call-proof"
        || !results[0].to_string().contains("record-value")
    {
        return Err("Anthropic result correlation");
    }
    Ok(())
}

fn check_xai_result(body: &Value) -> Result<(), &'static str> {
    // xAI's native input uses call_id; its output item ID stays in the saved transcript.

    let input = body["input"].as_array().ok_or("input")?;
    let results: Vec<_> = input
        .iter()
        .filter(|item| item["type"] == "function_call_output")
        .collect();
    if results.len() != 1
        || results[0]["call_id"] != "call-proof"
        || results[0]["output"] != "record-value"
    {
        return Err("xAI result correlation");
    }
    if !input.iter().any(|item| {
        item["type"] == "function_call"
            && item["call_id"] == "call-proof"
            && item.get("id").is_none()
    }) {
        return Err("xAI assistant call identity");
    }
    Ok(())
}

fn check_chat_result(body: &Value) -> Result<(), &'static str> {
    let messages = body["messages"].as_array().ok_or("messages")?;
    let results: Vec<_> = messages
        .iter()
        .filter(|item| item["role"] == "tool")
        .collect();
    if results.len() != 1
        || results[0]["tool_call_id"] != "call-proof"
        || results[0]["content"] != "record-value"
    {
        return Err("chat result correlation");
    }
    if !messages
        .iter()
        .any(|item| item["tool_calls"][0]["id"] == "call-proof")
    {
        return Err("assistant call history");
    }
    Ok(())
}
