//! Protocol-neutral MCP boundary. Sessions are owned by one logical run, never shared by agents.
use crate::{
    app_error::AppResult,
    entities::mcp::{McpDiscoveredTool, McpEndpoint, McpToolName},
};
use async_trait::async_trait;
use secrecy::SecretString;
use serde_json::Value;

#[async_trait]
pub trait McpClient: Send + Sync {
    /// Stop admission, cancel active sessions, and join every supervised transport worker.
    async fn shutdown(&self) -> AppResult<()>;
    /// Local validation only. This does not prove DNS, credentials or server availability.
    fn validate_endpoint(&self, endpoint: &McpEndpoint) -> AppResult<()>;
    async fn connect(
        &self,
        endpoint: &McpEndpoint,
        token: Option<SecretString>,
    ) -> AppResult<Box<dyn McpSession>>;
}

#[async_trait]
pub trait McpSession: Send + Sync {
    async fn discover(&self) -> AppResult<Vec<McpDiscoveredTool>>;
    /// An error after dispatch is an indeterminate effect. The owner must never retry blindly.
    async fn call(&self, name: &McpToolName, arguments: Value) -> AppResult<Value>;
    /// Always await shutdown, including after discovery or invocation errors.
    async fn close(self: Box<Self>) -> AppResult<()>;
}
