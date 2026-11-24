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
    use_cases::{company::CompanyUseCases, department::DepartmentUseCases},
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
            "/ui/companies/{company_id}/departments/{department_id}/archive",
            post(archive_department),
        )
        .route(
            "/ui/companies/{company_id}/departments/{department_id}/restore",
            post(restore_department),
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
            "/ui/companies/{company_id}/departments/{department_id}/models",
            post(save_models),
        )
        .route(
            "/ui/companies/{company_id}/departments/{department_id}/budgets",
            post(save_budgets),
        )
        .route(
            "/ui/companies/{company_id}/departments/{department_id}/lead/{principal_id}",
            delete(clear_lead),
        )
        .route(
            "/ui/companies/{company_id}/departments/{department_id}/sponsors",
            post(assign_sponsor),
        )
        .route(
            "/ui/companies/{company_id}/departments/{department_id}/sponsors/remove",
            post(remove_sponsor),
        )
        .route(
            "/ui/companies/{company_id}/departments/{department_id}/escalation",
            post(set_escalation),
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
pub(super) struct DepartmentSectionServices<'a> {
    pub departments: &'a DepartmentUseCases,
    /// The selected department's own model connections, for its managers (Plan07 02A).
    pub companies: &'a CompanyUseCases,
    pub knowledge: &'a crate::application::knowledge::KnowledgeLibrary,
    pub config: &'a AppConfig,
}

pub(super) async fn department_section(
    services: DepartmentSectionServices<'_>,
    user_id: Uuid,
    access: &CompanyAccess,
    selection: DepartmentSelection,
    error: Option<(DepartmentForm, &str)>,
    notice: Option<&str>,
) -> AppResult<String> {
    let DepartmentSectionServices {
        departments,
        companies,
        knowledge,
        config,
    } = services;
    let company_id = access.company.id;
    let managed = departments.managed_units(user_id, company_id).await?;
    let editable = !managed.is_empty();
    // Archived departments sit behind "Show archived" and still name parents and acting roles.
    let listed = departments
        .visible_departments(
            user_id,
            company_id,
            crate::entities::department::ArchivedUnits::Include,
        )
        .await?;
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
    // An archived department's connections take no edit (Plan08 02B.4), so its form is left out.
    let models = match &view {
        Some(view)
            if managed.contains(&view.detail.department.id)
                && !view.detail.department.is_archived() =>
        {
            Some(
                companies
                    .list_unit_model_connections(
                        user_id,
                        company_id,
                        Some(view.detail.department.id.as_uuid()),
                    )
                    .await?,
            )
        }
        _ => None,
    };
    let budgets = match &view {
        Some(view) if managed.contains(&view.detail.department.id) => Some(
            companies
                .unit_budget_states(user_id, company_id, view.detail.department.id.as_uuid())
                .await?,
        ),
        _ => None,
    };
    // Plan08 03D.2: a goal's status, for whoever may see it; anyone else sees no card.
    let goal = match &view {
        Some(view)
            if view.detail.department.kind == crate::entities::department::DepartmentKind::Goal =>
        {
            match departments
                .goal_rollup(user_id, company_id, view.detail.department.id)
                .await
            {
                Ok(rollup) => Some(rollup),
                Err(AppError::NotFound(_)) => None,
                Err(error) => return Err(error),
            }
        }
        _ => None,
    };
    let history = match &view {
        Some(view) if managed.contains(&view.detail.department.id) => {
            departments
                .administration_history(user_id, company_id, view.detail.department.id)
                .await?
        }
        _ => Vec::new(),
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
    let knowledge = if let Some(view) = &view {
        knowledge_card_layers(
            knowledge
                .documents()
                .list(
                    crate::application::knowledge::KnowledgeScope {
                        company: crate::domain::workflow::CompanyId::new(company_id),
                        actor_id: user_id,
                    },
                    Some(view.detail.department.id),
                )
                .await,
        )?
    } else {
        Some(Vec::new())
    };
    Ok(pages::company_departments_section(
        &pages::DepartmentSection {
            company: &access.company,
            app_domain_name: &config.app_domain_name,
            departments: &listed,
            detail,
            choices: &choices,
            choices_cut,
            membership: access.membership,
            managed: &managed,
            history: &history,
            error,
            notice,
            knowledge: knowledge.as_deref().map_or(
                pages::DepartmentKnowledge::ScopeLimit,
                pages::DepartmentKnowledge::Documents,
            ),
            models: models.as_deref(),
            budgets: budgets.as_deref(),
            goal: goal.as_ref(),
        },
    ))
}

fn knowledge_card_layers(
    result: AppResult<crate::application::knowledge::KnowledgeLibraryView>,
) -> AppResult<Option<Vec<crate::entities::knowledge::KnowledgeDocumentLayer>>> {
    match result {
        Ok(view) => Ok(Some(view.layers)),
        Err(AppError::BadRequest(code))
            if code == crate::application::knowledge::KNOWLEDGE_SCOPE_LIMIT_EXCEEDED =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    department_use_cases: Arc<DepartmentUseCases>,
    knowledge: Arc<crate::application::knowledge::KnowledgeLibrary>,
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
            knowledge: state.knowledge.clone(),
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

    /// The company, only for someone who manages at least one of its units: anyone else gets
    /// the stranger's `404`. Each write then checks its own department.
    async fn managed(&self, company_id: Uuid) -> AppResult<CompanyAccess> {
        self.company_use_cases
            .administered_company(self.user_id, company_id)
            .await
    }

    async fn section(
        &self,
        access: &CompanyAccess,
        selection: DepartmentSelection,
        error: Option<(DepartmentForm, &str)>,
        notice: Option<&str>,
    ) -> AppResult<String> {
        department_section(
            DepartmentSectionServices {
                departments: &self.department_use_cases,
                companies: &self.company_use_cases,
                knowledge: &self.knowledge,
                config: &self.config,
            },
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
    kind: Option<String>,
    target_date: Option<String>,
    sponsor: Option<String>,
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
            kind: self.kind.clone().unwrap_or_else(|| "standing".to_owned()),
            target_date: self.target_date.clone().unwrap_or_default(),
            sponsor: optional_id(self.sponsor.as_deref()).ok().flatten(),
        }
    }

    /// The submitted kind; a form without the field (the root's) is standing.
    fn kind(&self) -> AppResult<crate::entities::department::DepartmentKind> {
        match self.kind.as_deref().map(str::trim) {
            None | Some("") => Ok(crate::entities::department::DepartmentKind::Standing),
            Some(value) => crate::entities::department::DepartmentKind::parse(value)
                .ok_or_else(|| AppError::BadRequest("Choose standing or goal.".into())),
        }
    }

    /// A goal's target date; the form ignores the date when standing is chosen (Plan08 03A.3).
    fn target_date(&self) -> AppResult<Option<chrono::NaiveDate>> {
        if self.kind()? != crate::entities::department::DepartmentKind::Goal {
            return Ok(None);
        }
        match self.target_date.as_deref().map(str::trim) {
            None | Some("") => Ok(None),
            Some(value) => chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map(Some)
                .map_err(|_| AppError::BadRequest("Enter the target date as a date.".into())),
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
            escalation: None,
            name: self.name.clone(),
            slug: self.slug(),
            description: self.description.clone(),
            charter: self.charter.clone(),
            parent_id: self.parent()?,
            created_by: None,
            goal: match self.kind()? {
                crate::entities::department::DepartmentKind::Goal => {
                    Some(crate::application::ports::department::GoalWrite {
                        target_date: self.target_date()?,
                        sponsors: optional_id(self.sponsor.as_deref())?
                            .map(PrincipalId::new)
                            .into_iter()
                            .collect(),
                    })
                }
                crate::entities::department::DepartmentKind::Standing => None,
            },
        })
    }

    /// Every field the form shows is set; a blank slug keeps the current one, since the slug is
    /// the department desk's address.
    fn edit(&self) -> AppResult<DepartmentEdit> {
        Ok(DepartmentEdit {
            escalation: None,
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
            kind: Some(self.kind()?),
            target_date: Some(self.target_date()?),
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

#[derive(Debug, Default, Deserialize)]
struct ArchiveFormData {
    cancel_open_runs: Option<String>,
}

/// Archive `department` and its subtree (Plan08 02E.1). Open runs refuse it with their count;
/// when they may be cancelled the card then offers to cancel them and archive.
async fn archive_department(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<ArchiveFormData>,
) -> AppResult<Response> {
    use crate::application::ports::department::OpenWork;
    let access = workspace.managed(company_id).await?;
    let department = DepartmentId::new(department_id);
    let open = match form.cancel_open_runs.as_deref() {
        Some("true") => OpenWork::Cancel,
        _ => OpenWork::Refuse,
    };
    let selection = || DepartmentSelection::Department {
        id: department,
        draft: None,
    };
    match workspace
        .department_use_cases
        .archive_department(workspace.user_id, company_id, department, open)
        .await
    {
        Ok(_) => {
            workspace
                .saved(&access, Some(department), "Department archived.")
                .await
        }
        Err(AppError::UnitArchiveOpenWork {
            count,
            elsewhere,
            cancellable,
        }) => {
            let (form, message) = open_work_refusal(count, elsewhere, cancellable);
            workspace.refused(&access, selection(), form, message).await
        }
        Err(error) => {
            workspace
                .refused(
                    &access,
                    selection(),
                    DepartmentForm::Archive,
                    format!("Failed to archive department: {error}"),
                )
                .await
        }
    }
}

/// The archive card's form and wording for an open-work refusal. A goal's run in another
/// department that its own work did not start is that department's to end, so it gets its own
/// wording rather than the cancellation-bound one (Plan08 03B.5).
fn open_work_refusal(count: i64, elsewhere: i64, cancellable: bool) -> (DepartmentForm, String) {
    let runs = if count == 1 { "run is" } else { "runs are" };
    if elsewhere > 0 {
        let them = if elsewhere == 1 { "it" } else { "them" };
        (
            DepartmentForm::Archive,
            format!(
                "{count} {runs} still open for this department or a goal in it, {elsewhere} in other departments that its own work did not start. Only those departments can end {them}; archive once they finish."
            ),
        )
    } else if cancellable {
        (
            DepartmentForm::ArchiveOpenWork,
            format!("{count} {runs} still open here or below. Cancel them to archive."),
        )
    } else {
        (
            DepartmentForm::Archive,
            format!(
                "{count} {runs} still open here or below, more than can be cancelled at once. Let them finish or cancel some first."
            ),
        )
    }
}

async fn restore_department(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    let access = workspace.managed(company_id).await?;
    let department = DepartmentId::new(department_id);
    match workspace
        .department_use_cases
        .restore_department(workspace.user_id, company_id, department)
        .await
    {
        Ok(_) => {
            workspace
                .saved(&access, Some(department), "Department restored.")
                .await
        }
        Err(error) => {
            workspace
                .refused(
                    &access,
                    DepartmentSelection::Department {
                        id: department,
                        draft: None,
                    },
                    DepartmentForm::Archive,
                    format!("Failed to restore department: {error}"),
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
                    crate::application::ports::department::DepartmentRoleAssignment {
                        role: DepartmentRole::Lead,
                        principal,
                        decider: false,
                    },
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
            crate::application::ports::department::DepartmentRoleRemoval {
                role: DepartmentRole::Lead,
                principal: PrincipalId::new(principal_id),
                successor: None,
            },
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

#[derive(Deserialize)]
struct SponsorForm {
    principal_id: Uuid,
    decider: Option<bool>,
    successor: Option<String>,
}

async fn assign_sponsor(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<SponsorForm>,
) -> AppResult<Response> {
    let access = workspace.managed(company_id).await?;
    let department = DepartmentId::new(department_id);
    let result = workspace
        .department_use_cases
        .assign_role(
            workspace.user_id,
            company_id,
            department,
            crate::application::ports::department::DepartmentRoleAssignment {
                role: DepartmentRole::Sponsor,
                principal: PrincipalId::new(form.principal_id),
                decider: form.decider.unwrap_or(false),
            },
        )
        .await;
    principal_change(
        &workspace,
        &access,
        department,
        DepartmentForm::Sponsors,
        result,
        "Sponsor saved.",
        "Failed to save the sponsor",
    )
    .await
}

async fn remove_sponsor(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<SponsorForm>,
) -> AppResult<Response> {
    let access = workspace.managed(company_id).await?;
    let department = DepartmentId::new(department_id);
    let result = match optional_id(form.successor.as_deref()) {
        Ok(successor) => {
            workspace
                .department_use_cases
                .clear_role(
                    workspace.user_id,
                    company_id,
                    department,
                    crate::application::ports::department::DepartmentRoleRemoval {
                        role: DepartmentRole::Sponsor,
                        principal: PrincipalId::new(form.principal_id),
                        successor: successor.map(PrincipalId::new),
                    },
                )
                .await
        }
        Err(error) => Err(error),
    };
    principal_change(
        &workspace,
        &access,
        department,
        DepartmentForm::Sponsors,
        result,
        "Sponsor removed.",
        "Failed to remove the sponsor",
    )
    .await
}

#[derive(Deserialize)]
struct EscalationForm {
    notify: String,
    hops: u8,
}

async fn set_escalation(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<EscalationForm>,
) -> AppResult<Response> {
    let access = workspace.managed(company_id).await?;
    let department = DepartmentId::new(department_id);
    let policy = if form.notify.is_empty() {
        Ok(None)
    } else {
        form.notify
            .parse::<DepartmentRole>()
            .map_err(|error| AppError::BadRequest(error.to_string()))
            .and_then(|notify| {
                crate::entities::workflow_escalation::EscalationHops::try_from(form.hops)
                    .map(|hops| {
                        Some(crate::entities::workflow_escalation::EscalationPolicy {
                            notify,
                            hops,
                        })
                    })
                    .map_err(|error| AppError::BadRequest(error.into()))
            })
    };
    let result = match policy {
        Ok(policy) => workspace
            .department_use_cases
            .update_department(
                workspace.user_id,
                company_id,
                department,
                DepartmentEdit {
                    escalation: Some(policy),
                    ..Default::default()
                },
            )
            .await
            .map(|_| ()),
        Err(error) => Err(error),
    };
    principal_change(
        &workspace,
        &access,
        department,
        DepartmentForm::Escalation,
        result,
        "Escalation default saved.",
        "Failed to save the escalation default",
    )
    .await
}

#[cfg(test)]
#[path = "ui_departments_tests.rs"]
mod tests;

/// The department model form as submitted: the same provider rows as the company settings form.
#[derive(Debug, Deserialize)]
struct DepartmentModelsForm {
    default_model_provider: Option<usize>,
    #[serde(flatten)]
    connection_fields: std::collections::HashMap<String, String>,
}

/// POST /ui/companies/{company_id}/departments/{department_id}/models - Replace the
/// department's own model connections (Plan07 02A): whoever manages the department.
#[instrument(skip_all)]
async fn save_models(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<DepartmentModelsForm>,
) -> AppResult<Response> {
    let access = workspace.readable(company_id).await?;
    let department = DepartmentId::new(department_id);
    let saved = match super::ui_companies::submitted_model_connections(
        &form.connection_fields,
        form.default_model_provider,
    ) {
        Ok(connections) => {
            workspace
                .company_use_cases
                .replace_unit_model_connections(
                    workspace.user_id,
                    company_id,
                    department_id,
                    connections,
                )
                .await
        }
        Err(error) => Err(error),
    };
    match saved {
        Ok(()) => {
            workspace
                .saved(&access, Some(department), "Saved model connections.")
                .await
        }
        Err(AppError::BadRequest(message) | AppError::Conflict(message)) => Ok(Html(
            workspace
                .section(
                    &access,
                    DepartmentSelection::Department {
                        id: department,
                        draft: None,
                    },
                    None,
                    Some(&format!("Model connections not saved: {message}")),
                )
                .await?,
        )
        .into_response()),
        Err(error) => Err(error),
    }
}

/// The department cap form: USD amounts for the UTC day and month; blank removes that cap.
#[derive(Debug, Default, Deserialize)]
struct DepartmentBudgetsForm {
    #[serde(default)]
    day_cap: String,
    #[serde(default)]
    month_cap: String,
    /// A goal's lifetime cap; the form shows it on goals only (Plan08 03C.1).
    #[serde(default)]
    total_cap: Option<String>,
}

/// A USD amount with at most six decimals as micro-USD; blank is `None` (no cap).
fn cap_micros(value: &str) -> AppResult<Option<i64>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let micros = crate::services::model_usage::TokenPrice::parse(value)
        .map_err(|_| {
            AppError::BadRequest(
                "A cap is an amount in USD up to 1000000 with at most six decimal places.".into(),
            )
        })?
        .micros();
    if micros == 0 {
        return Err(AppError::BadRequest(
            "A cap is a positive amount; leave it blank to remove it.".into(),
        ));
    }
    Ok(Some(micros))
}

/// POST /ui/companies/{company_id}/departments/{department_id}/budgets - Set or clear the
/// department's spend caps (Plan07 02B.1, decision c): whoever manages the department.
#[instrument(skip_all)]
async fn save_budgets(
    workspace: Workspace,
    Path((company_id, department_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<DepartmentBudgetsForm>,
) -> AppResult<Response> {
    let access = workspace.readable(company_id).await?;
    let department = DepartmentId::new(department_id);
    // Both amounts are checked before anything is written, then applied together (B3-F1).
    let saved = async {
        let (day, month) = (cap_micros(&form.day_cap)?, cap_micros(&form.month_cap)?);
        let total = form.total_cap.as_deref().map(cap_micros).transpose()?;
        workspace
            .company_use_cases
            .replace_unit_budgets(
                workspace.user_id,
                company_id,
                department_id,
                day,
                month,
                total,
            )
            .await
    }
    .await;
    match saved {
        Ok(()) => {
            workspace
                .saved(&access, Some(department), "Saved spend caps.")
                .await
        }
        Err(AppError::BadRequest(message)) => Ok(Html(
            workspace
                .section(
                    &access,
                    DepartmentSelection::Department {
                        id: department,
                        draft: None,
                    },
                    None,
                    Some(&format!("Spend caps not saved: {message}")),
                )
                .await?,
        )
        .into_response()),
        Err(error) => Err(error),
    }
}
