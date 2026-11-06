//! Company workflow tool registrations: register the observed revision, revoke and list.
//! Agent-local assignments are edited through the guarded agent authoring command.
use super::*;
use crate::application::workflow::tools::{WorkflowToolRegistration, WorkflowToolWrite};
use crate::domain::workflow::{TypeName, VersionId, WorkflowId};
use axum::extract::{Path, State};
use serde::Deserialize;

fn registration_json(registration: &WorkflowToolRegistration) -> Value {
    json!({
        "name": registration.name.as_str(),
        "workflow_id": registration.workflow.as_uuid(),
        "version_id": registration.version.as_uuid(),
        "params": registration.params,
        "revision": registration.revision,
    })
}

fn tool_name(value: &str) -> AppResult<TypeName> {
    TypeName::parse(value)
        .map_err(|_| AppError::BadRequest(format!("'{value}' is not a valid tool name")))
}

pub async fn list(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> WorkflowResult<Json<Value>> {
    let tools = engine
        .workflow_tools()
        .workflow_tools(company(company_id), actor(&user)?)
        .await?;
    Ok(ok(json!(
        tools.iter().map(registration_json).collect::<Vec<_>>()
    )))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterBody {
    pub workflow_id: Uuid,
    pub version_id: Uuid,
    #[serde(default = "empty_params")]
    pub params: Value,
    /// Absent registers a new name; present must be the registration's current revision.
    pub expected_revision: Option<i64>,
}

fn empty_params() -> Value {
    json!({})
}

pub async fn register(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, name)): Path<(Uuid, String)>,
    Json(body): Json<RegisterBody>,
) -> WorkflowResult<(StatusCode, Json<Value>)> {
    let created = body.expected_revision.is_none();
    let registration = engine
        .workflow_tools()
        .register_workflow_tool(
            company(company_id),
            actor(&user)?,
            WorkflowToolWrite {
                name: tool_name(&name)?,
                workflow: WorkflowId::new(body.workflow_id),
                version: VersionId::new(body.version_id),
                params: body.params,
                expected: body.expected_revision,
            },
        )
        .await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, ok(registration_json(&registration))))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeBody {
    pub expected_revision: i64,
}

pub async fn revoke(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, name)): Path<(Uuid, String)>,
    Json(body): Json<RevokeBody>,
) -> WorkflowResult<StatusCode> {
    engine
        .workflow_tools()
        .revoke_workflow_tool(
            company(company_id),
            actor(&user)?,
            tool_name(&name)?,
            body.expected_revision,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
