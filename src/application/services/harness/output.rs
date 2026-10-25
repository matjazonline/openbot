//! What one agent run reports back: the model-step output the workflow checkpoint stores.
use serde::{Deserialize, Serialize};

/// Tokens one run used, as the provider reported or the reservation estimated them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TokenUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

impl TokenUsage {
    pub fn new(prompt_tokens: usize, completion_tokens: usize) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        }
    }
}

/// What one agent run produced: the checkpoint's final output, with the tokens it used.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentExecutionOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<crate::services::response_contract::StructuredResponse>,
    pub content: String,
    pub token_usage: TokenUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}
