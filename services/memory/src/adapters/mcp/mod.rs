//! MCP tools generated from the client operations of the registry; each call runs the same
//! `App::execute` as HTTP with the principal authenticated by the HTTP layer, under the same
//! per-operation body limit and request deadline.
use crate::{
    adapters::http::deadline_exceeded,
    application::{
        operations::{OPERATIONS, Surface, by_tool},
        service::{App, Principal},
    },
    domain::error::{ErrorCode, ServiceError},
};
use chrono::Utc;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, Implementation, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
};
use serde_json::Value;
use std::{future::Future, sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Clone)]
pub struct McpHandler {
    app: App,
    deadline: Duration,
}

impl McpHandler {
    pub fn new(app: App, deadline: Duration) -> Self {
        Self { app, deadline }
    }
}

/// Run one tool call under `deadline`. Dropping the future on expiry cancels the use case, as
/// the HTTP deadline does for `/v1`; the caller gets the `timeout` envelope as a tool error.
pub async fn bounded<F>(deadline: Duration, request_id: Uuid, call: F) -> CallToolResult
where
    F: Future<Output = Result<Value, ServiceError>>,
{
    match tokio::time::timeout(deadline, call).await {
        Ok(Ok(value)) => CallToolResult::structured(value),
        Ok(Err(error)) => structured_error(&error, request_id),
        Err(_) => structured_error(&deadline_exceeded(), request_id),
    }
}

pub fn tools() -> Vec<Tool> {
    OPERATIONS
        .iter()
        .filter(|spec| spec.surface == Surface::Client)
        .filter_map(|spec| {
            let name = spec.tool?;
            let schema = match (spec.request_root)().to_value() {
                Value::Object(object) => object,
                _ => return None,
            };
            Some(Tool::new(name, spec.summary, Arc::new(schema)))
        })
        .collect()
}

impl ServerHandler for McpHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("memory-service", env!("CARGO_PKG_VERSION")),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let request_id = Uuid::now_v7();
        let Some(spec) = by_tool(&request.name) else {
            return Err(McpError::invalid_params("Unknown tool", None));
        };
        let parts = context.extensions.get::<http::request::Parts>();
        let principal = parts
            .and_then(|parts| parts.extensions.get::<Principal>())
            .cloned();
        // The HTTP request's `x-correlation-id` applies to the one call it carries (stateless
        // mode, 06A A2-2); a malformed one is `invalid_request`.
        let correlation =
            match parts.map(|parts| crate::adapters::http::correlation(&parts.headers)) {
                Some(Err(error)) => return Ok(structured_error(&error, request_id)),
                Some(Ok(correlation)) => correlation,
                None => None,
            };
        let Some(principal) = principal else {
            let error = ServiceError::unauthenticated();
            return Ok(structured_error(&error, request_id));
        };
        let input = Value::Object(request.arguments.unwrap_or_default());
        // The same per-operation body limit as `/v1`, applied to the tool's argument object.
        let size = serde_json::to_vec(&input).map_or(usize::MAX, |bytes| bytes.len());
        let limit = spec.body_limit.bytes();
        if size > limit {
            let error = ServiceError::new(
                ErrorCode::PayloadTooLarge,
                format!("The request body exceeds {limit} bytes"),
            );
            return Ok(structured_error(&error, request_id));
        }
        let ids = crate::application::service::RequestIds {
            request: request_id,
            correlation,
        };
        let call = self.app.execute(spec, &principal, input, Utc::now(), ids);
        Ok(bounded(self.deadline, request_id, call).await)
    }
}

fn structured_error(error: &ServiceError, request_id: Uuid) -> CallToolResult {
    CallToolResult::structured_error(
        serde_json::to_value(error.body(request_id.to_string())).unwrap_or(Value::Null),
    )
}
