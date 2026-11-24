//! Response settings use the same revisioned command as the JSON API.
use super::*;
use crate::application::workflow::channel_reply::ChannelReplySettings;
use crate::application::workflow::reads::{BindingUse, DeskCallFacts};
use crate::entities::channel::ChannelAccessMode;
use serde_json::json;

pub struct ChannelResponseForm {
    pub revision: String,
    pub selection: String,
    pub contract: String,
    pub approval_policy: String,
}

impl ChannelResponseForm {
    pub fn stored(settings: Option<&ChannelReplySettings>, bindings: &[BindingUse]) -> Self {
        let selected = settings.and_then(|value| value.responder.as_ref());
        let selection = selected
            .map(|selected| {
                let revision = bindings
                    .iter()
                    .find(|row| row.binding.binding == selected.binding)
                    .map_or(0, |row| row.binding.state_revision);
                format!("{}:{revision}", selected.binding.as_uuid())
            })
            .unwrap_or_default();
        Self {
            revision: settings.map(|value| value.revision.get().to_string()).unwrap_or_default(),
            selection,
            contract: selected.map(|value| json!(value.contract)).unwrap_or_else(||
                json!({"format_version":1,"subject":{"literal":"Response"},"body":{"ref":"/input"}})
            ).to_string(),
            approval_policy: settings.and_then(|value| value.approval_policy.as_ref())
                .map(|value| json!(value).to_string()).unwrap_or_default(),
        }
    }
}

pub struct ChannelResponseSettingsView<'a> {
    pub company: Uuid,
    pub channel: Uuid,
    pub settings: Option<&'a ChannelReplySettings>,
    pub bindings: &'a [BindingUse],
    pub draft: Option<&'a ChannelResponseForm>,
    pub error: Option<&'a str>,
    /// The desk's call facts; `None` renders no "Calls from agents" block.
    pub desk: Option<DeskCallFacts>,
}

/// Whether `channel.call` can reach the desk, judged from what the card already holds. The
/// launch re-judges every call (writability per agent, ancestry, intake), so this is a
/// configuration summary, never an authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallStatus {
    Public,
    Members,
    Closed(&'static str),
}

impl CallStatus {
    fn of(facts: DeskCallFacts, view: &ChannelResponseSettingsView<'_>) -> Self {
        let selected = view.settings.and_then(|value| value.responder.as_ref());
        let paused = selected.is_some_and(|selected| {
            view.bindings
                .iter()
                .find(|row| row.binding.binding == selected.binding)
                .is_some_and(|row| !row.binding.active)
        });
        if !facts.enabled {
            Self::Closed("the desk is disabled")
        } else if selected.is_none() {
            Self::Closed("no responding workflow is selected")
        } else if paused {
            Self::Closed("the selected responding workflow is paused")
        } else if facts.unit_archived {
            Self::Closed("its unit is archived")
        } else {
            match facts.access_mode {
                ChannelAccessMode::Public => Self::Public,
                ChannelAccessMode::Members => Self::Members,
            }
        }
    }

    fn text(self) -> String {
        match self {
            Self::Public => "Any agent in the company can call this desk.".into(),
            Self::Members => {
                "Only agents granted participation can call this desk; it is not listed to agents."
                    .into()
            }
            Self::Closed(reason) => format!("Agents cannot call this desk: {reason}."),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Members => "members",
            Self::Closed(_) => "closed",
        }
    }
}

/// The read-only "Calls from agents" block: status, the copyable desk id, the responder and
/// whether a final review applies. It shows nothing from the responder's configuration.
fn calls_from_agents(view: &ChannelResponseSettingsView<'_>) -> String {
    let Some(facts) = view.desk else {
        return String::new();
    };
    let status = CallStatus::of(facts, view);
    let responder = match view.settings.and_then(|value| value.responder.as_ref()) {
        None => "None selected".to_string(),
        Some(selected) => view
            .bindings
            .iter()
            .find(|row| row.binding.binding == selected.binding)
            .map_or_else(
                || "A workflow outside this list".to_string(),
                |row| row.workflow_title.clone(),
            ),
    };
    let review = if view
        .settings
        .is_some_and(|value| value.approval_policy.is_some())
    {
        "Yes"
    } else {
        "No"
    };
    format!(
        r##"<section class="space-y-2 border-t border-base-300 pt-4" aria-labelledby="channel-calls-heading">
        <h4 id="channel-calls-heading" class="font-semibold">Calls from agents</h4>
        <p class="text-sm" data-call-status="{key}">{status}</p>
        <label class="text-sm" for="channel-call-id">Desk id for the call tool</label>
        <div class="flex gap-2"><input id="channel-call-id" class="input input-sm flex-1 min-w-0 font-mono" type="text" value="{channel}" readonly aria-describedby="channel-call-id-help">
        <button type="button" class="btn btn-sm" data-action="copy-text" data-copy-from="channel-call-id" data-copied-label="Copied">Copy</button></div>
        <p id="channel-call-id-help" class="text-xs opacity-70">An agent with the call tool names this id as channel_id.</p>
        <p class="text-sm">Responder: <span data-call-responder>{responder}</span></p>
        <p class="text-sm">Final review required: <span data-call-review>{review}</span></p>
        </section>"##,
        key = status.key(),
        status = escape_html_text(&status.text()),
        channel = view.channel,
        responder = escape_html_text(&responder),
    )
}

fn options(view: &ChannelResponseSettingsView<'_>, selected: &str) -> String {
    let mut found = selected.is_empty();
    let mut rows =
        String::from(r#"<option value="">No automatic response (observe only)</option>"#);
    for row in view.bindings {
        let binding = &row.binding;
        if binding.message_trigger.is_none()
            || !matches!(
                binding.association,
                crate::application::workflow::RelatedAssociation::Channel(_)
            )
        {
            continue;
        }
        let value = format!("{}:{}", binding.binding.as_uuid(), binding.state_revision);
        let chosen = value == selected;
        found |= chosen;
        rows.push_str(&format!(
            r#"<option value="{value}"{chosen}>{title}{paused}</option>"#,
            chosen = if chosen { " selected" } else { "" },
            title = escape_html_text(&row.workflow_title),
            paused = if binding.active {
                ""
            } else {
                " (paused — activate before selecting)"
            }
        ));
    }
    if !found {
        // A truncated list must never silently turn an unseen/stale selection into a clear.
        rows.push_str(&format!(r#"<option value="{}" selected>Selected binding changed or is outside this list; select it again</option>"#,
            escape_html_attr(selected)));
    }
    rows
}

pub fn channel_response_settings(view: &ChannelResponseSettingsView<'_>) -> String {
    let stored = ChannelResponseForm::stored(view.settings, view.bindings);
    let form = view.draft.unwrap_or(&stored);
    let error = view.error.map(|message| format!(r##"<p role="alert" class="text-sm text-error">{message}</p>
        <button type="button" class="btn btn-ghost btn-sm" hx-get="/ui/workflows/channels/{channel}/bindings?company_id={company}" hx-target="#channel-workflows-card" hx-swap="outerHTML" hx-sync="#channel-workflows-card:replace">Reload current settings</button>"##,
        message=escape_html_text(message),company=view.company,channel=view.channel)).unwrap_or_default();
    format!(
        r##"<section class="space-y-3 border-t border-base-300 pt-4" aria-labelledby="channel-response-heading">
        <h4 id="channel-response-heading" class="font-semibold">Automatic response</h4>
        <p class="text-sm">One selected workflow responds to each admitted message. Other bindings observe. The final business result supplies the response text.</p>
        {error}<form hx-post="/ui/workflows/channels/{channel}/response?company_id={company}" hx-target="#channel-workflows-card" hx-swap="outerHTML" hx-disabled-elt="find button[type='submit']" class="space-y-3">
        <input type="hidden" name="expected_revision" value="{revision}">
        <label class="form-control">Responding workflow<select class="select w-full" name="responder">{options}</select></label>
        <label class="form-control">Final result mapping (JSON)<textarea class="textarea w-full" name="contract" rows="4" maxlength="16384" aria-describedby="channel-response-help">{contract}</textarea></label>
        <p id="channel-response-help" class="text-xs opacity-70">Use format_version 1 with subject and body. Each selects a literal or a ref such as /input/body from the final result. Required string fields only; mapping is checked when saved.</p>
        <label class="form-control">Final response approval policy (optional JSON)<textarea class="textarea w-full" name="approval_policy" rows="2" maxlength="16384" aria-describedby="channel-response-review-help">{policy}</textarea></label>
        <p id="channel-response-review-help" class="text-xs opacity-70">Leave blank for no final review. Otherwise name static person-principal reviewers and review_timeout_seconds (1–86400). Review requests stay private.</p>
        <button type="submit" class="btn btn-primary btn-sm">Save response settings</button>
        </form></section>{calls}"##,
        calls = calls_from_agents(view),
        company = view.company,
        channel = view.channel,
        revision = escape_html_attr(&form.revision),
        options = options(view, &form.selection),
        contract = escape_html_text(&form.contract),
        policy = escape_html_text(&form.approval_policy)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::workflow::channel_reply::{
        ReplyApprovalPolicy, ReplySettingsRevision, SelectedResponder,
    };
    use crate::application::workflow::reads::BindingSummary;
    use crate::application::workflow::{RelatedAssociation, RelatedChannelId};
    use crate::domain::workflow::{BindingRevision, VersionId, WorkflowBindingId, WorkflowId};

    const SECRET_PARAM: &str = "secret-param-value";

    fn binding(id: WorkflowBindingId, channel: Uuid, active: bool) -> BindingUse {
        BindingUse {
            binding: BindingSummary {
                binding: id,
                state_revision: 3,
                configuration_revision: 1,
                active,
                association: RelatedAssociation::Channel(RelatedChannelId::new(channel)),
                workflow: WorkflowId::new(Uuid::new_v4()),
                version: VersionId::new(Uuid::new_v4()),
                params: json!({"token": SECRET_PARAM}),
                run_as: Uuid::new_v4(),
                message_trigger: None,
                escalation: None,
            },
            workflow_title: "Support <desk> answers".into(),
            channel_name: None,
            resources: Default::default(),
            schedules: vec![],
            schedules_truncated: false,
        }
    }

    fn settings(id: WorkflowBindingId, reviewed: bool) -> ChannelReplySettings {
        ChannelReplySettings {
            revision: ReplySettingsRevision::try_from(1).unwrap(),
            responder: Some(SelectedResponder {
                binding: id,
                configuration_revision: BindingRevision::new(1).unwrap(),
                contract: serde_json::from_value(json!({"format_version":1,
                    "subject":{"literal":"Response"},"body":{"ref":"/input"}}))
                .unwrap(),
            }),
            approval_policy: reviewed
                .then(|| ReplyApprovalPolicy::new(vec![Uuid::new_v4()], 60).unwrap()),
        }
    }

    fn facts(access_mode: ChannelAccessMode) -> DeskCallFacts {
        DeskCallFacts {
            access_mode,
            enabled: true,
            unit_archived: false,
        }
    }

    fn render(
        desk: Option<DeskCallFacts>,
        settings: Option<&ChannelReplySettings>,
        bindings: &[BindingUse],
        channel: Uuid,
    ) -> String {
        channel_response_settings(&ChannelResponseSettingsView {
            company: Uuid::new_v4(),
            channel,
            settings,
            bindings,
            draft: None,
            error: None,
            desk,
        })
    }

    /// C7: the block shows the status by access mode, the first failing reason, the copyable
    /// desk id, the responder's label and whether a final review applies, and nothing of the
    /// responder's configuration.
    #[test]
    fn channel_call_authoring_desk_card() {
        let channel = Uuid::new_v4();
        let id = WorkflowBindingId::new(Uuid::new_v4());
        let rows = [binding(id, channel, true)];
        let reviewed = settings(id, true);
        let html = render(
            Some(facts(ChannelAccessMode::Public)),
            Some(&reviewed),
            &rows,
            channel,
        );
        assert!(html.contains(
            r#"<h4 id="channel-calls-heading" class="font-semibold">Calls from agents</h4>"#
        ));
        assert!(html.contains(
            r#"data-call-status="public">Any agent in the company can call this desk.</p>"#
        ));
        assert!(html.contains(&format!(
            r#"<input id="channel-call-id" class="input input-sm flex-1 min-w-0 font-mono" type="text" value="{channel}" readonly aria-describedby="channel-call-id-help">"#
        )));
        assert!(html.contains(r#"<label class="text-sm" for="channel-call-id">"#));
        assert!(html.contains(r#"data-action="copy-text" data-copy-from="channel-call-id""#));
        assert!(html.contains("<span data-call-responder>Support &lt;desk&gt; answers</span>"));
        assert!(html.contains("<span data-call-review>Yes</span>"));
        assert!(!html.contains(SECRET_PARAM), "no params reach the card");

        let open = settings(id, false);
        let members = render(
            Some(facts(ChannelAccessMode::Members)),
            Some(&open),
            &rows,
            channel,
        );
        assert!(members.contains(
            r#"data-call-status="members">Only agents granted participation can call this desk; it is not listed to agents.</p>"#
        ));
        assert!(members.contains("<span data-call-review>No</span>"));

        let closed =
            |desk: DeskCallFacts, settings: Option<&ChannelReplySettings>, rows: &[BindingUse]| {
                let html = render(Some(desk), settings, rows, channel);
                let start = html.find(r#"data-call-status="closed">"#).expect("closed");
                html[start..].split("</p>").next().unwrap().to_string()
            };
        let public = facts(ChannelAccessMode::Public);
        // The first failing reason, in order: disabled, no responder, paused, archived.
        let everything_wrong = DeskCallFacts {
            enabled: false,
            unit_archived: true,
            ..public
        };
        assert!(closed(everything_wrong, None, &rows).ends_with("the desk is disabled."));
        let archived = DeskCallFacts {
            unit_archived: true,
            ..public
        };
        assert!(closed(archived, None, &rows).ends_with("no responding workflow is selected."));
        let paused = [binding(id, channel, false)];
        assert!(
            closed(archived, Some(&open), &paused)
                .ends_with("the selected responding workflow is paused.")
        );
        assert!(closed(archived, Some(&open), &rows).ends_with("its unit is archived."));
        let none = render(None, Some(&open), &rows, channel);
        assert!(!none.contains("Calls from agents"), "no facts, no block");
        let elsewhere = render(Some(public), Some(&open), &[], channel);
        assert!(
            elsewhere.contains("<span data-call-responder>A workflow outside this list</span>")
        );
    }
}
