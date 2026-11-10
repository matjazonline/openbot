//! Manual, company-authorized HTTP setup; no credentials or discovery are accepted here.
use super::*;
use crate::application::{
    agent::{ActionEffect, ActionRecovery, ApprovedActionPolicy},
    workflow::{
        http::{HttpConnectionWrite, HttpOperation, HttpOrigin},
        publication::ToolSnapshot,
        registry::ToolContract,
    },
};
use crate::domain::workflow::TypeName;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Create {
    origin: HttpOrigin,
}
pub(super) async fn create(
    State(s): State<ConnectionServices>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Json(draft): Json<Create>,
) -> AppResult<Json<HttpConnection>> {
    Ok(Json(
        s.workflow
            .http_connections()
            .create_http(
                CompanyId::new(company),
                WorkflowActor::authenticated(user.id)?,
                HttpConnectionWrite {
                    origin: draft.origin,
                    bearer: false,
                },
            )
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Register {
    expected_revision: i64,
    name: String,
    input_schema: Value,
    output_schema: Value,
    effect: ActionEffect,
    approval_required: bool,
}
pub(super) async fn register(
    State(s): State<ConnectionServices>,
    user: AuthenticatedUser,
    Path(ConnectionPath { company, id }): Path<ConnectionPath>,
    Json(draft): Json<Register>,
) -> AppResult<Json<HttpConnection>> {
    let company = CompanyId::new(company);
    let operation = HttpOperation {
        tool: ToolSnapshot {
            workflow: None,
            company_id: company,
            contract: ToolContract {
                name: TypeName::parse(&draft.name)
                    .map_err(|_| AppError::BadRequest("Invalid operation name".into()))?,
                connection: None,
                input_schema: draft.input_schema,
                output_schema: draft.output_schema,
            },
            policy: ApprovedActionPolicy {
                capability: TypeName::parse("http.request").expect("native capability"),
                policy_revision: 1,
                effect: draft.effect,
                recovery: ActionRecovery::Reconcile,
            },
        },
        approval_required: draft.approval_required,
    };
    Ok(Json(
        s.workflow
            .http_connections()
            .register_http_operation(
                company,
                WorkflowActor::authenticated(user.id)?,
                id,
                draft.expected_revision,
                operation,
            )
            .await?,
    ))
}
