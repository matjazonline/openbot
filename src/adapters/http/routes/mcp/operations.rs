use super::*;
use crate::application::{
    agent::ActionEffect,
    workflow::{WorkflowActor, mcp::*},
};
use crate::domain::workflow::{CompanyId, TypeName};
use crate::infra::workflow::WorkflowServices;
use axum::extract::{FromRef, Query};
use std::sync::Arc;

pub(super) fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<crate::use_cases::mcp::McpUseCases>: FromRef<S>,
    Arc<WorkflowServices>: FromRef<S>,
{
    Router::new()
        .route(
            "/api/companies/{company}/mcp-connections/{id}/operations",
            get(list),
        )
        .route(
            "/api/companies/{company}/mcp-connections/{id}/operations/{tool}",
            put(review).delete(revoke),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    after: Option<TypeName>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    expected_revision: i64,
    expected_operation_version: Option<Uuid>,
    effect: ActionEffect,
    approval_required: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Revoke {
    expected_revision: i64,
    expected_operation_version: Uuid,
}

async fn list(
    State(mcp): State<Arc<crate::use_cases::mcp::McpUseCases>>,
    State(engine): State<Arc<WorkflowServices>>,
    user: AuthenticatedUser,
    Path((company, connection)): Path<(Uuid, Uuid)>,
    Query(query): Query<PageQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let current = mcp.get(user.id, company, connection).await?;
    let page = engine
        .mcp_operations()
        .reviewed_mcp_operations(
            CompanyId::new(company),
            WorkflowActor::authenticated(user.id)?,
            McpOperationRead {
                connection,
                catalog_revision: current.revision,
                selection: McpOperationSelection::Page { after: query.after },
            },
        )
        .await?;
    Ok(Json(
        serde_json::json!({"catalog_revision":current.revision,"operations":page.operations,"next":page.next}),
    ))
}

async fn review(
    State(mcp): State<Arc<crate::use_cases::mcp::McpUseCases>>,
    State(engine): State<Arc<WorkflowServices>>,
    user: AuthenticatedUser,
    Path((company, connection, name)): Path<(Uuid, Uuid, String)>,
    Json(body): Json<Review>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    let current = mcp.get(user.id, company, connection).await?;
    revision(&current, body.expected_revision)?;
    let name = tool_name(&name)?;
    let company = CompanyId::new(company);
    let actor = WorkflowActor::authenticated(user.id)?;
    let mut previous = engine
        .mcp_operations()
        .reviewed_mcp_operations(
            company,
            actor,
            McpOperationRead {
                connection,
                catalog_revision: current.revision,
                selection: McpOperationSelection::Exact(name.clone()),
            },
        )
        .await?
        .operations;
    let policy_revision = match previous.pop() {
        Some(previous) => previous
            .operation
            .tool
            .policy
            .policy_revision
            .checked_add(1)
            .ok_or_else(invalid)?,
        None => 1,
    };
    let discovered = current
        .discovered_tools
        .iter()
        .find(|tool| tool.name.as_str() == name.as_str())
        .ok_or_else(invalid)?;
    let operation = McpOperation::review(
        company,
        discovered,
        body.effect,
        body.approval_required,
        policy_revision,
    )?;
    let version = engine
        .mcp_operations()
        .register_mcp_operation(
            company,
            actor,
            McpOperationWrite {
                connection,
                catalog_revision: current.revision,
                expected_version: body.expected_operation_version,
                operation: operation.clone(),
            },
        )
        .await?;
    let status = if body.expected_operation_version.is_none() {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        Json(
            serde_json::json!({"version":version,"catalog_revision":current.revision,"operation":operation}),
        ),
    ))
}

async fn revoke(
    State(mcp): State<Arc<crate::use_cases::mcp::McpUseCases>>,
    State(engine): State<Arc<WorkflowServices>>,
    user: AuthenticatedUser,
    Path((company, connection, name)): Path<(Uuid, Uuid, String)>,
    Json(body): Json<Revoke>,
) -> AppResult<StatusCode> {
    let current = mcp.get(user.id, company, connection).await?;
    revision(&current, body.expected_revision)?;
    engine
        .mcp_operations()
        .revoke_mcp_operation(
            CompanyId::new(company),
            WorkflowActor::authenticated(user.id)?,
            McpOperationRevoke {
                connection,
                catalog_revision: current.revision,
                name: tool_name(&name)?,
                version: body.expected_operation_version,
            },
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn tool_name(name: &str) -> AppResult<TypeName> {
    TypeName::parse(name).map_err(|_| AppError::BadRequest("Unsupported MCP operation name".into()))
}

#[cfg(test)]
#[path = "operation_tests.rs"]
mod tests;
