#[test]
fn rendered_markup_carries_no_inline_javascript() {
    fn collect(dir: &std::path::Path, into: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("readable source directory") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                collect(&path, into);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                into.push(path);
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapters/http");
    let mut sources = Vec::new();
    collect(&root, &mut sources);
    assert!(
        sources.len() > 20,
        "expected to walk the HTTP adapter, found {} files",
        sources.len()
    );

    let mut offenders = Vec::new();
    for path in sources {
        // This file deliberately holds hostile markup as test input.
        if path.ends_with("tests.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("readable source file");
        for (index, line) in source.lines().enumerate() {
            // Prose about these constructs is not an emission of them.
            if line.trim_start().starts_with("//") {
                continue;
            }
            let mut found = Vec::new();
            if inline_event_attribute(line) {
                found.push("inline event attribute");
            }
            if line.contains("hx-on") {
                found.push("hx-on expression");
            }
            // `<script src=...>` is fine; a `<script>` that carries a body is not. A `<script>`
            // that closes a Rust string literal right after the tag is hostile test input.
            if line.contains("<script>") && !line.contains("<script>\"") {
                found.push("inline <script> block");
            }
            for what in found {
                offenders.push(format!("{}:{}: {what}", path.display(), index + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "inline JavaScript is blocked by the page CSP and will never run:\n{}",
        offenders.join("\n")
    );
}

fn inline_event_attribute(line: &str) -> bool {
    line.match_indices(" on").any(|(at, _)| {
        let rest = &line[at + 3..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_lowercase())
            .collect();
        !name.is_empty() && rest[name.len()..].starts_with("=\"")
    })
}
