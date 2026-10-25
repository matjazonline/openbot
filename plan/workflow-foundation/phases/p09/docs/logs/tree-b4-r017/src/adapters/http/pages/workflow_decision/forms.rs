use super::*;
use crate::application::workflow::authoring::{
    ParameterField, ParameterInput, field_value, fields_for, parameter_fields,
};

pub(super) fn submission(view: &DecisionView, inputs: Option<&DecisionInputs>) -> String {
    let fresh_key = Uuid::new_v4().to_string();
    let revision = view.revision.to_string();
    let value = |name: &str, default: &str| {
        escape_html_attr(
            inputs
                .and_then(|inputs| inputs.get(name))
                .unwrap_or(default),
        )
    };
    let buttons:String=view.choices.iter().map(|choice|format!(r#"<button type="submit" name="choice" value="{value}" class="btn btn-outline">Submit decision: {choice}{required}</button>"#,
        value=escape_html_attr(choice),choice=escape_html_text(choice),required=if view.feedback_required.contains(choice){" (feedback required)"}else{""})).collect();
    format!(
        r##"<form id="decision-inputs-{decision}-{revision}" hx-preserve="true"
        hx-post="/ui/workflow-decisions/{decision}/submit" hx-target="#workflow-decision" hx-swap="outerHTML"
        hx-sync="#decision-shell:queue all" hx-disabled-elt="#workflow-decision button[type='submit']" class="space-y-3">
        <input type="hidden" name="command_key" value="{key}">
        <input type="hidden" name="expected_revision" value="{expected}">
        <input type="hidden" name="expected_artifact" value="{artifact}">
        {data}<label class="fieldset"><span class="fieldset-legend">Feedback</span>
        <textarea name="feedback" rows="3" maxlength="16384" class="textarea w-full">
{feedback}</textarea></label>
        <p class="text-xs opacity-70">Submitting settles this exact artifact. Required feedback and data are validated before the decision closes.</p>
        <div class="flex flex-wrap gap-2">{buttons}</div>
        <span class="hidden text-sm [.htmx-request_&]:inline" role="status">Submitting…</span></form>"##,
        decision = view.decision.0,
        key = value("command_key", &fresh_key),
        expected = value("expected_revision", &revision),
        artifact = value("expected_artifact", &view.artifact_digest),
        data = data(view, inputs),
        feedback = escape_html_text(
            inputs
                .and_then(|inputs| inputs.get("feedback"))
                .unwrap_or("")
        )
    )
}

fn data(view: &DecisionView, inputs: Option<&DecisionInputs>) -> String {
    if view.subject != DecisionSubject::Business {
        return r#"<input type="hidden" name="data_mode" value="json"><input type="hidden" name="data" value="{}"><p class="text-sm">The protected request above is immutable. Approve or reject it as shown.</p>"#.into();
    }
    let fields = match inputs.and_then(|inputs| inputs.get("data_mode")) {
        Some("json") => None,
        Some("fields") => parameter_fields(view.data_schema.as_ref()),
        _ => fields_for(view.data_schema.as_ref(), &view.proposal),
    };
    let controls = if let Some(fields) = fields {
        let controls: String = fields
            .iter()
            .map(|field| {
                let name = format!("data:{}", field.name);
                let initial = field_value(field, &view.proposal);
                let value = inputs
                    .and_then(|inputs| inputs.get(&name))
                    .unwrap_or(&initial);
                field_control(field, value)
            })
            .collect();
        format!(
            r#"<input type="hidden" name="data_mode" value="fields"><fieldset class="space-y-2"><legend class="font-semibold">Editable decision data</legend>{controls}</fieldset>"#
        )
    } else {
        let initial = serde_json::to_string_pretty(&view.proposal).unwrap_or_default();
        let raw = inputs
            .and_then(|inputs| inputs.get("data"))
            .unwrap_or(&initial);
        format!(
            r#"<input type="hidden" name="data_mode" value="json"><label class="fieldset"><span class="fieldset-legend">Decision data (JSON)</span>
            <textarea name="data" rows="8" class="textarea w-full font-mono text-sm">
{raw}</textarea></label><p class="text-xs opacity-70">This value uses JSON so all keys and values are preserved exactly.</p>"#,
            raw = escape_html_text(raw)
        )
    };
    format!(
        r#"{controls}<details><summary class="link text-xs">Data schema</summary><pre class="whitespace-pre-wrap text-xs">{schema}</pre></details>"#,
        schema = pretty(
            view.data_schema
                .as_ref()
                .unwrap_or(&serde_json::Value::Null)
        )
    )
}

fn field_control(field: &ParameterField, value: &str) -> String {
    let name = escape_html_attr(&format!("data:{}", field.name));
    let required = if field.required { " required" } else { "" };
    let control = match &field.input {
        ParameterInput::Text { max_length } => format!(
            r#"<textarea name="{name}" class="textarea w-full" rows="2"{required}{max}>
{value}</textarea>"#,
            max = max_length
                .map(|max| format!(r#" maxlength="{max}""#))
                .unwrap_or_default(),
            value = escape_html_text(value)
        ),
        ParameterInput::Integer | ParameterInput::Number => format!(
            r#"<input type="number" name="{name}" class="input w-full" step="{step}" value="{value}"{required}>"#,
            step = if field.input == ParameterInput::Integer {
                "1"
            } else {
                "any"
            },
            value = escape_html_attr(value)
        ),
        ParameterInput::Boolean | ParameterInput::Choice(_) => {
            let choices: Vec<&str> = match &field.input {
                ParameterInput::Choice(choices) => choices.iter().map(String::as_str).collect(),
                _ => vec!["true", "false"],
            };
            let mut choices: Vec<_> = std::iter::once("").chain(choices).collect();
            if !choices.contains(&value) {
                choices.push(value);
            }
            let options: String = choices
                .into_iter()
                .map(|choice| {
                    format!(
                        r#"<option value="{value}"{selected}>{label}</option>"#,
                        value = escape_html_attr(choice),
                        selected = if choice == value { " selected" } else { "" },
                        label = escape_html_text(if choice.is_empty() {
                            "(not set)"
                        } else {
                            choice
                        })
                    )
                })
                .collect();
            format!(r#"<select name="{name}" class="select w-full"{required}>{options}</select>"#)
        }
        ParameterInput::Json => format!(
            r#"<textarea name="{name}" rows="3" class="textarea w-full font-mono text-xs"{required}>
{value}</textarea>"#,
            value = escape_html_text(value)
        ),
    };
    format!(
        r#"<label class="fieldset"><span class="fieldset-legend">{label}{mark}</span>{control}<span class="text-xs opacity-70">{description}</span></label>"#,
        label = escape_html_text(&field.name),
        mark = if field.required { " (required)" } else { "" },
        description = escape_html_text(field.description.as_deref().unwrap_or(""))
    )
}
