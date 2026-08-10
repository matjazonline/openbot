use crate::entities::message::{Message, MessageRole};
use crate::entities::workflow::Workflow;
use ai_agents::{Agent, AgentBuilder};
use tracing::info;

pub struct AgentRunner;

impl AgentRunner {
    pub async fn execute(
        workflow_config: Option<&serde_json::Value>,
        prompt: &str,
        history: &[Message],
    ) -> anyhow::Result<String> {
        info!(
            "Executing AI Agent with prompt length {} and history count {}",
            prompt.len(),
            history.len()
        );

        let mut history_str = String::new();
        if !history.is_empty() {
            history_str.push_str("Conversation History:\n");
            for msg in history {
                let role_label = match msg.role {
                    MessageRole::Human => "User",
                    MessageRole::Agent => "Agent",
                    MessageRole::System => "System",
                };
                history_str.push_str(&format!(
                    "[{} ({})]: {}\n",
                    role_label, msg.sender, msg.clean_text_body
                ));
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

        let fallback_prompt = prompt.to_string();

        // Spawn on a separate Tokio task to prevent stack frame overflow on the caller thread
        let task_result = tokio::spawn(async move {
            let agent = AgentBuilder::from_yaml(&config_yaml)?
                .auto_configure_llms()?
                .auto_configure_features()?
                .auto_configure_mcp()
                .await?
                .auto_configure_spawner()
                .await?
                .build()?;

            let response = agent.chat(&full_prompt).await?;
            info!("{:?}", response);
            Ok::<String, anyhow::Error>(response.content)
        })
        .await;

        match task_result {
            Ok(Ok(content)) => Ok(content),
            Ok(Err(err)) => {
                tracing::warn!("AI Agent execution failed ({err}); using fallback response");
                Ok(format!(
                    "Thank you for contacting us. We received your request:\n\n> {}\n\nOur team/agent is processing this ticket under workflow rules.",
                    fallback_prompt.lines().next().unwrap_or(&fallback_prompt)
                ))
            }
            Err(join_err) => {
                tracing::warn!("AI Agent task panicked or was cancelled ({join_err}); using fallback response");
                Ok(format!(
                    "Thank you for contacting us. We received your request:\n\n> {}\n\nOur team/agent is processing this ticket under workflow rules.",
                    fallback_prompt.lines().next().unwrap_or(&fallback_prompt)
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_runner_uses_default_config_when_none() -> anyhow::Result<()> {
        let response = AgentRunner::execute(None, "Hello world", &[]).await?;
        assert!(response.contains("Thank you for contacting us"));
        Ok(())
    }
}
