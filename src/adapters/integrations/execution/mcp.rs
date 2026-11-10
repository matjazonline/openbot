use super::*;
use crate::{
    application::{
        integrations::{discovery::PreparedCatalog, private::InjectionMethod},
        services::mcp_client::McpSession,
    },
    entities::mcp::{McpDiscoveredTool, McpEndpoint},
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

fn matches(i: &AuthorizedInvocation, tools: &[McpDiscoveredTool]) -> bool {
    let OperationTarget::Mcp {
        tool,
        input_schema,
        output_schema,
    } = &i.target
    else {
        return false;
    };
    tools.iter().any(|offered| {
        offered.name == *tool
            && offered.input_schema == *input_schema
            && offered
                .output_schema
                .clone()
                .unwrap_or_else(|| json!({"type":"object"}))
                == output_schema
                    .clone()
                    .unwrap_or_else(|| json!({"type":"object"}))
    })
}
fn preparation_error(
    error: crate::application::services::mcp_client::McpPreparationFailure,
) -> ResolutionError {
    use crate::application::services::mcp_client::McpPreparationFailure as Failure;
    match error {
        Failure::Denied => IntegrationError::Denied.into(),
        // A remote credential refusal does not revoke local execution authority.
        // Keep the gateway's existing unavailable/retry outcome for this path.
        Failure::Changed | Failure::Deadline | Failure::Unavailable | Failure::Authentication => {
            ResolutionError::Retry(RetryReason::Unavailable, None)
        }
        Failure::Cancelled => ResolutionError::Cancelled,
    }
}
pub(super) async fn prepare(
    transports: &IntegrationTransports,
    i: &AuthorizedInvocation,
    _c: &ExecutionContext,
    injection: TypedInjection,
    work: crate::application::services::mcp_client::McpPreparation,
) -> ResolutionResult<Box<dyn PreparedBusinessCall>> {
    let OperationTarget::Mcp {
        tool, input_schema, ..
    } = &i.target
    else {
        return Err(IntegrationError::Invalid.into());
    };
    let schema = crate::application::services::tool_schema::compile(input_schema)
        .map_err(|_| IntegrationError::Invalid)?;
    if !schema.is_valid(i.arguments.value()) {
        return Err(IntegrationError::Invalid.into());
    }
    let opened = open(transports, &i.authorization.destination, injection, work).await?;
    if !matches(i, &opened.tools) {
        opened.close().await?;
        return Err(IntegrationError::Conflict.into());
    }
    Ok(Box::new(McpCall {
        opened,
        tool: tool.clone(),
        arguments: i.arguments.value().clone(),
    }))
}
pub(super) async fn discover(
    transports: &IntegrationTransports,
    c: &OwnerDiscoveryContext,
    injection: TypedInjection,
    work: crate::application::services::mcp_client::McpPreparation,
) -> ResolutionResult<Box<dyn PreparedCatalog>> {
    Ok(Box::new(
        open(transports, &c.authorization.destination, injection, work).await?,
    ))
}
async fn open(
    transports: &IntegrationTransports,
    destination: &crate::domain::integrations::authority::ApprovedDestination,
    injection: TypedInjection,
    work: crate::application::services::mcp_client::McpPreparation,
) -> ResolutionResult<OpenedCatalog> {
    if !matches!(injection.method, InjectionMethod::Bearer) {
        return Err(IntegrationError::Invalid.into());
    }
    let endpoint = McpEndpoint::try_from(destination.as_str().to_owned())
        .map_err(|_| IntegrationError::Invalid)?;
    transports
        .mcp
        .validate_endpoint(&endpoint)
        .map_err(|_| IntegrationError::Denied)?;
    let credential = injection.value.clone();
    // This port owns cancellation and awaited initialization cleanup. An outer timeout
    // must not drop its completion wait and leave SDK preparation work alive.
    let session = transports
        .mcp
        .connect_prepared(&endpoint, injection.value, work.clone())
        .await
        .map_err(preparation_error)?;
    let discovered = match work.check().await {
        Ok(()) => preflight(work.deadline, &work.cancellation, async {
            session
                .discover()
                .await
                .map_err(|_| IntegrationError::Unavailable)
        })
        .await
        .map_err(|error| {
            work.failure()
                .map(preparation_error)
                .unwrap_or_else(|| error.into())
        }),
        Err(error) => Err(preparation_error(error)),
    };
    let tools = match discovered
        .and_then(|tools| sanitized_catalog(tools, &credential).map_err(Into::into))
    {
        Ok(tools) => tools,
        Err(error) => {
            session
                .close()
                .await
                .map_err(|_| IntegrationError::Unavailable)?;
            return Err(error);
        }
    };
    Ok(OpenedCatalog {
        credential,
        session: Mutex::new(Some(session)),
        tools,
    })
}
fn sanitized_catalog(
    tools: Vec<McpDiscoveredTool>,
    credential: &secrecy::SecretString,
) -> IntegrationResult<Vec<McpDiscoveredTool>> {
    let tools = DiscoveredCatalog::from_redacted(tools)?.into_tools();
    let original = serde_json::to_value(&tools).map_err(|_| IntegrationError::Invalid)?;
    let mut redacted = original.clone();
    if !crate::adapters::redaction::redact_json(&mut redacted, Some(credential))
        || original != redacted
    {
        return Err(IntegrationError::Credential);
    }
    Ok(tools)
}
struct OpenedCatalog {
    credential: secrecy::SecretString,
    session: Mutex<Option<Box<dyn McpSession>>>,
    tools: Vec<McpDiscoveredTool>,
}
#[async_trait]
impl PreparedCatalog for OpenedCatalog {
    fn tools(&self) -> &[McpDiscoveredTool] {
        &self.tools
    }
    async fn close(&self) -> IntegrationResult<()> {
        if let Some(session) = self.session.lock().await.take() {
            // Reuse the supervised adapter's bounded35s close; never detach the worker.
            session
                .close()
                .await
                .map_err(|_| IntegrationError::Unavailable)?;
        }
        Ok(())
    }
}
struct McpCall {
    opened: OpenedCatalog,
    tool: crate::entities::mcp::McpToolName,
    arguments: Value,
}
#[async_trait]
impl PreparedBusinessCall for McpCall {
    async fn call(&self) -> IntegrationResult<ProviderData> {
        let session = self.opened.session.lock().await;
        let session = session.as_ref().ok_or(IntegrationError::Denied)?;
        let data = session
            .call(&self.tool, self.arguments.clone())
            .await
            .map_err(|_| IntegrationError::Unavailable)?;
        let mut data = data.envelope();
        if !crate::adapters::redaction::redact_json(&mut data, Some(&self.opened.credential)) {
            return Err(IntegrationError::Invalid);
        }
        ProviderData::from_redacted(data)
    }
    async fn close(&self) -> IntegrationResult<()> {
        self.opened.close().await
    }
}
