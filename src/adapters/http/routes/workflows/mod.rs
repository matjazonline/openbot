//! Company-scoped workflow routes (p08 B3b-1): JSON over the owning application commands for
//! definitions, templates, bindings, runs, decisions and workflow tools, and the decision page
//! the review notices link to. The commands authorize; a read authorizes the company and the
//! record's own association before showing it. Writes that create work take a client key;
//! edits take the revision the caller saw.
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser},
    app_error::{AppError, AppResult},
    application::workflow::{
        RelatedAssociation, WorkflowActor, WorkflowOperation, compiler::Diagnostic,
        lifecycle::LifecycleError,
    },
    domain::workflow::CompanyId,
    infra::workflow::WorkflowEngine,
};

mod bindings;
mod decisions;
mod definitions;
mod jev;
mod reconciliations;
mod tools;
mod ui;

/// A save carries a source of up to `MAX_SOURCE_BYTES` (256 KiB) as a JSON string, which escaping
/// can grow up to six-fold; nothing else here comes close.
const BODY_LIMIT: usize = 2 * 1024 * 1024;

pub fn router() -> Router<AppState> {
    let company = "/api/companies/{company_id}";
    Router::new()
        .route(
            &format!("{company}/workflow-definitions"),
            get(definitions::list),
        )
        .route(
            &format!("{company}/workflow-definitions/{{workflow_id}}"),
            get(definitions::read).put(definitions::save),
        )
        .route(
            &format!("{company}/workflow-definitions/{{workflow_id}}/validate"),
            post(definitions::validate),
        )
        .route(
            &format!("{company}/workflow-definitions/{{workflow_id}}/publish"),
            post(definitions::publish),
        )
        .route(
            &format!("{company}/workflow-definitions/{{workflow_id}}/archive"),
            post(definitions::archive),
        )
        .route(
            &format!("{company}/workflow-templates"),
            get(definitions::templates),
        )
        .route(
            &format!("{company}/workflow-templates/{{template_id}}/copy"),
            post(definitions::copy_template),
        )
        .route(&format!("{company}/workflow-bindings"), get(bindings::list))
        .route(
            &format!("{company}/workflow-bindings/{{binding_id}}"),
            get(bindings::read).put(bindings::configure),
        )
        .route(
            &format!("{company}/workflow-bindings/{{binding_id}}/activate"),
            post(bindings::activate),
        )
        .route(
            &format!("{company}/workflow-bindings/{{binding_id}}/deactivate"),
            post(bindings::deactivate),
        )
        .route(
            &format!("{company}/workflow-bindings/{{binding_id}}/runs"),
            post(bindings::start_run),
        )
        .route(&format!("{company}/workflow-runs"), get(bindings::runs))
        .route(
            &format!("{company}/workflow-runs/{{run_id}}"),
            get(bindings::run),
        )
        .route(
            &format!("{company}/workflow-runs/{{run_id}}/cancel"),
            post(bindings::cancel),
        )
        .route(
            &format!("{company}/workflow-runs/{{run_id}}/retry"),
            post(bindings::retry),
        )
        .route(
            &format!("{company}/workflow-runs/{{run_id}}/reconciliations"),
            post(reconciliations::reconcile),
        )
        .route(
            &format!("{company}/workflow-decisions/{{decision_id}}"),
            get(decisions::read),
        )
        .route(
            &format!("{company}/workflow-decisions/{{decision_id}}/submit"),
            post(decisions::submit),
        )
        .route(
            &format!("{company}/workflow-decisions/{{decision_id}}/comments"),
            post(decisions::comment),
        )
        .route("/ui/workflow-decisions/{decision_id}", get(decisions::page))
        .route(
            "/ui/workflow-decisions/{decision_id}/submit",
            post(decisions::ui_submit),
        )
        .route(
            "/ui/workflow-decisions/{decision_id}/comments",
            post(decisions::ui_comment),
        )
        .route(&format!("{company}/workflow-tools"), get(tools::list))
        .route(
            &format!("{company}/workflow-tools/{{name}}"),
            put(tools::register).delete(tools::revoke),
        )
        .route(
            &format!("{company}/agents/{{agent_id}}/workflow-tools"),
            put(tools::grant),
        )
        .merge(jev::router())
        .merge(ui::router())
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
}

/// A route's failure: an application error as the app maps it, or a compiler diagnostic as a
/// 422 the editor can point at.
#[derive(Debug)]
pub enum WorkflowError {
    App(AppError),
    Diagnostic(Box<Diagnostic>),
}
impl From<AppError> for WorkflowError {
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}
impl From<LifecycleError> for WorkflowError {
    fn from(error: LifecycleError) -> Self {
        match error {
            LifecycleError::Application(error) => Self::App(error),
            LifecycleError::Validation(diagnostic) => Self::Diagnostic(diagnostic),
        }
    }
}
impl From<crate::application::workflow::templates::TemplateError> for WorkflowError {
    fn from(error: crate::application::workflow::templates::TemplateError) -> Self {
        use crate::application::workflow::templates::TemplateError;
        match error {
            TemplateError::Application(error) => Self::App(error),
            TemplateError::Validation(diagnostic) => Self::Diagnostic(diagnostic),
            TemplateError::Missing | TemplateError::Duplicate(_) => {
                Self::App(AppError::NotFound("Workflow template not found".into()))
            }
            TemplateError::StaleRevision => Self::App(AppError::Conflict(
                "The template revision changed; reload the catalogue".into(),
            )),
            TemplateError::Limit(limit) => {
                Self::App(AppError::BadRequest(format!("Template limit: {limit}")))
            }
        }
    }
}
impl IntoResponse for WorkflowError {
    fn into_response(self) -> Response {
        match self {
            // Lock contention under the reconciliation budget is retryable (P08-D3 F070); the
            // app answers other timeouts with 500. Logged here, because this branch bypasses
            // `AppError::into_response`, which is where every other failed request is.
            Self::App(AppError::Timeout(message)) => {
                tracing::warn!(error = %message, "Workflow request timed out; answered 503");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [(axum::http::header::RETRY_AFTER, "1")],
                    message,
                )
                    .into_response()
            }
            Self::App(error) => error.into_response(),
            Self::Diagnostic(diagnostic) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"diagnostic": {
                    "code": diagnostic.code,
                    "message": diagnostic.message.as_ref(),
                    "field_path": diagnostic.field_path.as_ref(),
                    "line": diagnostic.span.line,
                    "column": diagnostic.span.column,
                    "instance_path": diagnostic.instance_path.as_deref(),
                    "schema_path": diagnostic.schema_path.as_deref(),
                }})),
            )
                .into_response(),
        }
    }
}

pub type WorkflowResult<T> = Result<T, WorkflowError>;

fn actor(user: &AuthenticatedUser) -> AppResult<WorkflowActor> {
    WorkflowActor::authenticated(user.id)
}

fn company(id: Uuid) -> CompanyId {
    CompanyId::new(id)
}

/// Owner/admin of the company who can see `association`; otherwise the record is not found.
async fn authorize(
    engine: &WorkflowEngine,
    company: CompanyId,
    actor: WorkflowActor,
    association: RelatedAssociation,
) -> AppResult<()> {
    engine
        .authorization()
        .authorize(company, actor, association, WorkflowOperation::Inspect)
        .await
}

fn association_json(association: RelatedAssociation) -> Value {
    match association {
        RelatedAssociation::Company => json!({"kind": "company"}),
        RelatedAssociation::Channel(channel) => {
            json!({"kind": "channel", "channel_id": channel.as_uuid()})
        }
        RelatedAssociation::Thread {
            channel_id,
            thread_id,
        } => json!({"kind": "thread", "channel_id": channel_id.as_uuid(),
            "thread_id": thread_id.as_uuid()}),
    }
}

/// `{kind: company}`, `{kind: channel, channel_id}` or `{kind: thread, channel_id, thread_id}`.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AssociationBody {
    Company,
    Channel { channel_id: Uuid },
    Thread { channel_id: Uuid, thread_id: Uuid },
}
impl From<AssociationBody> for RelatedAssociation {
    fn from(body: AssociationBody) -> Self {
        use crate::application::workflow::{RelatedChannelId, RelatedThreadId};
        match body {
            AssociationBody::Company => Self::Company,
            AssociationBody::Channel { channel_id } => {
                Self::Channel(RelatedChannelId::new(channel_id))
            }
            AssociationBody::Thread {
                channel_id,
                thread_id,
            } => Self::Thread {
                channel_id: RelatedChannelId::new(channel_id),
                thread_id: RelatedThreadId::new(thread_id),
            },
        }
    }
}

fn ok<T: Serialize>(value: T) -> Json<T> {
    Json(value)
}

/// `?after=<id of the last row seen>&limit=<1..=200>`.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    pub after: Option<Uuid>,
    pub limit: Option<i64>,
}
impl ListQuery {
    fn page(&self) -> AppResult<crate::application::workflow::reads::PageRequest> {
        crate::application::workflow::reads::PageRequest::new(self.after, self.limit)
    }
}

/// `{items, next}`: `next` is the cursor for the following page, `null` on the last one. A page
/// may hold fewer items than its size (rows the caller may not see are left out) and still have
/// a `next`.
fn page_json(items: Vec<Value>, next: Option<Uuid>) -> Json<Value> {
    Json(json!({"items": items, "next": next}))
}

/// `UUIDv8(SHA-256(domain ‖ company ‖ key))`: the identity a keyed create derives, so a retry
/// with the key names the same record.
fn keyed_id(domain: &str, company: CompanyId, key: &str) -> Uuid {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update(b"\0");
    digest.update(company.as_uuid().as_bytes());
    digest.update(key.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.finalize()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn revision<T>(value: u64, parse: impl FnOnce(u64) -> Option<T>) -> AppResult<T> {
    parse(value).ok_or_else(|| AppError::BadRequest("A revision is a positive number".into()))
}

/// The engine a workflow route runs over, from the app state.
pub type Engine = Arc<WorkflowEngine>;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
