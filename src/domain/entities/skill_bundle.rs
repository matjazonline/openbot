//! Portable skill assets. No paths are opened and no uploaded code runs here.
use std::{collections::BTreeSet, fmt};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

use super::value_objects::SkillSlug;

pub const MAX_SKILL_FILES: usize = 64;
pub const MAX_SKILL_FILE_BYTES: usize = 256 * 1024;
pub const MAX_SKILL_ASSET_BYTES: usize = 512 * 1024;
pub const MAX_SKILL_PATH_BYTES: usize = 240;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SkillFilePath(String);

impl SkillFilePath {
    pub fn parse(value: String) -> Result<Self, String> {
        if value.is_empty()
            || value.len() > MAX_SKILL_PATH_BYTES
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-/ ".contains(&b))
            || value.split('/').any(|part| {
                part.is_empty()
                    || matches!(part, "." | "..")
                    || part.ends_with(['.', ' '])
                    || part.starts_with(' ')
            })
            || value
                .split('/')
                .next()
                .is_some_and(|part| part.eq_ignore_ascii_case("SKILL.md"))
        {
            return Err(
                "Skill assets need portable relative paths; SKILL.md is the instruction text."
                    .into(),
            );
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SkillFilePath {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(decoder)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillFile {
    pub path: SkillFilePath,
    pub content_base64: String,
    pub executable: bool,
}

impl fmt::Debug for SkillFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SkillFile")
            .field("path", &self.path)
            .field("executable", &self.executable)
            .finish_non_exhaustive()
    }
}

impl SkillFile {
    pub fn from_bytes(path: String, bytes: &[u8], executable: bool) -> Result<Self, String> {
        if bytes.len() > MAX_SKILL_FILE_BYTES {
            return Err("A skill asset exceeds 256 KiB.".into());
        }
        Ok(Self {
            path: SkillFilePath::parse(path)?,
            content_base64: STANDARD.encode(bytes),
            executable,
        })
    }
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        if self.content_base64.len() > MAX_SKILL_FILE_BYTES.div_ceil(3) * 4 {
            return Err("A skill asset exceeds 256 KiB.".into());
        }
        let bytes = STANDARD
            .decode(&self.content_base64)
            .map_err(|_| "Invalid skill asset base64.")?;
        if bytes.len() > MAX_SKILL_FILE_BYTES || STANDARD.encode(&bytes) != self.content_base64 {
            return Err("Skill assets need bounded canonical base64.".into());
        }
        Ok(bytes)
    }
}

pub fn validate_skill_files(files: &[SkillFile]) -> Result<(), String> {
    if files.len() > MAX_SKILL_FILES {
        return Err("A skill may have at most 64 supporting files.".into());
    }
    let mut paths = BTreeSet::<String>::new();
    let mut total = 0usize;
    for file in files {
        let path = file.path.as_str().to_ascii_lowercase();
        if paths.iter().any(|other| {
            other == &path
                || path.starts_with(&format!("{other}/"))
                || other.starts_with(&format!("{path}/"))
        }) {
            return Err("Skill asset paths collide or replace a parent directory.".into());
        }
        paths.insert(path);
        total += file.bytes()?.len();
        if total > MAX_SKILL_ASSET_BYTES {
            return Err("Skill assets exceed 512 KiB in total.".into());
        }
    }
    Ok(())
}

pub struct SkillMarkdownMetadata {
    pub name: String,
    pub description: String,
}

fn markdown_parts(text: &str) -> Result<Option<(&str, &str)>, String> {
    let Some(first) = text.lines().next() else {
        return Ok(None);
    };
    if first != "---" {
        return Ok(None);
    }
    let mut offset = text.find('\n').ok_or("Incomplete skill frontmatter.")? + 1;
    let start = offset;
    for line in text[start..].split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Ok(Some((&text[start..offset], &text[offset + line.len()..])));
        }
        offset += line.len();
    }
    Err("Skill frontmatter needs a closing --- line.".into())
}

fn metadata(header: &str) -> Result<(serde_yaml::Mapping, SkillMarkdownMetadata), String> {
    let mut budget = serde_saphyr::Budget::default();
    budget.max_events = 1024;
    budget.max_nodes = 256;
    budget.max_depth = 16;
    budget.max_total_scalar_bytes = 16384;
    budget.max_documents = 1;
    budget.max_aliases = 0;
    budget.max_anchors = 0;
    budget.max_merge_keys = 0;
    budget.max_recorded_anchor_events = 0;
    budget.max_recorded_anchor_bytes = 0;
    let mut options = serde_saphyr::Options::default();
    options.budget = Some(budget);
    options.merge_keys = serde_saphyr::options::MergeKeyPolicy::Error;
    options.strict_booleans = true;
    options.reject_unsupported_tags = true;
    options.with_snippet = false;
    let value: serde_yaml::Mapping = serde_saphyr::from_str_with_options(header, options)
        .map_err(|_| "Invalid or oversized skill frontmatter.")?;
    let name = value
        .get("name")
        .and_then(serde_yaml::Value::as_str)
        .ok_or("Skill frontmatter needs a name.")?;
    SkillSlug::parse(name).map_err(|_| "Invalid skill frontmatter name.")?;
    let description = value
        .get("description")
        .and_then(serde_yaml::Value::as_str)
        .ok_or("Skill frontmatter needs a description.")?;
    if description.trim().is_empty()
        || description.chars().count() > super::skill::MAX_SKILL_DESCRIPTION_CHARS
    {
        return Err("Invalid skill frontmatter description.".into());
    }
    let result = SkillMarkdownMetadata {
        name: name.to_owned(),
        description: description.to_owned(),
    };
    Ok((value, result))
}

pub fn skill_markdown_metadata(text: &str) -> Result<Option<SkillMarkdownMetadata>, String> {
    if text.len() > super::skill::MAX_SKILL_INSTRUCTIONS_BYTES {
        return Err("SKILL.md exceeds 16 KiB.".into());
    }
    let Some((header, body)) = markdown_parts(text)? else {
        return Ok(None);
    };
    if body.trim().is_empty() {
        return Err("SKILL.md needs an instruction body.".into());
    }
    Ok(Some(metadata(header)?.1))
}

pub fn native_skill_markdown(
    slug: &str,
    description: &str,
    instructions: &str,
) -> Result<String, String> {
    if description.trim().is_empty()
        || description.chars().count() > super::skill::MAX_SKILL_DESCRIPTION_CHARS
        || instructions.trim().is_empty()
    {
        return Err("A native skill needs bounded description and instruction text.".into());
    }
    if let Some(meta) = skill_markdown_metadata(instructions)? {
        if meta.name != slug {
            return Err("SKILL.md frontmatter name must match the skill slug.".into());
        }
        return Ok(instructions.to_owned());
    }
    let description =
        serde_json::to_string(description).map_err(|_| "Invalid skill description.")?;
    Ok(format!(
        "---\nname: {slug}\ndescription: {description}\n---\n\n{instructions}"
    ))
}

pub fn rename_skill_markdown(instructions: &str, slug: &str) -> Result<String, String> {
    let Some((header, body)) = markdown_parts(instructions)? else {
        return Ok(instructions.to_owned());
    };
    let (mut values, meta) = metadata(header)?;
    if meta.name == slug {
        return Ok(instructions.to_owned());
    }
    values.insert("name".into(), slug.into());
    let header = serde_yaml::to_string(&values).map_err(|_| "Invalid copied skill metadata.")?;
    Ok(format!("---\n{header}---\n{body}"))
}

/// Inverse of native materialization. The generated header does not consume the existing body
/// budget; an opaque, user-authored header remains part of the exact stored instruction source.
pub fn import_skill_markdown(text: &str) -> Result<(SkillMarkdownMetadata, String), String> {
    if text.len() > super::skill::MAX_SKILL_INSTRUCTIONS_BYTES + 4096 {
        return Err("SKILL.md exceeds its bounded body and metadata capacity.".into());
    }
    let (header, body) = markdown_parts(text)?
        .ok_or("An imported SKILL.md needs YAML name and description frontmatter.")?;
    let (_, meta) = metadata(header)?;
    if body.trim().is_empty() {
        return Err("SKILL.md needs an instruction body.".into());
    }
    // Native materialization inserts one separator newline before the original body.
    // Strip only that exact generated separator; opaque source retains every byte.
    if let Some(original) = body.strip_prefix('\n')
        && native_skill_markdown(&meta.name, &meta.description, original)
            .is_ok_and(|generated| generated == text)
    {
        return Ok((meta, original.to_owned()));
    }
    if text.len() > super::skill::MAX_SKILL_INSTRUCTIONS_BYTES {
        return Err("Skill source exceeds 16 KiB.".into());
    }
    Ok((meta, text.to_owned()))
}

#[cfg(test)]
#[path = "skill_bundle_tests.rs"]
mod tests;
