//! Shared level controls use the same pure policy as both team writers.
use crate::entities::{
    access_level::{AccessLevel, LevelActor, LevelTarget, may_assign_level},
    company::CompanyAccess,
};

pub const LEVEL_HELP: &str =
    "Lowering a level takes effect at the next read. Mail already sent cannot be recalled.";

pub fn team_level_actor(access: &CompanyAccess) -> LevelActor {
    LevelActor {
        membership: access.membership,
        access_level: access.access_level,
    }
}
pub fn level_options(actor: LevelActor, target: LevelTarget, selected: AccessLevel) -> String {
    AccessLevel::ALL
        .into_iter()
        .filter(|level| may_assign_level(actor, target, *level).is_ok())
        .map(|level| {
            format!(
                r#"<option value="{}"{}>{}</option>"#,
                level.as_i16(),
                if level == selected { " selected" } else { "" },
                level.label()
            )
        })
        .collect()
}
pub struct LevelControl<'a> {
    pub actor: LevelActor,
    pub target: LevelTarget,
    pub current: AccessLevel,
    pub endpoint: &'a str,
    pub swap_target: &'a str,
}
pub fn level_control(control: LevelControl<'_>) -> String {
    let label = control.current.label();
    let options = level_options(control.actor, control.target, control.current);
    if options.is_empty() {
        let fixed = if control.target == LevelTarget::Owner {
            " (fixed for the owner)"
        } else {
            ""
        };
        return format!(
            "<p class=\"text-sm\">Access level: {label}{fixed}</p><p class=\"text-xs opacity-60\">{LEVEL_HELP}</p>"
        );
    }
    format!(
        r##"<form hx-put="{endpoint}" hx-target="{target}" hx-swap="outerHTML"
        hx-disabled-elt="find button[type='submit']" class="space-y-2 my-4">
        <label class="form-control"><span>Access level</span><select name="access_level" class="select">{options}</select></label>
        <p class="text-xs opacity-60">{LEVEL_HELP}</p>
        <button type="submit" class="btn btn-primary btn-sm"><span class="loading loading-spinner loading-xs hidden [.htmx-request_&]:inline-block"></span>Save level</button>
        </form>"##,
        endpoint = super::escape_html_attr(control.endpoint),
        target = super::escape_html_attr(control.swap_target)
    )
}
