//! One-shot completions on Rig for the caller that wants an answer rather than an agent: the
//! system-prompt generator. No tools, no checkpoint, no memory, no usage row -- one request whose
//! preamble is the caller's system prompt and whose only message is its user prompt, sent with
//! the credential the caller resolved.

use async_trait::async_trait;
use rig::completion::{AssistantContent, CompletionRequest, Message};
use secrecy::SecretString;

use super::providers::{ProviderRegistry, ResolvedModelRequest, complete};
use crate::app_error::{AppError, AppResult};
use crate::services::harness::{ClassificationRequest, TextClassifier};

/// A generated prompt, never an essay.
const MAX_OUTPUT_TOKENS: u64 = 4096;

/// Stateless per tenant: the credential arrives with each request and nothing is shared.
pub struct RigTextClassifier {
    providers: ProviderRegistry,
    /// Trusted server configuration (scripted listeners in tests), never request data.
    endpoint: Option<String>,
}

impl RigTextClassifier {
    pub fn new(providers: ProviderRegistry) -> Self {
        Self {
            providers,
            endpoint: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_endpoint(providers: ProviderRegistry, endpoint: &str) -> Self {
        Self {
            providers,
            endpoint: Some(endpoint.to_owned()),
        }
    }
}

#[async_trait]
impl TextClassifier for RigTextClassifier {
    async fn complete(&self, request: ClassificationRequest<'_>) -> AppResult<String> {
        let secret = SecretString::from(request.api_key.to_owned());
        let model = self
            .providers
            .model(&ResolvedModelRequest {
                provider: request.provider,
                model: request.model,
                secret: &secret,
                endpoint: self.endpoint.as_deref(),
            })
            .map_err(|_| AppError::BadRequest("Unsupported classification model".into()))?;
        let response = complete(
            &model,
            CompletionRequest {
                model: None,
                preamble: Some(request.system_prompt.to_owned()),
                chat_history: vec![Message::user(request.user_prompt)],
                documents: vec![],
                tools: vec![],
                temperature: None,
                max_tokens: Some(MAX_OUTPUT_TOKENS),
                tool_choice: None,
                additional_params: None,
                output_schema: None,
                record_telemetry_content: false,
            },
        )
        .await?;
        let mut text = String::new();
        for part in response.choice {
            if let AssistantContent::Text(value) = part {
                text.push_str(&value.text);
            }
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::value_objects::{ModelName, ModelProvider};
    use crate::services::test_support::{LlmTurn, scripted_llm};

    #[tokio::test]
    async fn rig_classifier_sends_one_tool_free_request_and_returns_the_reply_verbatim() {
        let mut llm = scripted_llm(vec![LlmTurn::text("You are Acme's support agent.")]).await;
        let classifier =
            RigTextClassifier::with_endpoint(ProviderRegistry::standard().unwrap(), &llm.base_url);
        let reply = classifier
            .complete(ClassificationRequest {
                system_prompt: "Write a system prompt.",
                user_prompt: "A support agent for Acme",
                provider: &ModelProvider::from("openai"),
                model: &ModelName::from("gpt-4o-mini"),
                api_key: "classifier-key",
            })
            .await
            .unwrap();
        assert_eq!(reply, "You are Acme's support agent.");
        let request = &llm.observed()[0];
        assert!(
            request
                .get("tools")
                .is_none_or(|tools| tools.as_array().is_none_or(Vec::is_empty))
        );
        assert!(
            request["messages"]
                .to_string()
                .contains("Write a system prompt.")
        );
        assert!(
            request["messages"]
                .to_string()
                .contains("A support agent for Acme")
        );
        assert_eq!(llm.finish().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn rig_classifier_refuses_an_unsupported_provider_before_any_request() {
        let classifier = RigTextClassifier::new(ProviderRegistry::standard().unwrap());
        let error = classifier
            .complete(ClassificationRequest {
                system_prompt: "s",
                user_prompt: "u",
                provider: &ModelProvider::from("unknown"),
                model: &ModelName::from("m"),
                api_key: "k",
            })
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::BadRequest(_)));
    }
}
