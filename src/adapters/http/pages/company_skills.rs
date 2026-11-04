//! Shared skill-library editor for company owners and platform operators.

use super::*;
use crate::entities::skill::{MAX_SKILL_INSTRUCTIONS_BYTES, Skill};

#[derive(Debug, Clone, Default)]
pub struct SkillDraft {
    pub id: Option<Uuid>,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub instructions: String,
}

impl SkillDraft {
    pub fn stored(skill: &Skill) -> Self {
        Self {
            id: Some(skill.id),
            slug: skill.slug.to_string(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            instructions: skill.instructions.clone(),
        }
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

    let draft = section.draft.cloned().unwrap_or_default();
    format!(
        r##"<div id="skill-library-section" class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
            {error}{notice}
            <div class="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(24rem,1.25fr)]">
                <section class="space-y-3">
                    <div class="flex items-center justify-between gap-3">
                        <div><h3 class="font-semibold">Skills</h3><p class="text-xs opacity-60">Reusable instructions for agents.</p></div>
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

pub fn skill_editor_fragment(company_id: Option<Uuid>, draft: &SkillDraft) -> String {
    let save_url = skill_url(company_id, draft.id);
    let method = if draft.id.is_some() { "put" } else { "post" };
    let title = if draft.id.is_some() {
        "Edit skill"
    } else {
        "New skill"
    };
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
            <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
                <label class="form-control"><span class="label text-xs opacity-70">Name</span>
                    <input class="input w-full" name="name" required maxlength="120" value="{name}"></label>
                <label class="form-control"><span class="label text-xs opacity-70">Slug</span>
                    <input class="input w-full font-mono" name="slug" required maxlength="80" value="{slug}"></label>
            </div>
            <label class="form-control"><span class="label text-xs opacity-70">Description</span>
                <textarea class="textarea w-full" name="description" maxlength="500" required>{description}</textarea></label>
            <label class="form-control"><span class="label text-xs opacity-70">Instructions</span>
                <textarea class="textarea min-h-48 w-full" name="instructions" maxlength="{max_instructions}" required>{instructions}</textarea>
                <span class="label text-xs opacity-60">Added to the prompt of every agent given this skill. At most {max_instructions} bytes.</span></label>
            <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                <button type="submit" class="btn btn-primary">
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
        name = escape_html_attr(&draft.name),
        slug = escape_html_attr(&draft.slug),
        description = escape_html_text(&draft.description),
        instructions = escape_html_text(&draft.instructions),
        max_instructions = MAX_SKILL_INSTRUCTIONS_BYTES,
        base_url = skill_url(company_id, None),
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
    fn skill_instructions_with_html_in_their_text_are_escaped() {
        let draft = SkillDraft {
            instructions: ["</textarea><scr", "ipt>alert(1)</scr", "ipt>"].concat(),
            ..SkillDraft::default()
        };
        let html = skill_editor_fragment(Some(Uuid::nil()), &draft);
        assert!(!html.contains(&["<scr", "ipt>"].concat()));
        // The whole form has honest `</textarea></label>` closes; only the injected one must not appear.
        assert!(!html.contains(&["</textarea><scr", "ipt>"].concat()));
        assert!(html.contains("&lt;/textarea&gt;&lt;script&gt;"));
    }
}
