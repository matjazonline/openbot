//! The reply-handling settings page: whether outside replies run the agent or wait for the team.
//!
//! Not to be confused with `manual_handoffs`, whose pages live in [`super::attention`].

use crate::entities::thread_handoff::{ExternalReplyHandling, ExternalReplyHandlingPolicy};

use super::{base_layout, escape_html_attr, escape_html_text};

/// What a reader calls each policy, as distinct from the `as_str` the database stores.
const fn policy_label(policy: ExternalReplyHandling) -> &'static str {
    match policy {
        ExternalReplyHandling::Automatic => "Automatic",
        ExternalReplyHandling::ManualHandoff => "Manual handoff",
    }
}

/// The one sentence that says what choosing this does to a customer's reply.
const fn policy_consequence(policy: ExternalReplyHandling) -> &'static str {
    match policy {
        ExternalReplyHandling::Automatic => "Outside replies run the channel's agent immediately.",
        ExternalReplyHandling::ManualHandoff => {
            "Outside replies to existing threads are filed and wait for the team."
        }
    }
}

/// The effective policy *and where it came from*, which is the support question this page answers.
fn effective_description(policy: &ExternalReplyHandlingPolicy) -> String {
    let provenance = if policy.is_inherited() {
        "inherited from company"
    } else {
        "channel override"
    };
    format!("{} ({provenance})", policy_label(policy.effective()))
}

fn selected(value: bool) -> &'static str {
    if value { "selected" } else { "" }
}

fn policy_option(value: ExternalReplyHandling, is_selected: bool) -> String {
    format!(
        r#"<option value="{value}" {selected}>{label} — {consequence}</option>"#,
        value = escape_html_attr(value.as_str()),
        selected = selected(is_selected),
        label = escape_html_text(policy_label(value)),
        consequence = escape_html_text(policy_consequence(value)),
    )
}

pub fn reply_handling_policy_page(
    company_id: uuid::Uuid,
    channel_id: uuid::Uuid,
    policy: &ExternalReplyHandlingPolicy,
) -> String {
    let company_options = [
        ExternalReplyHandling::Automatic,
        ExternalReplyHandling::ManualHandoff,
    ]
    .into_iter()
    .map(|value| policy_option(value, policy.company_default == value))
    .collect::<Vec<_>>()
    .join("");
    let channel_options = [
        ExternalReplyHandling::Automatic,
        ExternalReplyHandling::ManualHandoff,
    ]
    .into_iter()
    .map(|value| policy_option(value, policy.channel_override == Some(value)))
    .collect::<Vec<_>>()
    .join("");
    let body = format!(
        r#"<header class="mb-6"><h1 class="text-2xl font-bold">Reply handling policy</h1>
           <p class="text-slate-400">Effective policy: {effective}</p></header>
           <div id="policy-result"></div>
           <form method="post" action="/ui/reply-handling/company" class="mb-5 space-y-3 rounded-xl border border-slate-700 p-4">
             <input type="hidden" name="company_id" value="{company_id}">
             <input type="hidden" name="channel_id" value="{channel_id}">
             <label class="block font-semibold">Company default</label>
             <select name="policy" class="rounded-lg bg-slate-950 p-2">
               {company_options}
             </select>
             <button class="rounded-lg bg-indigo-700 px-4 py-2" type="submit">Save company default</button>
           </form>
           <form method="post" action="/ui/reply-handling/channel" class="space-y-3 rounded-xl border border-slate-700 p-4">
             <input type="hidden" name="company_id" value="{company_id}">
             <input type="hidden" name="channel_id" value="{channel_id}">
             <label class="block font-semibold">Channel override</label>
             <select name="policy_override" class="rounded-lg bg-slate-950 p-2">
               <option value="inherit" {inherit}>Inherit company default — this channel follows the company as it changes.</option>
               {channel_options}
             </select>
             <button class="rounded-lg bg-indigo-700 px-4 py-2" type="submit">Save channel override</button>
           </form>"#,
        effective = escape_html_text(&effective_description(policy)),
        company_id = escape_html_attr(&company_id.to_string()),
        channel_id = escape_html_attr(&channel_id.to_string()),
        inherit = selected(policy.is_inherited()),
    );
    base_layout("Reply handling policy", &body)
}

pub fn reply_handling_policy_saved(company_id: uuid::Uuid, channel_id: uuid::Uuid) -> String {
    format!(
        r#"<div class="rounded-xl border border-emerald-700 p-4">Reply handling saved.
        <a class="ml-2 underline" href="/ui/reply-handling?company_id={company_id}&amp;channel_id={channel_id}">Reply handling settings</a></div>"#,
        company_id = escape_html_attr(&company_id.to_string()),
        channel_id = escape_html_attr(&channel_id.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(
        company_default: ExternalReplyHandling,
        channel_override: Option<ExternalReplyHandling>,
    ) -> String {
        reply_handling_policy_page(
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            &ExternalReplyHandlingPolicy::resolve(company_default, channel_override),
        )
    }

    /// The `<option value="…">` that carries `selected`, for one `<select>` of the page.
    fn selected_values(html: &str, select_name: &str) -> Vec<String> {
        let after = html
            .split_once(&format!(r#"<select name="{select_name}""#))
            .expect("the select is rendered")
            .1;
        let block = after.split_once("</select>").expect("the select closes").0;
        block
            .split("<option value=\"")
            .skip(1)
            .filter_map(|option| {
                let (value, rest) = option.split_once('"')?;
                rest.split_once('>')?
                    .0
                    .contains("selected")
                    .then(|| value.to_string())
            })
            .collect()
    }

    #[test]
    fn every_company_and_override_combination_marks_exactly_one_option_per_select() {
        for company_default in [
            ExternalReplyHandling::Automatic,
            ExternalReplyHandling::ManualHandoff,
        ] {
            for channel_override in [
                None,
                Some(ExternalReplyHandling::Automatic),
                Some(ExternalReplyHandling::ManualHandoff),
            ] {
                let html = page(company_default, channel_override);
                assert_eq!(
                    selected_values(&html, "policy"),
                    vec![company_default.as_str().to_string()],
                    "company default {company_default:?}"
                );
                let expected_override = channel_override
                    .map_or_else(|| "inherit".to_string(), |value| value.as_str().to_string());
                assert_eq!(
                    selected_values(&html, "policy_override"),
                    vec![expected_override],
                    "override {channel_override:?}"
                );
            }
        }
    }

    #[test]
    fn the_header_says_which_of_the_two_settings_the_effective_value_came_from() {
        assert!(
            page(
                ExternalReplyHandling::Automatic,
                Some(ExternalReplyHandling::ManualHandoff)
            )
            .contains("Effective policy: Manual handoff (channel override)")
        );
        assert!(
            page(ExternalReplyHandling::Automatic, None)
                .contains("Effective policy: Automatic (inherited from company)")
        );
        // An override equal to the company default is still an override, and says so.
        assert!(
            page(
                ExternalReplyHandling::Automatic,
                Some(ExternalReplyHandling::Automatic)
            )
            .contains("Effective policy: Automatic (channel override)")
        );
    }

    #[test]
    fn each_option_carries_the_consequence_of_choosing_it() {
        let html = page(ExternalReplyHandling::Automatic, None);
        assert!(html.contains("Outside replies run the channel&#39;s agent immediately."));
        assert!(
            html.contains("Outside replies to existing threads are filed and wait for the team.")
        );
    }
}
