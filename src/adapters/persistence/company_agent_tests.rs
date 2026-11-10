//! The no-bypass guard of company-agent creation (01DEP-P03-D1, r3 F4; 01DEP-P03-B1-F3).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::visit::Visit;

/// Whether `attrs` keep an item out of the production build: `#[cfg(test)]` (or a `cfg` that
/// needs `test`), or a test function's `#[test]` / `#[tokio::test]`.
fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.path();
        if path
            .segments
            .last()
            .is_some_and(|last| last.ident == "test")
        {
            return true;
        }
        if !path.is_ident("cfg") {
            return false;
        }
        let condition = attr.meta.require_list().map_or(String::new(), |list| {
            list.tokens.to_string().replace(' ', "")
        });
        condition.contains("test") && !condition.contains("not(test)")
    })
}

/// Production calls of the two insert primitives in one file, by enclosing function; test-only
/// items are skipped, and the out-of-line modules the file declares are collected for the walk.
struct Calls<'a> {
    file: &'a str,
    function: Vec<String>,
    found: BTreeSet<(String, String, String)>,
    modules: Vec<(Vec<String>, syn::ItemMod)>,
    inline: Vec<String>,
}

impl<'ast> Visit<'ast> for Calls<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs = match item {
            syn::Item::Fn(item) => &item.attrs,
            syn::Item::Impl(item) => &item.attrs,
            syn::Item::Mod(item) => &item.attrs,
            syn::Item::Trait(item) => &item.attrs,
            syn::Item::Const(item) => &item.attrs,
            syn::Item::Static(item) => &item.attrs,
            _ => return syn::visit::visit_item(self, item),
        };
        if !test_only(attrs) {
            syn::visit::visit_item(self, item);
        }
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if item.content.is_none() {
            self.modules.push((self.inline.clone(), item.clone()));
            return;
        }
        self.inline.push(item.ident.to_string());
        syn::visit::visit_item_mod(self, item);
        self.inline.pop();
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.function.push(item.sig.ident.to_string());
        syn::visit::visit_item_fn(self, item);
        self.function.pop();
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if test_only(&item.attrs) {
            return;
        }
        self.function.push(item.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, item);
        self.function.pop();
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref()
            && let Some(last) = path.path.segments.last()
            && ["insert_agent_on", "create_agent_principal_on"].contains(&&*last.ident.to_string())
        {
            self.found.insert((
                self.file.to_owned(),
                self.function.last().cloned().unwrap_or_default(),
                last.ident.to_string(),
            ));
        }
        syn::visit::visit_expr_call(self, call);
    }
}

/// Every production call site, walking the crate's module tree from `src/lib.rs` and leaving out
/// every module the production build does not compile.
fn production_calls() -> BTreeSet<(String, String, String)> {
    let mut found = BTreeSet::new();
    let mut pending = vec![(PathBuf::from("src/lib.rs"), PathBuf::from("src"))];
    let mut walked = 0;
    while let Some((file, children)) = pending.pop() {
        let name = file.to_string_lossy().replace('\\', "/");
        let source =
            std::fs::read_to_string(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
        let parsed = syn::parse_file(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut calls = Calls {
            file: &name,
            function: Vec::new(),
            found: BTreeSet::new(),
            modules: Vec::new(),
            inline: Vec::new(),
        };
        calls.visit_file(&parsed);
        walked += 1;
        found.extend(calls.found);
        let directory = file.parent().unwrap_or(Path::new("src")).to_path_buf();
        for (inline, module) in calls.modules {
            let explicit = module.attrs.iter().find_map(|attr| match &attr.meta {
                syn::Meta::NameValue(value) if value.path.is_ident("path") => match &value.value {
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(path),
                        ..
                    }) => Some(path.value()),
                    _ => None,
                },
                _ => None,
            });
            let base = inline
                .iter()
                .fold(children.clone(), |base, name| base.join(name));
            let (path, grandchildren) = match explicit {
                Some(path) => {
                    let base = inline
                        .iter()
                        .fold(directory.clone(), |base, name| base.join(name));
                    let path = base.join(path);
                    let grandchildren = path.with_extension("");
                    (path, grandchildren)
                }
                None => {
                    let ident = module.ident.to_string();
                    let flat = base.join(format!("{ident}.rs"));
                    if flat.exists() {
                        (flat, base.join(&ident))
                    } else {
                        (base.join(&ident).join("mod.rs"), base.join(&ident))
                    }
                }
            };
            pending.push((path, grandchildren));
        }
    }
    assert!(walked > 500, "the walk reached only {walked} files");
    found
}

/// Production company-agent rows come only from `create_company_agent_on`: outside it, the two
/// insert primitives are called only where no company agent is created -- the library inserts in
/// `persistence/agent.rs` (`company_id = None`) and the lazy principal of an existing agent in
/// `thread/message.rs`. `AgentPersistence::create` is `#[cfg(test)]`, so it is not production.
#[test]
fn agent_default_desk_company_agents_have_one_creation_path() {
    let expected: BTreeSet<(String, String, String)> = [
        (
            "src/adapters/persistence/company_agent.rs",
            "create_company_agent_on",
            "create_agent_principal_on",
        ),
        (
            "src/adapters/persistence/company_agent.rs",
            "create_company_agent_on",
            "insert_agent_on",
        ),
        (
            "src/adapters/persistence/agent.rs",
            "create_library",
            "insert_agent_on",
        ),
        (
            "src/adapters/persistence/agent.rs",
            "ensure_library_agent",
            "insert_agent_on",
        ),
        (
            "src/adapters/persistence/thread/message.rs",
            "resolve_author",
            "create_agent_principal_on",
        ),
    ]
    .into_iter()
    .map(|(file, function, primitive)| (file.into(), function.into(), primitive.into()))
    .collect();
    assert_eq!(production_calls(), expected);
}
