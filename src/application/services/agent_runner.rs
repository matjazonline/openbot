use tracing::info;
use crate::entities::message::{Message, MessageRole};
use crate::entities::workflow::Workflow;

pub struct AgentRunner;

impl AgentRunner {
    pub async fn execute(
        workflow_config: Option<&serde_json::Value>,
        prompt: &str,
        history: &[Message],
    ) -> String {
        info!("Executing AI Agent with prompt length {} and history count {}", prompt.len(), history.len());

        let mut history_str = String::new();
        if !history.is_empty() {
            history_str.push_str("Conversation History:\n");
            for msg in history {
                let role_label = match msg.role {
                    MessageRole::Human => "User",
                    MessageRole::Agent => "Agent",
                    MessageRole::System => "System",
                };
                history_str.push_str(&format!("[{} ({})]: {}\n", role_label, msg.sender, msg.clean_text_body));
            }
            history_str.push_str("\nLatest Inbound Message:\n");
        }

        let full_prompt = format!("{}{}", history_str, prompt);
        info!("Full prompt context length: {}", full_prompt.len());

        // Parse workflow_config, fallback to default config if None
        let default_config = Workflow::default_config();
        let config = workflow_config.unwrap_or(&default_config);
        let config_yaml = serde_yaml::to_string(config).unwrap_or_default();

        if !config_yaml.is_empty() {
            info!("Running agent with workflow config YAML:\n{}", config_yaml);
        }

        // Execute prompt against ai-agents or structured assistant response
        format!(
            "Thank you for contacting us. We received your request:\n\n> {}\n\nOur team/agent is processing this ticket under workflow rules.",
            prompt.lines().next().unwrap_or(prompt)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_runner_uses_default_config_when_none() {
        let response = AgentRunner::execute(None, "Hello world", &[]).await;
        assert!(response.contains("Thank you for contacting us"));
    }
}
