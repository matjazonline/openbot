//! The dependency-direction gate.
//!
//! `src/AGENTS.md`: "The domain remains independent of application and adapters. The application
//! layer describes the ports it needs; adapters implement them." That rule is invisible to the
//! compiler -- everything is one crate -- so it is asserted here instead, over the source itself.
//!
//! Step 11 closed the temporary exception list. Every upward production import now fails.
//!
//! The agent runtime joined the list when the harness adapter landed. It is checked separately
//! from [`FORBIDDEN`] because prose may name it -- saying which types a module exists to keep out
//! is the documentation -- so only code counts.

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
    "infra::",
    "reqwest::",
    "tokio::net::",
    "adapters::protocols::slack",
];

/// Files compiled only for tests. A test may reach for a Postgres pool or a fake adapter; that is
/// what a test double is, and it says nothing about the shipped dependency direction.
fn is_test_only(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    path.components()
        .any(|part| matches!(part.as_os_str().to_str(), Some("tests" | "test_support")))
        || name == "tests.rs"
        || name == "test_support.rs"
        || name.ends_with("_tests.rs")
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

/// The agent runtime belongs in the rig adapter, and nothing above the adapter may name it.
///
/// A single `use rig::...` in the application layer would make a port an alias for one runtime's
/// types and nothing would fail.
#[test]
fn the_application_layer_names_no_agent_runtime() {
    let sources = application_sources();
    let mut offenders = Vec::new();

    for (file, source) in &sources {
        for (number, line) in source.lines().enumerate() {
            let code = line.trim();
            // Prose may name the runtime; a comment compiles to nothing.
            if code.starts_with("//") || !names_a_runtime_crate(code) {
                continue;
            }
            offenders.push(format!("{file}:{}: {code}", number + 1));
        }
    }

    assert!(
        offenders.is_empty(),
        "the application layer must not name the agent runtime. Translate in \
         `src/adapters/harness/rig/` instead:\n  {}",
        offenders.join("\n  ")
    );
}

/// External agent/embedding runtime paths, not internal modules merely ending in `rig`.
fn names_a_runtime_crate(code: &str) -> bool {
    const CRATES: &[&str] = &["rig", "rig_core", "rig_agent", "fastembed", "ort"];
    code.split(|character: char| {
        !character.is_alphanumeric() && character != '_' && character != ':'
    })
    .any(|token| {
        let token = token.strip_prefix("::").unwrap_or(token);
        CRATES.iter().any(|name| {
            token
                .strip_prefix(name)
                .is_some_and(|suffix| suffix.starts_with("::") || suffix.is_empty())
        })
    })
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

#[test]
fn workflow_and_transport_depend_on_shared_ports_without_reverse_usecase_imports() {
    let mut checked = 0;
    let mut unexpected = Vec::new();
    for (file, source) in application_sources() {
        if !(file.starts_with("workflow/") || file.starts_with("transport/")) {
            continue;
        }
        checked += 1;
        for (number, line) in source.lines().enumerate() {
            if line.contains("use_cases::") {
                unexpected.push(format!("{file}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(
        checked > 30,
        "reverse-import check did not scan the intended subsystems"
    );
    assert!(
        unexpected.is_empty(),
        "workflow/transport must consume shared ports, not use-case modules: {}",
        unexpected.join("\n")
    );
}

#[test]
fn runtime_boundary_scanner_covers_direct_sdk_paths_without_internal_false_positives() {
    for code in [
        "use rig::completion::Message;",
        "rig_core::completion::Message",
        "use rig_agent::agent::Prompt;",
        "fastembed::TextEmbedding",
        "ort::init_from(path)",
        "use ::rig_core::completion::Message;",
        "use rig_agent as agent_runtime;",
        "extern crate fastembed;",
    ] {
        assert!(names_a_runtime_crate(code), "{code}");
    }
    for code in [
        "crate::adapters::harness::rig::Harness",
        "support::helper()",
        "some_rig_core::Fake",
        "transport::value",
    ] {
        assert!(!names_a_runtime_crate(code), "{code}");
    }
}
