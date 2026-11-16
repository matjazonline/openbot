//! Pure editor previews: selecting a reference never saves a workflow or publishes an agent.
use super::*;
use crate::application::agent::*;
use crate::application::workflow::compiler::{
    AgentReferenceStep, agent_reference_steps, select_agent_reference, select_role_reference,
};
use crate::domain::workflow::StepId;
use crate::entities::agent_capability::AgentId;

pub(super) const PICKER_ROUTE: &str = "/ui/workflows/{workflow_id}/agent-picker";
pub(super) const APPLY_ROUTE: &str = "/ui/workflows/{workflow_id}/agent-reference";

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(PICKER_ROUTE, post(picker))
        .route(APPLY_ROUTE, post(apply))
}

#[derive(Deserialize)]
pub(super) struct PickerForm {
    source: String,
    step: Option<String>,
    agent: Option<String>,
    after_agent: Option<String>,
    after_step: Option<String>,
}
impl PickerForm {
    fn step(&self) -> AppResult<Option<StepId>> {
        step_id(self.step.as_deref())
    }
    fn after_step(&self) -> AppResult<Option<StepId>> {
        step_id(self.after_step.as_deref())
    }
    fn agent(&self) -> AppResult<Option<AgentId>> {
        self.agent
            .as_deref()
            .filter(|value| !value.is_empty() && !value.starts_with("role:"))
            .map(|value| {
                Uuid::parse_str(value)
                    .map(AgentId::new)
                    .map_err(|_| AppError::BadRequest("Invalid agent selection".into()))
            })
            .transpose()
    }
    fn role(&self) -> AppResult<Option<crate::entities::department::DepartmentRole>> {
        self.agent
            .as_deref()
            .and_then(|value| value.strip_prefix("role:"))
            .map(|role| {
                role.parse()
                    .map_err(|_| AppError::BadRequest("Invalid department role selection".into()))
            })
            .transpose()
    }
    fn after_agent(&self) -> AppResult<Option<AgentKey>> {
        self.after_agent
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(|value| {
                AgentKey::parse(value)
                    .ok_or_else(|| AppError::BadRequest("Invalid agent continuation".into()))
            })
            .transpose()
    }
}
fn step_id(value: Option<&str>) -> AppResult<Option<StepId>> {
    value
        .filter(|value| !value.is_empty())
        .map(|value| {
            StepId::parse(value)
                .map_err(|_| AppError::BadRequest("Invalid model step selection".into()))
        })
        .transpose()
}

pub(super) async fn scope(ui: &Ui, company: Uuid, workflow: Uuid) -> AppResult<AgentCompanyScope> {
    ui.company(company).await?;
    if ui.detail(company, workflow).await?.summary.archived {
        return Err(AppError::BadRequest(
            "An archived workflow cannot be edited".into(),
        ));
    }
    Ok(AgentCompanyScope {
        company: super::company(company),
        actor: ui.actor()?,
    })
}

struct StepPage {
    items: Vec<AgentReferenceStep>,
    selected: Option<AgentReferenceStep>,
    next: Option<StepId>,
}
fn step_page(
    steps: Vec<AgentReferenceStep>,
    selected: Option<StepId>,
    after: Option<StepId>,
) -> StepPage {
    let selected = match selected {
        Some(id) => steps.iter().find(|step| step.id == id),
        None => steps.first(),
    }
    .cloned();
    let mut items = steps
        .into_iter()
        .filter(|step| after.as_ref().is_none_or(|after| step.id > *after))
        .take(AGENT_CATALOG_PAGE + 1)
        .collect::<Vec<_>>();
    let next = (items.len() > AGENT_CATALOG_PAGE).then(|| items[AGENT_CATALOG_PAGE - 1].id.clone());
    items.truncate(AGENT_CATALOG_PAGE);
    if let Some(selected) = &selected
        && !items.iter().any(|step| step.id == selected.id)
    {
        items.insert(0, selected.clone());
    }
    StepPage {
        items,
        selected,
        next,
    }
}

async fn selected_agent(
    ui: &Ui,
    scope: AgentCompanyScope,
    form: &PickerForm,
    step: Option<&AgentReferenceStep>,
) -> AppResult<Option<AgentAuthoringState>> {
    if form.role()?.is_some() {
        return Ok(None);
    }
    let reads = ui.engine.agent_authoring_reads();
    let id = if let Some(id) = form.agent()? {
        Some(id)
    } else if let Some(key) = step.and_then(|step| step.key.clone()) {
        reads
            .agent_directory_entry(scope, key)
            .await?
            .map(|entry| entry.id)
    } else {
        None
    };
    match id {
        Some(agent) => reads
            .authoring_state(AgentPublicationScope {
                company: scope.company,
                actor: scope.actor,
                agent,
            })
            .await
            .map(Some),
        None => Ok(None),
    }
}

pub(super) async fn picker(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    axum::Form(form): axum::Form<PickerForm>,
) -> AppResult<Html<String>> {
    let scope = scope(&ui, query.company_id, workflow).await?;
    let html = match agent_reference_steps(&WorkflowSourceDecoder, &form.source) {
        Err(diagnostic) => {
            pages::workflow_agent_picker_error(query.company_id, workflow, &diagnostic.message)
        }
        Ok(steps) => {
            let steps = step_page(steps, form.step()?, form.after_step()?);
            let mut directory = ui
                .engine
                .agent_authoring_reads()
                .agent_directory(scope, form.after_agent()?)
                .await?;
            let selected = selected_agent(&ui, scope, &form, steps.selected.as_ref()).await?;
            if let Some(selected) = &selected
                && !directory
                    .items
                    .iter()
                    .any(|entry| entry.id.as_uuid() == selected.agent.id)
            {
                let key = AgentKey::parse(&selected.agent.slug)
                    .ok_or_else(|| AppError::Internal("Invalid stored agent key".into()))?;
                // The directory's own entry, so it names the departments and desk as a page
                // entry does (01DEP-P03-D10).
                if let Some(entry) = ui
                    .engine
                    .agent_authoring_reads()
                    .agent_directory_entry(scope, key)
                    .await?
                {
                    directory.items.insert(0, entry);
                }
            }
            pages::workflow_agent_picker(&pages::WorkflowAgentPicker {
                company: query.company_id,
                workflow,
                steps: &steps.items,
                selected_step: steps.selected.as_ref(),
                next_step: steps.next.as_ref(),
                agents: &directory,
                selected_agent: selected.as_ref(),
                selected_role: if form.agent.as_deref().is_some_and(|value| !value.is_empty()) {
                    form.role()?
                } else {
                    steps.selected.as_ref().and_then(|step| step.role)
                },
            })
        }
    };
    ui.engine
        .agent_authoring_reads()
        .authorize_company_authoring(scope)
        .await?;
    Ok(Html(html))
}

pub(super) async fn apply(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    axum::Form(form): axum::Form<PickerForm>,
) -> AppResult<Html<String>> {
    let scope = scope(&ui, query.company_id, workflow).await?;
    let step = form
        .step()?
        .ok_or_else(|| AppError::BadRequest("Select a model step".into()))?;
    let source = if let Some(role) = form.role()? {
        select_role_reference(&WorkflowSourceDecoder, &form.source, &step, role)
    } else {
        let agent = form
            .agent()?
            .ok_or_else(|| AppError::BadRequest("Select an agent or role".into()))?;
        let state = ui
            .engine
            .agent_authoring_reads()
            .authoring_state(AgentPublicationScope {
                company: scope.company,
                actor: scope.actor,
                agent,
            })
            .await?;
        let key = AgentKey::parse(&state.agent.slug)
            .ok_or_else(|| AppError::Internal("Invalid stored agent key".into()))?;
        select_agent_reference(&WorkflowSourceDecoder, &form.source, &step, &key)
    };
    let html = match source {
        Ok(source) => pages::workflow_agent_reference_applied(&source),
        Err(diagnostic) => pages::error_alert(&diagnostic.message),
    };
    ui.engine
        .agent_authoring_reads()
        .authorize_company_authoring(scope)
        .await?;
    Ok(Html(html))
}
