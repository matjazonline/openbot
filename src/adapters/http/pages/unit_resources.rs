//! Unit pickers and move forms for unit-owned skills and connections (Plan07).

use super::*;
use crate::application::ports::{
    administration::ManagedUnit,
    unit_resources::{MovedResource, ResourceUseKind},
};

fn unit_label(unit: &ManagedUnit) -> String {
    if unit.is_root {
        format!("{} (whole company)", unit.name)
    } else {
        unit.name.clone()
    }
}

/// The name of `id` among the person's managed units.
pub fn unit_name(units: &[ManagedUnit], id: Option<Uuid>) -> String {
    units
        .iter()
        .find(|unit| Some(unit.id.as_uuid()) == id)
        .map_or_else(|| "Another unit".to_string(), unit_label)
}

/// A required `unit_id` select over the units the person manages; `selected` is the default.
pub fn unit_picker(units: &[ManagedUnit], selected: Option<Uuid>) -> String {
    if units.is_empty() {
        return String::new();
    }
    let placeholder = if selected.is_none() {
        r#"<option value="" disabled selected>Choose a unit</option>"#
    } else {
        ""
    };
    let options = units
        .iter()
        .map(|unit| {
            format!(
                r#"<option value="{id}"{chosen}>{name}</option>"#,
                id = unit.id.as_uuid(),
                chosen = if Some(unit.id.as_uuid()) == selected {
                    " selected"
                } else {
                    ""
                },
                name = escape_html_text(&unit_label(unit)),
            )
        })
        .collect::<String>();
    format!(
        r#"<label class="form-control"><span class="label text-xs opacity-70">Unit</span>
            <select class="select w-full" name="unit_id" required>{placeholder}{options}</select>
            <span class="label text-xs opacity-60">Usable by this unit and the departments below it.</span></label>"#
    )
}

/// The owning unit and, when the person manages another unit, a form that moves the resource:
/// swapped into `target` with htmx, or posted as a page when there is none.
pub fn unit_move_form(
    action: &str,
    target: Option<&str>,
    units: &[ManagedUnit],
    current: Option<Uuid>,
) -> String {
    let others = units
        .iter()
        .filter(|unit| Some(unit.id.as_uuid()) != current)
        .map(|unit| {
            format!(
                r#"<option value="{}">{}</option>"#,
                unit.id.as_uuid(),
                escape_html_text(&unit_label(unit))
            )
        })
        .collect::<String>();
    let owner = escape_html_text(&unit_name(units, current));
    if others.is_empty() {
        return format!(r#"<p class="text-sm opacity-70">Unit: {owner}</p>"#);
    }
    let submit = match target {
        Some(target) => format!(
            r#"hx-post="{}" hx-target="{}" hx-swap="outerHTML" hx-disabled-elt="find button""#,
            escape_html_attr(action),
            escape_html_attr(target)
        ),
        None => format!(
            r#"method="post" action="{}" data-submit="busy-once""#,
            escape_html_attr(action)
        ),
    };
    format!(
        r##"<form class="flex flex-wrap items-end gap-3 rounded-box border border-base-300 p-3" {submit}>
            <p class="text-sm">Unit: <strong>{owner}</strong></p>
            <label class="form-control"><span class="label text-xs opacity-70">Move to</span>
                <select class="select select-sm" name="unit_id" required>{others}</select></label>
            <button class="btn btn-sm">Move</button>
        </form>"##
    )
}

/// Plain text for the moved notice: what still uses the resource after the move.
pub fn moved_notice(moved: &MovedResource) -> String {
    if moved.from == moved.to {
        return "Already in that unit.".to_string();
    }
    if moved.uses.is_empty() {
        return "Moved.".to_string();
    }
    let names = moved
        .uses
        .iter()
        .map(|used| match used.kind {
            ResourceUseKind::Agent => format!("agent {}", used.name),
            ResourceUseKind::Binding => format!("binding of {}", used.name),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let more = if moved.more_uses > 0 {
        format!(" and {} more", moved.more_uses)
    } else {
        String::new()
    };
    format!(
        "Moved. Still used by {names}{more}; where the new unit is out of their reach, republish \
         them or move their desks."
    )
}
