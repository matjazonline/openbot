use super::*;
use crate::application::workflow::sample::{MAX_SAMPLE_BYTES, SampleReport, SampleRequest};

#[derive(Default)]
pub(super) struct SampleState {
    revision: Option<u64>,
    inputs: pages::SampleInputs,
    selections: Option<pages::WorkflowSelections>,
    report: Option<SampleReport>,
    error: Option<String>,
}
impl Ui {
    pub(super) async fn sample_body(
        &self,
        company: Uuid,
        detail: &DefinitionDetail,
        state: SampleState,
    ) -> AppResult<String> {
        let needs = WorkflowSourceDecoder
            .decode(&detail.source)
            .and_then(|decoded| publication_needs(&decoded));
        let (options, too_many) = match &needs {
            Ok(needs) => self.editor_options(company, needs).await?,
            Err(_) => (PublicationOptions::default(), false),
        };
        let selections = state.selections.unwrap_or_else(|| {
            needs
                .as_ref()
                .map(|needs| default_selections(needs, &options))
                .unwrap_or_default()
        });
        let selection_html =
            pages::workflow_selection_fieldset(needs.as_ref(), &options, &selections, too_many, "");
        Ok(pages::workflow_sample(&pages::SampleView {
            company,
            workflow: detail.summary.workflow.as_uuid(),
            revision: state.revision.unwrap_or(detail.summary.revision),
            current_revision: detail.summary.revision,
            inputs: &state.inputs,
            selections_html: &selection_html,
            report: state.report.as_ref(),
            error: state.error.as_deref(),
        }))
    }
}
pub(super) async fn sample(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    // Form encoding can expand every byte to three characters; bound it before parsing.
    if body.len() > MAX_SAMPLE_BYTES * 3 {
        return Err(AppError::BadRequest("Sample form exceeds 768 KiB".into()).into());
    }
    ui.detail(company, workflow).await?;
    let form = FormFields::parse(&body);
    let revision = form.number("expected_revision")?;
    let selections = pages::WorkflowSelections::from_fields(form.pairs());
    let inputs = pages::SampleInputs {
        input: form.text("input"),
        params: form.text("params"),
        scripts: form.text("scripts"),
    };
    let result = run_sample(&ui, company, workflow, revision, &inputs, &selections).await;
    let mut state = SampleState {
        revision: Some(revision),
        inputs,
        selections: Some(selections),
        ..Default::default()
    };
    match result {
        Ok(report) => state.report = Some(report),
        Err(WorkflowError::Diagnostic(error)) => state.error = Some(error.message.into()),
        Err(WorkflowError::App(AppError::BadRequest(message) | AppError::Conflict(message))) => {
            state.error = Some(message)
        }
        Err(WorkflowError::App(AppError::NotFound(_))) => state.error = Some("A selected dependency is unavailable to this company. Check the draft and connection selections.".into()),
        Err(error) => return Err(error),
    }
    let detail = ui.detail(company, workflow).await?;
    let body = ui.sample_body(company, &detail, state).await?;
    Ok(Html(
        ui.render_pane(company, &detail, pages::WorkflowTab::Sample, body)
            .await,
    ))
}
async fn run_sample(
    ui: &Ui,
    company: Uuid,
    workflow: Uuid,
    revision: u64,
    inputs: &pages::SampleInputs,
    selections: &pages::WorkflowSelections,
) -> WorkflowResult<SampleReport> {
    let parse = |raw: &str| {
        serde_json::from_str(raw)
            .map_err(|error| AppError::BadRequest(format!("Invalid sample JSON: {error}")))
    };
    let request = SampleRequest {
        input: parse(&inputs.input)?,
        params: parse(&inputs.params)?,
        scripts: serde_json::from_str(&inputs.scripts)
            .map_err(|error| AppError::BadRequest(format!("Invalid scripts: {error}")))?,
    };
    let (http, mcp) = selection_bodies(selections);
    let bundle = validate_draft(
        &ui.engine,
        company,
        ui.actor()?,
        workflow,
        revision,
        &http,
        &mcp,
    )
    .await?;
    Ok(ui.engine.sample(bundle, request)?)
}
