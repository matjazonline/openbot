//! Workflow tools (p09 B1b, WF-C003): the registrations pane with revoke, the versions tab's
//! "Use as a workflow tool" form, and the agent pane's grants and "used by" cards. Every write
//! is the `WorkflowTools` command the JSON routes call (register with the revision seen,
//! revoke, grant against the list seen).
use super::*;
use crate::application::workflow::authoring::parameters_from_json;
use crate::application::workflow::reads::ToolTarget;
use crate::application::workflow::tools::{WorkflowToolRegistration, WorkflowToolWrite};
use crate::domain::workflow::TypeName;

fn tool_name(value: &str) -> Result<TypeName, String> {
    TypeName::parse(value.trim())
        .map_err(|_| format!("'{}' is not a valid tool name.", value.trim()))
}

impl Ui {
    async fn registrations(
        &self,
        company: Uuid,
    ) -> AppResult<(Vec<WorkflowToolRegistration>, Vec<ToolTarget>)> {
        let company_id = super::company(company);
        let tools = self
            .engine
            .workflow_tools()
            .workflow_tools(company_id, self.actor()?)
            .await?;
        let versions: Vec<VersionId> = tools.iter().map(|tool| tool.version).collect();
        let targets = self
            .engine
            .authoring_reads()
            .tool_targets(company_id, &versions)
            .await?;
        Ok((tools, targets))
    }

    pub(super) async fn tools_pane_html(
        &self,
        company: Uuid,
        banner: Option<&str>,
        error: Option<&str>,
    ) -> AppResult<String> {
        let (tools, targets) = self.registrations(company).await?;
        Ok(pages::workflow_tools_pane(&pages::ToolsView {
            company_id: company,
            tools: &tools,
            targets: &targets,
            banner,
            error,
        }))
    }

    /// The versions tab's "Use as a workflow tool" form, empty.
    pub(super) fn tool_form_html(&self, company: Uuid, detail: &DefinitionDetail) -> String {
        pages::workflow_tool_form(&pages::ToolFormView {
            company_id: company,
            workflow: detail.summary.workflow.as_uuid(),
            versions: &detail.versions,
            name: "",
            version: detail
                .versions
                .first()
                .map(|version| version.version.as_uuid()),
            params: "{}",
            existing: None,
            banner: None,
            error: None,
        })
    }
}

fn target_of(targets: &[ToolTarget], registration: &WorkflowToolRegistration) -> String {
    match targets
        .iter()
        .find(|target| target.version == registration.version)
    {
        Some(target) => format!(
            "registered for {} version {} (draft revision {})",
            target.workflow_title,
            pages::short_id(target.version.as_uuid()),
            target.draft_revision
        ),
        None => format!(
            "registered for version {}",
            pages::short_id(registration.version.as_uuid())
        ),
    }
}

/// GET /ui/workflows/tools — the company's workflow tools in `#workflow-pane`.
pub(super) async fn tools_pane(ui: Ui, Query(query): Query<CompanyQuery>) -> AppResult<Response> {
    let company = ui.company(query.company_id).await?.id;
    Ok(pushed(
        ui.tools_pane_html(company, None, None).await?,
        &format!("/ui/workflows?company_id={company}&tools=1"),
    ))
}

/// POST /ui/workflows/tools/revoke — revoke `name` at the registration revision seen.
pub(super) async fn revoke_tool(
    ui: Ui,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let name = tool_name(&form.text("name")).map_err(AppError::BadRequest)?;
    let revision = i64::try_from(form.number("expected_revision")?)
        .map_err(|_| AppError::BadRequest("The revision is out of range".into()))?;
    let outcome = ui
        .engine
        .workflow_tools()
        .revoke_workflow_tool(super::company(company), ui.actor()?, name.clone(), revision)
        .await;
    let (banner, error) = match outcome {
        Ok(()) => (
            Some(format!(
                "Revoked {}. New runs that need this agent tool and new calls to it are refused; a call already in progress still returns its result.",
                name.as_str()
            )),
            None,
        ),
        Err(AppError::Conflict(_) | AppError::NotFound(_)) => (
            None,
            Some(format!(
                "{} changed or was revoked since the page was shown; nothing was revoked. Here are the tools as they are now.",
                name.as_str()
            )),
        ),
        Err(error) => return Err(error),
    };
    Ok(Html(
        ui.tools_pane_html(company, banner.as_deref(), error.as_deref())
            .await?,
    ))
}

/// POST /ui/workflows/{workflow}/tools — register a version of this workflow as a tool. A name
/// already registered is replaced only after the form has said what that does and the author
/// submits again with its revision.
pub(super) async fn register_tool(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let detail = ui.detail(company, workflow).await?;
    let form = FormFields::parse(&body);
    let (name_text, params_text) = (form.text("name"), form.text("params_json"));
    let version = form
        .get("version_id")
        .and_then(|id| id.parse::<Uuid>().ok())
        .filter(|id| detail.versions.iter().any(|v| v.version.as_uuid() == *id));
    let expected = form
        .get("expected_revision")
        .and_then(|value| value.parse::<i64>().ok());
    let (tools, targets) = ui.registrations(company).await?;
    let render =
        |existing: Option<&WorkflowToolRegistration>, banner: Option<&str>, error: Option<&str>| {
            pages::workflow_tool_form(&pages::ToolFormView {
                company_id: company,
                workflow,
                versions: &detail.versions,
                name: &name_text,
                version,
                params: &params_text,
                existing: existing.map(|registration| pages::ExistingTool {
                    registration,
                    target: target_of(&targets, registration),
                    same_workflow: registration.workflow.as_uuid() == workflow,
                }),
                banner,
                error,
            })
        };
    let name = match tool_name(&name_text) {
        Ok(name) => name,
        Err(message) => return Ok(Html(render(None, None, Some(&message)))),
    };
    let Some(version) = version else {
        return Ok(Html(render(
            None,
            None,
            Some("Choose a published version of this workflow."),
        )));
    };
    let params = match parameters_from_json(&params_text) {
        Ok(params) => params,
        Err(message) => return Ok(Html(render(None, None, Some(&message)))),
    };
    let existing = tools.iter().find(|tool| tool.name == name);
    // The name is taken and the form has not yet said so: say what replacing it does.
    if let (Some(current), None) = (existing, expected) {
        return Ok(Html(render(Some(current), None, None)));
    }
    let outcome = ui
        .engine
        .workflow_tools()
        .register_workflow_tool(
            super::company(company),
            ui.actor()?,
            WorkflowToolWrite {
                name: name.clone(),
                workflow: WorkflowId::new(workflow),
                version: VersionId::new(version),
                params,
                expected,
            },
        )
        .await;
    match outcome {
        Ok(registration) => {
            let message = match existing {
                None => format!(
                    "Registered {} for version {}. Select this registered target in an agent draft, then publish the agent to use it.",
                    name.as_str(),
                    pages::short_id(version)
                ),
                Some(previous) if previous.workflow.as_uuid() == workflow => format!(
                    "Re-pinned {} to version {} (revision {}). Published agents keep their pinned workflow version.",
                    name.as_str(),
                    pages::short_id(version),
                    registration.revision
                ),
                Some(_) => format!(
                    "Pointed {} at this workflow, version {} (revision {}). Agent bindings to the old workflow refuse new runs and calls. Select the new target in the agent draft and publish the agent to use it.",
                    name.as_str(),
                    pages::short_id(version),
                    registration.revision
                ),
            };
            let detail = &detail;
            Ok(Html(pages::workflow_tool_form(&pages::ToolFormView {
                company_id: company,
                workflow,
                versions: &detail.versions,
                name: "",
                version: Some(version),
                params: "{}",
                existing: None,
                banner: Some(&message),
                error: None,
            })))
        }
        Err(AppError::Conflict(_)) => {
            // Registered meanwhile, or changed since the warning was shown: show it as it is now.
            let (tools, _) = ui.registrations(company).await?;
            let current = tools.iter().find(|tool| tool.name == name);
            Ok(Html(render(
                current,
                None,
                Some(
                    "This name changed since the form was shown; nothing was registered. Here it is as it is now.",
                ),
            )))
        }
        Err(AppError::BadRequest(message)) => Ok(Html(render(
            existing,
            None,
            Some(&format!("Not registered: {message}")),
        ))),
        Err(AppError::NotFound(_)) => Ok(Html(render(
            existing,
            None,
            Some("Not registered: the version is not available to register now."),
        ))),
        Err(error) => Err(error),
    }
}

/// The grants card for `agent`, with the names it grants resolved against the registrations.
/// GET /ui/workflows/agents/{agent}/usage — the agent pane's "Used by" card (F098.2).
pub(super) async fn agent_usage(
    ui: Ui,
    Path(agent): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let company_id = super::company(company);
    // Company-scoped: another company's agent is not found, never an empty list.
    crate::adapters::http::routes::workflows::bindings::company_agent(
        &ui.engine, company_id, agent,
    )
    .await?;
    let usage = ui
        .engine
        .authoring_reads()
        .agent_usage(company_id, agent)
        .await?;
    let truncated = usage.truncated;
    let (bindings, hidden) = ui
        .visible_usage(company, usage, "workflow-agent-usage")
        .await?;
    Ok(Html(pages::agent_usage_card(&pages::AgentUsageCard {
        company_id: company,
        agent,
        bindings: &bindings,
        hidden,
        truncated,
    })))
}
