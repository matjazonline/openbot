//! The library: the workspace page, the sidebar, new drafts, template copies, one
//! workflow's pane and the step catalogue.
use super::*;

/// GET /ui/workflows — the workspace for the selected company and workflow.
pub(super) async fn workspace(
    ui: Ui,
    Query(query): Query<WorkspaceQuery>,
) -> AppResult<Html<String>> {
    let frame = ui.frame(query.company_id).await?;
    let Some(company) = frame.company.as_ref() else {
        return Ok(Html(frame.render(&ui.config, |_, _| String::new())));
    };
    let company_id = company.id;
    let (items, next) = ui.list(company_id, None).await?;
    let creating = matches!(query.new.as_deref(), Some("1") | Some("true"));
    let tools = matches!(query.tools.as_deref(), Some("1") | Some("true"));
    let selected = query.workflow_id.or_else(|| {
        (!creating && !tools)
            .then(|| items.first().map(|item| item.workflow.as_uuid()))
            .flatten()
    });
    let pane = if creating {
        new_pane_html(&ui, company_id, None)
    } else if tools && query.workflow_id.is_none() {
        ui.tools_pane_html(company_id, None, None).await?
    } else {
        match selected {
            Some(workflow) => {
                ui.pane_html(
                    company_id,
                    workflow,
                    pages::WorkflowTab::parse(query.tab.as_deref()),
                    EditorState::default(),
                )
                .await?
            }
            None => pages::workflow_empty_pane(
                "No workflows yet. Choose New to copy a template or start a blank draft.",
            ),
        }
    };
    let list = pages::WorkflowList {
        company_id,
        items: &items,
        selected,
        next,
    };
    Ok(Html(frame.render(&ui.config, |user, company| {
        pages::workflow_workspace_page(&pages::WorkflowWorkspace {
            user,
            company,
            list: &list,
            pane_html: &pane,
        })
    })))
}

/// GET /ui/workflows/list — the next sidebar page, replacing its "Load more" row.
pub(super) async fn list_page(ui: Ui, Query(query): Query<ListQuery>) -> AppResult<Html<String>> {
    ui.company(query.company_id).await?;
    let (items, next) = ui.list(query.company_id, query.after).await?;
    Ok(Html(pages::workflow_list_entries(&pages::WorkflowList {
        company_id: query.company_id,
        items: &items,
        selected: None,
        next,
    })))
}

pub(super) fn new_pane_html(ui: &Ui, company: Uuid, error: Option<&str>) -> String {
    pages::workflow_new_pane(&pages::WorkflowNewPane {
        company_id: company,
        templates: ui.engine.template_catalogue().entries(),
        error,
    })
}

/// GET /ui/workflows/new — the templates and the blank-draft form.
pub(super) async fn new_pane(ui: Ui, Query(query): Query<CompanyQuery>) -> AppResult<Html<String>> {
    ui.company(query.company_id).await?;
    Ok(Html(new_pane_html(&ui, query.company_id, None)))
}

/// A blank draft that compiles: one `data.map` step passing the input through.
pub(super) fn blank_source(workflow: Uuid) -> String {
    format!(
        "format_version: 1\nworkflow_id: {workflow}\ninput_schema: {{type: object}}\nparameter_schema: {{type: object, additionalProperties: false}}\noutput_schema: true\nresources: []\nentry: start\nsteps:\n  start:\n    type: data.map\n    with:\n      value: {{ref: /input}}\n      output_schema: {{literal: true}}\n    routes: {{success: $end}}\nlimits: {{max_steps: 10, max_context_bytes: 65536}}\n"
    )
}

pub(super) fn workflow_url(company: Uuid, workflow: Uuid, tab: &str) -> String {
    format!("/ui/workflows?company_id={company}&workflow_id={workflow}&tab={tab}")
}

/// The pane of a draft a create or copy just made (or a retry of it found), with the sidebar.
pub(super) async fn created(
    ui: &Ui,
    company: Uuid,
    workflow: Uuid,
    message: String,
) -> AppResult<Response> {
    let pane = ui
        .pane_html(
            company,
            workflow,
            pages::WorkflowTab::Editor,
            EditorState {
                banner: Some(Banner::Saved(message)),
                ..Default::default()
            },
        )
        .await?;
    let menu = ui.menu_oob(company, workflow).await?;
    Ok(pushed(
        format!("{pane}{menu}"),
        &workflow_url(company, workflow, "editor"),
    ))
}

/// POST /ui/workflows/new — a blank draft whose identity the form's key derives, so a double
/// submit makes one draft.
pub(super) async fn create_draft(
    ui: Ui,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Response> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let key = crate::application::workflow::IdempotencyKey::parse(form.text("command_key"))?;
    let workflow = keyed_id("workflow.ui_draft", super::company(company), key.as_str());
    let title = form.text("title");
    let saved = save_draft(
        &ui.engine,
        company,
        ui.actor()?,
        workflow,
        None,
        DraftContent {
            title: title.trim().to_owned(),
            description: String::new(),
            source: blank_source(workflow),
        },
    )
    .await;
    match saved {
        Ok(_) => Ok(created(&ui, company, workflow, "Draft created.".into()).await?),
        // The key's draft exists: this is a retry of the create that made it.
        Err(WorkflowError::App(AppError::Conflict(_)))
            if ui
                .engine
                .reads()
                .definition(super::company(company), WorkflowId::new(workflow))
                .await?
                .is_some() =>
        {
            Ok(created(&ui, company, workflow, "Draft created.".into()).await?)
        }
        Err(WorkflowError::App(AppError::BadRequest(message))) => {
            Ok(Html(new_pane_html(&ui, company, Some(&message))).into_response())
        }
        Err(error) => Err(error),
    }
}

/// POST /ui/workflows/templates/{template}/copy — the template at the catalogue revision the
/// page offered, into a draft the key derives.
pub(super) async fn copy_template(
    ui: Ui,
    Path(template): Path<String>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Response> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let copied = copy_template_draft(
        &ui.engine,
        company,
        ui.actor()?,
        &template,
        form.number("revision")?,
        &form.text("command_key"),
    )
    .await;
    match copied {
        Ok(CopiedTemplate { workflow, .. }) => Ok(created(
            &ui,
            company,
            workflow,
            "Template copied into a draft of this company. Edit it, validate it, then publish."
                .into(),
        )
        .await?),
        Err(WorkflowError::App(AppError::Conflict(message) | AppError::NotFound(message))) => {
            Ok(Html(new_pane_html(
                &ui,
                company,
                Some(&format!("The template was not copied: {message}")),
            ))
            .into_response())
        }
        Err(error) => Err(error),
    }
}

/// GET /ui/workflows/{workflow} — one workflow's pane.
pub(super) async fn pane(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<PaneQuery>,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    Ok(Html(
        ui.pane_html(
            company,
            workflow,
            pages::WorkflowTab::parse(query.tab.as_deref()),
            EditorState::default(),
        )
        .await?,
    ))
}

/// GET /ui/workflows/catalogue — the step catalogue and the company's agents.
pub(super) async fn catalogue(
    ui: Ui,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let agents = ui.agents.list_company_agents(ui.user, company).await?;
    let references: Vec<pages::AgentReference<'_>> = agents
        .iter()
        .map(|agent| pages::AgentReference {
            slug: &agent.slug,
            name: &agent.name,
            model: match (&agent.provider, &agent.model) {
                (Some(provider), Some(model)) => Some(format!("{provider} / {model}")),
                _ => None,
            },
        })
        .collect();
    Ok(Html(pages::workflow_catalogue(
        ui.engine.step_catalogue(),
        &references,
    )))
}
