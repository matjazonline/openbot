//! The bindings tab (p09 B1b): every binding of the workflow the viewer may see, with its
//! version, state, trigger, run-as, resources and schedules; one form that creates or
//! configures a binding (association, version, schema-driven params, a resource per declared
//! slot, message trigger, run-as); activate and deactivate at the state revision seen. And the
//! channel pane's card of the bindings on that channel. Every write is the binding command the
//! JSON routes call; readiness, params and run-as are checked there, not here.
use super::*;
use crate::application::workflow::RelatedAssociation;
use crate::application::workflow::authoring::{ParameterField, ParameterInput};
use crate::application::workflow::reads::{BindingUse, ChoiceOption, VersionSummary};
use crate::domain::workflow::WorkflowDefinition;
use std::collections::BTreeMap;

const TRIGGERS: [(&str, &str); 5] = [
    ("", "No message trigger: messages never start it"),
    ("direct", "Addressed in To, or mentioned"),
    ("any", "Addressed in To or Cc, or mentioned"),
    ("mentioned", "Mentioned by name, alias, address or handle"),
    (
        "mentioned_or_reply",
        "Mentioned, or a direct reply to a message a run published",
    ),
];

/// What the last write on the tab did.
pub enum BindingBanner<'a> {
    Done(&'a str),
    /// A refused activation or deactivation, shown on its binding's row.
    RowRefused {
        binding: Uuid,
        message: &'a str,
    },
}

pub struct BindingsView<'a> {
    pub company_id: Uuid,
    pub workflow: Uuid,
    pub archived: bool,
    /// Newest first.
    pub versions: &'a [VersionSummary],
    /// The bindings the viewer may see.
    pub bindings: &'a [BindingUse],
    pub truncated: bool,
    /// Bindings left out (of those listed) because they are on channels the viewer cannot see.
    pub hidden: usize,
    /// Every person the company has, for naming the run-as.
    pub people: &'a [ChoiceOption],
    pub banner: Option<BindingBanner<'a>>,
    /// The form, already rendered (create unless a binding is being configured).
    pub form_html: &'a str,
}

pub fn workflow_bindings_body(view: &BindingsView<'_>) -> String {
    let banner = match &view.banner {
        Some(BindingBanner::Done(message)) => status_banner(message),
        _ => String::new(),
    };
    let rows: String = view
        .bindings
        .iter()
        .map(|binding| binding_row(view, binding))
        .collect();
    let empty = if view.bindings.is_empty() && view.hidden == 0 {
        r#"<div role="status" class="alert alert-warning text-sm">Not bound: nothing runs this workflow. Messages and schedules never fall back to a default workflow; a published version runs only once a binding selects it and is activated.</div>"#
    } else {
        ""
    };
    let hidden = if view.hidden > 0 {
        format!(
            r#"<p class="text-xs opacity-70">{} more of the listed binding{} {} on channels you cannot see.</p>"#,
            view.hidden,
            if view.hidden == 1 { "" } else { "s" },
            if view.hidden == 1 { "is" } else { "are" },
        )
    } else {
        String::new()
    };
    let cut = if view.truncated {
        r#"<p class="text-xs text-error">Only the first 200 bindings are listed.</p>"#
    } else {
        ""
    };
    format!(
        r#"{banner}<div class="space-y-6">
            <section class="space-y-2"><h3 class="font-semibold">Bindings</h3>{empty}<ul class="divide-y divide-base-300">{rows}</ul>{hidden}{cut}</section>
            {form}
        </div>"#,
        form = view.form_html,
    )
}

fn version_label(versions: &[VersionSummary], version: Uuid) -> String {
    match versions.iter().position(|v| v.version.as_uuid() == version) {
        Some(0) => format!(
            "version {} (newest, draft revision {})",
            short_id(version),
            versions[0].draft_revision
        ),
        Some(index) => format!(
            "older version {} (draft revision {})",
            short_id(version),
            versions[index].draft_revision
        ),
        None => format!("version {}", short_id(version)),
    }
}

fn person_label(people: &[ChoiceOption], user: Uuid) -> String {
    people
        .iter()
        .find(|person| person.id == user)
        .map(|person| format!("{} ({})", person.label, person.detail))
        .unwrap_or_else(|| format!("user {}", short_id(user)))
}

fn binding_row(view: &BindingsView<'_>, binding: &BindingUse) -> String {
    let (company_id, workflow) = (view.company_id, view.workflow);
    let summary = &binding.binding;
    let id = summary.binding.as_uuid();
    let state = if summary.active {
        r#"<span class="badge badge-sm badge-success">active</span>"#
    } else {
        r#"<span class="badge badge-sm badge-ghost">paused: admits nothing</span>"#
    };
    let trigger = match summary.message_trigger {
        Some(trigger) => format!(
            r#"Message trigger <span class="font-mono">{}</span>"#,
            trigger.as_str()
        ),
        None => "No message trigger".to_owned(),
    };
    let resources = if binding.resources.is_empty() {
        String::new()
    } else {
        let slots: Vec<String> = binding
            .resources
            .iter()
            .map(|(slot, resource)| {
                format!(
                    r#"<span class="font-mono">{}</span> → {}"#,
                    escape_html_text(slot),
                    short_id(*resource)
                )
            })
            .collect();
        format!(
            r#"<p class="text-xs opacity-70">Resources: {}</p>"#,
            slots.join(", ")
        )
    };
    let schedules = if binding.schedules.is_empty() {
        String::new()
    } else {
        let names: Vec<String> = binding
            .schedules
            .iter()
            .map(|schedule| {
                format!(
                    "{}{}",
                    escape_html_text(&schedule.name),
                    if schedule.enabled { "" } else { " (disabled)" }
                )
            })
            .collect();
        format!(
            r#"<p class="text-xs opacity-70">Schedules: {}{}</p>"#,
            names.join(", "),
            if binding.schedules_truncated {
                ", …"
            } else {
                ""
            }
        )
    };
    let refused = match &view.banner {
        Some(BindingBanner::RowRefused { binding, message }) if *binding == id => format!(
            r#"<div role="alert" class="alert alert-error mt-2 text-sm">{}</div>"#,
            escape_html_text(message)
        ),
        _ => String::new(),
    };
    // An archived workflow's binding can still be paused, never activated or reconfigured.
    let toggle = if view.archived && !summary.active {
        String::new()
    } else if view.archived {
        format!(
            r##"<button type="button" class="btn btn-xs btn-outline"
                hx-post="/ui/workflows/{workflow}/bindings/{id}/deactivate?company_id={company_id}"
                hx-vals='{{"expected_state_revision": "{revision}"}}'
                hx-target="#workflow-pane" hx-swap="outerHTML">Deactivate</button>"##,
            revision = summary.state_revision,
        )
    } else {
        let (action, label, class) = if summary.active {
            ("deactivate", "Deactivate", "btn-outline")
        } else {
            ("activate", "Activate", "btn-primary")
        };
        format!(
            r##"<button type="button" class="btn btn-xs {class}"
                hx-post="/ui/workflows/{workflow}/bindings/{id}/{action}?company_id={company_id}"
                hx-vals='{{"expected_state_revision": "{revision}"}}'
                hx-target="#workflow-pane" hx-swap="outerHTML">{label}</button>
            <button type="button" class="btn btn-ghost btn-xs"
                hx-get="/ui/workflows/{workflow}/bindings/form?company_id={company_id}&amp;binding_id={id}"
                hx-target="#binding-form" hx-swap="outerHTML">Configure</button>"##,
            revision = summary.state_revision,
        )
    };
    format!(
        r#"<li class="py-3" id="binding-{id}">
            <div class="flex flex-wrap items-center gap-2"><span class="font-semibold">{association}</span>{state}<span class="ml-auto flex gap-1">{toggle}</span></div>
            <p class="text-xs opacity-70">Runs {version} · {trigger} · as {run_as}</p>
            {resources}{schedules}
            <p class="font-mono text-[11px] opacity-60">binding {short} · state revision {revision} · <a class="link" href="/ui/workflow-runs?company_id={company_id}&amp;binding_id={id}">runs</a></p>{refused}
        </li>"#,
        association = escape_html_text(&super::versions::association_label(binding)),
        version = escape_html_text(&version_label(view.versions, summary.version.as_uuid())),
        run_as = escape_html_text(&person_label(view.people, summary.run_as)),
        short = short_id(id),
        revision = summary.state_revision,
    )
}

/// The params part of the form: one field per schema property, or the whole object as JSON.
pub enum BindingParams<'a> {
    Fields {
        fields: &'a [ParameterField],
        /// The value each field shows, by parameter name.
        values: &'a BTreeMap<String, String>,
    },
    Json(&'a str),
}

pub struct BindingFormView<'a> {
    pub company_id: Uuid,
    pub workflow: Uuid,
    /// The binding being configured; `None` creates one under `form_key`.
    pub editing: Option<&'a BindingUse>,
    pub form_key: &'a str,
    /// Newest first.
    pub versions: &'a [VersionSummary],
    pub version: Option<Uuid>,
    /// The chosen version's definition, when it is selectable.
    pub definition: Option<&'a WorkflowDefinition>,
    /// `company` or `channel:<id>`.
    pub association: &'a str,
    /// The channels the viewer can see.
    pub channels: &'a [ChoiceOption],
    pub params: BindingParams<'a>,
    /// The resource chosen per slot.
    pub resources: &'a BTreeMap<String, Uuid>,
    pub http: &'a [ChoiceOption],
    pub mcp: &'a [ChoiceOption],
    pub message_trigger: &'a str,
    pub run_as: Option<Uuid>,
    /// Only the company owner may name somebody else.
    pub owner: bool,
    pub viewer: Uuid,
    pub people: &'a [ChoiceOption],
    pub options_truncated: bool,
    pub error: Option<&'a str>,
}

pub fn workflow_binding_form(form: &BindingFormView<'_>) -> String {
    let (company_id, workflow) = (form.company_id, form.workflow);
    if form.versions.is_empty() {
        return r#"<section id="binding-form" class="space-y-2 rounded-box border border-base-300 p-4"><h3 class="font-semibold">New binding</h3><p class="text-sm opacity-70">Publish a version first: a binding selects a published version, never the draft.</p></section>"#.to_owned();
    }
    let heading = match form.editing {
        Some(binding) => format!(
            "Configure binding {}",
            short_id(binding.binding.binding.as_uuid())
        ),
        None => "New binding".to_owned(),
    };
    let identity = match form.editing {
        Some(binding) => format!(
            r#"<input type="hidden" name="binding_id" value="{id}">
            <input type="hidden" name="expected_state_revision" value="{revision}">"#,
            id = binding.binding.binding.as_uuid(),
            revision = binding.binding.state_revision,
        ),
        None => format!(
            r#"<input type="hidden" name="form_key" value="{}">"#,
            escape_html_attr(form.form_key)
        ),
    };
    let error = form_error_banner(form.error);
    format!(
        r##"<section id="binding-form" class="space-y-3 rounded-box border border-base-300 p-4">
            <h3 class="font-semibold">{heading}</h3>
            {error}
            <form hx-post="/ui/workflows/{workflow}/bindings?company_id={company_id}" hx-target="#workflow-pane" hx-swap="outerHTML"
                hx-disabled-elt="find button[type='submit']" class="space-y-4">
                {identity}
                <div class="grid gap-3 sm:grid-cols-2">{association}{version}</div>
                {params}
                {resources}
                <div class="grid gap-3 sm:grid-cols-2">{trigger}{run_as}</div>
                {cut}
                <div class="flex items-center gap-3 border-t border-base-300 pt-3">
                    <button type="submit" class="btn btn-primary btn-sm">Save binding</button>
                    <p class="text-xs opacity-70">A saved binding stays paused until you activate it; activation checks readiness again.</p>
                </div>
            </form>
        </section>"##,
        heading = escape_html_text(&heading),
        association = association_field(form),
        version = version_field(form),
        params = params_fields(form),
        resources = resource_fields(form),
        trigger = trigger_field(form),
        run_as = run_as_field(form),
        cut = if form.options_truncated {
            r#"<p class="text-xs text-error">Some lists are cut at 200 entries.</p>"#
        } else {
            ""
        },
    )
}

fn labelled(label: &str, hint: &str, control: &str) -> String {
    format!(
        // `.label` already dims its text; another opacity on top falls under 3:1 in light.
        r#"<label class="form-control w-full"><div class="label"><span class="text-xs">{label}</span></div>{control}{hint}</label>"#,
        label = escape_html_text(label),
        hint = if hint.is_empty() {
            String::new()
        } else {
            format!(
                r#"<div class="label"><span class="text-xs">{}</span></div>"#,
                escape_html_text(hint)
            )
        },
    )
}

fn association_field(form: &BindingFormView<'_>) -> String {
    if let Some(binding) = form.editing {
        return labelled(
            "Association",
            "Fixed once created: a different channel needs a new binding.",
            &format!(
                r#"<p class="py-2 text-sm font-semibold">{}</p>"#,
                escape_html_text(&super::versions::association_label(binding))
            ),
        );
    }
    let option = |value: String, label: &str| {
        format!(
            r#"<option value="{value}"{selected}>{label}</option>"#,
            selected = if value == form.association {
                " selected"
            } else {
                ""
            },
            value = escape_html_attr(&value),
            label = escape_html_text(label),
        )
    };
    let mut options = option(
        "company".into(),
        "Company-wide (manual starts and schedules)",
    );
    for channel in form.channels {
        options.push_str(&option(
            format!("channel:{}", channel.id),
            &format!("Channel {}", channel.label),
        ));
    }
    labelled(
        "Association",
        "A channel binding can answer that channel's messages; thread bindings are made through the API.",
        &format!(r#"<select class="select w-full" name="association">{options}</select>"#),
    )
}

fn version_field(form: &BindingFormView<'_>) -> String {
    let options: String = form
        .versions
        .iter()
        .map(|version| {
            let id = version.version.as_uuid();
            format!(
                r#"<option value="{id}"{selected}>{label}</option>"#,
                selected = if Some(id) == form.version {
                    " selected"
                } else {
                    ""
                },
                label = escape_html_text(&version_label(form.versions, id)),
            )
        })
        .collect();
    labelled(
        "Published version",
        "Changing it reloads the parameters and resources this version declares.",
        &format!(
            r##"<select class="select w-full" name="version_id"
                hx-get="/ui/workflows/{workflow}/bindings/form?company_id={company_id}" hx-include="closest form"
                hx-target="#binding-form" hx-swap="outerHTML" hx-trigger="change">{options}</select>"##,
            workflow = form.workflow,
            company_id = form.company_id,
        ),
    )
}

fn params_fields(form: &BindingFormView<'_>) -> String {
    let Some(_) = form.definition else {
        return r#"<p class="text-sm text-error">This version cannot be selected now (its workflow is archived).</p>"#.to_owned();
    };
    match &form.params {
        BindingParams::Json(raw) => format!(
            r#"<fieldset class="space-y-1"><legend class="text-sm font-semibold">Parameters</legend><input type="hidden" name="params_mode" value="json">{field}</fieldset>"#,
            field = labelled(
                "Parameters (JSON object)",
                "Typed as one JSON object: the version's parameter schema has no fields this form can show, or the stored parameters hold something the fields cannot carry exactly. The binding command validates the object against the schema.",
                &format!(
                    r#"<textarea class="textarea w-full font-mono text-xs" rows="6" name="params_json">
{}</textarea>"#,
                    escape_html_text(raw)
                ),
            ),
        ),
        BindingParams::Fields { fields, values } => {
            let people: String = form
                .people
                .iter()
                .map(|person| {
                    format!(
                        r#"<option value="{id}">{label}</option>"#,
                        id = person.id,
                        label = escape_html_attr(&format!("{} ({})", person.label, person.detail)),
                    )
                })
                .collect();
            let controls: String = fields
                .iter()
                .map(|field| param_field(field, values.get(&field.name).map_or("", String::as_str)))
                .collect();
            format!(
                r#"<fieldset class="space-y-1"><legend class="text-sm font-semibold">Parameters</legend>
                    <input type="hidden" name="params_mode" value="fields">
                    <div class="grid gap-3 sm:grid-cols-2">{controls}</div>
                    <datalist id="binding-people">{people}</datalist>
                    <p class="text-xs opacity-60">A text parameter suggests the company's people by id; an empty optional field is left out.</p>
                </fieldset>"#
            )
        }
    }
}

fn param_field(field: &ParameterField, value: &str) -> String {
    let name = format!("param:{}", field.name);
    let required = if field.required { " required" } else { "" };
    let control = match &field.input {
        ParameterInput::Text { max_length } => format!(
            r#"<input class="input w-full" name="{name}" value="{value}" list="binding-people"{max}{required}>"#,
            name = escape_html_attr(&name),
            value = escape_html_attr(value),
            max = max_length
                .map(|max| format!(r#" maxlength="{max}""#))
                .unwrap_or_default(),
        ),
        ParameterInput::Integer | ParameterInput::Number => format!(
            r#"<input class="input w-full" type="number" step="{step}" name="{name}" value="{value}"{required}>"#,
            step = if field.input == ParameterInput::Integer {
                "1"
            } else {
                "any"
            },
            name = escape_html_attr(&name),
            value = escape_html_attr(value),
        ),
        ParameterInput::Boolean | ParameterInput::Choice(_) => {
            let choices: Vec<&str> = match &field.input {
                ParameterInput::Choice(choices) => choices.iter().map(String::as_str).collect(),
                _ => vec!["true", "false"],
            };
            let options: String = std::iter::once("")
                .chain(choices)
                .map(|choice| {
                    format!(
                        r#"<option value="{choice}"{selected}>{label}</option>"#,
                        selected = if choice == value { " selected" } else { "" },
                        label = if choice.is_empty() {
                            "(not set)".to_owned()
                        } else {
                            escape_html_text(choice)
                        },
                        choice = escape_html_attr(choice),
                    )
                })
                .collect();
            format!(
                r#"<select class="select w-full" name="{name}">{options}</select>"#,
                name = escape_html_attr(&name),
            )
        }
        ParameterInput::Json => format!(
            r#"<textarea class="textarea w-full font-mono text-xs" rows="3" name="{name}">
{value}</textarea>"#,
            name = escape_html_attr(&name),
            value = escape_html_text(value),
        ),
    };
    let label = format!(
        "{}{}",
        field.name,
        if field.required { " (required)" } else { "" }
    );
    labelled(&label, field.description.as_deref().unwrap_or(""), &control)
}

fn resource_fields(form: &BindingFormView<'_>) -> String {
    let Some(definition) = form.definition else {
        return String::new();
    };
    if definition.resources.is_empty() {
        return String::new();
    }
    let fields: String = definition
        .resources
        .iter()
        .map(|requirement| {
            let slot = requirement.slot.as_str();
            let name = format!("resource:{slot}");
            let chosen = form.resources.get(slot).copied();
            let choices = match requirement.kind.as_str() {
                "messaging" => {
                    return labelled(
                        &format!("Slot {slot} (messaging)"),
                        "",
                        &format!(
                            r#"<input type="hidden" name="{name}" value="{company}"><p class="py-2 text-sm">This company's messaging</p>"#,
                            name = escape_html_attr(&name),
                            company = form.company_id,
                        ),
                    );
                }
                "http" => form.http,
                "mcp" => form.mcp,
                _ => &[][..],
            };
            let options: String = std::iter::once(format!(
                r#"<option value="">(choose a {} connection)</option>"#,
                escape_html_text(requirement.kind.as_str())
            ))
            .chain(choices.iter().map(|choice| {
                format!(
                    r#"<option value="{id}"{selected}>{label}{disabled}</option>"#,
                    id = choice.id,
                    selected = if Some(choice.id) == chosen { " selected" } else { "" },
                    label = escape_html_text(&choice.label),
                    disabled = if choice.enabled { "" } else { " (disabled)" },
                )
            }))
            .collect();
            let hint = match &requirement.contract {
                Some(contract) => format!("Must support {}.", contract.as_str()),
                None => String::new(),
            };
            labelled(
                &format!("Slot {slot} ({})", requirement.kind.as_str()),
                &hint,
                &format!(
                    r#"<select class="select w-full" name="{name}">{options}</select>"#,
                    name = escape_html_attr(&name),
                ),
            )
        })
        .collect();
    format!(
        r#"<fieldset class="space-y-1"><legend class="text-sm font-semibold">Resources</legend><div class="grid gap-3 sm:grid-cols-2">{fields}</div></fieldset>"#
    )
}

fn trigger_field(form: &BindingFormView<'_>) -> String {
    let options: String = TRIGGERS
        .iter()
        .map(|(value, label)| {
            format!(
                r#"<option value="{value}"{selected}>{label}</option>"#,
                selected = if *value == form.message_trigger {
                    " selected"
                } else {
                    ""
                },
                label = escape_html_text(label),
            )
        })
        .collect();
    labelled(
        "Message trigger",
        "Channel bindings only; a company-wide binding takes none.",
        &format!(r#"<select class="select w-full" name="message_trigger">{options}</select>"#),
    )
}

fn run_as_field(form: &BindingFormView<'_>) -> String {
    let current = form
        .run_as
        .or(form.editing.map(|binding| binding.binding.run_as))
        .unwrap_or(form.viewer);
    if !form.owner {
        let said = match form.editing {
            Some(binding) => format!(
                "Runs as {}; only the company owner may change who a binding runs as.",
                person_label(form.people, binding.binding.run_as)
            ),
            None => "Runs as you; only the company owner may name somebody else.".to_owned(),
        };
        return labelled(
            "Runs as",
            "",
            &format!(r#"<p class="py-2 text-sm">{}</p>"#, escape_html_text(&said)),
        );
    }
    let options: String = form
        .people
        .iter()
        .filter(|person| person.detail == "owner" || person.detail == "admin")
        .map(|person| {
            format!(
                r#"<option value="{id}"{selected}>{label}</option>"#,
                id = person.id,
                selected = if person.id == current {
                    " selected"
                } else {
                    ""
                },
                label = escape_html_text(&format!("{} ({})", person.label, person.detail)),
            )
        })
        .collect();
    labelled(
        "Runs as",
        "An owner or admin who can see the channel; automatic runs use their authority.",
        &format!(r#"<select class="select w-full" name="run_as">{options}</select>"#),
    )
}

pub struct ChannelBindingsCard<'a> {
    pub company_id: Uuid,
    pub bindings: &'a [BindingUse],
    pub truncated: bool,
}

/// "Workflows on this channel": the bindings on the channel (and its threads), and plainly
/// whether any active one answers its messages.
pub fn channel_bindings_card(card: &ChannelBindingsCard<'_>) -> String {
    let company_id = card.company_id;
    let answering = card.bindings.iter().any(|binding| {
        binding.binding.active
            && binding.binding.message_trigger.is_some()
            && matches!(binding.binding.association, RelatedAssociation::Channel(_))
    });
    let status = if answering {
        r#"<p class="text-sm">An active workflow answers this channel's messages under its trigger.</p>"#
    } else {
        r#"<div role="status" class="alert alert-warning text-sm">No active workflow answers this channel's messages. Messages are stored and nothing runs until a binding with a message trigger is activated.</div>"#
    };
    let rows: String = card
        .bindings
        .iter()
        .map(|binding| {
            let summary = &binding.binding;
            let workflow = summary.workflow.as_uuid();
            format!(
                r#"<li class="py-2"><div class="flex flex-wrap items-center gap-2">
                    <a class="link font-semibold" href="/ui/workflows?company_id={company_id}&amp;workflow_id={workflow}&amp;tab=bindings">{title}</a>{state}</div>
                    <p class="text-xs opacity-70">{association} · {trigger}</p></li>"#,
                title = escape_html_text(if binding.workflow_title.is_empty() {
                    "(untitled workflow)"
                } else {
                    &binding.workflow_title
                }),
                state = if summary.active {
                    r#"<span class="badge badge-sm badge-success">active</span>"#
                } else {
                    r#"<span class="badge badge-sm badge-ghost">paused</span>"#
                },
                association = match summary.association {
                    RelatedAssociation::Thread { .. } => "one thread",
                    _ => "the whole channel",
                },
                trigger = match summary.message_trigger {
                    Some(trigger) => format!("message trigger {}", trigger.as_str()),
                    None => "no message trigger".to_owned(),
                },
            )
        })
        .collect();
    let list = if card.bindings.is_empty() {
        r#"<p class="text-xs opacity-70">No workflow is bound to this channel.</p>"#.to_owned()
    } else {
        format!(r#"<ul class="divide-y divide-base-300">{rows}</ul>"#)
    };
    format!(
        r#"<section id="channel-workflows-card" class="space-y-3 rounded-box border border-base-300 p-4">
            <div><h3 class="text-sm font-bold">Workflows on this channel</h3>
            <p class="text-xs opacity-70">Bind a workflow from its Bindings tab in Workflows.</p></div>
            {status}{list}{cut}
        </section>"#,
        cut = if card.truncated {
            r#"<p class="text-xs text-error">Only the first 200 bindings are listed.</p>"#
        } else {
            ""
        },
    )
}
