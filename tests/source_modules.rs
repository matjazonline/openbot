//! Detect source files disconnected from Cargo module roots, including orphaned tests.
//! This checks syntactic reachability across cfg branches, not execution/behavioral coverage.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::{Expr, ItemMod, Lit, Meta, Token};

type CheckResult<T> = Result<T, String>;

struct Walker {
    visited: BTreeSet<PathBuf>,
    active: BTreeSet<PathBuf>,
}

struct ModuleVisitor<'a> {
    walker: &'a mut Walker,
    default_dir: PathBuf,
    attribute_dir: PathBuf,
    errors: Vec<String>,
}

fn path_attributes(attributes: &[syn::Attribute]) -> CheckResult<Vec<PathBuf>> {
    fn collect(meta: &Meta, paths: &mut Vec<PathBuf>) -> CheckResult<()> {
        if let Meta::NameValue(value) = meta
            && value.path.is_ident("path")
        {
            if let Expr::Lit(literal) = &value.value
                && let Lit::Str(path) = &literal.lit
            {
                paths.push(PathBuf::from(path.value()));
                return Ok(());
            }
            return Err("module #[path] must be a string literal".into());
        }
        if let Meta::List(list) = meta
            && list.path.is_ident("cfg_attr")
        {
            let nested = Punctuated::<Meta, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
                .map_err(|error| format!("unsupported cfg_attr: {error}"))?;
            for attribute in nested.iter().skip(1) {
                collect(attribute, paths)?;
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    for attribute in attributes {
        collect(&attribute.meta, &mut paths)?;
    }
    Ok(paths)
}

impl Walker {
    fn new() -> Self {
        Self {
            visited: BTreeSet::new(),
            active: BTreeSet::new(),
        }
    }

    fn file(&mut self, file: &Path, crate_root: bool) -> CheckResult<()> {
        let file = file
            .canonicalize()
            .map_err(|error| format!("cannot resolve {}: {error}", file.display()))?;
        if !self.active.insert(file.clone()) {
            return Err(format!("recursive module include: {}", file.display()));
        }
        self.visited.insert(file.clone());
        let source = fs::read_to_string(&file)
            .map_err(|error| format!("cannot read {}: {error}", file.display()))?;
        let syntax = syn::parse_file(&source)
            .map_err(|error| format!("cannot parse {}: {error}", file.display()))?;
        let parent = file.parent().ok_or("module has no parent directory")?;
        let default_dir = if crate_root || file.file_name().is_some_and(|name| name == "mod.rs") {
            parent.to_path_buf()
        } else {
            parent.join(file.file_stem().ok_or("module has no file stem")?)
        };
        let mut visitor = ModuleVisitor {
            walker: self,
            default_dir,
            attribute_dir: parent.to_path_buf(),
            errors: Vec::new(),
        };
        visitor.visit_file(&syntax);
        let errors = visitor.errors;
        self.active.remove(&file);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("{}:\n{}", file.display(), errors.join("\n")))
        }
    }
}

impl ModuleVisitor<'_> {
    fn module(&mut self, module: &ItemMod) -> CheckResult<()> {
        let paths = path_attributes(&module.attrs)?;
        if let Some((_, items)) = &module.content {
            let default = self.default_dir.join(module.ident.to_string());
            let mut directories = if paths.is_empty() {
                vec![default.clone()]
            } else {
                paths
                    .iter()
                    .map(|path| self.attribute_dir.join(path))
                    .collect()
            };
            if !paths.is_empty()
                && !module.attrs.iter().any(|attr| attr.path().is_ident("path"))
                && default.is_dir()
            {
                directories.push(default);
            }
            for directory in directories {
                let mut nested = ModuleVisitor {
                    walker: self.walker,
                    default_dir: directory.clone(),
                    attribute_dir: directory,
                    errors: Vec::new(),
                };
                for item in items {
                    nested.visit_item(item);
                }
                self.errors.extend(nested.errors);
            }
        } else {
            let direct = self.default_dir.join(format!("{}.rs", module.ident));
            let nested = self
                .default_dir
                .join(module.ident.to_string())
                .join("mod.rs");
            let mut candidates: Vec<PathBuf> = paths
                .iter()
                .map(|path| self.attribute_dir.join(path))
                .collect();
            // cfg_attr(path=...) may leave the ordinary path active in another cfg.
            let explicit = module.attrs.iter().any(|attr| attr.path().is_ident("path"));
            if !explicit && (candidates.is_empty() || direct.exists() || nested.exists()) {
                match (direct.exists(), nested.exists()) {
                    (true, false) => candidates.push(direct),
                    (false, true) => candidates.push(nested),
                    (true, true) => return Err(format!("ambiguous module {}", module.ident)),
                    (false, false) => return Err(format!("missing module {}", module.ident)),
                }
            }
            for path in candidates {
                self.walker.file(&path, false)?;
            }
        }
        Ok(())
    }
}

impl<'ast> Visit<'ast> for ModuleVisitor<'_> {
    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        if let Err(error) = self.module(module) {
            self.errors.push(error);
        }
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        if invocation.path.is_ident("include") {
            match syn::parse2::<syn::LitStr>(invocation.tokens.clone()) {
                Ok(path) => {
                    // include! expands in its caller's module context.
                    let path = self.attribute_dir.join(path.value());
                    if let Err(error) = self.include(&path) {
                        self.errors.push(error);
                    }
                }
                Err(_) => self.errors.push(
                    "unsupported nonliteral include!: add explicit module-resolution support"
                        .into(),
                ),
            }
        }
    }
}

impl ModuleVisitor<'_> {
    fn include(&mut self, path: &Path) -> CheckResult<()> {
        let path = path
            .canonicalize()
            .map_err(|error| format!("cannot resolve include {}: {error}", path.display()))?;
        if !self.walker.active.insert(path.clone()) {
            return Err(format!("recursive module include: {}", path.display()));
        }
        self.walker.visited.insert(path.clone());
        let source = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let syntax = syn::parse_file(&source)
            .map_err(|error| format!("unsupported include {}: {error}", path.display()))?;
        // Rust resolves modules and nested includes relative to the included file.
        let directory = path.parent().ok_or("include has no parent directory")?;
        let mut nested = ModuleVisitor {
            walker: self.walker,
            default_dir: directory.to_path_buf(),
            attribute_dir: directory.to_path_buf(),
            errors: Vec::new(),
        };
        nested.visit_file(&syntax);
        let errors = nested.errors;
        self.walker.active.remove(&path);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }
}

fn rust_files(directory: &Path, files: &mut BTreeSet<PathBuf>) -> CheckResult<()> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            rust_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.insert(path.canonicalize().map_err(|error| error.to_string())?);
        }
    }
    Ok(())
}

fn check(roots: &[PathBuf], directories: &[PathBuf]) -> CheckResult<()> {
    if roots.is_empty() {
        return Err("no Cargo source roots found".into());
    }
    let mut walker = Walker::new();
    for root in roots {
        walker.file(root, true)?;
    }
    let mut files = BTreeSet::new();
    for directory in directories {
        if directory.is_dir() {
            rust_files(directory, &mut files)?;
        }
    }
    let missing: Vec<_> = files
        .difference(&walker.visited)
        .map(|path| path.display().to_string())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Rust files disconnected from Cargo module roots:\n{}\nCheck declarations and module-producing macros; add resolution support rather than excluding live files.",
            missing.join("\n")
        ))
    }
}

#[test]
fn every_rust_source_is_connected_to_a_cargo_target() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--locked",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
        ])
        .current_dir(root)
        .output()
        .expect("Cargo metadata is available");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["manifest_path"].as_str() == root.join("Cargo.toml").to_str())
        .expect("the current package is in Cargo metadata");
    let roots: Vec<_> = package["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|target| PathBuf::from(target["src_path"].as_str().unwrap()))
        .collect();
    check(
        &roots,
        &[
            root.join("src"),
            root.join("tests"),
            root.join("examples"),
            root.join("benches"),
        ],
    )
    .unwrap_or_else(|error| panic!("{error}"));
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("openbot-source-modules-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn write(&self, name: &str, source: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn check(&self) -> CheckResult<()> {
        check(&[self.0.join("lib.rs")], std::slice::from_ref(&self.0))
    }

    fn assert_compiles(&self) {
        let output = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-type=lib",
                "--emit=metadata",
                "--cap-lints=allow",
            ])
            .arg(self.0.join("lib.rs"))
            .arg("-o")
            .arg(self.0.join("fixture.rmeta"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn removed_module_and_removed_ancestor_leave_detectable_orphans() {
    let fixture = Fixture::new();
    fixture.write("lib.rs", "mod owner;");
    fixture.write(
        "owner.rs",
        "#[cfg(test)] #[path=\"store_tests.rs\"] mod tests;",
    );
    fixture.write("store_tests.rs", "#[test] fn competing_claimants() {}");
    fixture.assert_compiles();
    fixture.check().unwrap();
    fixture.write("owner.rs", "");
    assert!(fixture.check().unwrap_err().contains("store_tests.rs"));
    fixture.write("lib.rs", "");
    assert!(fixture.check().unwrap_err().contains("owner.rs"));
}

#[test]
fn inline_path_and_conditional_modules_are_connected() {
    let fixture = Fixture::new();
    fixture.write("lib.rs", "mod ordinary; mod inline { mod leaf; } #[cfg(unix)] mod unix; #[cfg(windows)] mod windows;");
    fixture.write("ordinary.rs", "#[path=\"alternate.rs\"] mod alternate;");
    fixture.write("alternate.rs", "");
    fixture.write("inline/leaf.rs", "");
    fixture.write("unix.rs", "");
    fixture.write("windows.rs", "");
    fixture.assert_compiles();
    fixture.check().unwrap();
}

#[test]
fn cfg_attr_paths_and_literal_includes_are_connected() {
    let fixture = Fixture::new();
    fixture.write("lib.rs", "#[cfg_attr(unix,path=\"unix.rs\")] #[cfg_attr(windows,path=\"windows.rs\")] mod platform; include!(\"extra.rs\");");
    fixture.write("unix.rs", "");
    fixture.write("windows.rs", "");
    fixture.write("extra.rs", "mod nested;");
    fixture.write("nested.rs", "");
    fixture.assert_compiles();
    fixture.check().unwrap();
}

#[test]
fn included_files_resolve_nested_modules_and_includes_like_rustc() {
    let fixture = Fixture::new();
    fixture.write("lib.rs", "include!(\"parts/extra.rs\");");
    fixture.write(
        "parts/extra.rs",
        "include!(\"next.rs\"); #[path=\"leaf.rs\"] mod leaf;",
    );
    fixture.write("parts/next.rs", "mod sibling;");
    fixture.write("parts/sibling.rs", "");
    fixture.write("parts/leaf.rs", "");
    fixture.assert_compiles();
    fixture.check().unwrap();
}

#[test]
fn conditional_inline_paths_keep_the_ordinary_directory_reachable() {
    let fixture = Fixture::new();
    fixture.write(
        "lib.rs",
        "#[cfg_attr(windows,path=\"windows\")] mod inline {mod leaf;}",
    );
    fixture.write("inline/leaf.rs", "");
    fixture.write("windows/leaf.rs", "");
    fixture.assert_compiles();
    fixture.check().unwrap();
}

#[test]
fn comments_and_strings_do_not_declare_modules() {
    let fixture = Fixture::new();
    fixture.write(
        "lib.rs",
        "// mod orphan;\n const TEXT: &str = r#\"mod orphan;\"#;",
    );
    fixture.write("orphan.rs", "");
    assert!(fixture.check().unwrap_err().contains("orphan.rs"));
}

#[test]
fn unresolved_forms_and_recursive_includes_fail_explicitly() {
    let fixture = Fixture::new();
    fixture.write("lib.rs", "include!(concat!(\"extra\",\".rs\"));");
    assert!(
        fixture
            .check()
            .unwrap_err()
            .contains("unsupported nonliteral include")
    );
    fixture.write("lib.rs", "include!(\"lib.rs\");");
    assert!(
        fixture
            .check()
            .unwrap_err()
            .contains("recursive module include")
    );
}
