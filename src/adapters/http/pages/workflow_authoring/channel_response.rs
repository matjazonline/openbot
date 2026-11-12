//! Response settings use the same revisioned command as the JSON API.
use super::*;
use crate::application::workflow::channel_reply::ChannelReplySettings;
use crate::application::workflow::reads::BindingUse;
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
        </form></section>"##,
        company = view.company,
        channel = view.channel,
        revision = escape_html_attr(&form.revision),
        options = options(view, &form.selection),
        contract = escape_html_text(&form.contract),
        policy = escape_html_text(&form.approval_policy)
    )
}
