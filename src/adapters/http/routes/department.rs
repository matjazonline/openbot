//! JSON API for departments, their members and their roles (01-departments). Managing takes an
//! owner or admin; a plain member lists and reads only the departments they belong to or hold a
//! role in, and every write refuses them as if the department did not exist.
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
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

async fn list_departments_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let list = departments.visible_departments(user.id, company_id).await?;
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
    let write = DepartmentWrite {
        name: payload.name,
        slug,
        description: payload.description,
        charter: payload.charter,
        parent_id: payload.parent_id.map(DepartmentId::new),
        created_by: None,
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

async fn update_department_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<DepartmentUpdatePayload>,
) -> AppResult<impl IntoResponse> {
    let edit = DepartmentEdit {
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

async fn assign_role_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id, role, principal_id)): Path<(Uuid, Uuid, String, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let role = role_from_path(&role)?;
    departments
        .assign_role(
            user.id,
            company_id,
            DepartmentId::new(department_id),
            role,
            PrincipalId::new(principal_id),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_role_json(
    State(departments): State<Arc<DepartmentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, department_id, role, principal_id)): Path<(Uuid, Uuid, String, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let role = role_from_path(&role)?;
    departments
        .clear_role(
            user.id,
            company_id,
            DepartmentId::new(department_id),
            role,
            PrincipalId::new(principal_id),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[path = "department_tests.rs"]
mod tests;
