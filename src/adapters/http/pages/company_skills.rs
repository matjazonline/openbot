//! Shared skill-library editor for company owners and platform operators.

use super::*;
use crate::entities::{
    skill::{Skill, SkillInstruction},
    tool_catalogue::CatalogueTool,
};

#[derive(Debug, Clone, PartialEq)]
pub enum DraftInstruction {
    Prompt {
        text: String,
    },
    Tool {
        tool: String,
        args: String,
        output_as: String,
        args_error: Option<String>,
    },
}

impl DraftInstruction {
    pub fn prompt() -> Self {
        Self::Prompt {
            text: String::new(),
        }
    }

    pub fn tool() -> Self {
        Self::Tool {
            tool: CatalogueTool::grantable()
                .next()
                .map(|tool| tool.id.to_string())
                .unwrap_or_default(),
            args: String::new(),
            output_as: String::new(),
            args_error: None,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Prompt { .. } => "prompt",
            Self::Tool { .. } => "tool",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkillDraft {
    pub id: Option<Uuid>,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub trigger: String,
    pub instructions: Vec<DraftInstruction>,
}

impl SkillDraft {
    pub fn blank() -> Self {
        Self {
            instructions: vec![DraftInstruction::prompt()],
            ..Self::default()
        }
    }

    pub fn stored(skill: &Skill) -> Self {
        Self {
            id: Some(skill.id),
            slug: skill.slug.to_string(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            trigger: skill.trigger.clone(),
            instructions: skill
                .instructions
                .iter()
                .map(|instruction| match instruction {
                    SkillInstruction::Prompt { text } => {
                        DraftInstruction::Prompt { text: text.clone() }
                    }
                    SkillInstruction::Tool {
                        tool,
                        args,
                        output_as,
                    } => DraftInstruction::Tool {
                        tool: tool.to_string(),
                        args: args
                            .as_ref()
                            .map(|value| serde_json::to_string_pretty(value).unwrap_or_default())
                            .unwrap_or_default(),
                        output_as: output_as.clone().unwrap_or_default(),
                        args_error: None,
                    },
                })
                .collect(),
        }
    }

    pub fn ends_with_prompt(&self) -> bool {
        matches!(
            self.instructions.last(),
            Some(DraftInstruction::Prompt { .. })
        )
    }
}

/// Everything one skill-library section needs. `company_id = None` is the operator library.
pub struct SkillSection<'a> {
    pub company_id: Option<Uuid>,
    pub skills: &'a [Skill],
    pub next_url: Option<&'a str>,
    pub previous_url: Option<&'a str>,
    pub draft: Option<&'a SkillDraft>,
    pub error: Option<&'a str>,
    pub notice: Option<&'a str>,
    pub editable: bool,
}

pub fn company_skills_section(section: &SkillSection<'_>) -> String {
    if !section.editable {
        return r##"<div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
            <div class="rounded-box border border-base-300 bg-base-200 p-5">
                <h3 class="font-semibold">Company skills</h3>
                <p class="mt-1 text-sm opacity-70">Only the company owner can edit these settings.</p>
            </div>
        </div>"##
            .to_string();
    }

    let draft = section.draft.cloned().unwrap_or_else(SkillDraft::blank);
    format!(
        r##"<div id="skill-library-section" class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
            {error}{notice}
            <div class="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(24rem,1.25fr)]">
                <section class="space-y-3">
                    <div class="flex items-center justify-between gap-3">
                        <div><h3 class="font-semibold">Skills</h3><p class="text-xs opacity-60">Reusable, ordered instructions for agents.</p></div>
                        {browse}
                    </div>
                    {list}
                    {pagination}
                </section>
                <section>{editor}</section>
            </div>
        </div>"##,
        error = form_error_banner(section.error),
        notice = notice_banner(section.notice),
        browse = browse_button(section.company_id),
        list = skill_list(section),
        pagination = pagination(section.previous_url, section.next_url),
        editor = skill_editor_fragment(section.company_id, &draft),
    )
}

fn notice_banner(notice: Option<&str>) -> String {
    notice.map_or_else(String::new, |message| {
        format!(
            r#"<div role="status" class="alert alert-success mb-4 text-sm">{}</div>"#,
            escape_html_text(message)
        )
    })
}

fn browse_button(company_id: Option<Uuid>) -> String {
    company_id.map_or_else(String::new, |company_id| {
        format!(
            r##"<button type="button" class="btn btn-ghost btn-sm"
                hx-get="/ui/companies/{company_id}/skills/library"
                hx-target="#skill-library-browser" hx-swap="innerHTML"
                hx-sync="#skill-library-browser:replace" hx-disabled-elt="this">
                Browse library</button>"##
        )
    })
}

fn skill_list(section: &SkillSection<'_>) -> String {
    if section.skills.is_empty() {
        return r#"<p class="rounded-box border border-dashed border-base-300 p-5 text-sm opacity-60">No skills yet.</p><div id="skill-library-browser"></div>"#.to_string();
    }
    let company_id = section.company_id;
    let rows = section
        .skills
        .iter()
        .map(|skill| {
            let edit_url = skill_url(company_id, Some(skill.id));
            format!(
                r##"<article class="rounded-box border border-base-300 bg-base-200/40 p-4">
                    <div class="flex items-start justify-between gap-3">
                        <div class="min-w-0"><h4 class="truncate font-semibold">{name}</h4>
                            <p class="font-mono text-xs opacity-60">{slug}</p>
                            <p class="mt-1 text-sm opacity-70">{description}</p></div>
                        <button type="button" class="btn btn-ghost btn-xs"
                            hx-get="{edit_url}" hx-target="#skill-library-section" hx-swap="outerHTML"
                            hx-sync="#skill-library-section:replace">Edit</button>
                    </div>
                </article>"##,
                name = escape_html_text(&skill.name),
                slug = escape_html_text(skill.slug.as_str()),
                description = escape_html_text(&skill.description),
            )
        })
        .collect::<String>();
    format!(r#"<div class="space-y-2">{rows}</div><div id="skill-library-browser"></div>"#)
}

fn pagination(previous: Option<&str>, next: Option<&str>) -> String {
    if previous.is_none() && next.is_none() {
        return String::new();
    }
    let link = |label: &str, url: Option<&str>| {
        url.map_or_else(String::new, |url| {
            format!(
                r#"<a class="btn btn-ghost btn-sm" href="{}">{label}</a>"#,
                escape_html_attr(url)
            )
        })
    };
    format!(
        r#"<nav class="mt-3 flex justify-between" aria-label="Skill pages">{}{}</nav>"#,
        link("Previous", previous),
        link("Next", next)
    )
}

fn skill_url(company_id: Option<Uuid>, skill_id: Option<Uuid>) -> String {
    match (company_id, skill_id) {
        (Some(company_id), Some(skill_id)) => {
            format!("/ui/companies/{company_id}/skills/{skill_id}")
        }
        (Some(company_id), None) => format!("/ui/companies/{company_id}/skills"),
        (None, Some(skill_id)) => format!("/ui/skill-library/{skill_id}"),
        (None, None) => "/ui/skill-library".to_string(),
    }
}

fn instruction_url(company_id: Option<Uuid>) -> String {
    company_id.map_or_else(
        || "/ui/skill-library/instruction-row".to_string(),
        |id| format!("/ui/companies/{id}/skills/instruction-row"),
    )
}

pub fn skill_editor_fragment(company_id: Option<Uuid>, draft: &SkillDraft) -> String {
    let save_url = skill_url(company_id, draft.id);
    let method = if draft.id.is_some() { "put" } else { "post" };
    let title = if draft.id.is_some() {
        "Edit skill"
    } else {
        "New skill"
    };
    let invalid_last_step = !draft.ends_with_prompt();
    let skill_id = draft.id.map_or_else(String::new, |id| {
        format!(r#"<input type="hidden" name="skill_id" value="{id}">"#)
    });
    let delete = draft.id.map_or_else(String::new, |skill_id| {
        format!(
            r##"<button type="button" class="btn btn-error btn-outline ml-auto"
                hx-delete="{}" hx-target="#skill-library-section" hx-swap="outerHTML"
                hx-confirm="Delete this skill? Agents using it will continue without it."
                hx-disabled-elt="this">Delete</button>"##,
            skill_url(company_id, Some(skill_id))
        )
    });
    format!(
        r##"<form id="skill-editor" class="rounded-box border border-base-300 bg-base-200 p-4 space-y-4"
            hx-{method}="{save_url}" hx-target="#skill-library-section" hx-swap="outerHTML"
            hx-disabled-elt="find button[type='submit']">
            <h3 class="font-semibold">{title}</h3>
            {skill_id}
            <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
                <label class="form-control"><span class="label text-xs opacity-70">Name</span>
                    <input class="input w-full" name="name" required maxlength="120" value="{name}"></label>
                <label class="form-control"><span class="label text-xs opacity-70">Slug</span>
                    <input class="input w-full font-mono" name="slug" required maxlength="80" value="{slug}"></label>
            </div>
            <label class="form-control"><span class="label text-xs opacity-70">Description</span>
                <textarea class="textarea w-full" name="description" maxlength="500" required>{description}</textarea></label>
            <label class="form-control"><span class="label text-xs opacity-70">Trigger</span>
                <textarea class="textarea w-full" name="trigger" maxlength="500" required>{trigger}</textarea></label>
            {instructions}
            {hint}
            <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                <button type="submit" class="btn btn-primary"{disabled}>
                    <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                    <span class="[.htmx-request_&]:hidden">Save skill</span>
                    <span class="hidden [.htmx-request_&]:inline">Saving...</span></button>
                <button type="button" class="btn btn-ghost"
                    hx-get="{base_url}" hx-target="#skill-library-section" hx-swap="outerHTML"
                    hx-sync="#skill-library-section:replace">Clear</button>{delete}
            </div>
        </form>"##,
        method = method,
        save_url = save_url,
        title = title,
        skill_id = skill_id,
        name = escape_html_attr(&draft.name),
        slug = escape_html_attr(&draft.slug),
        description = escape_html_text(&draft.description),
        trigger = escape_html_text(&draft.trigger),
        instructions = skill_instruction_editor(company_id, draft),
        hint = if invalid_last_step {
            r#"<p class="text-sm text-error">A skill must end with a prompt step.</p>"#
        } else {
            ""
        },
        disabled = if invalid_last_step { " disabled" } else { "" },
        base_url = skill_url(company_id, None),
    )
}

/// The fragment replaced by every add/remove/reorder/kind action.
pub fn skill_instruction_editor(company_id: Option<Uuid>, draft: &SkillDraft) -> String {
    let rows = draft
        .instructions
        .iter()
        .enumerate()
        .map(|(index, instruction)| instruction_row(company_id, index, instruction))
        .collect::<String>();
    let url = instruction_url(company_id);
    let add = instruction_action_button(&url, "add", "Add instruction", "btn btn-ghost btn-sm");
    format!(
        r##"<section id="skill-instruction-editor" class="space-y-3" aria-live="polite">
            <div class="flex items-center justify-between"><div><h4 class="text-sm font-semibold">Instructions</h4>
                <p class="text-xs opacity-60">At most 32. The last instruction must be a prompt.</p></div>{add}</div>
            <input type="hidden" name="instruction_count" value="{count}">
            <div class="space-y-3">{rows}</div>
        </section>"##,
        count = draft.instructions.len(),
    )
}

fn instruction_action_button(url: &str, action: &str, label: &str, class: &str) -> String {
    format!(
        r##"<button type="button" class="{class}" name="action" value="{action}"
            hx-post="{url}" hx-include="#skill-editor" hx-target="#skill-editor"
            hx-swap="outerHTML" hx-sync="#skill-editor:replace" hx-disabled-elt="this">{label}</button>"##,
        action = escape_html_attr(action),
        label = escape_html_text(label),
    )
}

fn instruction_row(
    company_id: Option<Uuid>,
    index: usize,
    instruction: &DraftInstruction,
) -> String {
    let url = instruction_url(company_id);
    let prompt_selected = if instruction.kind() == "prompt" {
        " selected"
    } else {
        ""
    };
    let tool_selected = if instruction.kind() == "tool" {
        " selected"
    } else {
        ""
    };
    let body = match instruction {
        DraftInstruction::Prompt { text } => format!(
            r#"<textarea class="textarea min-h-24 w-full" name="instruction_{index}_text" maxlength="8000" required>{}</textarea>"#,
            escape_html_text(text)
        ),
        DraftInstruction::Tool {
            tool,
            args,
            output_as,
            args_error,
        } => tool_instruction_fields(index, tool, args, output_as, args_error.as_deref()),
    };
    format!(
        r##"<div class="rounded-box border border-base-300 bg-base-100 p-3" data-instruction-row="{index}">
            <div class="mb-2 flex flex-wrap items-center gap-2">
                <select class="select select-sm" name="instruction_{index}_kind"
                    hx-post="{url}" hx-vals='{{"action":"kind:{index}:__selected__"}}'
                    hx-include="#skill-editor" hx-target="#skill-editor" hx-swap="outerHTML"
                    hx-sync="#skill-editor:replace" hx-disabled-elt="this"
                    hx-indicator="#skill-kind-{index}-progress" data-action="skill-kind">
                    <option value="prompt"{prompt_selected}>Prompt</option>
                    <option value="tool"{tool_selected}>Tool</option>
                </select>
                <span id="skill-kind-{index}-progress" class="htmx-indicator loading loading-spinner loading-xs" aria-label="Updating instruction"></span>
                <span class="ml-auto flex gap-1">
                    {up}{down}{remove}
                </span>
            </div>{body}
        </div>"##,
        up = instruction_action_button(&url, &format!("up:{index}"), "↑", "btn btn-ghost btn-xs"),
        down =
            instruction_action_button(&url, &format!("down:{index}"), "↓", "btn btn-ghost btn-xs"),
        remove = instruction_action_button(
            &url,
            &format!("remove:{index}"),
            "×",
            "btn btn-ghost btn-xs"
        ),
    )
}

fn tool_instruction_fields(
    index: usize,
    selected_tool: &str,
    args: &str,
    output_as: &str,
    args_error: Option<&str>,
) -> String {
    let options = CatalogueTool::grantable()
        .map(|tool| {
            format!(
                r#"<option value="{}"{}>{}</option>"#,
                escape_html_attr(tool.id),
                if tool.id == selected_tool {
                    " selected"
                } else {
                    ""
                },
                escape_html_text(tool.label),
            )
        })
        .collect::<String>();
    let error = args_error.map_or_else(String::new, |message| {
        format!(
            r#"<p class="mt-1 text-xs text-error">{}</p>"#,
            escape_html_text(message)
        )
    });
    format!(
        r##"<div class="grid grid-cols-1 gap-3 md:grid-cols-2">
            <label class="form-control"><span class="label text-xs opacity-70">Tool</span>
                <select class="select w-full" name="instruction_{index}_tool">{options}</select></label>
            <label class="form-control"><span class="label text-xs opacity-70">Output as</span>
                <input class="input w-full font-mono" name="instruction_{index}_output_as" maxlength="64" value="{output_as}"></label>
            <label class="form-control md:col-span-2"><span class="label text-xs opacity-70">Arguments (JSON object)</span>
                <textarea class="textarea w-full font-mono text-xs" name="instruction_{index}_args" maxlength="524288" placeholder='{{"format":"iso8601"}}'>{args}</textarea>{error}</label>
        </div>"##,
        output_as = escape_html_attr(output_as),
        args = escape_html_text(args),
    )
}

pub fn library_skill_browser(company_id: Uuid, skills: &[Skill]) -> String {
    if skills.is_empty() {
        return r#"<p class="mt-3 text-sm opacity-60">The global skill library is empty.</p>"#
            .to_string();
    }
    let rows = skills
        .iter()
        .map(|skill| {
            format!(
                r##"<form class="mt-2 flex items-center gap-3 rounded-box border border-base-300 p-3"
                    hx-post="/ui/companies/{company_id}/skills/from-library"
                    hx-target="#skill-library-section" hx-swap="outerHTML" hx-disabled-elt="find button">
                    <input type="hidden" name="skill_id" value="{id}">
                    <span class="min-w-0 flex-1"><strong class="block truncate">{name}</strong>
                        <span class="block text-xs opacity-60">{description}</span></span>
                    <button class="btn btn-primary btn-outline btn-sm">
                        <span class="loading loading-spinner loading-xs hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">Copy</span><span class="hidden [.htmx-request_&]:inline">Copying...</span></button>
                </form>"##,
                id = skill.id,
                name = escape_html_text(&skill.name),
                description = escape_html_text(&skill.description),
            )
        })
        .collect::<String>();
    format!(
        r#"<div class="mt-3"><h4 class="text-sm font-semibold">Global library</h4>{rows}</div>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skill_instruction_with_html_in_its_text_is_escaped() {
        let draft = SkillDraft {
            instructions: vec![DraftInstruction::Prompt {
                text: ["</textarea><scr", "ipt>alert(1)</scr", "ipt>"].concat(),
            }],
            ..SkillDraft::default()
        };
        let html = skill_instruction_editor(Some(Uuid::nil()), &draft);
        assert!(!html.contains(&["<scr", "ipt>"].concat()));
        assert!(!html.contains("</textarea><"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn an_instruction_tool_select_offers_only_grantable_tools() {
        let draft = SkillDraft {
            instructions: vec![DraftInstruction::Tool {
                tool: "datetime".into(),
                args: "{}".into(),
                output_as: String::new(),
                args_error: None,
            }],
            ..SkillDraft::default()
        };
        let html = skill_instruction_editor(None, &draft);
        assert!(html.contains("datetime"));
        assert!(!html.contains("value=\"command\""));
    }
}
