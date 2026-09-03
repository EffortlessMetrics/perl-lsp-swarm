//! Parallel-unsafe test serialization policy (`#[serial]`).
//!
//! Issue #1269 action item 3: a blocking policy that keeps a new parallel-unsafe
//! test function from entering the workspace without a serialization guard.
//!
//! A test function is *parallel-unsafe* when its own body mutates process-global
//! state:
//!
//! - process environment: `std::env::set_var` / `std::env::remove_var`, plus
//!   calls through direct, renamed, grouped, glob, or module imports from
//!   `std::env`;
//! - process working directory: `env::set_current_dir`.
//!
//! Such a function must carry an unkeyed in-process serialization guard from
//! the `serial_test` crate (`#[serial]` or `#[serial_test::serial]`), or it must
//! be listed in the accepted identity registry
//! (`ci/serial_test_identities.json`) with a reason. Keyed `serial` guards and
//! `file_serial` use different lock domains and therefore cannot establish
//! mutual exclusion with every guard this policy accepts.
//!
//! The registry follows the `panic_test` identity-registry convention:
//! `schema_version` 2, `active` rows must stay present in the inventory,
//! `retired` rows must stay absent, and unknown identities fail. Repairing a
//! registered site (annotating it) turns the gate red until the row is retired,
//! so the accepted set only shrinks.
//!
//! Every Rust source under `crates/` and `xtask/` is parsed, independent of
//! Cargo-root discovery. A bounded parent/child module map carries only
//! relevant `std::env` bindings through explicit `super` imports; it does not
//! attempt general Rust name resolution. The source-only analysis does not
//! expand macros; macro-generated tests and process-global calls inside macro
//! invocations remain unsupported. It deliberately ignores helper-mediated
//! mutation outside the test's direct body, TCP port binds (current main binds
//! are ephemeral port 0), and `static` counter mutation.

use color_eyre::eyre::{Result, eyre};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::ext::IdentExt;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    Attribute, Block, Expr, ExprCall, ImplItem, Item, ItemUse, Meta, Pat, Stmt, Token, UseTree,
};

use perl_ci_hygiene::walk_rs_files;

use crate::{NC, RED, YELLOW};

const DEFAULT_REGISTRY: &str = "ci/serial_test_identities.json";
const SIGNAL_VOCABULARY: [&str; 3] = ["cwd", "env_remove", "env_set"];

fn walk_workspace_rust_files(repo_root: &Path) -> Vec<PathBuf> {
    let mut files = BTreeSet::new();
    for root in [repo_root.join("crates"), repo_root.join("xtask")] {
        files.extend(walk_rs_files(&root));
    }
    files.into_iter().collect()
}

fn is_nested_test_payload(path: &Path) -> bool {
    let components = path.components().collect::<Vec<_>>();
    components
        .iter()
        .position(|component| component.as_os_str() == "tests")
        .is_some_and(|tests_index| components.len().saturating_sub(tests_index) > 2)
}

fn signal_category(function: &str) -> Option<&'static str> {
    match function {
        "set_var" => Some("env_set"),
        "remove_var" => Some("env_remove"),
        "set_current_dir" => Some("cwd"),
        _ => None,
    }
}

fn ident_name(ident: &syn::Ident) -> String {
    ident.unraw().to_string()
}

/// A test function that mutates process-global state without a serialization
/// guard.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SerialSiteIdentity {
    path: String,
    test_function: String,
    signals: Vec<&'static str>,
    line: usize,
}

impl SerialSiteIdentity {
    fn key(&self) -> (String, String) {
        (self.path.clone(), self.test_function.clone())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct EnvBindings {
    module_aliases: BTreeSet<String>,
    direct_aliases: BTreeMap<String, &'static str>,
}

impl EnvBindings {
    fn install_env_glob(&mut self, item_use: &ItemUse) {
        let mut paths = Vec::new();
        flatten_use_tree(&item_use.tree, &mut Vec::new(), &mut paths);
        for (path, _, glob) in paths {
            if glob && path == ["std", "env"] {
                for function in ["set_var", "remove_var", "set_current_dir"] {
                    if let Some(signal) = signal_category(function) {
                        self.direct_aliases.insert(function.to_owned(), signal);
                    }
                }
            }
        }
    }

    fn install_explicit_env_use(&mut self, item_use: &ItemUse) {
        let mut paths = Vec::new();
        flatten_use_tree(&item_use.tree, &mut Vec::new(), &mut paths);
        for (path, alias, glob) in paths {
            if glob {
                continue;
            }
            if path == ["std", "env"] {
                self.module_aliases.insert(alias.unwrap_or_else(|| "env".to_owned()));
                continue;
            }
            if path.len() == 3 && path[0] == "std" && path[1] == "env" {
                let function = path[2].as_str();
                if let Some(signal) = signal_category(function) {
                    self.direct_aliases
                        .insert(alias.unwrap_or_else(|| function.to_owned()), signal);
                }
            }
        }
    }

    fn install_ancestor_use(&mut self, item_use: &ItemUse, ancestors: &[Self]) {
        let mut paths = Vec::new();
        flatten_use_tree(&item_use.tree, &mut Vec::new(), &mut paths);
        for (path, alias, glob) in paths {
            let (authority, inherited) = if path.first().is_some_and(|segment| segment == "crate") {
                let Some(root) = ancestors.last() else { continue };
                (root, &path[1..])
            } else {
                let super_count = path.iter().take_while(|segment| *segment == "super").count();
                if super_count == 0 {
                    continue;
                }
                let Some(parent) = ancestors.get(super_count - 1) else { continue };
                (parent, &path[super_count..])
            };
            if glob && inherited.is_empty() {
                self.module_aliases.extend(authority.module_aliases.iter().cloned());
                self.direct_aliases.extend(
                    authority.direct_aliases.iter().map(|(name, signal)| (name.clone(), *signal)),
                );
                continue;
            }
            if glob && inherited.len() == 1 && authority.module_aliases.contains(&inherited[0]) {
                for function in ["set_var", "remove_var", "set_current_dir"] {
                    if let Some(signal) = signal_category(function) {
                        self.direct_aliases.insert(function.to_owned(), signal);
                    }
                }
                continue;
            }
            if !glob
                && inherited.len() == 2
                && authority.module_aliases.contains(&inherited[0])
                && let Some(signal) = signal_category(&inherited[1])
            {
                self.direct_aliases.insert(alias.unwrap_or_else(|| inherited[1].clone()), signal);
                continue;
            }
            if glob || inherited.len() != 1 {
                continue;
            }
            let source = &inherited[0];
            let target = alias.unwrap_or_else(|| source.clone());
            if authority.module_aliases.contains(source) {
                self.module_aliases.insert(target.clone());
            }
            if let Some(signal) = authority.direct_aliases.get(source) {
                self.direct_aliases.insert(target, *signal);
            }
        }
    }

    fn shadow_value(&mut self, name: &str) {
        self.direct_aliases.remove(name);
    }

    fn shadow_module(&mut self, name: &str) {
        self.module_aliases.remove(name);
    }

    fn shadow_both(&mut self, name: &str) {
        self.shadow_value(name);
        self.shadow_module(name);
    }

    fn shadow_item(&mut self, item: &Item) {
        match item {
            Item::Use(item_use) => {
                for name in bound_use_names(item_use) {
                    self.shadow_both(&name);
                }
            }
            Item::Fn(item) => self.shadow_value(&ident_name(&item.sig.ident)),
            Item::Const(item) => self.shadow_value(&ident_name(&item.ident)),
            Item::Static(item) => self.shadow_value(&ident_name(&item.ident)),
            Item::Struct(item) => self.shadow_both(&ident_name(&item.ident)),
            Item::Mod(item) => self.shadow_module(&ident_name(&item.ident)),
            Item::Type(item) => self.shadow_module(&ident_name(&item.ident)),
            Item::Enum(item) => self.shadow_module(&ident_name(&item.ident)),
            Item::Trait(item) => self.shadow_module(&ident_name(&item.ident)),
            Item::TraitAlias(item) => self.shadow_module(&ident_name(&item.ident)),
            Item::Union(item) => self.shadow_module(&ident_name(&item.ident)),
            _ => {}
        }
    }
}

fn flatten_use_tree(
    tree: &UseTree,
    prefix: &mut Vec<String>,
    paths: &mut Vec<(Vec<String>, Option<String>, bool)>,
) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(ident_name(&path.ident));
            flatten_use_tree(&path.tree, prefix, paths);
            prefix.pop();
        }
        UseTree::Name(name) => {
            let ident = ident_name(&name.ident);
            if ident == "self" {
                paths.push((prefix.clone(), prefix.last().cloned(), false));
            } else {
                let mut path = prefix.clone();
                path.push(ident);
                paths.push((path, None, false));
            }
        }
        UseTree::Rename(rename) => {
            let ident = ident_name(&rename.ident);
            let alias = ident_name(&rename.rename);
            if ident == "self" {
                paths.push((prefix.clone(), Some(alias), false));
            } else {
                let mut path = prefix.clone();
                path.push(ident);
                paths.push((path, Some(alias), false));
            }
        }
        UseTree::Glob(_) => paths.push((prefix.clone(), None, true)),
        UseTree::Group(group) => {
            for item in &group.items {
                flatten_use_tree(item, prefix, paths);
            }
        }
    }
}

fn bound_use_names(item_use: &ItemUse) -> BTreeSet<String> {
    let mut paths = Vec::new();
    flatten_use_tree(&item_use.tree, &mut Vec::new(), &mut paths);
    paths
        .into_iter()
        .filter_map(
            |(path, alias, glob)| {
                if glob { None } else { alias.or_else(|| path.last().cloned()) }
            },
        )
        .collect()
}

fn attributes_match(attrs: &[Attribute], expected: &[&[&str]]) -> bool {
    attrs.iter().any(|attr| {
        let segments = attr
            .path()
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        !segments.iter().any(|segment| segment.starts_with("r#"))
            && expected.iter().any(|path| {
                segments.len() == path.len()
                    && segments.iter().zip(path.iter()).all(|(actual, expected)| actual == expected)
            })
    })
}

fn is_test_function(attrs: &[Attribute]) -> bool {
    attributes_match(attrs, &[&["test"], &["tokio", "test"], &["rstest"]])
}

fn has_serial_guard(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attributes_match(std::slice::from_ref(attr), &[&["serial"], &["serial_test", "serial"]])
        {
            return false;
        }

        match &attr.meta {
            Meta::Path(_) => true,
            Meta::NameValue(_) => false,
            Meta::List(list) if list.tokens.is_empty() => true,
            Meta::List(list) => {
                let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
                parser.parse2(list.tokens.clone()).is_ok_and(|arguments| {
                    !arguments.is_empty()
                        && arguments.iter().all(|argument| {
                            matches!(
                                argument,
                                Meta::NameValue(value)
                                    if value.path.is_ident("inner_attrs")
                                        && matches!(value.value, Expr::Array(_))
                            )
                        })
                })
            }
        }
    })
}

#[derive(Default)]
struct PatternNames {
    names: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for PatternNames {
    fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
        self.names.insert(ident_name(&pattern.ident));
        visit::visit_pat_ident(self, pattern);
    }
}

fn pattern_names(pattern: &Pat) -> BTreeSet<String> {
    let mut names = PatternNames::default();
    names.visit_pat(pattern);
    names.names
}

#[derive(Default)]
struct InstantiatedTypes {
    names: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for InstantiatedTypes {
    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        if expression.qself.is_none()
            && expression.path.segments.len() == 1
            && let Some(segment) = expression.path.segments.first()
        {
            self.names.insert(ident_name(&segment.ident));
        }
        visit::visit_expr_path(self, expression);
    }

    fn visit_expr_struct(&mut self, expression: &'ast syn::ExprStruct) {
        if expression.qself.is_none()
            && expression.path.segments.len() == 1
            && let Some(segment) = expression.path.segments.first()
        {
            self.names.insert(ident_name(&segment.ident));
        }
        visit::visit_expr_struct(self, expression);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if local.init.is_some()
            && let Pat::Type(typed) = &local.pat
            && let syn::Type::Path(path) = typed.ty.as_ref()
            && path.qself.is_none()
            && path.path.segments.len() == 1
            && let Some(segment) = path.path.segments.first()
        {
            self.names.insert(ident_name(&segment.ident));
        }
        visit::visit_local(self, local);
    }

    fn visit_expr_closure(&mut self, _closure: &'ast syn::ExprClosure) {}

    fn visit_item(&mut self, _item: &'ast Item) {}
}

fn instantiated_types(block: &Block) -> BTreeSet<String> {
    let mut visitor = InstantiatedTypes::default();
    visitor.visit_block(block);
    visitor.names
}

fn immediate_closure(expression: &Expr) -> Option<&syn::ExprClosure> {
    match expression {
        Expr::Closure(closure) => Some(closure),
        Expr::Group(group) => immediate_closure(&group.expr),
        Expr::Paren(paren) => immediate_closure(&paren.expr),
        Expr::Block(block) if block.block.stmts.len() == 1 => match &block.block.stmts[0] {
            Stmt::Expr(expression, None) => immediate_closure(expression),
            _ => None,
        },
        _ => None,
    }
}

struct SignalVisitor {
    bindings: EnvBindings,
    signals: BTreeSet<&'static str>,
}

impl SignalVisitor {
    fn new(bindings: EnvBindings) -> Self {
        Self { bindings, signals: BTreeSet::new() }
    }

    fn resolve_call(&self, call: &ExprCall) -> Option<&'static str> {
        let Expr::Path(function) = call.func.as_ref() else { return None };
        if function.qself.is_some() {
            return None;
        }
        let segments = function
            .path
            .segments
            .iter()
            .map(|segment| ident_name(&segment.ident))
            .collect::<Vec<_>>();
        match segments.as_slice() {
            [std, env, function] if std == "std" && env == "env" => signal_category(function),
            [module, function] if self.bindings.module_aliases.contains(module) => {
                signal_category(function)
            }
            [function] => self.bindings.direct_aliases.get(function).copied(),
            _ => None,
        }
    }

    fn scan_block(&mut self, block: &Block) {
        let instantiated = instantiated_types(block);
        for statement in &block.stmts {
            if let Stmt::Item(Item::Use(item_use)) = statement {
                self.bindings.install_env_glob(item_use);
            }
        }
        for statement in &block.stmts {
            if let Stmt::Item(item) = statement {
                self.bindings.shadow_item(item);
            }
        }
        for statement in &block.stmts {
            if let Stmt::Item(Item::Use(item_use)) = statement {
                self.bindings.install_explicit_env_use(item_use);
            }
        }
        for statement in &block.stmts {
            match statement {
                Stmt::Local(local) => {
                    if let Some(initializer) = &local.init {
                        self.visit_expr(&initializer.expr);
                        if let Some((_, diverge)) = &initializer.diverge {
                            self.visit_expr(diverge);
                        }
                    }
                    for name in pattern_names(&local.pat) {
                        self.bindings.shadow_value(&name);
                    }
                }
                // A local Drop implementation can run as part of the test body.
                // Inspect only its `drop` method: ordinary local impl methods
                // are unreachable unless called and are not mutation evidence.
                Stmt::Item(Item::Impl(item_impl))
                    if item_impl
                        .trait_
                        .as_ref()
                        .and_then(|(_, path, _)| path.segments.last())
                        .is_some_and(|segment| segment.ident == "Drop")
                        && matches!(
                            item_impl.self_ty.as_ref(),
                            syn::Type::Path(path)
                                if path.qself.is_none()
                                    && path.path.segments.len() == 1
                                    && path.path.segments.first().is_some_and(|segment| {
                                        instantiated.contains(&ident_name(&segment.ident))
                                    })
                        ) =>
                {
                    for item in &item_impl.items {
                        if let ImplItem::Fn(method) = item
                            && method.sig.ident == "drop"
                        {
                            self.visit_block(&method.block);
                        }
                    }
                }
                Stmt::Item(_) | Stmt::Macro(_) => {}
                Stmt::Expr(expression, _) => self.visit_expr(expression),
            }
        }
    }

    fn scan_condition(&mut self, condition: &Expr) {
        match condition {
            Expr::Let(let_expression) => {
                self.visit_expr(&let_expression.expr);
                for name in pattern_names(&let_expression.pat) {
                    self.bindings.shadow_value(&name);
                }
            }
            Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_)) => {
                self.scan_condition(&binary.left);
                self.scan_condition(&binary.right);
            }
            Expr::Group(group) => self.scan_condition(&group.expr),
            Expr::Paren(paren) => self.scan_condition(&paren.expr),
            _ => self.visit_expr(condition),
        }
    }
}

impl<'ast> Visit<'ast> for SignalVisitor {
    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Some(signal) = self.resolve_call(call) {
            self.signals.insert(signal);
        }
        if let Some(closure) = immediate_closure(&call.func) {
            let mut child = Self::new(self.bindings.clone());
            for input in &closure.inputs {
                for name in pattern_names(input) {
                    child.bindings.shadow_value(&name);
                }
            }
            child.visit_expr(&closure.body);
            self.signals.extend(child.signals);
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_block(&mut self, block: &'ast Block) {
        let mut child = Self::new(self.bindings.clone());
        child.scan_block(block);
        self.signals.extend(child.signals);
    }

    fn visit_expr_closure(&mut self, _closure: &'ast syn::ExprClosure) {}

    fn visit_expr_for_loop(&mut self, expression: &'ast syn::ExprForLoop) {
        self.visit_expr(&expression.expr);
        let mut body = Self::new(self.bindings.clone());
        for name in pattern_names(&expression.pat) {
            body.bindings.shadow_value(&name);
        }
        body.visit_block(&expression.body);
        self.signals.extend(body.signals);
    }

    fn visit_expr_if(&mut self, expression: &'ast syn::ExprIf) {
        let mut then_branch = Self::new(self.bindings.clone());
        then_branch.scan_condition(&expression.cond);
        then_branch.visit_block(&expression.then_branch);
        self.signals.extend(then_branch.signals);

        if let Some((_, else_branch)) = &expression.else_branch {
            let mut child = Self::new(self.bindings.clone());
            child.visit_expr(else_branch);
            self.signals.extend(child.signals);
        }
    }

    fn visit_expr_match(&mut self, expression: &'ast syn::ExprMatch) {
        self.visit_expr(&expression.expr);
        for arm in &expression.arms {
            let mut child = Self::new(self.bindings.clone());
            for name in pattern_names(&arm.pat) {
                child.bindings.shadow_value(&name);
            }
            if let Some((_, guard)) = &arm.guard {
                child.visit_expr(guard);
            }
            child.visit_expr(&arm.body);
            self.signals.extend(child.signals);
        }
    }

    fn visit_expr_while(&mut self, expression: &'ast syn::ExprWhile) {
        let mut body = Self::new(self.bindings.clone());
        body.scan_condition(&expression.cond);
        body.visit_block(&expression.body);
        self.signals.extend(body.signals);
    }

    fn visit_item_fn(&mut self, _function: &'ast syn::ItemFn) {}
}

fn module_bindings(items: &[Item], ancestors: &[EnvBindings]) -> EnvBindings {
    let mut bindings = EnvBindings::default();
    for item in items {
        if let Item::Use(item_use) = item {
            bindings.install_env_glob(item_use);
            bindings.install_ancestor_use(item_use, ancestors);
        }
    }
    for item in items {
        bindings.shadow_item(item);
    }
    for item in items {
        if let Item::Use(item_use) = item {
            bindings.install_explicit_env_use(item_use);
            bindings.install_ancestor_use(item_use, ancestors);
        }
    }
    bindings
}

struct ParsedRustFile {
    path: PathBuf,
    relative: String,
    syntax: syn::File,
}

fn parse_rust_file(repo_root: &Path, path: &Path) -> Result<Option<ParsedRustFile>> {
    let source = std::fs::read_to_string(path)
        .map_err(|err| eyre!("reading Rust source {:?}: {err}", path))?;
    let syntax = match syn::parse_file(&source) {
        Ok(syntax) => syntax,
        Err(_) if is_nested_test_payload(path) => {
            // Repositories commonly keep non-Rust fixture payloads or unused
            // support snippets under a nested `tests/` directory with an
            // `.rs` extension. They are not Cargo integration-test roots, and
            // invalid Rust cannot be a compiled external module. Valid nested
            // modules still parse and remain in the inventory.
            return Ok(None);
        }
        Err(err) => return Err(eyre!("parsing Rust source {:?}: {err}", path)),
    };
    let relative =
        path.strip_prefix(repo_root).unwrap_or(path).display().to_string().replace('\\', "/");
    Ok(Some(ParsedRustFile { path: path.to_path_buf(), relative, syntax }))
}

fn explicit_module_path(module: &syn::ItemMod) -> Option<PathBuf> {
    module.attrs.iter().find_map(|attribute| {
        if !attribute.path().is_ident("path") {
            return None;
        }
        let Meta::NameValue(value) = &attribute.meta else { return None };
        let Expr::Lit(literal) = &value.value else { return None };
        let syn::Lit::Str(path) = &literal.lit else { return None };
        Some(PathBuf::from(path.value()))
    })
}

fn external_module_path(
    module_directory: &Path,
    module: &syn::ItemMod,
    known: &BTreeMap<PathBuf, usize>,
) -> Option<PathBuf> {
    if let Some(explicit) = explicit_module_path(module) {
        let candidate = module_directory.join(explicit);
        return known.contains_key(&candidate).then_some(candidate);
    }

    let name = ident_name(&module.ident);
    let source = module_directory.join(format!("{name}.rs"));
    if known.contains_key(&source) {
        return Some(source);
    }
    let legacy = module_directory.join(&name).join("mod.rs");
    known.contains_key(&legacy).then_some(legacy)
}

struct ScanContext<'a> {
    parsed: &'a [ParsedRustFile],
    known: &'a BTreeMap<PathBuf, usize>,
    visited: BTreeSet<(usize, PathBuf, Vec<EnvBindings>)>,
    files_seen: BTreeSet<usize>,
    sites: Vec<SerialSiteIdentity>,
    serialized_sites: Vec<SerialSiteIdentity>,
}

impl ScanContext<'_> {
    fn scan_scope(
        &mut self,
        file_index: usize,
        items: &[Item],
        inline_modules: &[String],
        module_directory: &Path,
        ancestors: &[EnvBindings],
    ) {
        let file = &self.parsed[file_index];
        let bindings = module_bindings(items, ancestors);
        for item in items {
            match item {
                Item::Fn(function) if is_test_function(&function.attrs) => {
                    let serialized = has_serial_guard(&function.attrs);
                    let mut visitor = SignalVisitor::new(bindings.clone());
                    for input in &function.sig.inputs {
                        if let syn::FnArg::Typed(argument) = input {
                            for name in pattern_names(&argument.pat) {
                                visitor.bindings.shadow_value(&name);
                            }
                        }
                    }
                    visitor.visit_block(&function.block);
                    if !visitor.signals.is_empty() {
                        let site = SerialSiteIdentity {
                            path: file.relative.clone(),
                            test_function: if inline_modules.is_empty() {
                                function.sig.ident.to_string()
                            } else {
                                format!("{}::{}", inline_modules.join("::"), function.sig.ident)
                            },
                            signals: visitor.signals.into_iter().collect(),
                            line: function.sig.fn_token.span.start().line,
                        };
                        if serialized {
                            self.serialized_sites.push(site);
                        } else {
                            self.sites.push(site);
                        }
                    }
                }
                Item::Mod(module) => {
                    if let Some((_, child_items)) = &module.content {
                        let mut child_modules = inline_modules.to_vec();
                        let module_name = ident_name(&module.ident);
                        child_modules.push(module_name.clone());
                        let child_directory = module_directory.join(module_name);
                        let mut child_ancestors = Vec::with_capacity(ancestors.len() + 1);
                        child_ancestors.push(bindings.clone());
                        child_ancestors.extend_from_slice(ancestors);
                        self.scan_scope(
                            file_index,
                            child_items,
                            &child_modules,
                            &child_directory,
                            &child_ancestors,
                        );
                    } else if let Some(path) =
                        external_module_path(module_directory, module, self.known)
                        && let Some(child) = self.known.get(&path).copied()
                    {
                        let child_directory = module_directory.join(ident_name(&module.ident));
                        let mut child_ancestors = Vec::with_capacity(ancestors.len() + 1);
                        child_ancestors.push(bindings.clone());
                        child_ancestors.extend_from_slice(ancestors);
                        self.scan_file(child, &child_directory, &child_ancestors);
                    }
                }
                _ => {}
            }
        }
    }

    fn scan_file(&mut self, index: usize, module_directory: &Path, ancestors: &[EnvBindings]) {
        if !self.visited.insert((index, module_directory.to_path_buf(), ancestors.to_vec())) {
            return;
        }
        self.files_seen.insert(index);
        let items = self.parsed[index].syntax.items.clone();
        self.scan_scope(index, &items, &[], module_directory, ancestors);
    }
}

fn collect_external_children(
    items: &[Item],
    module_directory: &Path,
    known: &BTreeMap<PathBuf, usize>,
    children: &mut BTreeSet<usize>,
) {
    for item in items {
        let Item::Mod(module) = item else { continue };
        if let Some((_, child_items)) = &module.content {
            let child_directory = module_directory.join(ident_name(&module.ident));
            collect_external_children(child_items, &child_directory, known, children);
        } else if let Some(path) = external_module_path(module_directory, module, known)
            && let Some(child) = known.get(&path)
        {
            children.insert(*child);
        }
    }
}

fn complete_serial_site_inventory(repo_root: &Path) -> Result<Vec<SerialSiteIdentity>> {
    Ok(complete_serial_site_inventories(repo_root)?.0)
}

fn complete_serial_site_inventories(
    repo_root: &Path,
) -> Result<(Vec<SerialSiteIdentity>, Vec<SerialSiteIdentity>)> {
    // Keep the established unguarded inventory API while exposing guarded
    // direct mutations to the registry contract.
    let mut parsed = Vec::new();
    for path in walk_workspace_rust_files(repo_root) {
        if let Some(file) = parse_rust_file(repo_root, &path)? {
            parsed.push(file);
        }
    }
    let known = parsed
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut children = BTreeSet::new();
    for file in &parsed {
        if let Some(directory) = file.path.parent() {
            collect_external_children(&file.syntax.items, directory, &known, &mut children);
        }
    }
    let mut scan = ScanContext {
        parsed: &parsed,
        known: &known,
        visited: BTreeSet::new(),
        files_seen: BTreeSet::new(),
        sites: Vec::new(),
        serialized_sites: Vec::new(),
    };
    for (index, file) in parsed.iter().enumerate() {
        if !children.contains(&index)
            && let Some(directory) = file.path.parent()
        {
            scan.scan_file(index, directory, &[]);
        }
    }
    for (index, file) in parsed.iter().enumerate() {
        if !scan.files_seen.contains(&index)
            && let Some(directory) = file.path.parent()
        {
            scan.scan_file(index, directory, &[]);
        }
    }
    // Keep the guard classification attached to each site while preserving its
    // fully-qualified identity.  Inline-module identity must not change when a
    // sibling is added, removed, or changes guard status.
    let mut all_sites = scan.sites.into_iter().map(|site| (site, false)).collect::<Vec<_>>();
    all_sites.extend(scan.serialized_sites.into_iter().map(|site| (site, true)));
    let mut classified = all_sites;
    classified.sort_by(|(left, _), (right, _)| left.cmp(right));
    classified.dedup_by(|(left, _), (right, _)| left == right);
    let mut unguarded = Vec::new();
    let mut serialized = Vec::new();
    for (site, is_serialized) in classified {
        if is_serialized {
            serialized.push(site);
        } else {
            unguarded.push(site);
        }
    }
    Ok((unguarded, serialized))
}

/// Emit the current parallel-unsafe inventory as JSON. This is the measurement
/// surface used to seed and adjudicate `ci/serial_test_identities.json`.
pub(crate) fn write_inventory(repo_root: &Path) -> Result<i32> {
    let sites = complete_serial_site_inventory(repo_root)?;
    let json_sites = sites
        .iter()
        .map(|site| {
            serde_json::json!({
                "path": site.path,
                "test_function": site.test_function,
                "signals": site.signals,
                "line": site.line,
            })
        })
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&json_sites)?);
    Ok(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistryState {
    Active,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Remediation {
    Pending,
    Serialized,
    Eliminated,
}

#[derive(Debug)]
struct SerialSiteRecord {
    path: String,
    test_function: String,
    signals: String,
    accepted_reason: String,
    state: RegistryState,
    remediation: Remediation,
}

fn validate_registry_signals(entry: usize, signals: &str) -> Result<()> {
    let tokens = signals.split(',').collect::<BTreeSet<_>>();
    if let Some(unknown) = tokens.iter().find(|token| !SIGNAL_VOCABULARY.contains(*token)) {
        return Err(eyre!(
            "serial identity registry entry {entry} has unknown signal {unknown:?}; expected only {}",
            SIGNAL_VOCABULARY.join(",")
        ));
    }
    let canonical = tokens.iter().copied().collect::<Vec<_>>().join(",");
    if signals != canonical {
        return Err(eyre!(
            "serial identity registry entry {entry} signals must be sorted, unique canonical CSV; expected {canonical:?}"
        ));
    }
    Ok(())
}

impl SerialSiteRecord {
    fn key(&self) -> (String, String) {
        (self.path.clone(), self.test_function.clone())
    }
}

fn registry_path(repo_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { repo_root.join(path) }
}

fn read_identity_registry(path: &Path) -> Result<BTreeMap<(String, String), SerialSiteRecord>> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| eyre!("reading serial identity registry {:?}: {err}", path))?;
    let document: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|err| eyre!("parsing serial identity registry {:?}: {err}", path))?;
    let root = document
        .as_object()
        .ok_or_else(|| eyre!("serial identity registry root must be an object"))?;
    if let Some(field) =
        root.keys().find(|field| !["schema_version", "sites"].contains(&field.as_str()))
    {
        return Err(eyre!("serial identity registry has unknown root field {field:?}"));
    }
    let schema_version = document
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| eyre!("serial identity registry schema_version must be an integer"))?;
    if schema_version != 1 && schema_version != 2 {
        return Err(eyre!(
            "unsupported serial identity registry schema_version {schema_version}; expected 1 or 2"
        ));
    }
    let sites = document
        .get("sites")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| eyre!("serial identity registry sites must be an array"))?;

    let mut records = BTreeMap::new();
    for (index, value) in sites.iter().enumerate() {
        let object = value.as_object().ok_or_else(|| {
            eyre!("serial identity registry entry {} must be an object", index + 1)
        })?;
        if let Some(field) = object.keys().find(|field| {
            !["path", "test_function", "signals", "accepted_reason", "state", "remediation"]
                .contains(&field.as_str())
        }) {
            return Err(eyre!(
                "serial identity registry entry {} has unknown field {field:?}",
                index + 1
            ));
        }
        let required_string = |field: &str| {
            object.get(field).and_then(serde_json::Value::as_str).map(str::to_owned).ok_or_else(
                || eyre!("serial identity registry entry {} requires string {field}", index + 1),
            )
        };
        let state = match required_string("state")?.as_str() {
            "active" => RegistryState::Active,
            "retired" => RegistryState::Retired,
            other => {
                return Err(eyre!(
                    "serial identity registry entry {} has invalid state {other:?}",
                    index + 1
                ));
            }
        };
        // Version 1 registries predate the orthogonal remediation field.  Only
        // active rows have an unambiguous legacy default; retired rows must be
        // migrated with an explicit serialized/eliminated disposition.
        let remediation = match object.get("remediation") {
            None if schema_version == 1 && state == RegistryState::Active => Remediation::Pending,
            None if schema_version == 1 => {
                return Err(eyre!(
                    "serial identity registry entry {} legacy v1 retired rows require explicit remediation",
                    index + 1
                ));
            }
            None => {
                return Err(eyre!(
                    "serial identity registry entry {} schema_version 2 requires remediation",
                    index + 1
                ));
            }
            Some(value) => match value.as_str() {
                Some("pending") => Remediation::Pending,
                Some("serialized") => Remediation::Serialized,
                Some("eliminated") => Remediation::Eliminated,
                Some(other) => {
                    return Err(eyre!(
                        "serial identity registry entry {} has invalid remediation {other:?}",
                        index + 1
                    ));
                }
                None => {
                    return Err(eyre!(
                        "serial identity registry entry {} remediation must be a string",
                        index + 1
                    ));
                }
            },
        };
        if state == RegistryState::Active && remediation != Remediation::Pending {
            return Err(eyre!(
                r#"serial identity registry entry {} active rows require remediation="pending""#,
                index + 1
            ));
        }
        if state == RegistryState::Retired && remediation == Remediation::Pending {
            return Err(eyre!(
                r#"serial identity registry entry {} retired rows require remediation="serialized" or "eliminated""#,
                index + 1
            ));
        }
        let record = SerialSiteRecord {
            path: required_string("path")?,
            test_function: required_string("test_function")?,
            signals: required_string("signals")?,
            accepted_reason: required_string("accepted_reason")?,
            state,
            remediation,
        };
        let fields = [
            ("path", record.path.trim()),
            ("test_function", record.test_function.trim()),
            ("signals", record.signals.trim()),
            ("accepted_reason", record.accepted_reason.trim()),
        ];
        if let Some((field, _)) = fields.iter().find(|(_, value)| value.is_empty()) {
            return Err(eyre!("serial identity registry entry {} has an empty {field}", index + 1));
        }
        validate_registry_signals(index + 1, &record.signals)?;
        if records.insert(record.key(), record).is_some() {
            return Err(eyre!(
                "serial identity registry entry {} duplicates a stable site identity",
                index + 1
            ));
        }
    }
    Ok(records)
}

/// Validate the parallel-unsafe inventory against the accepted registry.
///
/// Fail-closed transitions:
/// - an unserialized parallel-unsafe test fn absent from the registry is NEW
///   and fails the gate (this is the regression door);
/// - an `active` registry row whose site is no longer detected means the site
///   was repaired — retire the row before the gate passes (ratchet tightens);
/// - a `retired` row detected again means the guard was removed — fails.
pub(crate) fn check_serial_test_with_registry(repo_root: &Path, path: &Path) -> Result<i32> {
    let resolved = registry_path(repo_root, path);
    let registry = read_identity_registry(&resolved)?;
    let registry_schema_version = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(&resolved)
            .map_err(|err| eyre!("reading serial identity registry {:?}: {err}", resolved))?,
    )?
    .get("schema_version")
    .and_then(serde_json::Value::as_u64)
    .ok_or_else(|| eyre!("serial identity registry schema_version must be an integer"))?;
    let (inventory, serialized_inventory) = complete_serial_site_inventories(repo_root)?;
    let mut current = BTreeMap::new();
    for site in inventory {
        if current.insert(site.key(), site).is_some() {
            return Err(eyre!(
                "parallel-unsafe inventory contains duplicate stable identity (path, test_function); \
                 rename one of the colliding test functions"
            ));
        }
    }
    // Accept pre-v2 bare registry keys as a read-only migration bridge for
    // existing rows; new inventory identities remain fully qualified and
    // collision-stable.
    let mut lookup_current = current.keys().cloned().collect::<BTreeSet<_>>();
    let mut qualified_by_bare = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for (path, function) in current.keys() {
        let bare = function.rsplit("::").next().unwrap_or(function).to_owned();
        qualified_by_bare.entry((path.clone(), bare)).or_default().insert(function.clone());
    }

    let active_count =
        registry.values().filter(|record| record.state == RegistryState::Active).count();
    let mut serialized = BTreeMap::<(String, String), SerialSiteIdentity>::new();
    for site in serialized_inventory {
        let key = site.key();
        if let Some(existing) = serialized.get_mut(&key) {
            let signals = existing
                .signals
                .iter()
                .chain(site.signals.iter())
                .copied()
                .collect::<BTreeSet<_>>();
            existing.signals = signals.into_iter().collect();
        } else {
            serialized.insert(key, site);
        }
    }
    for (path, function) in serialized.keys() {
        let bare = function.rsplit("::").next().unwrap_or(function).to_owned();
        qualified_by_bare.entry((path.clone(), bare)).or_default().insert(function.clone());
    }
    if registry_schema_version == 1 {
        for ((path, bare), qualified) in &qualified_by_bare {
            if qualified.len() == 1 {
                lookup_current.insert((path.clone(), bare.clone()));
            }
        }
    }
    println!(
        "parallel-unsafe test identities: current={} active_registry={} registry={:?}",
        current.len(),
        active_count,
        resolved
    );

    let mut failures = Vec::new();
    for (key, site) in &current {
        let bare_key = (key.0.clone(), key.1.rsplit("::").next().unwrap_or(&key.1).to_owned());
        let bare_match = qualified_by_bare.get(&bare_key).is_some_and(|ids| ids.len() == 1);
        match registry.get(key).or_else(|| {
            (registry_schema_version == 1 && bare_match).then(|| registry.get(&bare_key)).flatten()
        }) {
            None => failures.push(format!(
                "NEW parallel-unsafe test: {}:{} {} ({}) — add #[serial] or adjudicate a registry row",
                site.path,
                site.line,
                site.test_function,
                site.signals.join(",")
            )),
            Some(record) if record.state == RegistryState::Retired => failures.push(format!(
                "RETIRED identity returned: {} {} ({})",
                site.path,
                site.test_function,
                site.signals.join(",")
            )),
            Some(record) if record.remediation == Remediation::Eliminated => failures.push(format!(
                "ELIMINATED identity returned: {} {} ({})",
                site.path, site.test_function, site.signals.join(",")
            )),
            Some(record) => {
                let current_signals = site.signals.join(",");
                if record.signals != current_signals {
                    failures.push(format!(
                        "ACTIVE identity signals changed: {} {} registry=({}) current=({}) — adjudicate the registry row",
                        site.path, site.test_function, record.signals, current_signals
                    ));
                }
            }
        }
    }
    for (key, site) in &serialized {
        let bare_key = (key.0.clone(), key.1.rsplit("::").next().unwrap_or(&key.1).to_owned());
        let bare_match = qualified_by_bare.get(&bare_key).is_some_and(|ids| ids.len() == 1);
        match registry.get(key).or_else(|| {
            (registry_schema_version == 1 && bare_match).then(|| registry.get(&bare_key)).flatten()
        }) {
            Some(record)
                if record.state == RegistryState::Retired
                    && record.remediation == Remediation::Serialized =>
            {
                let current_signals = site.signals.join(",");
                if record.signals != current_signals {
                    failures.push(format!(
                        "SERIALIZED identity signals changed: {} {}",
                        site.path, site.test_function
                    ));
                }
            }
            Some(record) if record.remediation == Remediation::Eliminated => {
                failures.push(format!(
                    "ELIMINATED identity returned: {} {} ({})",
                    site.path,
                    site.test_function,
                    site.signals.join(",")
                ))
            }
            Some(_) => failures.push(format!(
                "serialized identity has invalid registry disposition: {} {}",
                site.path, site.test_function
            )),
            None => {}
        }
    }
    for (key, record) in &registry {
        let serialized_keys = serialized.keys().cloned().collect::<BTreeSet<_>>();
        let serialized_with_legacy = serialized_keys
            .iter()
            .flat_map(|(path, function)| {
                let bare =
                    (path.clone(), function.rsplit("::").next().unwrap_or(function).to_owned());
                let alias = (registry_schema_version == 1)
                    .then(|| {
                        qualified_by_bare.get(&bare).filter(|ids| ids.len() == 1).map(|_| bare)
                    })
                    .flatten();
                std::iter::once((path.clone(), function.clone())).chain(alias)
            })
            .collect::<BTreeSet<_>>();
        if record.remediation == Remediation::Serialized && !serialized_with_legacy.contains(key) {
            failures.push(format!(
                "SERIALIZED identity no longer has a direct mutation and canonical guard: {} {}",
                record.path, record.test_function
            ));
        }
        if record.remediation == Remediation::Eliminated
            && (lookup_current.contains(key) || serialized_with_legacy.contains(key))
        {
            failures.push(format!(
                "ELIMINATED identity has a direct process mutation: {} {}",
                record.path, record.test_function
            ));
        }
    }
    for (key, record) in &registry {
        if record.state == RegistryState::Active && !lookup_current.contains(key) {
            failures.push(format!(
                "ACTIVE identity no longer detected: {} {} — if repaired, retire the registry row",
                record.path, record.test_function
            ));
        }
    }

    if failures.is_empty() {
        println!("{NC}PASS: every parallel-unsafe test is serialized or adjudicated{NC}");
        return Ok(0);
    }

    println!(
        "{RED}FAIL: {} parallel-unsafe test transition(s) require adjudication{NC}",
        failures.len()
    );
    for failure in failures {
        println!("{YELLOW}- {failure}{NC}");
    }
    Ok(1)
}

/// Enforce the parallel-unsafe test serialization registry.
pub(crate) fn check_serial_test(repo_root: &Path) -> Result<i32> {
    let default_path = repo_root.join(DEFAULT_REGISTRY);
    if !default_path.is_file() {
        return Err(eyre!(
            "serial identity registry {:?} is missing; the parallel-unsafe test gate cannot run",
            default_path
        ));
    }
    check_serial_test_with_registry(repo_root, &default_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new(label: &str) -> Result<Self> {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = std::env::temp_dir().join(format!(
                "perl-ci-hygiene-serial-test-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(path.join("ci"))?;
            fs::create_dir_all(path.join("crates/demo/tests/support"))?;
            fs::create_dir_all(path.join("crates/demo/src"))?;
            Ok(Self { path })
        }

        fn write_test(&self, contents: &str) -> Result<()> {
            fs::write(self.path.join("crates/demo/tests/demo.rs"), contents)
                .map_err(color_eyre::eyre::Report::from)
        }

        fn write_registry(&self, sites: serde_json::Value) -> Result<PathBuf> {
            let path = self.path.join("ci/serial_test_identities.json");
            fs::write(&path, serde_json::to_vec_pretty(&sites)?)?;
            Ok(path)
        }

        fn empty_registry(&self) -> Result<PathBuf> {
            self.write_registry(serde_json::json!({ "schema_version": 1, "sites": [] }))
        }

        fn check(&self, registry: &Path) -> Result<i32> {
            check_serial_test_with_registry(&self.path, registry)
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn registry_row(
        path: &str,
        test_function: &str,
        signals: &str,
        reason: &str,
        state: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "path": path,
            "test_function": test_function,
            "signals": signals,
            "accepted_reason": reason,
            "state": state,
            "remediation": if state == "retired" { "serialized" } else { "pending" },
        })
    }

    const UNANNOTATED_ENV_TEST: &str = "#[test]\nfn flips_toolchain_env() {\n    unsafe {\n        std::env::set_var(\"PERLBREW_ROOT\", \"/tmp/plenv\");\n    }\n}\n";

    /// Discriminating mutant: injecting an unannotated parallel-unsafe test
    /// must fail the gate.
    #[test]
    fn mutant_unannotated_parallel_unsafe_test_fails() -> Result<()> {
        let repo = TempRepo::new("mutant-new")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    /// The identical function with the repo's serialization guard passes.
    #[test]
    fn annotated_twin_passes() -> Result<()> {
        let repo = TempRepo::new("mutant-annotated")?;
        let annotated = UNANNOTATED_ENV_TEST.replace("#[test]", "#[test]\n#[serial]");
        repo.write_test(&annotated)?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn only_unkeyed_in_process_serial_guards_pass() -> Result<()> {
        let repo = TempRepo::new("serial-idioms")?;
        repo.write_test(concat!(
            "#[test]\n#[serial]\nfn plain() {\n    std::env::set_var(\"A\", \"1\");\n}\n",
            "#[test]\n#[serial_test::serial]\nfn qualified() {\n    std::env::remove_var(\"B\");\n}\n",
            "#[test]\n#[serial(inner_attrs = [cfg(unix)])]\nfn with_inner_attrs() {\n    std::env::set_var(\"C\", \"1\");\n}\n",
        ))?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn heterogeneous_serial_keys_do_not_satisfy_one_lock_domain() -> Result<()> {
        let repo = TempRepo::new("serial-key-domains")?;
        repo.write_test(concat!(
            "#[test]\n#[serial(first)]\nfn first_key() {\n    std::env::set_var(\"A\", \"1\");\n}\n",
            "#[test]\n#[serial(second)]\nfn second_key() {\n    std::env::remove_var(\"B\");\n}\n",
            "#[test]\n#[serial(third, inner_attrs = [cfg(unix)])]\nfn key_with_inner_attrs() {\n    std::env::set_var(\"C\", \"1\");\n}\n",
        ))?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 3);
        assert_eq!(sites[0].test_function, "first_key");
        assert_eq!(sites[1].test_function, "key_with_inner_attrs");
        assert_eq!(sites[2].test_function, "second_key");
        Ok(())
    }

    #[test]
    fn file_serial_does_not_coordinate_with_in_process_serial() -> Result<()> {
        let repo = TempRepo::new("serial-file-domain")?;
        repo.write_test(concat!(
            "#[test]\n#[serial]\nfn process_lock() {\n    std::env::set_var(\"A\", \"1\");\n}\n",
            "#[test]\n#[file_serial]\nfn file_lock() {\n    std::env::remove_var(\"B\");\n}\n",
        ))?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].test_function, "file_lock");
        assert_eq!(sites[0].signals, vec!["env_remove"]);
        Ok(())
    }

    #[test]
    fn tokio_and_rstest_surfaces_are_detected() -> Result<()> {
        let repo = TempRepo::new("attr-macros")?;
        repo.write_test(concat!(
            "#[tokio::test]\nasync fn async_env_flip() {\n    std::env::set_var(\"A\", \"1\");\n}\n",
            "#[rstest]\n#[case(1)]\nfn rstest_env_flip() {\n    std::env::set_var(\"B\", \"1\");\n}\n",
        ))?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn helper_functions_are_not_flagged() -> Result<()> {
        let repo = TempRepo::new("helpers")?;
        fs::write(
            repo.path.join("crates/demo/tests/support/env_guard.rs"),
            "pub fn set(key: &str, value: &str) {\n    unsafe { std::env::set_var(key, value) };\n}\n",
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn local_drop_cleanup_mutation_is_part_of_the_test_body() -> Result<()> {
        let repo = TempRepo::new("local-drop")?;
        repo.write_test(
            r#"
#[test]
fn restores_environment_on_drop() {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("A"); }
        }
    }
    let _guard = Guard;
    unsafe { std::env::set_var("A", "1"); }
}
"#,
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].test_function, "restores_environment_on_drop");
        assert_eq!(sites[0].signals, vec!["env_remove", "env_set"]);
        Ok(())
    }

    #[test]
    fn unused_ordinary_local_impl_method_is_not_mutation_evidence() -> Result<()> {
        let repo = TempRepo::new("unused-local-method")?;
        repo.write_test(
            r#"
#[test]
fn never_calls_helper_method() {
    struct Helper;
    impl Helper {
        fn mutate_environment(&self) {
            unsafe { std::env::remove_var("A"); }
        }
    }
    let _helper = Helper;
}
"#,
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert!(sites.is_empty());
        Ok(())
    }

    #[test]
    fn uninstantiated_drop_impl_is_not_mutation_evidence() -> Result<()> {
        let repo = TempRepo::new("unused-drop")?;
        repo.write_test(
            r#"
#[test]
fn never_constructs_guard() {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("A"); }
        }
    }
}
"#,
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert!(sites.is_empty());
        Ok(())
    }

    #[test]
    fn only_immediately_invoked_closures_are_mutation_evidence() -> Result<()> {
        let repo = TempRepo::new("closure-reachability")?;
        repo.write_test(
            r#"
#[test]
fn stores_unused_closure() {
    let _later = || unsafe { std::env::set_var("A", "1"); };
}

#[test]
fn invokes_closure_now() {
    ({ || unsafe { std::env::remove_var("B"); } })();
}
"#,
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].test_function, "invokes_closure_now");
        assert_eq!(sites[0].signals, vec!["env_remove"]);
        Ok(())
    }

    #[test]
    fn typed_default_construction_instantiates_drop_guard() -> Result<()> {
        let repo = TempRepo::new("typed-drop")?;
        repo.write_test(
            r#"
#[test]
fn constructs_typed_guard() {
    struct Guard;
    impl Default for Guard {
        fn default() -> Self { Guard }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("A"); }
        }
    }
    let _guard: Guard = Default::default();
}
"#,
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].test_function, "constructs_typed_guard");
        assert_eq!(sites[0].signals, vec!["env_remove"]);
        Ok(())
    }

    #[test]
    fn method_call_set_var_is_not_process_env() -> Result<()> {
        let repo = TempRepo::new("method-call")?;
        repo.write_test(
            "#[test]\nfn builds_child_env() {\n    let mut cmd = std::process::Command::new(\"perl\");\n    cmd.env(\"A\", \"1\").env_remove(\"B\").env_clear();\n    let mut harness = Harness::default();\n    harness.set_var(\"A\", \"1\");\n}\n",
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn std_env_import_forms_and_turbofish_are_resolved() -> Result<()> {
        let repo = TempRepo::new("import-forms")?;
        repo.write_test(
            r#"
mod direct {
    use std::env::set_var;
    #[test]
    fn direct_import() { unsafe { set_var("A", "1"); } }
}
mod renamed {
    use std::env::set_var as change_env;
    #[test]
    fn renamed_turbofish() { unsafe { change_env::<_, _>("A", "1"); } }
}
mod grouped {
    use std::env::{remove_var as clear_env, set_current_dir};
    #[test]
    fn grouped_imports() { unsafe { clear_env("A"); } set_current_dir("/tmp"); }
}
mod module_self {
    use std::env::{self as process_env};
    #[test]
    fn module_alias() { unsafe { process_env::set_var("A", "1"); } }
}
mod globbed {
    use std::env::*;
    #[test]
    fn glob_import() { unsafe { remove_var("A"); } }
}
mod raw_renamed {
    use std::env::set_var as r#change_env;
    #[test]
    fn raw_alias() { unsafe { r#change_env("A", "1"); } }
}
"#,
        )?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        let actual = sites
            .iter()
            .map(|site| (site.test_function.as_str(), site.signals.as_slice()))
            .collect::<BTreeMap<_, _>>();
        let expected = BTreeMap::from([
            ("direct::direct_import", ["env_set"].as_slice()),
            ("globbed::glob_import", ["env_remove"].as_slice()),
            ("grouped::grouped_imports", ["cwd", "env_remove"].as_slice()),
            ("module_self::module_alias", ["env_set"].as_slice()),
            ("raw_renamed::raw_alias", ["env_set"].as_slice()),
            ("renamed::renamed_turbofish", ["env_set"].as_slice()),
        ]);
        assert_eq!(actual, expected);
        Ok(())
    }

    #[test]
    fn explicit_scope_names_beat_environment_glob_imports() -> Result<()> {
        let repo = TempRepo::new("glob-priority")?;
        repo.write_test(
            r#"
mod named_item {
    use std::env::*;
    fn set_var(_key: &str, _value: &str) {}

    #[test]
    fn local_function_wins() { set_var("A", "1"); }
}

mod unrelated {
    pub fn remove_var(_key: &str) {}
}

#[test]
fn explicit_block_import_wins() {
    use std::env::*;
    use self::unrelated::remove_var;
    remove_var("A");
}

mod positive {
    use std::env::*;

    #[test]
    fn glob_only_environment_call() { unsafe { set_var("A", "1"); } }
}
"#,
        )?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/demo.rs");
        assert_eq!(sites[0].test_function, "positive::glob_only_environment_call");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn import_aliases_are_lexically_scoped_to_their_module() -> Result<()> {
        let repo = TempRepo::new("scoped-alias")?;
        repo.write_test(
            r#"
mod importing_sibling {
    use std::env::set_var as change_env;
    #[test]
    fn imported_call() { change_env("A", "1"); }
}
mod unrelated_sibling {
    fn change_env(_key: &str, _value: &str) {}
    #[test]
    fn unrelated_call() { change_env("A", "1"); }
}
"#,
        )?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].test_function, "importing_sibling::imported_call");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn external_child_inherits_only_relevant_parent_environment_bindings() -> Result<()> {
        let repo = TempRepo::new("external-parent-bindings")?;
        repo.write_test(
            r#"
use std::env as process_environment;
fn unrelated_set_var(_key: &str, _value: &str) {}
mod inherited;
"#,
        )?;
        fs::write(
            repo.path.join("crates/demo/tests/inherited.rs"),
            r#"
use super::*;

#[test]
fn inherited_environment_alias() {
    unsafe { process_environment::set_var("A", "1"); }
}

#[test]
fn unrelated_inherited_helper_is_not_environment() {
    unrelated_set_var("A", "1");
}
"#,
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/inherited.rs");
        assert_eq!(sites[0].test_function, "inherited_environment_alias");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn external_child_can_glob_an_inherited_environment_module_alias() -> Result<()> {
        let repo = TempRepo::new("external-parent-module-glob")?;
        repo.write_test(
            r#"
use std::env as process_environment;
mod inherited_environment;

mod unrelated_environment {
    pub fn set_var(_key: &str, _value: &str) {}
}
mod inherited_unrelated;
"#,
        )?;
        fs::write(
            repo.path.join("crates/demo/tests/inherited_environment.rs"),
            r#"
use super::process_environment::*;

#[test]
fn inherited_environment_function() {
    unsafe { set_var("A", "1"); }
}
"#,
        )?;
        fs::write(
            repo.path.join("crates/demo/tests/inherited_unrelated.rs"),
            r#"
use super::unrelated_environment::*;

#[test]
fn unrelated_inherited_function() {
    set_var("A", "1");
}
"#,
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/inherited_environment.rs");
        assert_eq!(sites[0].test_function, "inherited_environment_function");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn crate_and_multi_super_imports_use_only_proven_environment_authority() -> Result<()> {
        let repo = TempRepo::new("ancestor-imports")?;
        repo.write_test(
            r#"
use std::env as root_environment;
use std::env::remove_var as root_remove;

mod unrelated_environment {
    pub fn set_var(_key: &str, _value: &str) {}
}

mod outer {
    mod via_crate {
        use crate::{root_environment as process_environment, root_remove as clear_environment};
        use crate::root_environment::set_var as change_environment;

        #[test]
        fn crate_module_alias() {
            unsafe { process_environment::set_var("A", "1"); }
        }

        #[test]
        fn crate_direct_alias() {
            unsafe { clear_environment("B"); }
        }

        #[test]
        fn crate_function_through_module_alias() {
            unsafe { change_environment("B2", "1"); }
        }
    }

    mod via_super_super {
        use super::super::{root_environment as process_environment, root_remove as clear_environment};
        use super::super::root_environment::remove_var as clear_module_environment;

        #[test]
        fn ancestor_module_alias() {
            unsafe { process_environment::set_var("C", "1"); }
        }

        #[test]
        fn ancestor_direct_alias() {
            unsafe { clear_environment("D"); }
        }

        #[test]
        fn ancestor_function_through_module_alias() {
            unsafe { clear_module_environment("D2"); }
        }
    }

    mod unrelated {
        use crate::unrelated_environment as process_environment;

        #[test]
        fn similar_unrelated_module_stays_clean() {
            process_environment::set_var("E", "1");
        }
    }
}
"#,
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        let actual = sites
            .iter()
            .map(|site| (site.test_function.as_str(), site.signals.as_slice()))
            .collect::<BTreeMap<_, _>>();
        let expected = BTreeMap::from([
            ("outer::via_super_super::ancestor_direct_alias", ["env_remove"].as_slice()),
            (
                "outer::via_super_super::ancestor_function_through_module_alias",
                ["env_remove"].as_slice(),
            ),
            ("outer::via_super_super::ancestor_module_alias", ["env_set"].as_slice()),
            ("outer::via_crate::crate_direct_alias", ["env_remove"].as_slice()),
            ("outer::via_crate::crate_function_through_module_alias", ["env_set"].as_slice()),
            ("outer::via_crate::crate_module_alias", ["env_set"].as_slice()),
        ]);
        assert_eq!(actual, expected);
        Ok(())
    }

    #[test]
    fn external_child_of_inline_module_inherits_environment_bindings() -> Result<()> {
        let repo = TempRepo::new("inline-parent-external-child")?;
        repo.write_test(
            r#"
mod parent {
    use std::env as process_environment;
    fn unrelated_set_var(_key: &str, _value: &str) {}
    mod child;
}
"#,
        )?;
        fs::create_dir_all(repo.path.join("crates/demo/tests/parent"))?;
        fs::write(
            repo.path.join("crates/demo/tests/parent/child.rs"),
            r#"
use super::*;

#[test]
fn nested_inherited_environment_alias() {
    unsafe { process_environment::set_var("A", "1"); }
}

#[test]
fn nested_unrelated_helper() {
    unrelated_set_var("A", "1");
}
"#,
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/parent/child.rs");
        assert_eq!(sites[0].test_function, "nested_inherited_environment_alias");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn shared_external_module_is_scanned_in_each_relevant_parent_context() -> Result<()> {
        let repo = TempRepo::new("shared-parent-contexts")?;
        fs::write(
            repo.path.join("crates/demo/tests/a.rs"),
            r#"
mod process_environment {
    pub fn set_var(_key: &str, _value: &str) {}
}
#[path = "shared.rs"]
mod shared;
"#,
        )?;
        fs::write(
            repo.path.join("crates/demo/tests/b.rs"),
            r#"
use std::env as process_environment;
#[path = "shared.rs"]
mod shared;
"#,
        )?;
        fs::write(
            repo.path.join("crates/demo/tests/shared.rs"),
            r#"
use super::*;
#[test]
fn inherited_from_one_parent() {
    unsafe { process_environment::set_var("A", "1"); }
}
"#,
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/shared.rs");
        assert_eq!(sites[0].test_function, "inherited_from_one_parent");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn root_child_path_is_not_ambiguous_with_stem_nested_file() -> Result<()> {
        let repo = TempRepo::new("root-child-path")?;
        repo.write_test(
            r#"
use std::env as process_environment;
mod child;
"#,
        )?;
        fs::write(
            repo.path.join("crates/demo/tests/child.rs"),
            r#"
use super::*;
#[test]
fn root_child_inherits_alias() {
    unsafe { process_environment::set_var("A", "1"); }
}
"#,
        )?;
        fs::create_dir_all(repo.path.join("crates/demo/tests/demo"))?;
        fs::write(
            repo.path.join("crates/demo/tests/demo/child.rs"),
            "#[test]\nfn unrelated_file() {}\n",
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/child.rs");
        assert_eq!(sites[0].test_function, "root_child_inherits_alias");
        Ok(())
    }

    #[test]
    fn same_named_tests_in_inline_modules_have_distinct_identities() -> Result<()> {
        let repo = TempRepo::new("nested-identities")?;
        repo.write_test(
            r#"
mod first {
    #[test]
    fn mutates_environment() {
        unsafe { std::env::set_var("A", "1"); }
    }
}
mod second {
    #[test]
    fn mutates_environment() {
        unsafe { std::env::remove_var("B"); }
    }
}
"#,
        )?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].test_function, "first::mutates_environment");
        assert_eq!(sites[1].test_function, "second::mutates_environment");
        Ok(())
    }

    #[test]
    fn unrelated_paths_bare_calls_and_raw_identifiers_are_ignored() -> Result<()> {
        let repo = TempRepo::new("unrelated-calls")?;
        repo.write_test(
            r#"
mod env { pub fn set_var(_key: &str, _value: &str) {} }
struct Harness;
impl Harness { fn set_var(_key: &str, _value: &str) {} }
fn set_var(_key: &str, _value: &str) {}
fn r#remove_var(_key: &str) {}
#[test]
fn unrelated_calls() {
    env::set_var("A", "1");
    Harness::set_var("A", "1");
    set_var("A", "1");
    r#remove_var("A");
}
"#,
        )?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert!(sites.is_empty());
        Ok(())
    }

    #[test]
    fn block_imports_and_value_shadowing_obey_lexical_scope() -> Result<()> {
        let repo = TempRepo::new("block-scope")?;
        repo.write_test(
            r#"
#[test]
fn block_import_is_detected() {
    { use std::env::set_var as change_env; change_env("A", "1"); }
}
#[test]
fn sibling_block_does_not_inherit_alias() {
    { use std::env::set_var as change_env; }
    {
        fn change_env(_key: &str, _value: &str) {}
        change_env("A", "1");
    }
}
mod shadowed {
    use std::env::set_var as change_env;
    #[test]
    fn local_value_shadows_import() {
        let change_env = |_key: &str, _value: &str| {};
        change_env("A", "1");
    }
}
"#,
        )?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].test_function, "block_import_is_detected");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn block_items_and_unrelated_imports_shadow_inherited_env_aliases() -> Result<()> {
        let repo = TempRepo::new("item-shadow")?;
        repo.write_test(
            r#"
mod inherited_aliases {
    use std::env::{self as process_env, set_var as change_env};

    fn unrelated_call(_key: &str, _value: &str) {}

    #[test]
    fn local_items_are_not_environment_calls() {
        {
            fn change_env(_key: &str, _value: &str) {}
            change_env("A", "1");
        }
        {
            use self::unrelated_call as change_env;
            change_env("A", "1");
        }
        {
            mod process_env {
                pub fn set_var(_key: &str, _value: &str) {}
            }
            process_env::set_var("A", "1");
        }
    }

    #[test]
    fn inherited_aliases_return_after_inner_block() {
        {
            fn change_env(_key: &str, _value: &str) {}
            change_env("A", "1");
        }
        unsafe {
            change_env("B", "2");
            process_env::set_var("C", "3");
        }
    }
}
"#,
        )?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/demo.rs");
        assert_eq!(
            sites[0].test_function,
            "inherited_aliases::inherited_aliases_return_after_inner_block"
        );
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn pattern_bindings_shadow_imports_only_in_their_lexical_scope() -> Result<()> {
        let repo = TempRepo::new("pattern-scope")?;
        repo.write_test(
            r#"
use std::env::set_var as change_env;

#[test]
fn pattern_shadows() {
    if let Some(change_env) = Some(|_key: &str, _value: &str| {})
        && { change_env("A", "1"); true }
    {}

    let mut callbacks = [|_key: &str, _value: &str| {}].into_iter();
    while let Some(change_env) = callbacks.next() {
        change_env("A", "1");
    }

    match Some(|_key: &str, _value: &str| {}) {
        Some(change_env) if { change_env("A", "1"); true } => {}
        Some(_) => {}
        None => {}
    }

    for change_env in [|_key: &str, _value: &str| {}] {
        change_env("A", "1");
    }
}

#[test]
fn imported_alias_remains_visible_after_pattern_scopes() {
    if let Some(change_env) = Some(|_key: &str, _value: &str| {}) {
        change_env("A", "1");
    }
    unsafe { change_env("B", "2"); }
}
"#,
        )?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/demo.rs");
        assert_eq!(sites[0].test_function, "imported_alias_remains_visible_after_pattern_scopes");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn literal_and_comment_signal_tokens_are_ignored() -> Result<()> {
        let repo = TempRepo::new("literal-false-positive")?;
        repo.write_test(
            r####"#[test]
fn only_mentions_parallel_unsafe_tokens() {
    let normal = "} set_var(";
    let bytes = b"} remove_var(";
    let c_string = c"} set_current_dir(";
    let raw = r###"} set_var("###;
    let raw_bytes = br#"} remove_var("#;
    let character = '}';
    let byte_character = b'}';
    // set_var(
    /* outer remove_var( /* nested set_current_dir( */ still comment */
}
"####,
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn every_lexical_class_preserves_a_later_real_signal() -> Result<()> {
        let repo = TempRepo::new("literal-true-positive")?;
        repo.write_test(
            r####"#[test]
fn mutates_after_every_lexical_class() {
    fn borrowed<'a>(value: &'a str) -> &'a str { value }
    let _normal = "\"} set_var(";
    let _bytes = b"\"} remove_var(";
    let _c_string = c"\"} set_current_dir(";
    let _raw = borrowed(r###"}}} set_var("###);
    let _raw_bytes = br###"}}} remove_var("###;
    let _raw_c_string = cr###"}}} set_current_dir("###;
    let _character = '\'';
    let _byte_character = b'\'';
    'scan: loop { break 'scan; }
    // }} set_var(
    /* outer }} remove_var( /* nested set_current_dir( */ still comment */
    std::env::set_var("A", "1");
}
"####,
        )?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/demo.rs");
        assert_eq!(sites[0].test_function, "mutates_after_every_lexical_class");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn multiline_test_attribute_is_detected_without_a_guard() -> Result<()> {
        let repo = TempRepo::new("multiline-test-attribute")?;
        repo.write_test(
            "#[test]\n#[allow(\n    dead_code\n)]\n/* policy explanation */\nfn unguarded() {\n    std::env::set_var(\"A\", \"1\");\n}\n",
        )?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/demo.rs");
        assert_eq!(sites[0].test_function, "unguarded");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn multiline_serial_attribute_guards_an_ordinary_test() -> Result<()> {
        let repo = TempRepo::new("multiline-serial-attribute")?;
        repo.write_test(
            "#[test]\n/* policy explanation */\n#[serial_test::serial(\n)]\nfn guarded() {\n    std::env::set_var(\"A\", \"1\");\n}\n",
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn multiline_file_serial_attribute_does_not_guard_process_state() -> Result<()> {
        let repo = TempRepo::new("multiline-file-serial-attribute")?;
        repo.write_test(
            "#[test]\n/* policy explanation */\n#[serial_test::file_serial(\n)]\nfn guarded() {\n    std::env::set_var(\"A\", \"1\");\n}\n",
        )?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].test_function, "guarded");
        Ok(())
    }

    #[test]
    fn cwd_mutation_requires_serialization() -> Result<()> {
        let repo = TempRepo::new("cwd")?;
        repo.write_test(
            "#[test]\nfn chdirs_into_fixture() {\n    std::env::set_current_dir(\"/tmp\");\n}\n",
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn registered_identity_passes_and_new_still_fails() -> Result<()> {
        let repo = TempRepo::new("registry-mixed")?;
        repo.write_test(concat!(
            "#[test]\nfn legacy_env_flip() {\n    std::env::set_var(\"A\", \"1\");\n}\n",
            "#[test]\nfn fresh_env_flip() {\n    std::env::set_var(\"B\", \"1\");\n}\n",
        ))?;
        let path = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "legacy_env_flip",
                "env_set",
                "tracked #1269 long tail",
                "active"
            )]
        }))?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn repairing_active_row_requires_registry_retirement() -> Result<()> {
        let repo = TempRepo::new("repair")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let active = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_set",
                "tracked #1269 long tail",
                "active"
            )]
        }))?;
        assert_eq!(repo.check(&active)?, 0);

        // The site is repaired with #[serial]; the stale active row now fails.
        let annotated = UNANNOTATED_ENV_TEST.replace("#[test]", "#[test]\n#[serial]");
        repo.write_test(&annotated)?;
        assert_eq!(repo.check(&active)?, 1);

        // Retiring the row restores green and tightens the accepted set.
        let retired = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_set",
                "repaired with #[serial]",
                "retired"
            )]
        }))?;
        assert_eq!(repo.check(&retired)?, 0);
        Ok(())
    }

    #[test]
    fn retired_identity_returning_fails() -> Result<()> {
        let repo = TempRepo::new("retired-return")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let path = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_set",
                "previously repaired",
                "retired"
            )]
        }))?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn serialized_requires_direct_mutation_and_unkeyed_guard() -> Result<()> {
        let repo = TempRepo::new("serialized-contract")?;
        let guarded = UNANNOTATED_ENV_TEST.replace("#[test]", "#[test]\n#[serial]");
        repo.write_test(&guarded)?;
        let path = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs", "flips_toolchain_env", "env_set",
                "serialized fixture", "retired"
            )]
        }))?;
        assert_eq!(repo.check(&path)?, 0);

        let keyed = guarded.replace("#[serial]", "#[serial(keyed)]");
        repo.write_test(&keyed)?;
        assert_eq!(repo.check(&path)?, 1);

        let file_serial = guarded.replace("#[serial]", "#[file_serial]");
        repo.write_test(&file_serial)?;
        assert_eq!(repo.check(&path)?, 1);

        let cwd_guarded = "#[test]\n#[serial]\nfn flips_toolchain_env() {\n    std::env::set_current_dir(\"/tmp\");\n}\n";
        repo.write_test(cwd_guarded)?;
        let cwd_path = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs", "flips_toolchain_env", "cwd",
                "serialized fixture", "retired"
            )]
        }))?;
        assert_eq!(repo.check(&cwd_path)?, 0);

        let cwd_file_serial = cwd_guarded.replace("#[serial]", "#[file_serial]");
        repo.write_test(&cwd_file_serial)?;
        assert_eq!(repo.check(&cwd_path)?, 1);
        Ok(())
    }

    #[test]
    fn eliminated_rejects_returned_mutation() -> Result<()> {
        let repo = TempRepo::new("eliminated-contract")?;
        let mut row = registry_row(
            "crates/demo/tests/demo.rs",
            "flips_toolchain_env",
            "env_set",
            "eliminated fixture",
            "retired",
        );
        row.as_object_mut()
            .ok_or_else(|| eyre!("registry test row was not an object"))?
            .insert("remediation".to_owned(), serde_json::json!("eliminated"));
        let path =
            repo.write_registry(serde_json::json!({ "schema_version": 1, "sites": [row] }))?;
        repo.write_test("#[test]\nfn unrelated() {}\n")?;
        assert_eq!(repo.check(&path)?, 0);
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn mixed_guard_status_preserves_qualified_identity() -> Result<()> {
        let repo = TempRepo::new("mixed-guard-status")?;
        repo.write_test(
            "mod first {\n    #[test]\n    fn same_name() {\n        std::env::set_var(\"A\", \"1\");\n    }\n}\nmod second {\n    #[test]\n    #[serial]\n    fn same_name() {\n        std::env::set_var(\"B\", \"1\");\n    }\n}\n",
        )?;
        let path = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [
                registry_row(
                    "crates/demo/tests/demo.rs",
                    "first::same_name",
                    "env_set",
                    "active fixture",
                    "active",
                ),
                registry_row(
                    "crates/demo/tests/demo.rs",
                    "second::same_name",
                    "env_set",
                    "serialized fixture",
                    "retired",
                ),
            ]
        }))?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn unique_guarded_nested_identity_is_population_independent() -> Result<()> {
        let repo = TempRepo::new("unique-guarded-nested")?;
        repo.write_test(
            "mod nested {\n    #[test]\n    #[serial]\n    fn only_nested() {\n        std::env::set_var(\"A\", \"1\");\n    }\n}\n",
        )?;
        let path = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "nested::only_nested",
                "env_set",
                "serialized fixture",
                "retired",
            )]
        }))?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn legacy_v1_rows_default_remediation_from_state() -> Result<()> {
        let repo = TempRepo::new("legacy-v1-remediation")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let path = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [
                {
                    "path": "crates/demo/tests/demo.rs",
                    "test_function": "flips_toolchain_env",
                    "signals": "env_set",
                    "accepted_reason": "legacy fixture",
                    "state": "active"
                }
            ]
        }))?;
        assert_eq!(repo.check(&path)?, 0);
        let retired = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [{
                "path": "crates/demo/tests/demo.rs",
                "test_function": "flips_toolchain_env",
                "signals": "env_set",
                "accepted_reason": "legacy fixture",
                "state": "retired"
            }]
        }))?;
        assert!(repo.check(&retired).is_err());
        Ok(())
    }

    #[test]
    fn v2_rejects_missing_or_non_string_remediation() -> Result<()> {
        let repo = TempRepo::new("v2-remediation-types")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let values = [
            serde_json::Value::Null,
            serde_json::json!(true),
            serde_json::json!(7),
            serde_json::json!(["pending"]),
            serde_json::json!({"value": "pending"}),
        ];
        for remediation in values {
            let mut row = registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_set",
                "v2 fixture",
                "active",
            );
            row.as_object_mut()
                .ok_or_else(|| eyre!("registry test row was not an object"))?
                .insert("remediation".to_owned(), remediation);
            let path = repo.write_registry(serde_json::json!({
                "schema_version": 2,
                "sites": [row]
            }))?;
            assert!(read_identity_registry(&path).is_err());
        }
        let missing = repo.write_registry(serde_json::json!({
            "schema_version": 2,
            "sites": [{
                "path": "crates/demo/tests/demo.rs",
                "test_function": "flips_toolchain_env",
                "signals": "env_set",
                "accepted_reason": "v2 fixture",
                "state": "active"
            }]
        }))?;
        assert!(read_identity_registry(&missing).is_err());
        Ok(())
    }

    #[test]
    fn legacy_bare_key_does_not_mask_ambiguous_nested_identity() -> Result<()> {
        let repo = TempRepo::new("legacy-ambiguous-bare-key")?;
        repo.write_test(
            "mod first {\n    #[test]\n    fn same_name() { unsafe { std::env::set_var(\"A\", \"1\"); } }\n}\nmod second {\n    #[test]\n    fn same_name() { unsafe { std::env::set_var(\"B\", \"1\"); } }\n}\n",
        )?;
        let path = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "same_name",
                "env_set",
                "legacy fixture",
                "active",
            )]
        }))?;
        let identities = complete_serial_site_inventory(&repo.path)?
            .into_iter()
            .map(|site| site.test_function)
            .collect::<BTreeSet<_>>();
        assert!(identities.contains("first::same_name"));
        assert!(identities.contains("second::same_name"));
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn inline_cfg_test_module_is_scanned() -> Result<()> {
        let repo = TempRepo::new("inline-mod")?;
        fs::write(
            repo.path.join("crates/demo/src/lib.rs"),
            concat!(
                "pub fn identity(value: u8) -> u8 { value }\n",
                "#[cfg(test)]\nmod tests {\n",
                "    #[test]\n    fn inline_env_flip() {\n        std::env::set_var(\"A\", \"1\");\n    }\n",
                "}\n",
            ),
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn every_rust_source_is_scanned_independent_of_cargo_roots() -> Result<()> {
        let repo = TempRepo::new("all-rust-files")?;
        fs::create_dir_all(repo.path.join("crates/demo/custom-target/deep"))?;
        fs::write(
            repo.path.join("crates/demo/custom-target/deep/policy_fixture.rs"),
            UNANNOTATED_ENV_TEST,
        )?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/custom-target/deep/policy_fixture.rs");
        assert_eq!(sites[0].test_function, "flips_toolchain_env");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }

    #[test]
    fn invalid_nested_test_payloads_are_not_treated_as_rust_targets() -> Result<()> {
        let repo = TempRepo::new("fixture-payload")?;
        fs::create_dir_all(repo.path.join("crates/demo/tests/fixtures"))?;
        fs::write(
            repo.path.join("crates/demo/tests/fixtures/perl_sample.rs"),
            "this is fixture text with a Rust-looking extension, not a Rust target",
        )?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;

        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/demo.rs");
        assert_eq!(sites[0].test_function, "flips_toolchain_env");
        Ok(())
    }

    #[test]
    fn production_code_is_not_flagged() -> Result<()> {
        let repo = TempRepo::new("production")?;
        fs::write(
            repo.path.join("crates/demo/src/lib.rs"),
            "pub fn configure(key: &str, value: &str) {\n    unsafe { std::env::set_var(key, value) };\n}\n",
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn formerly_excluded_owned_test_surface_is_scanned() -> Result<()> {
        let repo = TempRepo::new("owned-support")?;
        fs::create_dir_all(repo.path.join("crates/perl-tdd-support/tests"))?;
        fs::write(repo.path.join("crates/perl-tdd-support/tests/runner.rs"), UNANNOTATED_ENV_TEST)?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/perl-tdd-support/tests/runner.rs");
        assert_eq!(sites[0].test_function, "flips_toolchain_env");
        Ok(())
    }

    #[test]
    fn missing_default_registry_is_a_structural_error() -> Result<()> {
        let repo = TempRepo::new("missing-registry")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let mut err_text = String::new();
        if let Err(err) = check_serial_test(&repo.path) {
            err_text = err.to_string();
        }
        assert!(
            err_text.contains("serial identity registry"),
            "expected missing-registry error, got: {err_text}"
        );
        Ok(())
    }

    #[test]
    fn empty_reason_and_duplicate_rows_are_rejected() -> Result<()> {
        let repo = TempRepo::new("invalid-rows")?;
        let empty_reason = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_set",
                " ",
                "active"
            )]
        }))?;
        let mut empty_reason_err = String::new();
        if let Err(err) = read_identity_registry(&empty_reason) {
            empty_reason_err = err.to_string();
        }
        assert!(
            empty_reason_err.contains("accepted_reason"),
            "expected empty-reason error, got: {empty_reason_err}"
        );

        let duplicate = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [
                registry_row(
                    "crates/demo/tests/demo.rs",
                    "flips_toolchain_env",
                    "env_set",
                    "first",
                    "active"
                ),
                registry_row(
                    "crates/demo/tests/demo.rs",
                    "flips_toolchain_env",
                    "env_set",
                    "second",
                    "retired"
                )
            ]
        }))?;
        let mut duplicate_err = String::new();
        if let Err(err) = read_identity_registry(&duplicate) {
            duplicate_err = err.to_string();
        }
        assert!(
            duplicate_err.contains("duplicates"),
            "expected duplicate-row error, got: {duplicate_err}"
        );
        Ok(())
    }

    #[test]
    fn remediation_is_orthogonal_to_registry_state() -> Result<()> {
        let repo = TempRepo::new("remediation-schema")?;
        for (state, remediation) in [("active", "serialized"), ("retired", "pending")] {
            let mut row = registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_set",
                "schema fixture",
                state,
            );
            row.as_object_mut()
                .ok_or_else(|| eyre!("registry test row was not an object"))?
                .insert("remediation".to_owned(), serde_json::json!(remediation));
            let path =
                repo.write_registry(serde_json::json!({ "schema_version": 1, "sites": [row] }))?;
            assert!(read_identity_registry(&path).is_err());
        }
        Ok(())
    }

    #[test]
    fn registry_signals_require_canonical_vocabulary() -> Result<()> {
        let repo = TempRepo::new("invalid-signals")?;
        let unknown = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_sets",
                "typo",
                "active"
            )]
        }))?;
        let unknown_err = read_identity_registry(&unknown)
            .err()
            .ok_or_else(|| eyre!("registry with an unknown signal unexpectedly parsed"))?
            .to_string();
        assert!(unknown_err.contains("unknown signal"));

        let noncanonical = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_set,env_remove,env_set",
                "not sorted and unique",
                "active"
            )]
        }))?;
        let noncanonical_err = read_identity_registry(&noncanonical)
            .err()
            .ok_or_else(|| eyre!("registry with noncanonical signals unexpectedly parsed"))?
            .to_string();
        assert!(noncanonical_err.contains("sorted, unique canonical CSV"));
        Ok(())
    }

    #[test]
    fn registry_rejects_unknown_metadata() -> Result<()> {
        let repo = TempRepo::new("registry-unknown-metadata")?;
        let unknown_root = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [],
            "ignored": true,
        }))?;
        let root_error = read_identity_registry(&unknown_root)
            .err()
            .ok_or_else(|| eyre!("registry with unknown root metadata unexpectedly parsed"))?
            .to_string();
        assert!(root_error.contains("unknown root field"));

        let mut row = registry_row(
            "crates/demo/tests/demo.rs",
            "flips_toolchain_env",
            "env_set",
            "accepted baseline",
            "active",
        );
        row.as_object_mut()
            .ok_or_else(|| eyre!("registry test row was not an object"))?
            .insert("ignored".to_owned(), serde_json::Value::Bool(true));
        let unknown_row = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [row],
        }))?;
        let row_error = read_identity_registry(&unknown_row)
            .err()
            .ok_or_else(|| eyre!("registry row with unknown metadata unexpectedly parsed"))?
            .to_string();
        assert!(row_error.contains("unknown field"));
        Ok(())
    }

    #[test]
    fn active_registry_signals_must_match_current_site() -> Result<()> {
        let repo = TempRepo::new("signal-drift")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let path = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_remove",
                "stale signal census",
                "active"
            )]
        }))?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn inventory_reports_site_identity() -> Result<()> {
        let repo = TempRepo::new("inventory")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/demo.rs");
        assert_eq!(sites[0].test_function, "flips_toolchain_env");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }
}
