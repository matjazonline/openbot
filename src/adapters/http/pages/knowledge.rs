use super::*;
use crate::{
    application::knowledge::KnowledgeLibraryView,
    entities::{
        department::DepartmentId, knowledge::KnowledgeDocumentLayer,
        value_objects::KnowledgeDocumentKey,
    },
};
use std::collections::BTreeMap;

pub struct KnowledgeSearchView {
    pub question: String,
    pub result: Option<serde_json::Value>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct UnitKey {
    non_root: bool,
    name: String,
    id: DepartmentId,
}

pub fn knowledge_library(
    company: Uuid,
    view: &KnowledgeLibraryView,
    error: Option<&str>,
    selected: Option<DepartmentId>,
    search: Option<&KnowledgeSearchView>,
) -> String {
    let feedback = error
        .map(|error| {
            format!(
                r#"<div class="alert alert-error" role="alert">{}</div>"#,
                escape_html_text(error)
            )
        })
        .unwrap_or_default();
    let entries = entries(company, view, selected);
    let upload = upload_form(company, view, selected);
    let search = search_form(company, selected, search);
    let filter = if selected.is_some() {
        format!(
            r#"<p>Showing one department. <a class="link" href="/ui/knowledge?company_id={company}">Show all readable departments</a></p>"#
        )
    } else {
        String::new()
    };
    format!(
        r#"<main class="min-w-0 flex-1 overflow-y-auto p-6"><div class="mx-auto max-w-4xl space-y-5">
<h1 class="text-2xl font-bold">Knowledge</h1><p>Department reference documents. Your access level is {level}.</p>{feedback}{filter}
<a class="link" href="/ui/companies?company_id={company}&amp;tab=departments">Departments</a>{search}{upload}<div class="space-y-4">{entries}</div></div></main>"#,
        level = view.actor.access_level.as_i16()
    )
}

fn entries(company: Uuid, view: &KnowledgeLibraryView, selected: Option<DepartmentId>) -> String {
    let mut units: BTreeMap<UnitKey, BTreeMap<KnowledgeDocumentKey, Vec<&KnowledgeDocumentLayer>>> =
        BTreeMap::new();
    for layer in &view.layers {
        units
            .entry(UnitKey {
                non_root: !layer.unit_is_root,
                name: layer.unit_name.to_lowercase(),
                id: layer.unit_id,
            })
            .or_default()
            .entry(layer.name.key())
            .or_default()
            .push(layer);
    }
    if units.is_empty() {
        return "<p class=\"text-base-content/70\">No files at your access level in these departments.</p>".into();
    }
    units.values_mut().map(|groups| {
        let unit = groups.values().next().unwrap()[0];
        let title = escape_html_text(&unit.unit_name);
        let documents = groups.values_mut().map(|layers| {
            layers.sort_by_key(|layer| layer.access_level);
            let anchor = layers[0];
            let name = escape_html_text(anchor.name.as_str());
            let rows = layers.iter().map(|layer| layer_row(company, layer, selected, view)).collect::<String>();
            let moving = if writable(view, anchor.unit_id) {
                format!(r#"<a class="link mt-3" href="/ui/knowledge/{id}/move?company_id={company}{filter}">Move document</a>"#, id=anchor.id, filter=filter_suffix(selected))
            } else { String::new() };
            format!(r#"<section class="card bg-base-200 p-4"><h3 class="font-semibold break-words">{name}</h3><ul class="mt-2 space-y-2">{rows}</ul>{moving}</section>"#)
        }).collect::<String>();
        format!(r#"<section class="space-y-3" data-knowledge-unit="{unit_id}"><h2 class="text-lg font-semibold"><a class="link" href="/ui/knowledge?company_id={company}&amp;unit_id={unit_id}">{title}</a></h2>{documents}</section>"#, unit_id=unit.unit_id)
    }).collect()
}

fn filter_suffix(selected: Option<DepartmentId>) -> String {
    selected
        .map(|unit| format!("&amp;unit_id={unit}"))
        .unwrap_or_default()
}

fn options(
    view: &KnowledgeLibraryView,
    selected: Option<DepartmentId>,
    excluded: Option<DepartmentId>,
) -> String {
    view.upload_units
        .iter()
        .filter(|unit| Some(unit.id) != excluded)
        .map(|unit| {
            let selected = if Some(unit.id) == selected {
                " selected"
            } else {
                ""
            };
            format!(
                r#"<option value="{id}"{selected}>{name}</option>"#,
                id = unit.id,
                name = escape_html_text(&unit.name)
            )
        })
        .collect()
}

fn upload_form(
    company: Uuid,
    view: &KnowledgeLibraryView,
    selected: Option<DepartmentId>,
) -> String {
    if view.upload_units.is_empty() {
        return "<p>To upload, join a department or ask a department lead for help.</p>".into();
    }
    let options = options(view, view.default_unit, None);
    let notice = if view.units_truncated {
        "<p>Showing the first 256 departments and the selected department. Open another department from Departments to upload there.</p>"
    } else {
        ""
    };
    format!(
        r#"<form method="post" enctype="multipart/form-data" action="/ui/knowledge/upload?company_id={company}{filter}" class="card bg-base-200 p-4 space-y-3" data-submit="busy-once" data-pending-label="Indexing…">
<label for="knowledge-unit" class="font-medium">Upload department</label><select id="knowledge-unit" name="unit_id" class="select w-full" required><option value="">Choose a department</option>{options}</select>{notice}
<label for="knowledge-files" class="font-medium">Files</label><input id="knowledge-files" class="file-input w-full" type="file" name="files" multiple accept=".txt,.md,text/plain,text/markdown" required aria-describedby="knowledge-help">
<p id="knowledge-help" class="text-sm text-base-content/70">Use UTF-8 .txt or .md files with a level prefix, such as 0_sales.md or 5-sales.md. Upload up to your own level. Up to 10 files, 1 MiB each; 10 MiB, 4096 file layers and 8192 indexed chunks per company. A matching department, name and level replaces that layer. Very long headings or more than 4096 chunks per file (8192 per upload) are refused.</p>{button}</form>"#,
        filter = filter_suffix(selected),
        button = super::onboarding::busy_submit_button("Upload and index")
    )
}

fn move_form(
    company: Uuid,
    anchor: &KnowledgeDocumentLayer,
    view: &KnowledgeLibraryView,
    state: &KnowledgeMoveState<'_>,
) -> String {
    if !writable(view, anchor.unit_id) {
        return String::new();
    }
    let options = options(view, state.target, Some(anchor.unit_id));
    if options.is_empty() {
        return "<p>No other writable departments are available.</p>".into();
    }
    format!(
        r#"<form method="post" action="/ui/knowledge/{id}/move?company_id={company}{filter}" class="mt-3 space-y-2" data-submit="busy-once" data-pending-label="Moving…"><input type="hidden" name="expected_revision" value="{revision}"><label for="knowledge-move-{id}">Move all levels to</label><select id="knowledge-move-{id}" name="target_unit_id" class="select" required><option value="">Choose a department</option>{options}</select>{button}</form>"#,
        id = anchor.id,
        revision = anchor.revision,
        filter = filter_suffix(state.return_unit),
        button = super::onboarding::busy_submit_button("Move document")
    )
}

#[derive(Default)]
pub struct KnowledgeMoveState<'a> {
    pub return_unit: Option<DepartmentId>,
    pub target: Option<DepartmentId>,
    pub next: Option<DepartmentId>,
    pub error: Option<&'a str>,
}

/// One destination picker on an action page keeps the maximum library from multiplying it
/// by every document group.
pub fn knowledge_move(
    company: Uuid,
    anchor: &KnowledgeDocumentLayer,
    view: &KnowledgeLibraryView,
    state: KnowledgeMoveState<'_>,
) -> String {
    let feedback = state
        .error
        .map(|error| {
            format!(
                r#"<div class="alert alert-error" role="alert">{}</div>"#,
                escape_html_text(error)
            )
        })
        .unwrap_or_default();
    let notice = if view.units_truncated {
        format!(
            r#"<p>More writable departments are available. <a class="link" data-knowledge-destinations href="{}">More destination departments</a></p>"#,
            destinations_url(company, anchor, state.return_unit, state.next)
        )
    } else {
        String::new()
    };
    format!(
        r#"<main class="min-w-0 flex-1 overflow-y-auto p-6"><div class="mx-auto max-w-4xl space-y-5"><h1 class="text-2xl font-bold">Move {name}</h1><p>Every level of this document moves together.</p>{feedback}{notice}{form}<a class="link" href="/ui/knowledge?company_id={company}{filter}">Back to library</a></div></main>"#,
        name = escape_html_text(anchor.name.as_str()),
        form = move_form(company, anchor, view, &state),
        filter = filter_suffix(state.return_unit)
    )
}

fn destinations_url(
    company: Uuid,
    anchor: &KnowledgeDocumentLayer,
    return_unit: Option<DepartmentId>,
    after: Option<DepartmentId>,
) -> String {
    let after = after
        .map(|unit| format!("&amp;after={unit}"))
        .unwrap_or_default();
    format!(
        "/ui/knowledge/{}/move/destinations?company_id={company}{}{after}",
        anchor.id,
        filter_suffix(return_unit)
    )
}

pub fn knowledge_move_destinations(
    company: Uuid,
    anchor: &KnowledgeDocumentLayer,
    view: &KnowledgeLibraryView,
    next: Option<DepartmentId>,
    return_unit: Option<DepartmentId>,
) -> String {
    let mut units = String::new();
    for unit in view
        .upload_units
        .iter()
        .filter(|unit| unit.id != anchor.unit_id)
    {
        units.push_str(&format!(r#"<li><a class="link" data-knowledge-destination="{unit}" href="/ui/knowledge/{id}/move?company_id={company}{filter}&amp;target_unit_id={unit}">Choose {name}</a></li>"#,
            unit=unit.id,id=anchor.id,filter=filter_suffix(return_unit),name=escape_html_text(&unit.name)));
    }
    if units.is_empty() {
        units.push_str("<li>No other destinations on this page.</li>");
    }
    let more = next
        .map(|after| {
            format!(
                r#"<a class="link" data-knowledge-next href="{}">More departments</a>"#,
                destinations_url(company, anchor, return_unit, Some(after))
            )
        })
        .unwrap_or_default();
    format!(
        r#"<main class="min-w-0 flex-1 overflow-y-auto p-6"><div class="mx-auto max-w-4xl space-y-5"><h1 class="text-2xl font-bold">Choose a destination for {name}</h1><ul class="space-y-2">{units}</ul>{more}<a class="link" href="/ui/knowledge/{id}/move?company_id={company}{filter}">Back to move</a></div></main>"#,
        name = escape_html_text(anchor.name.as_str()),
        id = anchor.id,
        filter = filter_suffix(return_unit)
    )
}

fn writable(view: &KnowledgeLibraryView, unit: DepartmentId) -> bool {
    view.writable_units.contains(&unit)
}

fn layer_row(
    company: Uuid,
    layer: &KnowledgeDocumentLayer,
    selected: Option<DepartmentId>,
    view: &KnowledgeLibraryView,
) -> String {
    let name = escape_html_text(&layer.file_name());
    let delete = if writable(view, layer.unit_id) {
        format!(
            r#"<form method="post" action="/ui/knowledge/{id}/delete?company_id={company}{filter}" data-submit="busy-once" data-pending-label="Deleting…"><input type="hidden" name="expected_revision" value="{revision}"><button type="submit" class="btn btn-ghost btn-sm"><span data-progress class="hidden" aria-hidden="true">{spinner}</span><span data-label>Delete</span><span class="sr-only"> {name}</span></button></form>"#,
            id = layer.id,
            revision = layer.revision,
            filter = filter_suffix(selected),
            spinner = icon(Icon::Sync, "size-4 animate-spin")
        )
    } else {
        String::new()
    };
    format!(
        r#"<li class="flex flex-wrap items-center justify-between gap-3"><span class="min-w-0 break-words">{name} <span class="text-sm text-base-content/60">revision {revision}</span></span>{delete}</li>"#,
        revision = layer.revision
    )
}

// HTML counts UTF-16 code units; this coarse cap admits 1000 four-byte characters.
// The server still rejects more than 1000 Unicode characters and enforces the token budget.
fn search_form(
    company: Uuid,
    selected: Option<DepartmentId>,
    search: Option<&KnowledgeSearchView>,
) -> String {
    let query = super::layout::escape_html_attr(
        search
            .map(|search| search.question.as_str())
            .unwrap_or_default(),
    );
    let results = search
        .and_then(|search| search.result.as_ref())
        .map(search_results)
        .unwrap_or_default();
    format!(
        r#"<form method="post" action="/ui/knowledge/search?company_id={company}{filter}" class="space-y-2" data-submit="busy-once" data-pending-label="Searching…"><label for="knowledge-question">Search readable knowledge</label><input id="knowledge-question" name="query" value="{query}" class="input w-full" required maxlength="2000" aria-describedby="knowledge-query-help"><p id="knowledge-query-help" class="text-sm text-base-content/70">Up to 1000 characters; the local model may require a shorter question.</p>{button}</form>{results}"#,
        filter = filter_suffix(selected),
        button = super::onboarding::busy_submit_button("Search knowledge")
    )
}

fn search_results(result: &serde_json::Value) -> String {
    let passages = result.get("passages").and_then(serde_json::Value::as_array);
    let Some(passages) = passages.filter(|passages| !passages.is_empty()) else {
        return "<p role=\"status\">No passages found.</p>".into();
    };
    let entries = passages.iter().map(|passage| format!(r#"<article class="card bg-base-200 p-4"><h3>{}</h3><p>{}</p><p class="whitespace-pre-wrap">{}</p></article>"#,
        escape_html_text(passage["file"].as_str().unwrap_or_default()),
        escape_html_text(passage["heading"].as_str().unwrap_or_default()),
        escape_html_text(passage["text"].as_str().unwrap_or_default()))).collect::<String>();
    format!(
        r#"<section aria-label="Knowledge search results" class="space-y-3">{entries}</section>"#
    )
}

#[cfg(test)]
#[path = "knowledge_tests.rs"]
mod tests;
