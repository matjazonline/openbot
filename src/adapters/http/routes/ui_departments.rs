//! The company pane's Departments tab (01-departments 04A): reads for every member of the company,
//! writes for its owner and admins only.
//!
//! Every request returns the tab's `#department-section` whole. A write first checks that the
//! caller manages the company, so a member or a stranger gets the same `404` and learns nothing;
//! past that, a refusal from [`DepartmentUseCases`] — a taken slug, a cycle, a department that
//! still owns desks — comes back on the form that asked for the change.

use std::sync::Arc;

use axum::{
    Form, Router,
    extract::{FromRequestParts, Path},
    http::request::Parts,
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{
        app_state::AppState,
        auth::{AuthError, AuthenticatedUser},
        pages::{self, DepartmentDetailPane, DepartmentDraft, DepartmentForm},
    },
    app_error::{AppError, AppResult},
    application::ports::department::{DepartmentEdit, DepartmentWrite},
    entities::{
        company::CompanyAccess,
        department::{DepartmentId, DepartmentRole},
        transport::PrincipalId,
    },
    infra::config::AppConfig,
    use_cases::{
        company::{CompanyUseCases, company_not_found},
        department::DepartmentUseCases,
    },
};

use super::channel::slugify;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/ui/companies/{company_id}/departments",
            get(close_department).post(create_department),
        )
        .route(
            "/ui/companies/{company_id}/departments/new",
            get(new_department),
        )
        .route(
            "/ui/companies/{company_id}/departments/{department_id}",
            get(show_department)
                .put(update_department)
                .delete(delete_department),
        )
        .route(
            "/ui/companies/{company_id}/departments/{department_id}/members",
            post(add_member),
        )
        .route(
            "/ui/companies/{company_id}/departments/{department_id}/members/{principal_id}",
            delete(remove_member),
        )
        .route(
            "/ui/companies/{company_id}/departments/{department_id}/lead",
            put(assign_lead),
        )
        .route(
            "/ui/companies/{company_id}/departments/{department_id}/lead/{principal_id}",
            delete(clear_lead),
        )
}

/// What the tab's right-hand column shows.
pub(super) enum DepartmentSelection {
    None,
    Create(DepartmentDraft),
    Department {
        id: DepartmentId,
        draft: Option<DepartmentDraft>,
    },
}

/// Renders the tab's section for `access`'s company: the departments the caller may see, the
/// selection, and the forms when they manage the company. Shared with the company pane, which
/// draws the same section under its tabs.
pub(super) async fn department_section(
    departments: &DepartmentUseCases,
    config: &AppConfig,
    user_id: Uuid,
    access: &CompanyAccess,
    selection: DepartmentSelection,
    error: Option<(DepartmentForm, &str)>,
    notice: Option<&str>,
) -> AppResult<String> {
    let company_id = access.company.id;
    let editable = access.membership.manages_company_operations();
    let listed = departments.visible_departments(user_id, company_id).await?;
    let (choices, choices_cut) = if editable {
        departments.principal_choices(user_id, company_id).await?
    } else {
        (Vec::new(), false)
    };
    let view = match &selection {
        DepartmentSelection::Department { id, .. } => Some(
            departments
                .department_view(user_id, company_id, *id)
                .await?,
        ),
        _ => None,
    };
    let detail = match (&selection, &view) {
        (DepartmentSelection::Create(draft), _) if editable => DepartmentDetailPane::Create(draft),
        (DepartmentSelection::Department { draft, .. }, Some(view)) => {
            DepartmentDetailPane::Department {
                view,
                draft: draft.as_ref(),
            }
        }
        _ => DepartmentDetailPane::None,
    };
    Ok(pages::company_departments_section(
        &pages::DepartmentSection {
            company: &access.company,
            app_domain_name: &config.app_domain_name,
            departments: &listed,
            detail,
            choices: &choices,
            choices_cut,
            editable,
            error,
            notice,
        },
    ))
}

struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    department_use_cases: Arc<DepartmentUseCases>,
    config: Arc<AppConfig>,
    user_id: Uuid,
}

impl FromRequestParts<AppState> for Workspace {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;
        Ok(Self {
            company_use_cases: state.company_use_cases.clone(),
            department_use_cases: state.department_use_cases.clone(),
            config: state.config.clone(),
            user_id: user.id,
        })
    }
}

impl Workspace {
    async fn readable(&self, company_id: Uuid) -> AppResult<CompanyAccess> {
        self.company_use_cases
            .readable_company(self.user_id, company_id)
            .await
    }

    /// The company, only for its owner or an admin: anyone else gets the stranger's `404`.
    async fn managed(&self, company_id: Uuid) -> AppResult<CompanyAccess> {
        match self
            .company_use_cases
            .managed_company(self.user_id, company_id)
            .await
        {
            Err(AppError::Forbidden(_)) => Err(company_not_found()),
            other => other,
        }
    }

    async fn section(
        &self,
        access: &CompanyAccess,
        selection: DepartmentSelection,
        error: Option<(DepartmentForm, &str)>,
        notice: Option<&str>,
    ) -> AppResult<String> {
        department_section(
            &self.department_use_cases,
            &self.config,
            self.user_id,
            access,
            selection,
            error,
            notice,
        )
        .await
    }

    /// A successful write: the section showing `department` (or nothing), at its own URL.
    async fn saved(
        &self,
        access: &CompanyAccess,
        department: Option<DepartmentId>,
        notice: &str,
    ) -> AppResult<Response> {
        let selection = match department {
            Some(id) => DepartmentSelection::Department { id, draft: None },
            None => DepartmentSelection::None,
        };
        let html = self.section(access, selection, None, Some(notice)).await?;
        let company_id = access.company.id;
        let url = match department {
            Some(department) => format!(
                "/ui/companies?company_id={company_id}&tab=departments&department_id={department}"
            ),
            None => format!("/ui/companies?company_id={company_id}&tab=departments"),
        };
        Ok(([("HX-Push-Url", url)], Html(html)).into_response())
    }

    /// A refused write: the section as it was, with the refusal on `form`.
    async fn refused(
        &self,
        access: &CompanyAccess,
        selection: DepartmentSelection,
        form: DepartmentForm,
        message: String,
    ) -> AppResult<Response> {
        Ok(Html(
            self.section(access, selection, Some((form, &message)), None)
                .await?,
        )
        .into_response())
    }
}

/// The department form as submitted. Every field is text so a refused submit can come back as
/// it was typed.
#[derive(Debug, Clone, Default, Deserialize)]
struct DepartmentFormData {
    #[serde(default)]
    name: String,
    slug: Option<String>,
    description: Option<String>,
    charter: Option<String>,
    parent_id: Option<String>,
    lead: Option<String>,
}

impl DepartmentFormData {
    fn draft(&self) -> DepartmentDraft {
        DepartmentDraft {
            name: self.name.clone(),
            slug: self.slug.clone().unwrap_or_default(),
            description: self.description.clone().unwrap_or_default(),
            charter: self.charter.clone().unwrap_or_default(),
            parent_id: optional_id(self.parent_id.as_deref()).ok().flatten(),
            lead: optional_id(self.lead.as_deref()).ok().flatten(),
        }
    }

    /// A blank slug follows the name, as a new desk's does.
    fn slug(&self) -> String {
        self.slug
            .as_deref()
            .map(str::trim)
            .filter(|slug| !slug.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| slugify(&self.name))
    }

    fn parent(&self) -> AppResult<Option<DepartmentId>> {
        Ok(optional_id(self.parent_id.as_deref())?.map(DepartmentId::new))
    }

    fn write(&self) -> AppResult<DepartmentWrite> {
        Ok(DepartmentWrite {
            name: self.name.clone(),
            slug: self.slug(),
            description: self.description.clone(),
            charter: self.charter.clone(),
            parent_id: self.parent()?,
            created_by: None,
        })
    }

    /// Every field the form shows is set; a blank slug keeps the current one, since the slug is
    /// the department desk's address.
    fn edit(&self) -> AppResult<DepartmentEdit> {
        Ok(DepartmentEdit {
            name: Some(self.name.clone()),
            slug: self
                .slug
                .as_deref()
                .map(str::trim)
                .filter(|slug| !slug.is_empty())
                .map(str::to_owned),
            description: Some(self.description.clone()),
            charter: Some(self.charter.clone()),
            parent_id: Some(self.parent()?),
        })
    }

    fn lead(&self) -> AppResult<Option<PrincipalId>> {
        Ok(optional_id(self.lead.as_deref())?.map(PrincipalId::new))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PrincipalFormData {
    principal_id: Option<String>,
}

impl PrincipalFormData {
    fn principal(&self) -> AppResult<PrincipalId> {
        optional_id(self.principal_id.as_deref())?
            .map(PrincipalId::new)
            .ok_or_else(|| AppError::BadRequest("Choose a person or agent.".into()))
    }
}

/// A select's value: blank is none, anything else must be an id.
fn optional_id(value: Option<&str>) -> AppResult<Option<Uuid>> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(value) => Uuid::parse_str(value)
            .map(Some)
            .map_err(|_| AppError::BadRequest("That choice is not valid.".into())),
    }
}

/// GET /ui/companies/{company_id}/departments - The tab with nothing selected (Protected).
#[instrument(skip_all)]
async fn close_department(
    workspace: Workspace,
    Path(company_id): Path<Uuid>,
) -> AppResult<Html<String>> {
    let access = workspace.readable(company_id).await?;
    Ok(Html(
        workspace
            .section(&access, DepartmentSelection::None, None, None)
            .await?,
    ))
}

/// GET /ui/companies/{company_id}/departments/new - The create form (Protected, owner or admin).
#[instrument(skip_all)]
async fn new_department(
    workspace: Workspace,
    Path(company_id): Path<Uuid>,
) -> AppResult<Html<String>> {
    let access = workspace.managed(company_id).await?;
    Ok(Html(
        workspace
            .section(
                &access,
                DepartmentSelection::Create(DepartmentDraft::default()),
                None,
                None,
            )
            .await?,
    ))
}

/// GET /ui/companies/{company_id}/departments/{department_id} - One department (Protected; a
/// member sees only their own).
#[instrument(skip_all)]
async fn show_department(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    let access = workspace.readable(company_id).await?;
    Ok(Html(
        workspace
            .section(
                &access,
                DepartmentSelection::Department {
                    id: DepartmentId::new(department_id),
                    draft: None,
                },
                None,
                None,
            )
            .await?,
    ))
}

/// POST /ui/companies/{company_id}/departments - Create a department (Protected, owner or admin).
#[instrument(skip_all)]
async fn create_department(
    workspace: Workspace,
    Path(company_id): Path<Uuid>,
    Form(form): Form<DepartmentFormData>,
) -> AppResult<Response> {
    let access = workspace.managed(company_id).await?;
    let created = match (form.write(), form.lead()) {
        (Ok(write), Ok(lead)) => {
            workspace
                .department_use_cases
                .create_department(workspace.user_id, company_id, write, lead)
                .await
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
    };
    match created {
        Ok(department) => {
            workspace
                .saved(&access, Some(department.id), "Department created.")
                .await
        }
        Err(error) => {
            workspace
                .refused(
                    &access,
                    DepartmentSelection::Create(form.draft()),
                    DepartmentForm::Create,
                    format!("Failed to create department: {error}"),
                )
                .await
        }
    }
}

/// PUT /ui/companies/{company_id}/departments/{department_id} - Save one department (Protected,
/// owner or admin).
#[instrument(skip_all)]
async fn update_department(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<DepartmentFormData>,
) -> AppResult<Response> {
    let access = workspace.managed(company_id).await?;
    let department = DepartmentId::new(department_id);
    let saved = match form.edit() {
        Ok(edit) => {
            workspace
                .department_use_cases
                .update_department(workspace.user_id, company_id, department, edit)
                .await
        }
        Err(error) => Err(error),
    };
    match saved {
        Ok(saved) => {
            workspace
                .saved(&access, Some(saved.id), "Department saved.")
                .await
        }
        Err(error) => {
            workspace
                .refused(
                    &access,
                    DepartmentSelection::Department {
                        id: department,
                        draft: Some(form.draft()),
                    },
                    DepartmentForm::Edit,
                    format!("Failed to save department: {error}"),
                )
                .await
        }
    }
}

/// DELETE /ui/companies/{company_id}/departments/{department_id} - Delete a department
/// (Protected, owner or admin).
#[instrument(skip_all)]
async fn delete_department(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    let access = workspace.managed(company_id).await?;
    let department = DepartmentId::new(department_id);
    match workspace
        .department_use_cases
        .delete_department(workspace.user_id, company_id, department)
        .await
    {
        Ok(()) => workspace.saved(&access, None, "Department deleted.").await,
        Err(error) => {
            workspace
                .refused(
                    &access,
                    DepartmentSelection::Department {
                        id: department,
                        draft: None,
                    },
                    DepartmentForm::Delete,
                    format!("Failed to delete department: {error}"),
                )
                .await
        }
    }
}

/// One membership or lead change on `department`: the section after it, or the refusal on
/// `form`.
async fn principal_change(
    workspace: &Workspace,
    access: &CompanyAccess,
    department: DepartmentId,
    form: DepartmentForm,
    result: AppResult<()>,
    notice: &str,
    failure: &str,
) -> AppResult<Response> {
    match result {
        Ok(()) => workspace.saved(access, Some(department), notice).await,
        Err(error) => {
            workspace
                .refused(
                    access,
                    DepartmentSelection::Department {
                        id: department,
                        draft: None,
                    },
                    form,
                    format!("{failure}: {error}"),
                )
                .await
        }
    }
}

/// POST /ui/companies/{company_id}/departments/{department_id}/members - Add a member (Protected,
/// owner or admin).
#[instrument(skip_all)]
async fn add_member(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<PrincipalFormData>,
) -> AppResult<Response> {
    let access = workspace.managed(company_id).await?;
    let department = DepartmentId::new(department_id);
    let result = match form.principal() {
        Ok(principal) => {
            workspace
                .department_use_cases
                .add_member(workspace.user_id, company_id, department, principal)
                .await
        }
        Err(error) => Err(error),
    };
    principal_change(
        &workspace,
        &access,
        department,
        DepartmentForm::Members,
        result,
        "Member added.",
        "Failed to add member",
    )
    .await
}

/// DELETE /ui/companies/{company_id}/departments/{department_id}/members/{principal_id} - Remove
/// a member (Protected, owner or admin).
#[instrument(skip_all)]
async fn remove_member(
    workspace: Workspace,
    Path((company_id, department_id, principal_id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<Response> {
    let access = workspace.managed(company_id).await?;
    let department = DepartmentId::new(department_id);
    let result = workspace
        .department_use_cases
        .remove_member(
            workspace.user_id,
            company_id,
            department,
            PrincipalId::new(principal_id),
        )
        .await;
    principal_change(
        &workspace,
        &access,
        department,
        DepartmentForm::Members,
        result,
        "Member removed.",
        "Failed to remove member",
    )
    .await
}

/// PUT /ui/companies/{company_id}/departments/{department_id}/lead - Assign or change the lead
/// (Protected, owner or admin).
#[instrument(skip_all)]
async fn assign_lead(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<PrincipalFormData>,
) -> AppResult<Response> {
    let access = workspace.managed(company_id).await?;
    let department = DepartmentId::new(department_id);
    let result = match form.principal() {
        Ok(principal) => {
            workspace
                .department_use_cases
                .assign_role(
                    workspace.user_id,
                    company_id,
                    department,
                    DepartmentRole::Lead,
                    principal,
                )
                .await
        }
        Err(error) => Err(error),
    };
    principal_change(
        &workspace,
        &access,
        department,
        DepartmentForm::Lead,
        result,
        "Lead assigned.",
        "Failed to assign the lead",
    )
    .await
}

/// DELETE /ui/companies/{company_id}/departments/{department_id}/lead/{principal_id} - Clear the
/// lead (Protected, owner or admin).
#[instrument(skip_all)]
async fn clear_lead(
    workspace: Workspace,
    Path((company_id, department_id, principal_id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<Response> {
    let access = workspace.managed(company_id).await?;
    let department = DepartmentId::new(department_id);
    let result = workspace
        .department_use_cases
        .clear_role(
            workspace.user_id,
            company_id,
            department,
            DepartmentRole::Lead,
            PrincipalId::new(principal_id),
        )
        .await;
    principal_change(
        &workspace,
        &access,
        department,
        DepartmentForm::Lead,
        result,
        "Lead cleared.",
        "Failed to clear the lead",
    )
    .await
}

#[cfg(test)]
#[path = "ui_departments_tests.rs"]
mod tests;
