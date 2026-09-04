//! The dependency-direction gate.
//!
//! `src/AGENTS.md`: "The domain remains independent of application and adapters. The application
//! layer describes the ports it needs; adapters implement them." That rule is invisible to the
//! compiler -- everything is one crate -- so it is asserted here instead, over the source itself.
//!
//! Step 11 closed the temporary exception list. Every upward production import now fails.

use std::{fs, path::Path};

/// Import fragments the application layer may never contain.
///
/// `sqlx`, `axum` and `lettre` are the frameworks `src/AGENTS.md` names; `adapters::` is the
/// direction rule itself; the Slack client is named ahead of its arrival so phase B cannot reach
/// inward the way the email adapter did.
const FORBIDDEN: &[&str] = &[
    "adapters::",
    "sqlx::",
    "axum::",
    "lettre::",
    "slack_morphism",
    "adapters::protocols::slack",
];

/// Files compiled only for tests. A test may reach for a Postgres pool or a fake adapter; that is
/// what a test double is, and it says nothing about the shipped dependency direction.
fn is_test_only(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    name == "tests.rs" || name == "test_support.rs" || name.ends_with("_tests.rs")
}

/// Everything from the file's inline `#[cfg(test)] mod` onwards.
///
/// `src/AGENTS.md` puts the inline test module last in the file, so truncating there is exact
/// rather than approximate -- and a file that ever stopped following that convention would show up
/// as a violation to look at rather than as a silent gap.
fn production_source(source: &str) -> String {
    let mut lines = source.lines().peekable();
    let mut kept = Vec::new();
    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("#[cfg(test)]")
            && lines
                .peek()
                .is_some_and(|next| next.trim_start().starts_with("mod "))
        {
            break;
        }
        kept.push(line);
    }
    kept.join("\n")
}

fn application_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/application");
    let mut sources = Vec::new();
    let mut pending = vec![root.clone()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("the application layer is readable") {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") && !is_test_only(&path) {
                let relative = path
                    .strip_prefix(&root)
                    .expect("every source is under the application root")
                    .to_string_lossy()
                    .replace('\\', "/");
                let source = fs::read_to_string(&path).expect("a readable source file");
                sources.push((relative, production_source(&source)));
            }
        }
    }
    sources
}

#[test]
fn the_application_layer_imports_no_adapter_framework_or_provider_type() {
    let sources = application_sources();
    assert!(
        sources.len() > 20,
        "the scan found only {} files; it is not looking where it thinks it is",
        sources.len()
    );

    let mut unexpected = Vec::new();
    for (file, source) in &sources {
        for (number, line) in source.lines().enumerate() {
            let Some(forbidden) = FORBIDDEN.iter().find(|forbidden| line.contains(*forbidden))
            else {
                continue;
            };
            unexpected.push(format!(
                "{file}:{}: '{forbidden}' in `{}`",
                number + 1,
                line.trim()
            ));
        }
    }

    assert!(
        unexpected.is_empty(),
        "the application layer must not depend on adapters, SQLx, Axum, Lettre or a provider \
         client. Move the port into `src/application` and let the adapter implement it:\n  {}",
        unexpected.join("\n  ")
    );
}

#[cfg(test)]
mod scanner_tests {
    use super::production_source;

    #[test]
    fn the_inline_test_module_is_not_scanned() {
        let source = "use crate::entities::message::Message;\n\
                      fn work() {}\n\
                      #[cfg(test)]\n\
                      mod tests {\n    use sqlx::query;\n}\n";
        let production = production_source(source);
        assert!(production.contains("use crate::entities::message::Message;"));
        assert!(!production.contains("sqlx"));
    }

    /// A `#[cfg(test)]` on anything but a module must not blind the scanner to the rest of the
    /// file -- several application files carry one on a single field or method.
    #[test]
    fn a_cfg_test_attribute_on_an_item_does_not_truncate_the_file() {
        let source = "#[cfg(test)]\n\
                      pub fn helper() {}\n\
                      use sqlx::query;\n";
        assert!(production_source(source).contains("sqlx"));
    }
}
