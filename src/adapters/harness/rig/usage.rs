//! Adapt absent optional accounting to Rig's required wire fields. Zero remains "unreported"
//! in `protocol::captured_usage`, which charges conservative reservation estimates instead.
use bytes::Bytes;
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub(super) enum UsageProtocol {
    Chat,
    Messages,
    Responses,
    Native,
}
impl UsageProtocol {
    pub fn for_path(path: &str) -> Self {
        if path.ends_with("/chat/completions") {
            Self::Chat
        } else if path.ends_with("/messages") {
            Self::Messages
        } else if path.ends_with("/responses") {
            Self::Responses
        } else {
            Self::Native
        }
    }

    pub fn normalize(self, body: Bytes) -> Result<Bytes, serde_json::Error> {
        let fields: &[&str] = match self {
            Self::Chat => &["prompt_tokens", "completion_tokens", "total_tokens"],
            Self::Messages => &["input_tokens", "output_tokens"],
            Self::Responses => &["input_tokens", "output_tokens", "total_tokens"],
            Self::Native => return Ok(body),
        };
        // Let the provider decoder classify malformed payloads; never invent a completion.
        let Ok(Value::Object(mut response)) = serde_json::from_slice::<Value>(&body) else {
            return Ok(body);
        };
        let usage = response.entry("usage").or_insert_with(|| json!({}));
        if usage.is_null() {
            *usage = json!({});
        }
        let Some(usage) = usage.as_object_mut() else {
            return Ok(body);
        };
        for field in fields {
            let value = usage.entry(*field).or_insert(json!(0));
            if value.is_null() {
                *value = json!(0);
            }
        }
        serde_json::to_vec(&response).map(Bytes::from)
    }
}
