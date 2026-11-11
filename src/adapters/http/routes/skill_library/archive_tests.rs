use super::*;
use crate::entities::{creation::CreationProvenance, value_objects::SkillSlug};

const MARKDOWN: &str = "---\nname: research\ndescription: Research facts\nallowed-tools: [Read, Bash]\n---\n\nRun scripts/check.py with its neighboring data.\n\n";

#[test]
fn skill_archive_caps_footer_counts_before_constructing_the_zip_reader() {
    let mut bytes = zip(&[("SKILL.md", MARKDOWN.as_bytes(), 0o644)]);
    assert!(bounded_directory(&bytes).is_ok());
    let footer = bytes.len() - 22;
    bytes[footer + 8..footer + 12].fill(0xff);
    assert!(bounded_directory(&bytes).is_err());
    assert!(decode(&bytes).is_err());
    let mut bytes = zip(&[("SKILL.md", MARKDOWN.as_bytes(), 0o644)]);
    bytes.push(0);
    assert!(bounded_directory(&bytes).is_err());
}

#[test]
fn skill_archive_rejects_unicode_metadata_that_aliases_distinct_directory_names() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("SKILL.md", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(MARKDOWN.as_bytes()).unwrap();
    // Info-ZIP Unicode Path: version, CRC32 of the original "other.md", replacement name.
    let mut field = vec![1];
    field.extend(0xa8d25f1au32.to_le_bytes());
    field.extend(b"SKILL.md");
    let marker = [b"\x00\xa0\x0d\x00".as_slice(), field.as_slice()].concat();
    let mut options = zip::write::FullFileOptions::default();
    // The writer validates Unicode fields before a filename exists. Add opaque metadata, then
    // change its ID to the real Unicode Path ID in the completed untrusted input fixture.
    options
        .add_extra_data(0xa000, field.into_boxed_slice(), false)
        .unwrap();
    writer.start_file("other.md", options).unwrap();
    writer.write_all(MARKDOWN.as_bytes()).unwrap();
    let mut bytes = writer.finish().unwrap().into_inner();
    for offset in 0..=bytes.len() - marker.len() {
        if bytes[offset..offset + marker.len()] == marker {
            bytes[offset..offset + 2].copy_from_slice(&0x7075u16.to_le_bytes());
        }
    }
    assert_eq!(bounded_directory(&bytes).unwrap(), 2);
    assert_eq!(ZipArchive::new(Cursor::new(&bytes)).unwrap().len(), 1);
    assert!(decode(&bytes).is_err());
}

#[test]
fn skill_archive_rejects_duplicates_before_the_reader_can_hide_them() {
    let mut bytes = zip(&[
        ("SKILL.md", MARKDOWN.as_bytes(), 0o644),
        ("other.md", MARKDOWN.as_bytes(), 0o644),
    ]);
    for offset in 0..=bytes.len() - 8 {
        if &bytes[offset..offset + 8] == b"other.md" {
            bytes[offset..offset + 8].copy_from_slice(b"SKILL.md");
        }
    }
    assert_eq!(ZipArchive::new(Cursor::new(&bytes)).unwrap().len(), 1);
    assert!(bounded_directory(&bytes).is_err());
    assert!(decode(&bytes).is_err());
    let mut bytes = zip(&[("SKILL.md", MARKDOWN.as_bytes(), 0o644)]);
    let footer = bytes.len() - 22;
    bytes[footer + 8..footer + 12].fill(0);
    assert!(bounded_directory(&bytes).is_err());
}

fn zip(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, mode) in files {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default()
                    .unix_permissions(*mode)
                    .compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn skill(write: SkillWrite) -> Skill {
    let now = chrono::Utc::now();
    Skill {
        id: uuid::Uuid::new_v4(),
        company_id: Some(uuid::Uuid::new_v4()),
        slug: SkillSlug::parse(&write.slug).unwrap(),
        name: write.name,
        description: write.description,
        instructions: write.instructions,
        files: write.files.unwrap(),
        created_by: CreationProvenance::system(),
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn skill_archive_round_trip_preserves_source_binary_files_nested_scripts_and_modes() {
    let script = b"#!/usr/bin/env python3\nfrom pathlib import Path\nprint(Path(__file__).with_name('data.bin').read_bytes())\n";
    let input = zip(&[
        ("SKILL.md", MARKDOWN.as_bytes(), 0o644),
        ("scripts/check.py", script, 0o755),
        ("scripts/data.bin", b"\0\xff\xfe\r\n", 0o644),
    ]);
    let imported = decode(&input).unwrap();
    assert_eq!(imported.instructions, MARKDOWN);
    let stored = skill(imported);
    let output = encode(&stored).unwrap();
    let mut opened = ZipArchive::new(Cursor::new(&output)).unwrap();
    let mut body = String::new();
    opened
        .by_name("SKILL.md")
        .unwrap()
        .read_to_string(&mut body)
        .unwrap();
    assert_eq!(body, MARKDOWN);
    assert_ne!(
        opened
            .by_name("scripts/check.py")
            .unwrap()
            .unix_mode()
            .unwrap()
            & 0o111,
        0
    );
    assert_eq!(
        opened
            .by_name("scripts/data.bin")
            .unwrap()
            .unix_mode()
            .unwrap()
            & 0o111,
        0
    );
    let again = decode(&output).unwrap();
    assert_eq!(again.files.unwrap(), stored.files);
    assert_eq!(again.instructions, stored.instructions);
}

#[test]
fn skill_archive_one_directory_wrapper_is_supported_without_escaping_its_root() {
    let input = zip(&[
        ("research/SKILL.md", MARKDOWN.as_bytes(), 0o644),
        ("research/scripts/check.py", b"print('ok')", 0o755),
    ]);
    let imported = decode(&input).unwrap();
    assert_eq!(imported.files.unwrap()[0].path.as_str(), "scripts/check.py");
    assert!(
        decode(&zip(&[
            ("research/SKILL.md", MARKDOWN.as_bytes(), 0o644),
            ("outside.txt", b"x", 0o644)
        ]))
        .is_err()
    );
    assert!(
        decode(&zip(&[
            ("a/SKILL.md", MARKDOWN.as_bytes(), 0o644),
            ("b/SKILL.md", MARKDOWN.as_bytes(), 0o644)
        ]))
        .is_err()
    );
}

#[test]
fn skill_archive_rejects_unsafe_paths_case_collisions_links_and_bombs() {
    for name in [
        "../escape",
        "/absolute",
        "a/../escape",
        "C:/escape",
        "a\\escape",
        "a//b",
    ] {
        assert!(
            decode(&zip(&[
                ("SKILL.md", MARKDOWN.as_bytes(), 0o644),
                (name, b"x", 0o644)
            ]))
            .is_err(),
            "{name}"
        );
    }
    assert!(
        decode(&zip(&[
            ("SKILL.md", MARKDOWN.as_bytes(), 0o644),
            ("scripts/a", b"x", 0o644),
            ("SCRIPTS/A", b"y", 0o644)
        ]))
        .is_err()
    );
    let mut links = ZipWriter::new(Cursor::new(Vec::new()));
    links
        .start_file("SKILL.md", SimpleFileOptions::default())
        .unwrap();
    links.write_all(MARKDOWN.as_bytes()).unwrap();
    links
        .add_symlink(
            "scripts/escape",
            "../../outside",
            SimpleFileOptions::default(),
        )
        .unwrap();
    assert!(decode(&links.finish().unwrap().into_inner()).is_err());
    let huge = vec![0; crate::entities::skill_bundle::MAX_SKILL_FILE_BYTES + 1];
    let bomb = zip(&[
        ("SKILL.md", MARKDOWN.as_bytes(), 0o644),
        ("scripts/bomb", &huge, 0o644),
    ]);
    assert!(bomb.len() < 4096);
    assert!(decode(&bomb).is_err());
    assert!(decode(&vec![0; MAX_ARCHIVE_BYTES + 1]).is_err());
}

#[test]
fn skill_archive_maximum_files_and_bytes_are_accepted_then_one_more_is_refused() {
    let mut source = vec![Entry {
        path: "SKILL.md".into(),
        bytes: MARKDOWN.as_bytes().to_vec(),
        executable: false,
    }];
    for n in 0..MAX_SKILL_FILES {
        source.push(Entry {
            path: format!("assets/f{n:03}"),
            bytes: vec![b'x'; MAX_SKILL_ASSET_BYTES / MAX_SKILL_FILES],
            executable: n % 2 == 0,
        });
    }
    let encode_entries = |source: &[Entry]| {
        zip(&source
            .iter()
            .map(|e| {
                (
                    e.path.as_str(),
                    e.bytes.as_slice(),
                    if e.executable { 0o755 } else { 0o644 },
                )
            })
            .collect::<Vec<_>>())
    };
    assert_eq!(
        decode(&encode_entries(&source))
            .unwrap()
            .files
            .unwrap()
            .len(),
        MAX_SKILL_FILES
    );
    source.push(Entry {
        path: "extra".into(),
        bytes: vec![0],
        executable: false,
    });
    assert!(decode(&encode_entries(&source)).is_err());
}

#[test]
fn skill_archive_large_skill_suffix_assets_round_trip_at_root_and_inside_a_wrapper() {
    let bytes = vec![0xff; 32 * 1024];
    for prefix in ["", "research/"] {
        let root = format!("{prefix}SKILL.md");
        let suffix = format!("{prefix}reference-SKILL.md");
        let nested = format!("{prefix}references/SKILL.md");
        let imported = decode(&zip(&[
            (&root, MARKDOWN.as_bytes(), 0o644),
            (&suffix, &bytes, 0o644),
            (&nested, &bytes, 0o755),
        ]))
        .unwrap();
        let stored = skill(imported);
        let reimported = decode(&encode(&stored).unwrap()).unwrap();
        assert_eq!(reimported.files.as_ref().unwrap(), &stored.files);
        assert!(
            stored
                .files
                .iter()
                .all(|file| file.bytes().unwrap() == bytes)
        );
        assert!(
            stored
                .files
                .iter()
                .any(|file| file.path.as_str() == "references/SKILL.md" && file.executable)
        );
    }
}

#[test]
fn skill_archive_maximum_text_only_skill_has_a_byte_exact_native_round_trip() {
    let body = "x".repeat(crate::entities::skill::MAX_SKILL_INSTRUCTIONS_BYTES);
    let mut write = decode(&zip(&[("SKILL.md", MARKDOWN.as_bytes(), 0o644)])).unwrap();
    write.instructions = body.clone();
    let original = skill(write);
    let archive = encode(&original).unwrap();
    let imported = decode(&archive).unwrap();
    assert_eq!(imported.instructions, body);
    assert_eq!(encode(&skill(imported)).unwrap(), archive);
}
