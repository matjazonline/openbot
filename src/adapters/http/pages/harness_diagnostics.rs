use crate::services::harness::runs::RunDiagnostics;

pub(super) fn render(diagnostics: Option<&RunDiagnostics>) -> String {
    let Some(value) = diagnostics else {
        return String::new();
    };
    // Serialize only the allowlisted projection, never a checkpoint or task payload.
    let json = serde_json::to_string_pretty(value).expect("fixed diagnostic fields serialize");
    format!(
        r#"<details class="rounded-box border border-base-300 bg-base-200"><summary class="cursor-pointer px-4 py-2 text-xs font-semibold uppercase opacity-70">Harness continuation</summary><pre class="overflow-x-auto px-4 py-3 text-xs">{}</pre></details>"#,
        super::escape_html_text(&json)
    )
}
