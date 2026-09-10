//! Guarded Streamable HTTP adapter. All rmcp types stay behind the application session port.
mod http;
pub mod policy;
mod worker;
use crate::{
    app_error::{AppError, AppResult},
    entities::mcp::*,
    services::mcp_client::{McpClient, McpSession},
};
use async_trait::async_trait;
use policy::EndpointPolicy;
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, PaginatedRequestParams},
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub struct HttpMcpClient {
    policy: EndpointPolicy,
    capacity: Arc<Semaphore>,
    endpoints: std::sync::Mutex<std::collections::HashMap<McpEndpoint, std::sync::Weak<Semaphore>>>,
    workers: tokio::sync::Mutex<tokio::task::JoinSet<()>>,
    shutdown: tokio_util::sync::CancellationToken,
}
impl HttpMcpClient {
    pub fn new(policy: EndpointPolicy) -> Self {
        Self {
            policy,
            capacity: Arc::new(Semaphore::new(32)),
            endpoints: Default::default(),
            workers: Default::default(),
            shutdown: Default::default(),
        }
    }
}
fn failed(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}
#[async_trait]
impl McpClient for HttpMcpClient {
    async fn shutdown(&self) -> AppResult<()> {
        self.capacity.close();
        self.shutdown.cancel();
        let mut workers = self.workers.lock().await;
        let mut failed_worker = false;
        while let Some(result) = workers.join_next().await {
            failed_worker |= result.is_err();
        }
        if failed_worker {
            return Err(failed("MCP worker shutdown failed"));
        }
        Ok(())
    }

    fn validate_endpoint(&self, endpoint: &McpEndpoint) -> AppResult<()> {
        self.policy.validate(endpoint)
    }
    async fn connect(
        &self,
        endpoint: &McpEndpoint,
        token: Option<SecretString>,
    ) -> AppResult<Box<dyn McpSession>> {
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| failed("MCP connection capacity exhausted"))?;
        let endpoint_permit = self.endpoint_permit(endpoint)?;
        let http = self.policy.client(endpoint).await?;
        let mut config =
            StreamableHttpClientTransportConfig::with_uri(endpoint.as_str().to_owned());
        config.auth_header = token.as_ref().map(|t| t.expose_secret().to_owned());
        config.allow_stateless = true;
        config.reinit_on_expired_session = false;
        config.channel_buffer_capacity = 8;
        let remaining = Arc::new(std::sync::atomic::AtomicUsize::new(MAX_MCP_DISCOVERY_BYTES));
        let transport = StreamableHttpClientTransport::with_client(
            http::BoundedHttp {
                client: http,
                remaining: remaining.clone(),
            },
            config,
        );
        let (session, receiver) = worker::ProxySession::channel();
        let (ready, initialized) = tokio::sync::oneshot::channel();
        let mut workers = self.workers.lock().await;
        while let Some(result) = workers.try_join_next() {
            if result.is_err() {
                tracing::warn!("MCP worker failed");
            }
        }
        if self.shutdown.is_cancelled() {
            return Err(failed("MCP client is shutting down"));
        }
        let shutdown = self.shutdown.clone();
        workers.spawn(async move {
            let result = tokio::time::timeout(Duration::from_secs(30), ().serve(transport))
                .await
                .map_err(|_| failed("MCP initialization timed out"))
                .and_then(|r| r.map_err(|_| failed("MCP initialization failed")));
            match result {
                Ok(client) => {
                    let session = Session {
                        client,
                        _permit: permit,
                        _endpoint_permit: endpoint_permit,
                        token,
                        remaining,
                        pending_call: Default::default(),
                    };
                    let _ = ready.send(Ok(()));
                    worker::run(session, receiver, shutdown).await;
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
            }
        });
        drop(workers);
        initialized
            .await
            .map_err(|_| failed("MCP initialization failed"))??;
        Ok(Box::new(session))
    }
}
struct Session {
    client: RunningService<RoleClient, ()>,
    _permit: OwnedSemaphorePermit,
    _endpoint_permit: OwnedSemaphorePermit,
    token: Option<SecretString>,
    remaining: Arc<std::sync::atomic::AtomicUsize>,
    pending_call: std::sync::Mutex<Option<rmcp::model::RequestId>>,
}
impl Session {
    fn reject_secret(&self, value: &Value) -> AppResult<()> {
        if self
            .token
            .as_ref()
            .is_some_and(|s| contains_secret(value, s.expose_secret()))
        {
            return Err(failed("MCP response contained protected data"));
        }
        Ok(())
    }
    async fn list(&self) -> AppResult<Vec<McpDiscoveredTool>> {
        let mut tools = Vec::new();
        let mut cursors = std::collections::HashSet::new();
        let mut cursor = None;
        let mut bytes = 0;
        let mut names = std::collections::HashSet::new();
        for _ in 0..=MAX_MCP_DISCOVERED_TOOLS {
            let page = self
                .client
                .list_tools(cursor.map(|cursor| {
                    let mut params = PaginatedRequestParams::default();
                    params.cursor = Some(cursor);
                    params
                }))
                .await
                .map_err(|_| failed("MCP discovery failed"))?;
            for tool in page.tools {
                let name = McpToolName::try_from(tool.name.into_owned())
                    .map_err(|_| failed("Invalid MCP tool name"))?;
                if !names.insert(name.clone()) {
                    return Err(failed("Duplicate discovered MCP tool name"));
                }
                let tool = McpDiscoveredTool {
                    name,
                    description: tool.description.map(|v| v.into_owned()).unwrap_or_default(),
                    input_schema: Value::Object((*tool.input_schema).clone()),
                };
                crate::services::tool_schema::compile(&tool.input_schema)?;
                let encoded =
                    serde_json::to_string(&tool).map_err(|_| failed("Invalid MCP discovery"))?;
                self.reject_secret(
                    &serde_json::to_value(&tool).map_err(|_| failed("Invalid discovery"))?,
                )?;
                bytes += encoded.len();
                tools.push(tool);
                if tools.len() > MAX_MCP_DISCOVERED_TOOLS || bytes > MAX_MCP_DISCOVERY_BYTES {
                    return Err(failed("MCP discovery exceeds server limits"));
                }
            }
            match page.next_cursor {
                None => return Ok(tools),
                Some(next) if next.len() <= 4096 && cursors.insert(next.clone()) => {
                    cursor = Some(next)
                }
                _ => return Err(failed("Invalid MCP discovery pagination")),
            }
        }
        Err(failed("MCP discovery page limit exceeded"))
    }
}
#[async_trait]
impl McpSession for Session {
    async fn discover(&self) -> AppResult<Vec<McpDiscoveredTool>> {
        self.remaining
            .store(MAX_MCP_DISCOVERY_BYTES, std::sync::atomic::Ordering::SeqCst);
        tokio::time::timeout(Duration::from_secs(30), self.list())
            .await
            .map_err(|_| failed("MCP discovery timed out"))?
    }
    async fn call(&self, name: &McpToolName, arguments: Value) -> AppResult<Value> {
        self.remaining
            .store(MAX_MCP_DISCOVERY_BYTES, std::sync::atomic::Ordering::SeqCst);
        if arguments.to_string().len() > MAX_MCP_SCHEMA_BYTES {
            return Err(failed("MCP arguments exceed 64 KiB"));
        }
        let args = arguments
            .as_object()
            .cloned()
            .ok_or_else(|| failed("MCP arguments must be an object"))?;
        let request = rmcp::model::CallToolRequest::new(
            CallToolRequestParams::new(name.as_str().to_owned()).with_arguments(args),
        );
        let handle = self
            .client
            .send_cancellable_request(
                request.into(),
                rmcp::service::PeerRequestOptions::no_options(),
            )
            .await
            .map_err(|_| failed("MCP effect indeterminate; do not retry"))?;
        *self
            .pending_call
            .lock()
            .map_err(|_| failed("MCP session unavailable"))? = Some(handle.id.clone());
        let response = tokio::time::timeout(Duration::from_secs(30), handle.await_response()).await;
        if response.is_ok() {
            self.pending_call
                .lock()
                .map_err(|_| failed("MCP session unavailable"))?
                .take();
        }
        let result = response
            .map_err(|_| failed("MCP effect indeterminate; do not retry"))?
            .map_err(|_| failed("MCP effect indeterminate; do not retry"))?;
        let rmcp::model::ServerResult::CallToolResult(result) = result else {
            return Err(failed("MCP effect indeterminate; do not retry"));
        };
        let result = serde_json::to_value(result).map_err(|_| failed("Invalid MCP result"))?;
        self.reject_secret(&result)?;
        Ok(result)
    }
    async fn close(mut self: Box<Self>) -> AppResult<()> {
        self.cancel_pending().await;
        self.client
            .close_with_timeout(Duration::from_secs(35))
            .await
            .map_err(|_| failed("MCP shutdown failed"))?
            .ok_or_else(|| failed("MCP shutdown timed out"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

fn contains_secret(value: &Value, secret: &str) -> bool {
    match value {
        Value::String(text) => text.contains(secret),
        Value::Array(values) => values.iter().any(|v| contains_secret(v, secret)),
        Value::Object(values) => values
            .iter()
            .any(|(key, v)| key.contains(secret) || contains_secret(v, secret)),
        _ => false,
    }
}

impl HttpMcpClient {
    fn endpoint_permit(&self, endpoint: &McpEndpoint) -> AppResult<OwnedSemaphorePermit> {
        let mut endpoints = self
            .endpoints
            .lock()
            .map_err(|_| failed("MCP capacity unavailable"))?;
        endpoints.retain(|_, v| v.strong_count() > 0);
        let capacity = endpoints
            .get(endpoint)
            .and_then(std::sync::Weak::upgrade)
            .unwrap_or_else(|| Arc::new(Semaphore::new(4)));
        endpoints.insert(endpoint.clone(), Arc::downgrade(&capacity));
        capacity
            .try_acquire_owned()
            .map_err(|_| failed("MCP endpoint capacity exhausted"))
    }
}

impl Session {
    async fn cancel_pending(&self) {
        let pending = self.pending_call.lock().ok().and_then(|mut id| id.take());
        if let Some(request_id) = pending {
            let params = rmcp::model::CancelledNotificationParam {
                request_id,
                reason: Some("Local execution cancelled".into()),
            };
            let _ =
                tokio::time::timeout(Duration::from_secs(3), self.client.notify_cancelled(params))
                    .await;
        }
    }
}
