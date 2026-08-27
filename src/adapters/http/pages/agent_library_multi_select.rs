//! Reusable multi-select cards for provisioning one channel per library agent.

use super::*;

/// A self-contained library picker. Checkboxes are presentation controls; the hidden input is the
/// stable, comma-separated value submitted by ordinary URL-encoded forms.
pub fn agent_library_multi_select(agents: &[Agent], selected: &[Uuid], input_name: &str) -> String {
    let library = agents
        .iter()
        .filter(|agent| agent.is_library())
        .collect::<Vec<_>>();
    if library.is_empty() {
        return String::new();
    }

    let cards = library
        .iter()
        .map(|agent| {
            let description = agent.description.as_deref().unwrap_or("Ready-to-use agent");
            format!(
                r##"<label class="flex cursor-pointer items-start gap-3 rounded-box border border-base-300 bg-base-200/40 p-4 hover:border-primary">
                    <input type="checkbox" value="{id}" class="checkbox checkbox-primary mt-1"{checked}
                        data-action="library-multi-select">
                    <span class="min-w-0"><span class="block font-semibold">{name}</span><span class="block font-mono text-xs opacity-60">{slug}</span><span class="mt-1 block text-sm opacity-70">{description}</span></span>
                </label>"##,
                id = agent.id,
                checked = if selected.contains(&agent.id) { " checked" } else { "" },
                name = escape_html_text(&agent.name),
                slug = escape_html_text(&agent.slug),
                description = escape_html_text(description),
            )
        })
        .collect::<String>();
    let value = selected
        .iter()
        .map(Uuid::to_string)
        .collect::<Vec<_>>()
        .join(",");

    format!(
        r##"<section class="space-y-3" data-library-multi-select>
            <input type="hidden" name="{input_name}" value="{value}">
            <div><h2 class="font-semibold">Start from the agent library</h2><p class="text-sm opacity-60">Select any number. Each agent gets a channel with the same name and email slug.</p></div>
            <div class="grid grid-cols-1 gap-3 md:grid-cols-2">{cards}</div>
        </section>"##,
        input_name = escape_html_text(input_name),
    )
}
