//! Bounded ZIP decoding; never extract a caller-supplied path to this process's filesystem.
use std::io::{Cursor, Read, Write};

use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        skill::Skill,
        skill_bundle::{
            MAX_SKILL_ASSET_BYTES, MAX_SKILL_FILES, SkillFile, SkillFilePath,
            import_skill_markdown, native_skill_markdown, validate_skill_files,
        },
    },
    use_cases::skill::SkillWrite,
};

pub(super) const MAX_ARCHIVE_BYTES: usize = 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = MAX_SKILL_FILES * 2 + 2;
const MAX_ENTRY_BYTES: usize = crate::entities::skill::MAX_SKILL_INSTRUCTIONS_BYTES + 4096;

fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}

struct Entry {
    path: String,
    bytes: Vec<u8>,
    executable: bool,
}

struct ArchiveEntries {
    files: Vec<Entry>,
    directories: Vec<String>,
    entrypoint: String,
}

fn entrypoint(archive: &ZipArchive<Cursor<&[u8]>>) -> AppResult<String> {
    let at_root = archive.file_names().any(|name| name == "SKILL.md");
    let mut candidates = archive.file_names().filter(|name| {
        if at_root {
            *name == "SKILL.md"
        } else {
            name.split('/').count() == 2 && name.ends_with("/SKILL.md")
        }
    });
    let name = candidates.next().ok_or_else(|| {
        invalid("ZIP needs exactly one root SKILL.md, optionally inside one skill directory.")
    })?;
    if candidates.next().is_some() {
        return Err(invalid("ZIP has more than one skill entrypoint."));
    }
    Ok(name.to_owned())
}

fn bounded_directory(bytes: &[u8]) -> AppResult<usize> {
    if bytes.len() < 22 {
        return Err(invalid("Invalid skill ZIP footer."));
    }
    let start = bytes.len().saturating_sub(65_557);
    let position = (start..=bytes.len() - 22)
        .rev()
        .find(|offset| {
            bytes[*offset..*offset + 4] == *b"PK\x05\x06"
                && *offset
                    + 22
                    + usize::from(u16::from_le_bytes([
                        bytes[*offset + 20],
                        bytes[*offset + 21],
                    ]))
                    == bytes.len()
        })
        .ok_or_else(|| invalid("Invalid skill ZIP footer."))?;
    let short =
        |offset| u16::from_le_bytes([bytes[position + offset], bytes[position + offset + 1]]);
    let long = |offset| {
        u32::from_le_bytes(
            bytes[position + offset..position + offset + 4]
                .try_into()
                .expect("footer length checked"),
        )
    };
    let count = short(10);
    if short(4) != 0
        || short(6) != 0
        || short(8) != count
        || usize::from(count) > MAX_ARCHIVE_ENTRIES
        || long(12) == u32::MAX
        || long(16) == u32::MAX
        || u64::from(long(12)) + u64::from(long(16)) != position as u64
        || (position >= 20 && &bytes[position - 20..position - 16] == b"PK\x06\x07")
    {
        return Err(invalid(
            "Skill ZIP needs one disk, at most 130 entries and no ZIP64.",
        ));
    }
    bounded_names(bytes, long(16) as usize, position, count)?;
    Ok(usize::from(count))
}

fn bounded_names(bytes: &[u8], mut position: usize, end: usize, count: u16) -> AppResult<()> {
    let mut names = std::collections::BTreeSet::new();
    for _ in 0..count {
        if position + 46 > end || &bytes[position..position + 4] != b"PK\x01\x02" {
            return Err(invalid("Invalid skill ZIP directory."));
        }
        let short = |offset| {
            usize::from(u16::from_le_bytes([
                bytes[position + offset],
                bytes[position + offset + 1],
            ]))
        };
        let name_end = position + 46 + short(28);
        let next = name_end + short(30) + short(32);
        if next > end {
            return Err(invalid("Invalid skill ZIP directory lengths."));
        }
        let name = std::str::from_utf8(&bytes[position + 46..name_end])
            .map_err(|_| invalid("Skill ZIP paths need portable ASCII names."))?;
        let name = name.trim_end_matches('/');
        validate_archive_path(name)?;
        // ZipArchive indexes by filename and would otherwise silently discard duplicates.
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(invalid("Duplicate or case-colliding skill ZIP paths."));
        }
        position = next;
    }
    if position != end {
        return Err(invalid(
            "Skill ZIP directory count does not match its entries.",
        ));
    }
    Ok(())
}

fn validate_archive_path(path: &str) -> AppResult<()> {
    if path == "SKILL.md" || SkillFilePath::parse(path.into()).is_ok() {
        return Ok(());
    }
    // One directory wrapper has its own bounded name; validate it before stripping it.
    let (wrapper, relative) = path
        .split_once('/')
        .ok_or_else(|| invalid("Invalid ZIP path."))?;
    SkillFilePath::parse(wrapper.into()).map_err(AppError::BadRequest)?;
    if relative != "SKILL.md" {
        SkillFilePath::parse(relative.into()).map_err(AppError::BadRequest)?;
    }
    Ok(())
}

fn entries(bytes: &[u8]) -> AppResult<ArchiveEntries> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(invalid("Skill ZIP exceeds 1 MiB."));
    }
    // Cap untrusted directory metadata before ZipArchive can allocate from its entry count.
    let declared_count = bounded_directory(bytes)?;
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid("Invalid skill ZIP."))?;
    if archive.len() != declared_count {
        return Err(invalid(
            "Skill ZIP metadata aliases more than one entry to the same path.",
        ));
    }
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(invalid("Too many ZIP entries."));
    }
    let entrypoint = entrypoint(&archive)?;
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut total = 0usize;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|_| invalid("Cannot read skill ZIP entry."))?;
        let path = file.name().trim_end_matches('/').to_owned();
        validate_archive_path(&path)?;
        if file.encrypted()
            || file.is_symlink()
            || !matches!(
                file.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
        {
            return Err(invalid(
                "Skill ZIP must contain ordinary unencrypted files.",
            ));
        }
        let mode = file.unix_mode().unwrap_or(0);
        if mode & 0o7000 != 0 || !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000) {
            return Err(invalid(
                "Skill ZIP contains a special file or privileged mode.",
            ));
        }
        if mode & 0o170000 == 0o040000 && !file.is_dir() {
            return Err(invalid("Skill ZIP has inconsistent directory metadata."));
        }
        if file.is_dir() {
            if file.size() != 0 {
                return Err(invalid("Skill ZIP directory has file data."));
            }
            directories.push(path);
            continue;
        }
        let max = if path == entrypoint {
            MAX_ENTRY_BYTES
        } else {
            crate::entities::skill_bundle::MAX_SKILL_FILE_BYTES
        };
        if file.size() > max as u64 {
            return Err(invalid("Skill ZIP entry exceeds its byte limit."));
        }
        let mut data = Vec::new();
        file.by_ref()
            .take((max + 1) as u64)
            .read_to_end(&mut data)
            .map_err(|_| invalid("Invalid skill ZIP file data."))?;
        total += data.len();
        if data.len() > max || total > MAX_SKILL_ASSET_BYTES + MAX_ENTRY_BYTES {
            return Err(invalid("Skill ZIP exceeds decoded byte limits."));
        }
        files.push(Entry {
            path,
            bytes: data,
            executable: mode & 0o111 != 0,
        });
    }
    Ok(ArchiveEntries {
        files,
        directories,
        entrypoint,
    })
}

pub(super) fn decode(bytes: &[u8]) -> AppResult<SkillWrite> {
    let ArchiveEntries {
        files: entries,
        directories,
        entrypoint,
    } = entries(bytes)?;
    let root = entries
        .iter()
        .find(|entry| entry.path == entrypoint)
        .ok_or_else(|| invalid("SKILL.md must be a regular file."))?;
    let prefix = root
        .path
        .strip_suffix("SKILL.md")
        .ok_or_else(|| invalid("Invalid skill root."))?;
    if directories.iter().any(|path| {
        !prefix.is_empty() && path != prefix.trim_end_matches('/') && !path.starts_with(prefix)
    }) {
        return Err(invalid("ZIP contains directories outside the skill root."));
    }
    for directory in &directories {
        let directory = directory.to_ascii_lowercase();
        if entries.iter().any(|file| {
            let file = file.path.to_ascii_lowercase();
            directory == file || directory.starts_with(&format!("{file}/"))
        }) {
            return Err(invalid(
                "A skill ZIP directory collides with a regular file.",
            ));
        }
    }
    let text =
        std::str::from_utf8(&root.bytes).map_err(|_| invalid("SKILL.md needs UTF-8 text."))?;
    let (metadata, instructions) = import_skill_markdown(text).map_err(AppError::BadRequest)?;
    let mut files = Vec::new();
    for entry in entries.iter().filter(|entry| entry.path != root.path) {
        let path = entry
            .path
            .strip_prefix(prefix)
            .ok_or_else(|| invalid("ZIP contains files outside the skill root."))?;
        files.push(
            SkillFile::from_bytes(path.into(), &entry.bytes, entry.executable)
                .map_err(AppError::BadRequest)?,
        );
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    validate_skill_files(&files).map_err(AppError::BadRequest)?;
    Ok(SkillWrite {
        slug: metadata.name.clone(),
        name: metadata.name,
        description: metadata.description,
        instructions,
        files: Some(files),
        required_tools: None,
        created_by: None,
    })
}

pub(super) fn encode(skill: &Skill) -> AppResult<Vec<u8>> {
    skill.validate().map_err(AppError::BadRequest)?;
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .last_modified_time(zip::DateTime::default())
        .compression_method(CompressionMethod::Deflated);
    let markdown =
        native_skill_markdown(skill.slug.as_str(), &skill.description, &skill.instructions)
            .map_err(AppError::BadRequest)?;
    archive
        .start_file("SKILL.md", options.unix_permissions(0o644))
        .map_err(|_| invalid("Cannot write skill ZIP."))?;
    archive
        .write_all(markdown.as_bytes())
        .map_err(|_| invalid("Cannot write skill ZIP."))?;
    for file in &skill.files {
        archive
            .start_file(
                file.path.as_str(),
                options.unix_permissions(if file.executable { 0o755 } else { 0o644 }),
            )
            .map_err(|_| invalid("Cannot write skill ZIP."))?;
        archive
            .write_all(&file.bytes().map_err(AppError::BadRequest)?)
            .map_err(|_| invalid("Cannot write skill ZIP."))?;
    }
    let bytes = archive
        .finish()
        .map_err(|_| invalid("Cannot finish skill ZIP."))?
        .into_inner();
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(invalid("Skill ZIP exceeds its output byte limit."));
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod tests;
