//! The bindings tab (p09 B1b): the tab body, its form (rendered again when the chosen version
//! changes, since params and slots are the version's), create or configure through the JSON
//! route's own command (`bindings::configure_binding`), activate and deactivate at the state
//! revision seen, and the channel pane's card of the bindings on one channel.
use super::super::super::bindings::{
    BindingRequest, Visibility, configure_binding, set_binding_activity,
};
use super::*;
use crate::application::workflow::authoring::{
    field_value, parameter_fields, parameters_from_fields, parameters_from_json,
};
use crate::application::workflow::reads::{BindingOptions, BindingUse, WorkflowUsage};
use crate::application::workflow::{RelatedAssociation, RelatedChannelId};
use crate::domain::workflow::WorkflowDefinition;

/// What the tab's last write did.
pub(super) enum BindingsBanner {
    Done(String),
    RowRefused { binding: Uuid, message: String },
}

/// The params the form shows: a stored binding's, or what the author typed.
pub(super) enum ParamsInput {
    Stored(serde_json::Value),
    Typed {
        values: BTreeMap<String, String>,
        json: String,
    },
}

/// The form as opened or as submitted.
pub(super) struct BindingForm {
    /// The binding being configured; `None` creates one under `form_key`.
    binding: Option<Uuid>,
    form_key: String,
    version: Option<Uuid>,
    association: String,
    params: ParamsInput,
    resources: BTreeMap<String, Uuid>,
    message_trigger: String,
    run_as: Option<Uuid>,
    error: Option<String>,
}

impl BindingForm {
    fn new_binding(newest: Option<Uuid>) -> Self {
        Self {
            binding: None,
            form_key: Uuid::new_v4().to_string(),
            version: newest,
            association: "company".into(),
            params: ParamsInput::Stored(serde_json::json!({})),
            resources: BTreeMap::new(),
            message_trigger: String::new(),
            run_as: None,
            error: None,
        }
    }

    fn stored(binding: &BindingUse) -> Self {
        let summary = &binding.binding;
        Self {
            binding: Some(summary.binding.as_uuid()),
            form_key: String::new(),
            version: Some(summary.version.as_uuid()),
            association: association_value(summary.association),
            params: ParamsInput::Stored(summary.params.clone()),
            resources: binding.resources.clone(),
            message_trigger: summary
                .message_trigger
                .map(|trigger| trigger.as_str().to_owned())
                .unwrap_or_default(),
            run_as: Some(summary.run_as),
            error: None,
        }
    }

    fn typed(form: &FormFields) -> Self {
        let mut values = BTreeMap::new();
        let mut resources = BTreeMap::new();
        for (name, value) in form.pairs() {
            if let Some(parameter) = name.strip_prefix("param:") {
                values.insert(parameter.to_owned(), value.to_owned());
            } else if let Some(slot) = name.strip_prefix("resource:")
                && let Ok(resource) = value.parse()
            {
                resources.insert(slot.to_owned(), resource);
            }
        }
        Self {
            binding: form.get("binding_id").and_then(|id| id.parse().ok()),
            form_key: form.text("form_key"),
            version: form.get("version_id").and_then(|id| id.parse().ok()),
            association: form.get("association").unwrap_or("company").to_owned(),
            params: ParamsInput::Typed {
                values,
                json: form.text("params_json"),
            },
            resources,
            message_trigger: form.text("message_trigger"),
            run_as: form.get("run_as").and_then(|id| id.parse().ok()),
            error: None,
        }
    }
}

fn association_value(association: RelatedAssociation) -> String {
    match association {
        RelatedAssociation::Company => "company".into(),
        RelatedAssociation::Channel(channel) => format!("channel:{}", channel.as_uuid()),
        RelatedAssociation::Thread { channel_id, .. } => {
            format!("channel:{}", channel_id.as_uuid())
        }
    }
}

fn parse_association(value: &str) -> AppResult<RelatedAssociation> {
    match value.strip_prefix("channel:") {
        None if value == "company" => Ok(RelatedAssociation::Company),
        Some(channel) => channel
            .parse()
            .map(|channel| RelatedAssociation::Channel(RelatedChannelId::new(channel)))
            .map_err(|_| AppError::BadRequest("The association names no channel".into())),
        None => Err(AppError::BadRequest(
            "The association is the company or a channel".into(),
        )),
    }
}

/// The form's params and the params object they build, by the version's schema.
fn params_of(
    definition: &WorkflowDefinition,
    form: &FormFields,
) -> Result<serde_json::Value, String> {
    match parameter_fields(definition.parameter_schema.as_ref()) {
        Some(fields) => parameters_from_fields(&fields, |name| {
            form.get(&format!("param:{name}")).map(str::to_owned)
        }),
        None => parameters_from_json(&form.text("params_json")),
    }
}

/// The message a refused configure or activation shows; `None` is not the author's to fix.
fn binding_refusal(error: &WorkflowError) -> Option<String> {
    match error {
        WorkflowError::Diagnostic(diagnostic) => Some(match &diagnostic.instance_path {
            Some(path) if !path.is_empty() => format!("{} (at {path})", diagnostic.message),
            _ => diagnostic.message.to_string(),
        }),
        WorkflowError::App(AppError::BadRequest(message)) => Some(message.clone()),
        WorkflowError::App(AppError::NotFound(_)) => Some(
            "A chosen channel, version, run-as or resource is not available: it may be hidden from \
             you, disabled, revoked or not ready, or a tool the version froze is no longer \
             registered or approved."
                .into(),
        ),
        _ => None,
    }
}

impl Ui {
    /// The bindings of a workflow listing the viewer may see, and how many were left out.
    pub(super) async fn visible_usage(
        &self,
        company: Uuid,
        usage: WorkflowUsage,
        listing: &'static str,
    ) -> AppResult<(Vec<BindingUse>, usize)> {
        let mut visibility = Visibility::new(&self.engine, super::company(company), self.actor()?);
        let mut visible = Vec::new();
        let mut hidden = 0;
        for binding in usage.bindings {
            if visibility.allows(binding.binding.association).await? {
                visible.push(binding);
            } else {
                hidden += 1;
            }
        }
        visibility.report(listing);
        Ok((visible, hidden))
    }

    /// The choices a binding form offers, with the channels the viewer cannot see left out.
    async fn binding_options(&self, company: Uuid) -> AppResult<BindingOptions> {
        let company_id = super::company(company);
        let mut options = self
            .engine
            .authoring_reads()
            .binding_options(company_id)
            .await?;
        let mut visibility = Visibility::new(&self.engine, company_id, self.actor()?);
        let mut channels = Vec::new();
        for channel in std::mem::take(&mut options.channels) {
            let association = RelatedAssociation::Channel(RelatedChannelId::new(channel.id));
            if visibility.allows(association).await? {
                channels.push(channel);
            }
        }
        visibility.report("workflow-binding-channels");
        options.channels = channels;
        Ok(options)
    }

    /// The chosen version's definition, when it is a selectable version of this workflow.
    async fn binding_definition(
        &self,
        company: Uuid,
        workflow: Uuid,
        version: Option<Uuid>,
    ) -> AppResult<Option<WorkflowDefinition>> {
        let Some(version) = version else {
            return Ok(None);
        };
        let bundle = self
            .engine
            .selectable_bundle(super::company(company), VersionId::new(version))
            .await?;
        Ok(bundle
            .map(|bundle| bundle.compiled().graph().definition().clone())
            .filter(|definition| definition.workflow_id.as_uuid() == workflow))
    }

    async fn binding_form_html(
        &self,
        company: &crate::entities::company::Company,
        detail: &DefinitionDetail,
        form: &BindingForm,
        options: &BindingOptions,
        editing: Option<&BindingUse>,
    ) -> AppResult<String> {
        let workflow = detail.summary.workflow.as_uuid();
        let definition = self
            .binding_definition(company.id, workflow, form.version)
            .await?;
        let fields = definition
            .as_ref()
            .and_then(|definition| parameter_fields(definition.parameter_schema.as_ref()));
        let (values, json) = match (&form.params, &fields) {
            (ParamsInput::Stored(params), Some(fields)) => (
                fields
                    .iter()
                    .map(|field| (field.name.clone(), field_value(field, params)))
                    .collect(),
                String::new(),
            ),
            (ParamsInput::Stored(params), None) => (
                BTreeMap::new(),
                serde_json::to_string_pretty(params).unwrap_or_default(),
            ),
            (ParamsInput::Typed { values, json }, _) => (values.clone(), json.clone()),
        };
        let params = match &fields {
            Some(fields) => pages::BindingParams::Fields {
                fields,
                values: &values,
            },
            None => pages::BindingParams::Json(&json),
        };
        Ok(pages::workflow_binding_form(&pages::BindingFormView {
            company_id: company.id,
            workflow,
            editing,
            form_key: &form.form_key,
            versions: &detail.versions,
            version: form.version,
            definition: definition.as_ref(),
            association: &form.association,
            channels: &options.channels,
            params,
            resources: &form.resources,
            http: &options.http,
            mcp: &options.mcp,
            message_trigger: &form.message_trigger,
            run_as: form.run_as,
            owner: company.user_id == self.user,
            viewer: self.user,
            people: &options.people,
            options_truncated: options.truncated,
            error: form.error.as_deref(),
        }))
    }

    /// The whole pane on the bindings tab, with `form` open (a fresh create form when `None`).
    pub(super) async fn bindings_pane(
        &self,
        company: Uuid,
        workflow: Uuid,
        banner: Option<BindingsBanner>,
        form: Option<BindingForm>,
    ) -> AppResult<String> {
        let detail = self.detail(company, workflow).await?;
        let body = self.bindings_body(company, &detail, banner, form).await?;
        Ok(self
            .render_pane(company, &detail, pages::WorkflowTab::Bindings, body)
            .await)
    }

    pub(super) async fn bindings_body(
        &self,
        company: Uuid,
        detail: &DefinitionDetail,
        banner: Option<BindingsBanner>,
        form: Option<BindingForm>,
    ) -> AppResult<String> {
        let managed = self.company(company).await?;
        let usage = self
            .engine
            .authoring_reads()
            .workflow_usage(super::company(company), detail.summary.workflow)
            .await?;
        let truncated = usage.truncated;
        let (bindings, hidden) = self
            .visible_usage(company, usage, "workflow-bindings-tab")
            .await?;
        let options = self.binding_options(company).await?;
        let form = form.unwrap_or_else(|| {
            BindingForm::new_binding(detail.versions.first().map(|v| v.version.as_uuid()))
        });
        let editing = match form.binding {
            Some(binding) => Some(
                self.visible_binding(company, detail.summary.workflow.as_uuid(), binding)
                    .await?,
            ),
            None => None,
        };
        let form_html = self
            .binding_form_html(&managed, detail, &form, &options, editing.as_ref())
            .await?;
        let banner = match &banner {
            Some(BindingsBanner::Done(message)) => Some(pages::BindingBanner::Done(message)),
            Some(BindingsBanner::RowRefused { binding, message }) => {
                Some(pages::BindingBanner::RowRefused {
                    binding: *binding,
                    message,
                })
            }
            None => None,
        };
        Ok(pages::workflow_bindings_body(&pages::BindingsView {
            company_id: company,
            workflow: detail.summary.workflow.as_uuid(),
            archived: detail.summary.archived,
            versions: &detail.versions,
            bindings: &bindings,
            truncated,
            hidden,
            people: &options.people,
            banner,
            form_html: &form_html,
        }))
    }

    /// A binding of this workflow the viewer may see.
    async fn visible_binding(
        &self,
        company: Uuid,
        workflow: Uuid,
        binding: Uuid,
    ) -> AppResult<BindingUse> {
        let not_found = || AppError::NotFound("Workflow binding not found".into());
        let found = self
            .engine
            .authoring_reads()
            .binding_use(super::company(company), binding)
            .await?
            .filter(|found| found.binding.workflow.as_uuid() == workflow)
            .ok_or_else(not_found)?;
        let mut visibility = Visibility::new(&self.engine, super::company(company), self.actor()?);
        if !visibility.allows(found.binding.association).await? {
            return Err(not_found());
        }
        Ok(found)
    }
}

#[derive(Deserialize)]
pub(super) struct BindingFormQuery {
    pub(super) company_id: Uuid,
    pub(super) binding_id: Option<Uuid>,
}

/// GET /ui/workflows/{workflow}/bindings/form — the form for `binding_id` as stored, or the
/// fields it carries (a version change), rendered for the chosen version.
pub(super) async fn binding_form(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<BindingFormQuery>,
    uri: axum::http::Uri,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?;
    let detail = ui.detail(company.id, workflow).await?;
    let fields = FormFields::parse(uri.query().unwrap_or_default().as_bytes());
    let typed = fields.get("version_id").is_some();
    let editing = match query.binding_id {
        Some(binding) => Some(ui.visible_binding(company.id, workflow, binding).await?),
        None => None,
    };
    let form = match (&editing, typed) {
        (_, true) => BindingForm::typed(&fields),
        (Some(binding), false) => BindingForm::stored(binding),
        (None, false) => {
            BindingForm::new_binding(detail.versions.first().map(|v| v.version.as_uuid()))
        }
    };
    let options = ui.binding_options(company.id).await?;
    Ok(Html(
        ui.binding_form_html(&company, &detail, &form, &options, editing.as_ref())
            .await?,
    ))
}

/// POST /ui/workflows/{workflow}/bindings — create (keyed by the form) or configure a binding
/// through the binding command. A refusal keeps every choice in the form.
pub(super) async fn save_binding(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let fields = FormFields::parse(&body);
    let mut form = BindingForm::typed(&fields);
    let refused = |mut form: BindingForm, message: String| {
        form.error = Some(message);
        form
    };
    let editing = match form.binding {
        Some(binding) => Some(ui.visible_binding(company, workflow, binding).await?),
        None => None,
    };
    let Some(definition) = ui
        .binding_definition(company, workflow, form.version)
        .await?
    else {
        let form = refused(
            form,
            "Choose a published version of this workflow; its workflow may have been archived."
                .into(),
        );
        return Ok(Html(
            ui.bindings_pane(company, workflow, None, Some(form))
                .await?,
        ));
    };
    let params = match params_of(&definition, &fields) {
        Ok(params) => params,
        Err(message) => {
            let form = refused(form, message);
            return Ok(Html(
                ui.bindings_pane(company, workflow, None, Some(form))
                    .await?,
            ));
        }
    };
    let association = match &editing {
        Some(binding) => binding.binding.association,
        None => parse_association(&form.association)?,
    };
    let binding = match form.binding {
        Some(binding) => binding,
        None => {
            let key = crate::application::workflow::IdempotencyKey::parse(form.form_key.clone())?;
            keyed_id("workflow.ui_binding", super::company(company), key.as_str())
        }
    };
    // The revision the author saw; a binding changed since then is the command's conflict.
    let expected_state_revision = match &editing {
        Some(_) => Some(fields.number("expected_state_revision")?),
        None => None,
    };
    let request = BindingRequest {
        expected_state_revision,
        association,
        version: definition.version_id.as_uuid(),
        params,
        resources: form.resources.clone(),
        run_as: form.run_as,
        message_trigger: Some(form.message_trigger.clone()).filter(|trigger| !trigger.is_empty()),
    };
    match configure_binding(&ui.engine, company, ui.actor()?, binding, request).await {
        Ok(state) => {
            let message = format!(
                "Saved binding {} at state revision {}; it is {}.",
                pages::short_id(binding),
                state.revision.get(),
                if state.active {
                    "active"
                } else {
                    "paused until you activate it"
                },
            );
            Ok(Html(
                ui.bindings_pane(company, workflow, Some(BindingsBanner::Done(message)), None)
                    .await?,
            ))
        }
        Err(WorkflowError::App(AppError::Conflict(_))) if editing.is_none() => {
            // A double submit of the create (the key names the binding) or a reused key.
            Ok(Html(
                ui.bindings_pane(
                    company,
                    workflow,
                    Some(BindingsBanner::Done(format!(
                        "Binding {} was already created; it is listed above.",
                        pages::short_id(binding)
                    ))),
                    None,
                )
                .await?,
            ))
        }
        Err(WorkflowError::App(AppError::Conflict(_))) => {
            form.error = Some(
                "Someone changed this binding while you saved it. Your choices are kept below; \
                 saving again replaces its configuration."
                    .into(),
            );
            Ok(Html(
                ui.bindings_pane(company, workflow, None, Some(form))
                    .await?,
            ))
        }
        Err(error) => match binding_refusal(&error) {
            Some(message) => {
                let form = refused(form, format!("Not saved: {message}"));
                Ok(Html(
                    ui.bindings_pane(company, workflow, None, Some(form))
                        .await?,
                ))
            }
            None => Err(error),
        },
    }
}

#[derive(Deserialize)]
pub(super) struct ActivityForm {
    pub(super) expected_state_revision: u64,
}

async fn set_activity(
    ui: Ui,
    company_id: Uuid,
    workflow: Uuid,
    binding: Uuid,
    expected: u64,
    active: bool,
) -> WorkflowResult<Html<String>> {
    let company = ui.company(company_id).await?.id;
    ui.visible_binding(company, workflow, binding).await?;
    let banner =
        match set_binding_activity(&ui.engine, company, ui.actor()?, binding, expected, active)
            .await
        {
            Ok(state) => BindingsBanner::Done(format!(
                "{} binding {} (state revision {}).",
                if state.active {
                    "Activated"
                } else {
                    "Deactivated"
                },
                pages::short_id(binding),
                state.revision.get(),
            )),
            Err(WorkflowError::App(AppError::Conflict(_))) => BindingsBanner::RowRefused {
                binding,
                message: "This binding changed since the page was shown; here it is as it is now. \
                      Nothing was changed."
                    .into(),
            },
            Err(error) => match binding_refusal(&error) {
                Some(message) => BindingsBanner::RowRefused {
                    binding,
                    message: format!(
                        "Not {}: {message}",
                        if active { "activated" } else { "deactivated" }
                    ),
                },
                None => return Err(error),
            },
        };
    Ok(Html(
        ui.bindings_pane(company, workflow, Some(banner), None)
            .await?,
    ))
}

/// POST /ui/workflows/{workflow}/bindings/{binding}/activate — readiness is checked again.
pub(super) async fn activate_binding(
    ui: Ui,
    Path((workflow, binding)): Path<(Uuid, Uuid)>,
    Query(query): Query<CompanyQuery>,
    axum::Form(form): axum::Form<ActivityForm>,
) -> WorkflowResult<Html<String>> {
    set_activity(
        ui,
        query.company_id,
        workflow,
        binding,
        form.expected_state_revision,
        true,
    )
    .await
}

/// POST /ui/workflows/{workflow}/bindings/{binding}/deactivate.
pub(super) async fn deactivate_binding(
    ui: Ui,
    Path((workflow, binding)): Path<(Uuid, Uuid)>,
    Query(query): Query<CompanyQuery>,
    axum::Form(form): axum::Form<ActivityForm>,
) -> WorkflowResult<Html<String>> {
    set_activity(
        ui,
        query.company_id,
        workflow,
        binding,
        form.expected_state_revision,
        false,
    )
    .await
}

/// GET /ui/workflows/channels/{channel}/bindings — the channel pane's "Workflows on this
/// channel" card, for a channel the viewer can see.
pub(super) async fn channel_bindings(
    ui: Ui,
    Path(channel): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let company_id = super::company(company);
    let mut visibility = Visibility::new(&ui.engine, company_id, ui.actor()?);
    if !visibility
        .allows(RelatedAssociation::Channel(RelatedChannelId::new(channel)))
        .await?
    {
        return Err(AppError::NotFound("Channel not found".into()));
    }
    let usage = ui
        .engine
        .authoring_reads()
        .channel_usage(company_id, channel)
        .await?;
    Ok(Html(pages::channel_bindings_card(
        &pages::ChannelBindingsCard {
            company_id: company,
            bindings: &usage.bindings,
            truncated: usage.truncated,
        },
    )))
}
