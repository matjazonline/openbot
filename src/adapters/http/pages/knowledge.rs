use super::*;
use crate::{
    application::knowledge::KnowledgeLibraryView,
    entities::{knowledge::KnowledgeDocumentLayer, value_objects::KnowledgeDocumentKey},
};
use std::collections::BTreeMap;
pub fn knowledge_library(
    company: Uuid,
    view: &KnowledgeLibraryView,
    error: Option<&str>,
) -> String {
    let mut groups: BTreeMap<KnowledgeDocumentKey, Vec<&KnowledgeDocumentLayer>> = BTreeMap::new();
    for layer in &view.layers {
        groups.entry(layer.name.key()).or_default().push(layer);
    }
    let entries=groups.values_mut().map(|layers| {
        layers.sort_by_key(|layer|layer.access_level);
        let title=escape_html_text(layers[0].name.as_str());
        let rows=layers.iter().map(|layer|layer_row(company,layer)).collect::<String>();
        format!(r#"<section class="card bg-base-200 p-4"><h2 class="font-semibold break-words">{title}</h2><ul class="mt-2 space-y-2">{rows}</ul></section>"#)
    }).collect::<String>();
    let entries = if entries.is_empty() {
        "<p class=\"text-base-content/70\">No files at your access level yet.</p>".into()
    } else {
        entries
    };
    let feedback = error
        .map(|error| {
            format!(
                r#"<div class="alert alert-error" role="alert">{}</div>"#,
                escape_html_text(error)
            )
        })
        .unwrap_or_default();
    format!(
        r#"<main class="min-w-0 flex-1 overflow-y-auto p-6"><div class="mx-auto max-w-4xl space-y-5">
<h1 class="text-2xl font-bold">Knowledge</h1><p>Upload reference documents for your team. Your access level is {level}.</p>{feedback}
<form method="post" enctype="multipart/form-data" action="/ui/knowledge/upload?company_id={company}" class="card bg-base-200 p-4 space-y-3" data-submit="busy-once" data-pending-label="Indexing…">
<label for="knowledge-files" class="font-medium">Files</label><input id="knowledge-files" class="file-input w-full" type="file" name="files" multiple accept=".txt,.md,text/plain,text/markdown" required aria-describedby="knowledge-help">
<p id="knowledge-help" class="text-sm text-base-content/70">Use UTF-8 .txt or .md files with a level prefix, such as 0_sales.md or 5-sales.md. Upload up to your own level. Up to 10 files, 1 MiB each; 10 MiB and 4096 file layers per company. A matching name and level replaces that layer. Very long headings or more than 4096 chunks per file (8192 per upload) are refused.</p>
{upload_button}</form><div class="space-y-4">{entries}</div></div></main>"#,
        upload_button = super::onboarding::busy_submit_button("Upload and index"),
        level = view.actor.access_level.as_i16()
    )
}
fn layer_row(company: Uuid, layer: &KnowledgeDocumentLayer) -> String {
    format!(
        r#"<li class="flex flex-wrap items-center justify-between gap-3"><span class="min-w-0 break-words">{name} <span class="text-sm text-base-content/60">revision {revision}</span></span><form method="post" action="/ui/knowledge/{id}/delete?company_id={company}" data-submit="busy-once" data-pending-label="Deleting…"><input type="hidden" name="expected_revision" value="{revision}"><button type="submit" class="btn btn-ghost btn-sm"><span data-progress class="hidden" aria-hidden="true">{spinner}</span><span data-label>Delete</span><span class="sr-only"> {name}</span></button></form></li>"#,
        spinner = icon(Icon::Sync, "size-4 animate-spin"),
        name = escape_html_text(&layer.file_name()),
        revision = layer.revision,
        id = layer.id
    )
}

#[cfg(test)]
#[path = "knowledge_tests.rs"]
mod tests;
