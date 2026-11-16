//! Reviewer edits are previews over the unsaved editor, with company-scoped choices.
use super::*;
use crate::application::workflow::compiler::{
    ReviewerReferenceStep, reviewer_reference_steps, select_reviewer_references,
};
use crate::application::workflow::reviewers::ReviewerSelector;
use crate::application::workflow::roles::{RoleDepartment, RoleHolder, RoleNeed, resolve_role};
use crate::domain::workflow::StepId;
use crate::entities::department::{DepartmentRole, MAX_DEPARTMENT_DEPTH};
use crate::entities::transport::PrincipalId;

pub(super) const PICKER_ROUTE: &str = "/ui/workflows/{workflow_id}/reviewer-picker";
pub(super) const APPLY_ROUTE: &str = "/ui/workflows/{workflow_id}/reviewer-reference";
pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(PICKER_ROUTE, post(picker))
        .route(APPLY_ROUTE, post(apply))
}

#[derive(Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReviewerEdit {
    Add,
}

#[derive(Deserialize)]
pub(super) struct ReviewerForm {
    source: String,
    step: Option<String>,
    reviewer: Option<String>,
    desk: Option<String>,
    after_person: Option<String>,
    after_desk: Option<String>,
    after_step: Option<String>,
    edit: Option<ReviewerEdit>,
}
fn optional_uuid(value: Option<&str>) -> AppResult<Option<Uuid>> {
    value
        .filter(|value| !value.is_empty())
        .map(|value| {
            Uuid::parse_str(value)
                .map_err(|_| AppError::BadRequest("Invalid reviewer choice".into()))
        })
        .transpose()
}
fn optional_step(value: Option<&str>) -> AppResult<Option<StepId>> {
    value
        .filter(|value| !value.is_empty())
        .map(|value| {
            StepId::parse(value).map_err(|_| AppError::BadRequest("Invalid review step".into()))
        })
        .transpose()
}
async fn selection(
    ui: &Ui,
    company: CompanyId,
    value: Option<&str>,
) -> AppResult<Option<ReviewerSelector>> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if let Some(role) = value.strip_prefix("role:") {
        return Ok(Some(ReviewerSelector::Role(role.parse().map_err(
            |_| AppError::BadRequest("Invalid reviewer role".into()),
        )?)));
    }
    let principal = PrincipalId::new(
        Uuid::parse_str(value)
            .map_err(|_| AppError::BadRequest("Invalid reviewer principal".into()))?,
    );
    ui.engine
        .authoring_reads()
        .reviewer_choice(company, principal)
        .await?
        .ok_or_else(|| AppError::NotFound("Reviewer not found".into()))?;
    Ok(Some(ReviewerSelector::Principal(principal)))
}

struct StepChoices {
    items: Vec<ReviewerReferenceStep>,
    selected: Option<ReviewerReferenceStep>,
    next: Option<StepId>,
}
fn step_choices(
    steps: Vec<ReviewerReferenceStep>,
    selected: Option<StepId>,
    after: Option<StepId>,
) -> AppResult<StepChoices> {
    let selected = match selected {
        Some(id) => Some(
            steps
                .iter()
                .find(|step| step.id == id)
                .ok_or_else(|| {
                    AppError::BadRequest("Select a review step in the current source".into())
                })?
                .clone(),
        ),
        None => steps.first().cloned(),
    };
    let mut items: Vec<_> = steps
        .into_iter()
        .filter(|step| after.as_ref().is_none_or(|after| step.id > *after))
        .take(51)
        .collect();
    let next = (items.len() > 50).then(|| items[49].id.clone());
    items.truncate(50);
    if let Some(selected) = &selected
        && !items.iter().any(|step| step.id == selected.id)
    {
        items.insert(0, selected.clone());
    }
    Ok(StepChoices {
        items,
        selected,
        next,
    })
}

pub(super) async fn picker(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    axum::Form(form): axum::Form<ReviewerForm>,
) -> AppResult<Html<String>> {
    let scope = super::agent_picker::scope(&ui, query.company_id, workflow).await?;
    let steps = match reviewer_reference_steps(&WorkflowSourceDecoder, &form.source) {
        Ok(steps) => step_choices(
            steps,
            optional_step(form.step.as_deref())?,
            optional_step(form.after_step.as_deref())?,
        )?,
        Err(error) => return Ok(Html(pages::error_alert(&error.message))),
    };
    let selected = selection(&ui, scope.company, form.reviewer.as_deref()).await?;
    let mut people = ui
        .engine
        .authoring_reads()
        .reviewer_choices(
            scope.company,
            optional_uuid(form.after_person.as_deref())?.map(PrincipalId::new),
        )
        .await?;
    if let Some(ReviewerSelector::Principal(id)) = selected
        && !people.items.iter().any(|person| person.principal == id)
    {
        people.items.insert(
            0,
            ui.engine
                .authoring_reads()
                .reviewer_choice(scope.company, id)
                .await?
                .ok_or_else(|| AppError::NotFound("Reviewer not found".into()))?,
        );
    }
    let options = ui.binding_options(query.company_id).await?;
    let desk = optional_uuid(form.desk.as_deref())?;
    if desk.is_some_and(|desk| !options.channels.iter().any(|choice| choice.id == desk)) {
        return Err(AppError::NotFound("Desk not found".into()));
    }
    let after = optional_uuid(form.after_desk.as_deref())?;
    let mut desks = options.channels.clone();
    desks.sort_by_key(|choice| choice.id);
    let mut desks: Vec<_> = desks
        .into_iter()
        .filter(|choice| after.is_none_or(|after| choice.id > after))
        .take(51)
        .collect();
    let next_desk = (desks.len() > 50).then(|| desks[49].id);
    desks.truncate(50);
    if let Some(desk) = desk
        && !desks.iter().any(|choice| choice.id == desk)
    {
        desks.insert(
            0,
            options
                .channels
                .iter()
                .find(|choice| choice.id == desk)
                .ok_or_else(|| AppError::NotFound("Desk not found".into()))?
                .clone(),
        );
    }
    let preview = preview(&ui, query.company_id, desk).await?;
    let html = pages::workflow_reviewer_picker(&pages::WorkflowReviewerPicker {
        company: query.company_id,
        workflow,
        steps: &steps.items,
        step: steps.selected.as_ref(),
        people: &people,
        selected: form.reviewer.as_deref().filter(|value| !value.is_empty()),
        desks: &desks,
        desk,
        preview: &preview,
        next_step: steps.next.as_ref(),
        next_desk,
        desks_cut: options.truncated,
    });
    ui.engine
        .agent_authoring_reads()
        .authorize_company_authoring(scope)
        .await?;
    Ok(Html(html))
}

async fn preview(
    ui: &Ui,
    company: Uuid,
    desk: Option<Uuid>,
) -> AppResult<Vec<pages::ReviewerRolePreview>> {
    let Some(desk) = desk else {
        return Ok(Vec::new());
    };
    let departments = ui
        .departments
        .desk_departments(ui.user, company, &[desk])
        .await?;
    let mut department = departments.owners.get(&desk).copied();
    let mut chain = Vec::new();
    let mut labels = BTreeMap::new();
    while let Some(id) = department {
        if chain.len() > MAX_DEPARTMENT_DEPTH {
            break;
        }
        let view = ui.departments.department_view(ui.user, company, id).await?;
        let mut holders = Vec::with_capacity(view.detail.roles.len());
        for role in &view.detail.roles {
            let principal = view.principal(role.principal_id).ok_or_else(|| {
                AppError::Conflict("Role holders changed; refresh the preview".into())
            })?;
            labels.insert(principal.principal_id, principal.name.clone());
            holders.push(RoleHolder {
                role: role.role,
                principal: principal.principal_id,
                kind: principal.kind,
            });
        }
        chain.push(RoleDepartment {
            department: id,
            holders,
        });
        department = view.detail.department.parent_id;
    }
    Ok(DepartmentRole::ALL
        .into_iter()
        .map(|role| match resolve_role(&chain, role, RoleNeed::People) {
            Ok(resolved) => pages::ReviewerRolePreview {
                role,
                holders: resolved
                    .principals
                    .iter()
                    .filter_map(|id| labels.get(id).cloned())
                    .collect(),
                refusal: None,
            },
            Err(reason) => pages::ReviewerRolePreview {
                role,
                holders: Vec::new(),
                refusal: Some(reason.code()),
            },
        })
        .collect())
}

pub(super) async fn apply(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    axum::Form(form): axum::Form<ReviewerForm>,
) -> AppResult<Html<String>> {
    let scope = super::agent_picker::scope(&ui, query.company_id, workflow).await?;
    let selected = selection(&ui, scope.company, form.reviewer.as_deref())
        .await?
        .ok_or_else(|| AppError::BadRequest("Choose a reviewer".into()))?;
    let step = optional_step(form.step.as_deref())?
        .ok_or_else(|| AppError::BadRequest("Choose a review step".into()))?;
    let steps = reviewer_reference_steps(&WorkflowSourceDecoder, &form.source)
        .map_err(|error| AppError::BadRequest(error.message.to_string()))?;
    let current = steps
        .iter()
        .find(|candidate| candidate.id == step)
        .and_then(|step| step.selectors.as_ref())
        .ok_or_else(|| {
            AppError::BadRequest("Edit parameter-controlled reviewers in source".into())
        })?;
    let mut reviewers = if matches!(form.edit, Some(ReviewerEdit::Add)) {
        current.clone()
    } else {
        Vec::new()
    };
    if !reviewers.contains(&selected) {
        reviewers.push(selected);
    }
    let html =
        match select_reviewer_references(&WorkflowSourceDecoder, &form.source, &step, &reviewers) {
            Ok(source) => pages::workflow_agent_reference_applied(&source),
            Err(error) => pages::error_alert(&error.message),
        };
    ui.engine
        .agent_authoring_reads()
        .authorize_company_authoring(scope)
        .await?;
    Ok(Html(html))
}
