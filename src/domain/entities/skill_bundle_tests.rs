use super::*;

#[test]
fn skill_bundle_paths_reject_escape_aliases_and_entrypoint_replacement() {
    for path in [
        "",
        "/tmp/a",
        "../a",
        "a/../b",
        "a//b",
        "./x",
        "C:/x",
        "a\\b",
        "a\0b",
        "a\nb",
        "SKILL.md",
        "skill.MD/x",
        "a./x",
        "a /x",
        " x",
        "é.py",
    ] {
        assert!(SkillFilePath::parse(path.into()).is_err(), "{path:?}");
    }
    for path in [
        "scripts/run.py",
        "references/data.bin",
        "assets/chart image.png",
        ".metadata/info.json",
    ] {
        assert!(SkillFilePath::parse(path.into()).is_ok());
    }
}

#[test]
fn skill_bundle_round_trips_binary_content_and_executable_intent() {
    let file = SkillFile::from_bytes("scripts/run.py".into(), b"\0\xff\xfe\r\n", true).unwrap();
    let encoded = serde_json::to_vec(&file).unwrap();
    let decoded: SkillFile = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, file);
    assert_eq!(decoded.bytes().unwrap(), b"\0\xff\xfe\r\n");
    assert!(decoded.executable);
    assert!(!format!("{file:?}").contains(&file.content_base64));
}

#[test]
fn skill_bundle_file_count_byte_budgets_and_collisions_fail_early() {
    let max = vec![0xff; MAX_SKILL_FILE_BYTES];
    let a = SkillFile::from_bytes("scripts/run.py".into(), &max, true).unwrap();
    let b = SkillFile::from_bytes("assets/data".into(), &max, false).unwrap();
    assert!(validate_skill_files(&[a.clone(), b.clone()]).is_ok());
    let extra = SkillFile::from_bytes("one".into(), &[0], false).unwrap();
    assert!(validate_skill_files(&[a.clone(), b, extra]).is_err());
    assert!(SkillFile::from_bytes("x".into(), &vec![0; MAX_SKILL_FILE_BYTES + 1], false).is_err());
    let collision = SkillFile::from_bytes("SCRIPTS/RUN.py".into(), &[], false).unwrap();
    assert!(validate_skill_files(&[a, collision]).is_err());
    let prefix =
        ["scripts", "scripts/a"].map(|p| SkillFile::from_bytes(p.into(), &[], false).unwrap());
    assert!(validate_skill_files(&prefix).is_err());
    let mut files: Vec<_> = (0..MAX_SKILL_FILES)
        .map(|n| SkillFile::from_bytes(format!("f{n}"), &[], false).unwrap())
        .collect();
    assert!(validate_skill_files(&files).is_ok());
    files.push(SkillFile::from_bytes("overflow".into(), &[], false).unwrap());
    assert!(validate_skill_files(&files).is_err());
}

#[test]
fn skill_bundle_invalid_or_noncanonical_base64_is_refused() {
    for invalid in ["not base64!", "YQ", "YR==", "YQ==\n"] {
        let file = SkillFile {
            path: SkillFilePath::parse("x".into()).unwrap(),
            content_base64: invalid.into(),
            executable: false,
        };
        assert!(validate_skill_files(&[file]).is_err());
    }
}

#[test]
fn skill_bundle_markdown_preserves_frontmatter_body_and_copy_identity() {
    let text = "---\nname: research\ndescription: Research facts\nallowed-tools: [Bash, Read]\n---\n\nRun scripts/check.py.\n\n";
    assert_eq!(
        native_skill_markdown("research", "ignored", text).unwrap(),
        text
    );
    let renamed = rename_skill_markdown(text, "research-2").unwrap();
    assert_eq!(
        skill_markdown_metadata(&renamed).unwrap().unwrap().name,
        "research-2"
    );
    let (header, body) = markdown_parts(&renamed).unwrap().unwrap();
    assert!(header.contains("allowed-tools:"));
    assert_eq!(body, "\nRun scripts/check.py.\n\n");
    assert!(native_skill_markdown("research-2", "ignored", text).is_err());
    let generated =
        native_skill_markdown("research", "A quoted \"description\"", "Read data.\n").unwrap();
    assert_eq!(
        skill_markdown_metadata(&generated)
            .unwrap()
            .unwrap()
            .description,
        "A quoted \"description\""
    );
    assert!(generated.ends_with("Read data.\n"));
}

#[test]
fn skill_bundle_generated_frontmatter_import_preserves_the_exact_original_body() {
    for body in ["Read data.\n", "\nRead data.\n\n"] {
        let native = native_skill_markdown("research", "Research facts", body).unwrap();
        let (meta, restored) = import_skill_markdown(&native).unwrap();
        assert_eq!(restored, body);
        assert_eq!(
            native_skill_markdown(&meta.name, &meta.description, &restored).unwrap(),
            native
        );
    }
    let opaque =
        "---\nname: research\ndescription: Research facts\ncustom: yes\n---\n\nRead data.\n";
    assert_eq!(import_skill_markdown(opaque).unwrap().1, opaque);
}

#[test]
fn skill_bundle_malformed_or_empty_frontmatter_is_refused() {
    for text in [
        "---",
        "---\nname: x\n",
        "---\nname: []\ndescription: a\n---\ntext",
        "---\nname: x\n---\ntext",
        "---\nname: x\ndescription: a\n---\n ",
    ] {
        assert!(skill_markdown_metadata(text).is_err(), "{text:?}");
    }
}

#[test]
fn skill_bundle_parent_case_spellings_are_portable_and_identical_parents_repeat() {
    for paths in [
        ["Scripts/one.txt", "scripts/two.txt"],
        ["a/Files/one", "a/files/two"],
    ] {
        let files = paths.map(|path| SkillFile::from_bytes(path.into(), b"x", false).unwrap());
        assert!(validate_skill_files(&files).is_err());
    }
    let files = [
        "scripts/one.txt",
        "scripts/two.txt",
        "scripts/deep/three.txt",
    ]
    .map(|path| SkillFile::from_bytes(path.into(), b"x", false).unwrap());
    assert!(validate_skill_files(&files).is_ok());
}
