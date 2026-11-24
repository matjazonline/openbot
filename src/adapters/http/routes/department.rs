//! JSON API for departments, their members and their roles (01-departments). Managing takes an
//! owner or admin; a plain member lists and reads only the departments they belong to or hold a
//! role in, and every write refuses them as if the department did not exist.
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser},
    app_error::{AppError, AppResult},
    application::ports::department::{DepartmentEdit, DepartmentWrite},
    entities::{
        department::{Department, DepartmentId, DepartmentRole},
        transport::PrincipalId,
    },
    use_cases::department::DepartmentUseCases,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/companies/{company_id}/departments",
            get(list_departments_json).post(create_department_json),
        )
        .route(
            "/api/companies/{company_id}/departments/{department_id}",
            get(get_department_json)
                .patch(update_department_json)
                .delete(delete_department_json),
        )
        .route(
            "/api/companies/{company_id}/departments/{department_id}/rollup",
            get(goal_rollup_json),
        )
        .route(
            "/api/companies/{company_id}/departments/{department_id}/archive",
            post(archive_department_json),
        )
        .route(
            "/api/companies/{company_id}/departments/{department_id}/restore",
            post(restore_department_json),
        )
        .route(
            "/api/companies/{company_id}/departments/{department_id}/members/{principal_id}",
            put(add_member_json).delete(remove_member_json),
        )
        .route(
            "/api/companies/{company_id}/departments/{department_id}/roles/{role}/{principal_id}",
            put(assign_role_json).delete(clear_role_json),
        )
}

#[derive(Debug, Deserialize)]
pub struct DepartmentCreatePayload {
    pub name: String,
    /// Derived from the name when omitted, as a channel's is.
    pub slug: Option<String>,
    pub description: Option<String>,
    pub charter: Option<String>,
    pub parent_id: Option<Uuid>,
    /// The principal to hold the `lead` role from the start.
    pub lead: Option<Uuid>,
    pub escalation: Option<crate::entities::workflow_escalation::EscalationPolicy>,
    /// `goal` makes a goal unit (Plan08 03A); absent means standing.
    #[serde(default)]
    pub kind: crate::entities::department::DepartmentKind,
    /// A goal's target date, `YYYY-MM-DD`.
    pub target_date: Option<chrono::NaiveDate>,
    /// A goal's sponsors, in order; the first one decides.
    #[serde(default)]
    pub sponsors: Vec<Uuid>,
}

impl DepartmentCreatePayload {
    /// The goal part of a create: only a goal takes a target date or sponsors.
    fn goal(&self) -> AppResult<Option<crate::application::ports::department::GoalWrite>> {
        use crate::entities::department::DepartmentKind;
        match self.kind {
            DepartmentKind::Goal => Ok(Some(crate::application::ports::department::GoalWrite {
                target_date: self.target_date,
                sponsors: self.sponsors.iter().copied().map(PrincipalId::new).collect(),
            })),
            DepartmentKind::Standing if self.target_date.is_some() || !self.sponsors.is_empty() => {
                Err(AppError::BadRequest(
                    "department.target_date_goal_only: only a goal takes a target date or sponsors."
                        .into(),
                ))
            }
            DepartmentKind::Standing => Ok(None),
        }
    }
}

/// Only the fields present change. `null` clears an optional field; for `parent_id` it makes the
/// department a root.
#[derive(Debug, Default, Deserialize)]
pub struct DepartmentUpdatePayload {
    pub name: Option<String>,
    pub slug: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    pub charter: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    pub parent_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "present")]
    pub escalation: Option<Option<crate::entities::workflow_escalation::EscalationPolicy>>,
    pub kind: Option<crate::entities::department::DepartmentKind>,
    #[serde(default, deserialize_with = "present")]
    pub target_date: Option<Option<chrono::NaiveDate>>,
}

/// A field that is present, even as `null`, is `Some`; an absent one stays `None` via `default`.
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Serialize)]
struct DepartmentResponse {
    success: bool,
    department: Department,
}

fn role_from_path(role: &str) -> AppResult<DepartmentRole> {
    role.parse().map_err(
        |error: crate::entities::department::InvalidDepartmentRole| {
            AppError::BadRequest(error.to_string())
        },
    )
}

/// `?include_archived=true` lists archived departments too (Plan08 02A.5).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    #[serde(default)]
    include_archived: bool,
}

async fn list_departments_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> AppResult<impl IntoResponse> {
    use crate::entities::department::ArchivedUnits;
    let archived = match query.include_archived {
        true => ArchivedUnits::Include,
        false => ArchivedUnits::Exclude,
    };
    let list = departments
        .visible_departments(user.id, company_id, archived)
        .await?;
    Ok((StatusCode::OK, Json(list)))
}

async fn create_department_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Json(payload): Json<DepartmentCreatePayload>,
) -> AppResult<impl IntoResponse> {
    let slug = payload
        .slug
        .as_deref()
        .map(str::trim)
        .filter(|slug| !slug.is_empty())
        .map(String::from)
        .unwrap_or_else(|| crate::adapters::http::routes::channel::slugify(&payload.name));
    let goal = payload.goal()?;
    let write = DepartmentWrite {
        goal,
        name: payload.name,
        slug,
        description: payload.description,
        charter: payload.charter,
        parent_id: payload.parent_id.map(DepartmentId::new),
        created_by: None,
        escalation: payload.escalation,
    };
    let department = departments
        .create_department(
            user.id,
            company_id,
            write,
            payload.lead.map(PrincipalId::new),
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(DepartmentResponse {
            success: true,
            department,
        }),
    ))
}

async fn get_department_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let detail = departments
        .visible_department(user.id, company_id, DepartmentId::new(department_id))
        .await?;
    Ok((StatusCode::OK, Json(detail)))
}

/// A goal's status (Plan08 03D.3): the same rollup the goal's page shows.
async fn goal_rollup_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let rollup = departments
        .goal_rollup(user.id, company_id, DepartmentId::new(department_id))
        .await?;
    Ok((StatusCode::OK, Json(rollup)))
}

async fn update_department_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<DepartmentUpdatePayload>,
) -> AppResult<impl IntoResponse> {
    let edit = DepartmentEdit {
        kind: payload.kind,
        target_date: payload.target_date,
        escalation: payload.escalation,
        name: payload.name,
        slug: payload.slug,
        description: payload.description,
        charter: payload.charter,
        parent_id: payload
            .parent_id
            .map(|parent| parent.map(DepartmentId::new)),
    };
    let department = departments
        .update_department(user.id, company_id, DepartmentId::new(department_id), edit)
        .await?;
    Ok((
        StatusCode::OK,
        Json(DepartmentResponse {
            success: true,
            department,
        }),
    ))
}

async fn delete_department_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    departments
        .delete_department(user.id, company_id, DepartmentId::new(department_id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `cancel_open_runs` asks to cancel the subtree's unfinished runs and archive in one write;
/// without it, open work refuses with `unit.archive_open_work` and its count.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivePayload {
    #[serde(default)]
    cancel_open_runs: bool,
}

async fn archive_department_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    payload: Option<Json<ArchivePayload>>,
) -> AppResult<impl IntoResponse> {
    use crate::application::ports::department::OpenWork;
    let open = if payload.unwrap_or_default().cancel_open_runs {
        OpenWork::Cancel
    } else {
        OpenWork::Refuse
    };
    let department = departments
        .archive_department(user.id, company_id, DepartmentId::new(department_id), open)
        .await?;
    Ok((
        StatusCode::OK,
        Json(DepartmentResponse {
            success: true,
            department,
        }),
    ))
}

async fn restore_department_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let department = departments
        .restore_department(user.id, company_id, DepartmentId::new(department_id))
        .await?;
    Ok((
        StatusCode::OK,
        Json(DepartmentResponse {
            success: true,
            department,
        }),
    ))
}

async fn add_member_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id, principal_id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    departments
        .add_member(
            user.id,
            company_id,
            DepartmentId::new(department_id),
            PrincipalId::new(principal_id),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_member_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id, principal_id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    departments
        .remove_member(
            user.id,
            company_id,
            DepartmentId::new(department_id),
            PrincipalId::new(principal_id),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct RoleAssignmentPayload {
    decider: bool,
}

#[derive(Default, Deserialize)]
struct RoleRemovalQuery {
    successor: Option<Uuid>,
}

async fn assign_role_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id, role, principal_id)): Path<(Uuid, Uuid, String, Uuid)>,
    Json(payload): Json<RoleAssignmentPayload>,
) -> AppResult<impl IntoResponse> {
    let role = role_from_path(&role)?;
    departments
        .assign_role(
            user.id,
            company_id,
            DepartmentId::new(department_id),
            crate::application::ports::department::DepartmentRoleAssignment {
                role,
                principal: PrincipalId::new(principal_id),
                decider: payload.decider,
            },
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_role_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id, role, principal_id)): Path<(Uuid, Uuid, String, Uuid)>,
    axum::extract::Query(query): axum::extract::Query<RoleRemovalQuery>,
) -> AppResult<impl IntoResponse> {
    let role = role_from_path(&role)?;
    departments
        .clear_role(
            user.id,
            company_id,
            DepartmentId::new(department_id),
            crate::application::ports::department::DepartmentRoleRemoval {
                role,
                principal: PrincipalId::new(principal_id),
                successor: query.successor.map(PrincipalId::new),
            },
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The archive and restore routes alone, over just the use case, for tests that drive them with a
/// verified session and a database fixture of their own.
#[cfg(test)]
pub(crate) fn lifecycle_router() -> Router<Arc<DepartmentUseCases>> {
    Router::new()
        .route(
            "/api/companies/{company_id}/departments/{department_id}/archive",
            post(archive_department_json),
        )
        .route(
            "/api/companies/{company_id}/departments/{department_id}/restore",
            post(restore_department_json),
        )
}

#[cfg(test)]
#[path = "department_tests.rs"]
mod tests;
