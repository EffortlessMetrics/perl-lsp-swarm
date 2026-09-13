use super::model::{
    Discovered, Entrypoint, FileRecord, Instrument, InstrumentStatus, RawDeclaration, RawSite,
    TargetKind, Topology, Vocabulary,
};
use super::topology::is_complete_test_file;
use super::vocabulary::{macro_family, method_family};
use proc_macro2::LineColumn;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Attribute, Expr, ExprCall, ExprMethodCall, ItemFn, ItemMod, Macro, Meta};

#[derive(Clone, Debug)]
struct ModuleWork {
    path: PathBuf,
    treat_as_test: bool,
    package: String,
    target_kind: TargetKind,
    target_name: String,
    feature: Option<String>,
    required_features: Vec<String>,
    platform: Option<String>,
    /// Covering already in force at the `mod` item, including crate-level and
    /// `#[allow]`/`#[expect]` attributes on the outline module itself. rustc
    /// applies those to the child file; a fresh `scan_file` would otherwise
    /// drop them. Later edges to the same path intersect covering identities so
    /// one module's allowance cannot own another edge's sites. A later edge's
    /// `deny` does not erase covering minted on the shared file (`allow`
    /// overrides `deny`); `forbid` cannot be overridden.
    inherited_covering: Vec<CoveringLayer>,
}

pub(crate) fn scan(
    root: &Path,
    topology: &Topology,
    vocabulary: &Vocabulary,
) -> color_eyre::eyre::Result<Discovered> {
    let mut entrypoints = Vec::new();
    let mut sites = Vec::new();
    let mut declarations = Vec::new();
    let mut instruments = Vec::new();
    let mut extra_files = BTreeMap::new();
    let mut covered_paths = BTreeSet::new();

    for file in &topology.files {
        match scan_file(root, file, vocabulary, true, false, Vec::new()) {
            Ok(mut scanned) => {
                covered_paths.insert(file.path.clone());
                for work in scanned.external_modules {
                    enqueue_module(&mut extra_files, work);
                }
                entrypoints.append(&mut scanned.entrypoints);
                sites.append(&mut scanned.sites);
                declarations.append(&mut scanned.declarations);
                instruments.append(&mut scanned.instruments);
            }
            Err(err) => instruments.push(Instrument {
                kind: "source_parse".to_string(),
                subject: file.path.clone(),
                status: InstrumentStatus::NotProven,
                detail: err,
            }),
        }
    }

    let mut pending = extra_files;
    let mut scanned_extra = BTreeMap::new();
    while let Some((path, work)) = pending.pop_first() {
        if already_scanned_with_sufficient_context(&scanned_extra, &path, work.treat_as_test) {
            if let Ok(relative) = super::repo_relative_path(&path, root) {
                restrict_site_covering(&mut sites, &relative, &work.inherited_covering);
            }
            continue;
        }
        let relative = match super::repo_relative_path(&path, root) {
            Ok(relative) => relative,
            Err(detail) => {
                instruments.push(Instrument {
                    kind: "module_path".to_string(),
                    subject: path.to_string_lossy().replace('\\', "/"),
                    status: InstrumentStatus::NotProven,
                    detail,
                });
                continue;
            }
        };
        let already = topology.files.iter().find(|file| file.path == relative);
        if already.is_some_and(|file| is_complete_test_file(file.target_kind, &file.path)) {
            continue;
        }
        let file = already.cloned().unwrap_or_else(|| FileRecord {
            package: work.package.clone(),
            target_kind: work.target_kind,
            path: relative,
            target_name: work.target_name.clone(),
            feature: work.feature.clone(),
            required_features: work.required_features.clone(),
            platform: work.platform.clone(),
        });
        scanned_extra.insert(path.clone(), work.treat_as_test);
        match scan_file(root, &file, vocabulary, true, work.treat_as_test, work.inherited_covering)
        {
            Ok(mut scanned) => {
                covered_paths.insert(file.path.clone());
                for nested in scanned.external_modules {
                    enqueue_module(&mut pending, nested);
                }
                entrypoints.append(&mut scanned.entrypoints);
                sites.append(&mut scanned.sites);
                declarations.append(&mut scanned.declarations);
                instruments.append(&mut scanned.instruments);
            }
            Err(err) => instruments.push(Instrument {
                kind: "source_parse".to_string(),
                subject: file.path,
                status: InstrumentStatus::NotProven,
                detail: err,
            }),
        }
    }

    entrypoints.sort_by(|left, right| (&left.path, &left.name).cmp(&(&right.path, &right.name)));
    entrypoints.dedup_by(|left, right| left.path == right.path && left.name == right.name);
    sites.sort_by(|left, right| {
        (&left.path, left.line, left.column, &left.family, &left.snippet).cmp(&(
            &right.path,
            right.line,
            right.column,
            &right.family,
            &right.snippet,
        ))
    });
    sites.dedup_by(|left, right| {
        left.path == right.path
            && left.line == right.line
            && left.column == right.column
            && left.family == right.family
            && left.snippet == right.snippet
    });
    declarations.sort_by(|left, right| {
        (&left.path, left.line, &left.lint).cmp(&(&right.path, right.line, &right.lint))
    });
    declarations.dedup_by(|left, right| {
        left.path == right.path
            && left.line == right.line
            && left.lint == right.lint
            && left.form == right.form
    });
    Ok(Discovered { entrypoints, sites, declarations, instruments, covered_paths })
}

/// A later `treat_as_test=true` edge must rescan a path first seen as production.
fn already_scanned_with_sufficient_context(
    scanned: &BTreeMap<PathBuf, bool>,
    path: &Path,
    treat_as_test: bool,
) -> bool {
    match scanned.get(path) {
        Some(true) => true,
        Some(false) => !treat_as_test,
        None => false,
    }
}

fn enqueue_module(pending: &mut BTreeMap<PathBuf, ModuleWork>, mut work: ModuleWork) {
    if let Some(normalized) = super::lexically_normalize(&work.path) {
        work.path = normalized;
    }
    if let Some(existing) = pending.get_mut(&work.path) {
        existing.treat_as_test |= work.treat_as_test;
        existing.inherited_covering =
            intersect_covering_layers(&existing.inherited_covering, &work.inherited_covering);
        return;
    }
    pending.insert(work.path.clone(), work);
}

fn covering_identities(layers: &[CoveringLayer]) -> BTreeSet<String> {
    layers.iter().flat_map(|layer| layer.coverings().map(|item| item.identity.clone())).collect()
}

fn denied_lints(layers: &[CoveringLayer]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for layer in layers {
        for op in &layer.ops {
            if let LintAction::Deny(lints) = op {
                out.extend(lints.iter().cloned());
            }
        }
    }
    out
}

fn forbidden_lints(layers: &[CoveringLayer]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for layer in layers {
        for op in &layer.ops {
            if let LintAction::Forbid(lints) = op {
                out.extend(lints.iter().cloned());
            }
        }
    }
    out
}

fn intersect_covering_layers(
    left: &[CoveringLayer],
    right: &[CoveringLayer],
) -> Vec<CoveringLayer> {
    let right_ids = covering_identities(right);
    let mut ops: Vec<LintAction> = left
        .iter()
        .flat_map(|layer| layer.coverings().cloned())
        .filter(|item| right_ids.contains(&item.identity))
        .map(LintAction::Cover)
        .collect();
    let denied = denied_lints(left).union(&denied_lints(right)).cloned().collect::<BTreeSet<_>>();
    let forbidden =
        forbidden_lints(left).union(&forbidden_lints(right)).cloned().collect::<BTreeSet<_>>();
    if !denied.is_empty() {
        ops.push(LintAction::Deny(denied));
    }
    if !forbidden.is_empty() {
        ops.push(LintAction::Forbid(forbidden));
    }
    vec![CoveringLayer { ops }]
}

fn covering_identity_is_local(identity: &str, relative: &str) -> bool {
    identity.strip_prefix(relative).is_some_and(|rest| rest.starts_with(':'))
}

fn restrict_site_covering(sites: &mut [RawSite], relative: &str, inherited: &[CoveringLayer]) {
    let allowed = covering_identities(inherited);
    let denied = denied_lints(inherited);
    let forbidden = forbidden_lints(inherited);
    for site in sites {
        if site.path != relative {
            continue;
        }
        let lint = family_lint(&site.family);
        let keep = site.covering_declaration.as_ref().is_some_and(|identity| {
            let local = covering_identity_is_local(identity, relative);
            if forbidden.contains(lint) {
                return false;
            }
            if local {
                return true;
            }
            allowed.contains(identity) && !denied.contains(lint)
        });
        if !keep {
            site.covering_declaration = None;
            site.covering_scope = None;
            site.covering_owner = None;
        }
    }
}

/// Directory rustc uses to resolve child `mod` items of `file`.
/// `lib.rs` / `main.rs` / `mod.rs` keep children in the parent directory;
/// `foo.rs` resolves children under `foo/`.
fn rustc_child_module_dir(file: &Path) -> PathBuf {
    let parent = file.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    match file.file_name().and_then(|name| name.to_str()) {
        Some("lib.rs" | "main.rs" | "mod.rs") => parent,
        _ => file.file_stem().map(|stem| parent.join(stem)).unwrap_or(parent),
    }
}

struct ScannedFile {
    entrypoints: Vec<Entrypoint>,
    sites: Vec<RawSite>,
    declarations: Vec<RawDeclaration>,
    instruments: Vec<Instrument>,
    external_modules: Vec<ModuleWork>,
}

fn scan_file(
    root: &Path,
    file: &FileRecord,
    vocabulary: &Vocabulary,
    follow_modules: bool,
    treat_as_test: bool,
    inherited_covering: Vec<CoveringLayer>,
) -> Result<ScannedFile, String> {
    let abs = root.join(&file.path);
    let source = std::fs::read_to_string(&abs).map_err(|err| err.to_string())?;
    let lines: Vec<&str> = source.lines().collect();
    let parsed = syn::parse_file(&source).map_err(|err| err.to_string())?;
    let complete = is_complete_test_file(file.target_kind, &file.path);
    let file_dir = abs.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let module_dir = rustc_child_module_dir(&abs);
    let file_cfg_test = attrs_have_cfg_test(&parsed.attrs);
    let mut visitor = DebtVisitor {
        file,
        lines: &lines,
        vocabulary,
        follow_modules,
        file_dir,
        module_dir,
        inside_inline: false,
        in_test: treat_as_test || complete || file_cfg_test,
        current_fn: "<file>".to_string(),
        current_feature: file.feature.clone(),
        current_platform: file.platform.clone(),
        declaration_stack: inherited_covering,
        entrypoints: Vec::new(),
        sites: Vec::new(),
        declarations: Vec::new(),
        external_modules: Vec::new(),
        instruments: Vec::new(),
    };
    visitor.push_attrs(&parsed.attrs, if complete { "file" } else { "crate" });
    visitor.visit_file(&parsed);
    visitor.pop_attrs();
    Ok(ScannedFile {
        entrypoints: visitor.entrypoints,
        sites: visitor.sites,
        declarations: visitor.declarations,
        instruments: visitor.instruments,
        external_modules: visitor.external_modules,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Covering {
    identity: String,
    scope: String,
    owner: String,
    lints: BTreeSet<String>,
}

/// One source-order lint action on a syntax scope.
///
/// rustc applies later attributes over earlier ones, except that `forbid`
/// cannot be lowered. Same-item `deny` then `allow` therefore covers; `allow`
/// then `deny` does not; `forbid` stays sticky.
#[derive(Clone, Debug, PartialEq, Eq)]
enum LintAction {
    Cover(Covering),
    Deny(BTreeSet<String>),
    Forbid(BTreeSet<String>),
}

/// One declaration-stack frame.
///
/// rustc lets an inner `allow`/`expect` override an outer `deny`. `forbid`
/// cannot be weakened by a later attribute, including a child-file inner allow.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CoveringLayer {
    ops: Vec<LintAction>,
}

impl Covering {
    fn covers(&self, lint: &str, family: &str) -> bool {
        self.lints.contains(lint) || self.lints.iter().any(|name| source_lint_matches(name, family))
    }
}

impl CoveringLayer {
    fn coverings(&self) -> impl Iterator<Item = &Covering> {
        self.ops.iter().filter_map(|op| match op {
            LintAction::Cover(covering) => Some(covering),
            _ => None,
        })
    }
}

struct DebtVisitor<'a> {
    file: &'a FileRecord,
    lines: &'a [&'a str],
    vocabulary: &'a Vocabulary,
    follow_modules: bool,
    /// Directory containing the current source file. Non-inline `#[path]` is
    /// relative to this, matching rustc (`tests/via_parent.rs` +
    /// `#[path = "../src/twin.rs"]` → `tests/../src/twin.rs`).
    file_dir: PathBuf,
    module_dir: PathBuf,
    inside_inline: bool,
    in_test: bool,
    current_fn: String,
    current_feature: Option<String>,
    current_platform: Option<String>,
    declaration_stack: Vec<CoveringLayer>,
    entrypoints: Vec<Entrypoint>,
    sites: Vec<RawSite>,
    declarations: Vec<RawDeclaration>,
    external_modules: Vec<ModuleWork>,
    instruments: Vec<Instrument>,
}

impl DebtVisitor<'_> {
    fn push_attrs(&mut self, attrs: &[Attribute], scope: &str) {
        let mut ops = Vec::new();
        for attr in attrs {
            if let Some(covering) = self.declaration_from_attr(attr, scope) {
                ops.push(LintAction::Cover(covering));
            }
            let (denied, forbidden) = self.masks_from_attr(attr);
            if !forbidden.is_empty() {
                ops.push(LintAction::Forbid(forbidden));
            }
            if !denied.is_empty() {
                ops.push(LintAction::Deny(denied));
            }
        }
        self.declaration_stack.push(CoveringLayer { ops });
    }

    fn pop_attrs(&mut self) {
        self.declaration_stack.pop();
    }

    fn covering_for(&self, family: &str) -> Option<&Covering> {
        let lint = family_lint(family);
        let mut found = None;
        let mut forbidden = false;
        for layer in &self.declaration_stack {
            for op in &layer.ops {
                match op {
                    LintAction::Forbid(lints) if lints.contains(lint) => {
                        forbidden = true;
                        found = None;
                    }
                    LintAction::Deny(lints) if lints.contains(lint) && !forbidden => {
                        found = None;
                    }
                    LintAction::Cover(covering) if !forbidden && covering.covers(lint, family) => {
                        found = Some(covering);
                    }
                    _ => {}
                }
            }
        }
        found
    }

    fn declaration_from_attr(&mut self, attr: &Attribute, scope: &str) -> Option<Covering> {
        let ident = attr.path().segments.last()?.ident.to_string();
        if ident != "allow" && ident != "expect" && ident != "cfg_attr" {
            return None;
        }
        let Meta::List(_) = &attr.meta else {
            return None;
        };
        let mentioned = lint_names_from_attr(&ident, &attr.meta);
        let mut lints = BTreeSet::new();
        for lint in &self.vocabulary.lints {
            if mentioned.iter().any(|name| name == lint) {
                lints.insert(lint.clone());
            }
        }
        if lints.is_empty() {
            return None;
        }
        let collapsed = collapse(&attr.meta.to_token_string());
        let owner = extract_owner(&collapsed);
        let snippet = collapse(&attr.meta.to_token_string());
        let line = attr.span().start().line;
        let identity = format!(
            "{}:{}:{ident}:{}",
            self.file.path,
            line,
            lints.iter().cloned().collect::<Vec<_>>().join(",")
        );
        self.declarations.push(RawDeclaration {
            package: self.file.package.clone(),
            target_kind: self.file.target_kind,
            path: self.file.path.clone(),
            entrypoint: self.current_fn.clone(),
            lint: lints.iter().cloned().collect::<Vec<_>>().join(","),
            form: ident.clone(),
            scope: scope.to_string(),
            owner: owner.clone(),
            snippet,
            line,
        });
        let covers = if ident == "cfg_attr" {
            match cfg_attr_cover_kind(&attr.meta) {
                CfgAttrCover::Effective => true,
                CfgAttrCover::Inactive => false,
                CfgAttrCover::NotProven => {
                    self.instruments.push(Instrument {
                        kind: "cfg_attr_cover".to_string(),
                        subject: self.file.path.clone(),
                        status: InstrumentStatus::NotProven,
                        detail: format!(
                            "cfg_attr predicate {collapsed} cannot be established as covering for a test invocation"
                        ),
                    });
                    false
                }
            }
        } else {
            true
        };
        if !covers {
            return None;
        }
        Some(Covering { identity, scope: scope.to_string(), owner, lints })
    }

    fn masks_from_attr(&mut self, attr: &Attribute) -> (BTreeSet<String>, BTreeSet<String>) {
        let Some(ident) = attr.path().segments.last().map(|seg| seg.ident.to_string()) else {
            return (BTreeSet::new(), BTreeSet::new());
        };
        match ident.as_str() {
            "deny" => (self.vocabulary_lints_from_attr(&ident, &attr.meta), BTreeSet::new()),
            "forbid" => (BTreeSet::new(), self.vocabulary_lints_from_attr(&ident, &attr.meta)),
            "cfg_attr" => {
                match cfg_attr_cover_kind(&attr.meta) {
                    CfgAttrCover::Effective => {}
                    CfgAttrCover::Inactive => return (BTreeSet::new(), BTreeSet::new()),
                    CfgAttrCover::NotProven => {
                        self.instruments.push(Instrument {
                            kind: "cfg_attr_cover".to_string(),
                            subject: self.file.path.clone(),
                            status: InstrumentStatus::NotProven,
                            detail: format!(
                                "cfg_attr predicate {} cannot be established as a lint-level mask",
                                collapse(&attr.meta.to_token_string())
                            ),
                        });
                        return (BTreeSet::new(), BTreeSet::new());
                    }
                }
                let mut denied = BTreeSet::new();
                let mut forbidden = BTreeSet::new();
                for inner in cfg_attr_inner_metas(&attr.meta) {
                    let Some(inner_ident) = inner.path().get_ident() else {
                        continue;
                    };
                    let name = inner_ident.to_string();
                    let lints = self.vocabulary_lints_from_attr(&name, &inner);
                    if name == "deny" {
                        denied.extend(lints);
                    } else if name == "forbid" {
                        forbidden.extend(lints);
                    }
                }
                (denied, forbidden)
            }
            _ => (BTreeSet::new(), BTreeSet::new()),
        }
    }

    fn vocabulary_lints_from_attr(&self, ident: &str, meta: &Meta) -> BTreeSet<String> {
        let mentioned = lint_names_from_attr(ident, meta);
        self.vocabulary
            .lints
            .iter()
            .filter(|lint| mentioned.iter().any(|name| name == *lint))
            .cloned()
            .collect()
    }

    fn record_site(&mut self, family: &'static str, span: proc_macro2::Span) {
        if !self.in_test {
            return;
        }
        if family.ends_with('!') {
            if !self.vocabulary.macro_families.contains(family) {
                return;
            }
        } else if !self.vocabulary.method_families.contains(family) {
            return;
        }
        let snippet = normalized_invocation(self.lines, span.start());
        let covering = self.covering_for(family);
        self.sites.push(RawSite {
            package: self.file.package.clone(),
            target_kind: self.file.target_kind,
            path: self.file.path.clone(),
            entrypoint: self.current_fn.clone(),
            family: family.to_string(),
            snippet,
            line: span.start().line,
            column: span.start().column,
            feature: self.current_feature.clone(),
            platform: self.current_platform.clone(),
            covering_declaration: covering.map(|item| item.identity.clone()),
            covering_scope: covering.map(|item| item.scope.clone()),
            covering_owner: covering.map(|item| item.owner.clone()),
        });
    }
}

impl<'ast> Visit<'ast> for DebtVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let cfg_test = attrs_have_cfg_test(&node.attrs);
        let feature = first_feature(&node.attrs).or_else(|| self.current_feature.clone());
        let platform = first_platform(&node.attrs).or_else(|| self.current_platform.clone());
        let previous_test = self.in_test;
        let previous_feature = self.current_feature.clone();
        let previous_platform = self.current_platform.clone();
        let previous_dir = self.module_dir.clone();
        let previous_inline = self.inside_inline;
        if cfg_test {
            self.in_test = true;
        }
        self.current_feature = feature;
        self.current_platform = platform;
        self.push_attrs(&node.attrs, "module");
        if node.content.is_some() {
            self.inside_inline = true;
            self.module_dir = inline_module_dir(&self.module_dir, node);
        } else if self.follow_modules
            && let Some(path) =
                outline_module_path(&self.file_dir, &self.module_dir, self.inside_inline, node)
        {
            self.external_modules.push(ModuleWork {
                path,
                treat_as_test: self.in_test,
                package: self.file.package.clone(),
                target_kind: self.file.target_kind,
                target_name: self.file.target_name.clone(),
                feature: self.current_feature.clone(),
                required_features: self.file.required_features.clone(),
                platform: self.current_platform.clone(),
                inherited_covering: self.declaration_stack.clone(),
            });
        }
        syn::visit::visit_item_mod(self, node);
        self.pop_attrs();
        self.in_test = previous_test;
        self.current_feature = previous_feature;
        self.current_platform = previous_platform;
        self.module_dir = previous_dir;
        self.inside_inline = previous_inline;
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let is_test = node.attrs.iter().any(is_test_attribute);
        let cfg_test = attrs_have_cfg_test(&node.attrs);
        let previous_fn = self.current_fn.clone();
        let previous_test = self.in_test;
        let previous_feature = self.current_feature.clone();
        let previous_platform = self.current_platform.clone();
        self.current_fn = node.sig.ident.to_string();
        if let Some(feature) = first_feature(&node.attrs) {
            self.current_feature = Some(feature);
        }
        if let Some(platform) = first_platform(&node.attrs) {
            self.current_platform = Some(platform);
        }
        if is_test || cfg_test {
            self.in_test = true;
        }
        if is_test {
            self.entrypoints.push(Entrypoint {
                package: self.file.package.clone(),
                target_kind: self.file.target_kind,
                path: self.file.path.clone(),
                name: self.current_fn.clone(),
                feature: self.current_feature.clone(),
                platform: self.current_platform.clone(),
            });
        }
        self.push_attrs(&node.attrs, "item");
        syn::visit::visit_item_fn(self, node);
        self.pop_attrs();
        self.current_fn = previous_fn;
        self.in_test = previous_test;
        self.current_feature = previous_feature;
        self.current_platform = previous_platform;
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let previous_test = self.in_test;
        let previous_feature = self.current_feature.clone();
        let previous_platform = self.current_platform.clone();
        if attrs_have_cfg_test(&node.attrs) {
            self.in_test = true;
        }
        if let Some(feature) = first_feature(&node.attrs) {
            self.current_feature = Some(feature);
        }
        if let Some(platform) = first_platform(&node.attrs) {
            self.current_platform = Some(platform);
        }
        self.push_attrs(&node.attrs, "item");
        syn::visit::visit_item_impl(self, node);
        self.pop_attrs();
        self.in_test = previous_test;
        self.current_feature = previous_feature;
        self.current_platform = previous_platform;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let previous_fn = self.current_fn.clone();
        let previous_test = self.in_test;
        self.current_fn = node.sig.ident.to_string();
        if node.attrs.iter().any(is_test_attribute) || attrs_have_cfg_test(&node.attrs) {
            self.in_test = true;
        }
        self.push_attrs(&node.attrs, "item");
        syn::visit::visit_impl_item_fn(self, node);
        self.pop_attrs();
        self.current_fn = previous_fn;
        self.in_test = previous_test;
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if let Some(family) = method_family(&node.method.to_string())
            && panic_method_shape(family, &node.args)
        {
            self.record_site(family, node.method.span());
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path) = &*node.func
            && let Some(ident) = path.path.segments.last()
            && let Some(family) = method_family(&ident.ident.to_string())
            && panic_call_shape(family, &node.args)
        {
            self.record_site(family, ident.ident.span());
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        self.inspect_macro(&node.mac);
        syn::visit::visit_expr_macro(self, node);
    }

    fn visit_stmt_macro(&mut self, node: &'ast syn::StmtMacro) {
        self.inspect_macro(&node.mac);
        syn::visit::visit_stmt_macro(self, node);
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        self.inspect_macro(&node.mac);
        syn::visit::visit_item_macro(self, node);
    }
}

impl DebtVisitor<'_> {
    fn inspect_macro(&mut self, mac: &Macro) {
        let Some(ident) = mac.path.segments.last() else {
            return;
        };
        let name = ident.ident.to_string();
        if test_generating_macro(&name) {
            self.instruments.push(Instrument {
                kind: "macro_test".to_string(),
                subject: self.file.path.clone(),
                status: InstrumentStatus::NotProven,
                detail: format!("{name}! generates tests that were not expanded"),
            });
        }
        if let Some(family) = macro_family(&name) {
            self.record_site(family, mac.path.span());
        }
    }
}

trait MetaText {
    fn to_token_string(&self) -> String;
}

impl MetaText for Meta {
    fn to_token_string(&self) -> String {
        match self {
            Meta::Path(path) => path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
            Meta::List(list) => format!(
                "{}({})",
                list.path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::"),
                list.tokens
            ),
            Meta::NameValue(nv) => format!(
                "{}=...",
                nv.path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::")
            ),
        }
    }
}

fn is_test_attribute(attr: &Attribute) -> bool {
    if attr.path().segments.last().is_some_and(|segment| segment.ident == "test") {
        return true;
    }
    attr.path().is_ident("cfg_attr") && cfg_attr_applies_test_item(&attr.meta)
}

fn attrs_have_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("cfg") && meta_list_requires_test(&attr.meta))
}

/// `#[cfg_attr(test, test)]` creates a test item. `#[cfg_attr(test, allow(...))]` does not.
fn cfg_attr_applies_test_item(meta: &Meta) -> bool {
    let Meta::List(list) = meta else {
        return false;
    };
    let mut groups = split_top_level_commas(list.tokens.clone());
    let Some(predicate) = groups.first() else {
        return false;
    };
    if !cfg_predicate_requires_test(predicate.clone()) {
        return false;
    }
    groups.drain(..1);
    groups.into_iter().any(|payload| {
        syn::parse2::<Meta>(payload).ok().is_some_and(|meta| {
            meta.path().segments.last().is_some_and(|segment| segment.ident == "test")
        })
    })
}

fn meta_list_requires_test(meta: &Meta) -> bool {
    let Meta::List(list) = meta else {
        return false;
    };
    cfg_predicate_requires_test(list.tokens.clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CfgAttrCover {
    Effective,
    Inactive,
    NotProven,
}

fn cfg_attr_cover_kind(meta: &Meta) -> CfgAttrCover {
    let Meta::List(list) = meta else {
        return CfgAttrCover::NotProven;
    };
    split_top_level_commas(list.tokens.clone())
        .into_iter()
        .next()
        .map(cfg_predicate_cover_kind)
        .unwrap_or(CfgAttrCover::NotProven)
}

fn cfg_predicate_cover_kind(tokens: proc_macro2::TokenStream) -> CfgAttrCover {
    let collapsed = collapse(&tokens.to_string());
    if collapsed == "test" {
        return CfgAttrCover::Effective;
    }
    let Ok(meta) = syn::parse2::<Meta>(tokens) else {
        return if collapsed == "test" { CfgAttrCover::Effective } else { CfgAttrCover::NotProven };
    };
    match meta {
        Meta::Path(path) if path.is_ident("test") => CfgAttrCover::Effective,
        Meta::List(list) if list.path.is_ident("not") => {
            match cfg_predicate_cover_kind(list.tokens) {
                CfgAttrCover::Effective => CfgAttrCover::Inactive,
                _ => CfgAttrCover::NotProven,
            }
        }
        Meta::List(list) if list.path.is_ident("all") => {
            let kinds: Vec<CfgAttrCover> = split_top_level_commas(list.tokens)
                .into_iter()
                .map(cfg_predicate_cover_kind)
                .collect();
            if kinds.contains(&CfgAttrCover::Inactive) {
                CfgAttrCover::Inactive
            } else if !kinds.is_empty() && kinds.iter().all(|kind| *kind == CfgAttrCover::Effective)
            {
                CfgAttrCover::Effective
            } else {
                CfgAttrCover::NotProven
            }
        }
        Meta::List(list) if list.path.is_ident("any") => CfgAttrCover::NotProven,
        _ => CfgAttrCover::NotProven,
    }
}

fn cfg_attr_inner_metas(meta: &Meta) -> Vec<Meta> {
    let Meta::List(list) = meta else {
        return Vec::new();
    };
    split_top_level_commas(list.tokens.clone())
        .into_iter()
        .skip(1)
        .filter_map(|group| syn::parse2::<Meta>(group).ok())
        .collect()
}

fn lint_names_from_attr(ident: &str, meta: &Meta) -> Vec<String> {
    match ident {
        "allow" | "expect" | "deny" | "forbid" => lint_names_from_list_tokens(meta),
        "cfg_attr" => {
            let Meta::List(list) = meta else {
                return Vec::new();
            };
            split_top_level_commas(list.tokens.clone())
                .into_iter()
                .skip(1)
                .filter_map(|group| syn::parse2::<Meta>(group).ok())
                .flat_map(|inner| lint_names_from_list_tokens(&inner))
                .collect()
        }
        _ => Vec::new(),
    }
}

fn lint_names_from_list_tokens(meta: &Meta) -> Vec<String> {
    let Meta::List(list) = meta else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for group in split_top_level_commas(list.tokens.clone()) {
        let Ok(item) = syn::parse2::<Meta>(group) else {
            continue;
        };
        match item {
            Meta::NameValue(nv) if nv.path.is_ident("reason") => {}
            Meta::Path(path) => names.push(meta_path_string(&path)),
            Meta::List(list) => names.push(meta_path_string(&list.path)),
            Meta::NameValue(nv) => names.push(meta_path_string(&nv.path)),
        }
    }
    names
}

fn meta_path_string(path: &syn::Path) -> String {
    path.segments.iter().map(|segment| segment.ident.to_string()).collect::<Vec<_>>().join("::")
}

fn cfg_predicate_requires_test(tokens: proc_macro2::TokenStream) -> bool {
    let collapsed = collapse(&tokens.to_string());
    if collapsed == "test" {
        return true;
    }
    let Ok(meta) = syn::parse2::<Meta>(tokens) else {
        return collapsed == "test";
    };
    match meta {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) if list.path.is_ident("all") => {
            split_top_level_commas(list.tokens).into_iter().any(cfg_predicate_requires_test)
        }
        Meta::List(list) if list.path.is_ident("any") || list.path.is_ident("not") => false,
        _ => false,
    }
}

fn split_top_level_commas(tokens: proc_macro2::TokenStream) -> Vec<proc_macro2::TokenStream> {
    let mut groups = vec![proc_macro2::TokenStream::new()];
    for tree in tokens {
        if let proc_macro2::TokenTree::Punct(punct) = &tree
            && punct.as_char() == ','
        {
            groups.push(proc_macro2::TokenStream::new());
            continue;
        }
        if let Some(current) = groups.last_mut() {
            current.extend(std::iter::once(tree));
        }
    }
    groups.into_iter().filter(|group| !group.is_empty()).collect()
}

fn panic_method_shape(
    family: &str,
    args: &syn::punctuated::Punctuated<Expr, syn::token::Comma>,
) -> bool {
    match family {
        "unwrap" | "unwrap_err" => args.is_empty(),
        "expect" | "expect_err" => {
            let Some(first) = args.first() else {
                return false;
            };
            if is_panic_message_expr(first) {
                return true;
            }
            if is_custom_typed_expect_arg(first) {
                return false;
            }
            args.len() == 1
        }
        _ => true,
    }
}

fn panic_call_shape(
    family: &str,
    args: &syn::punctuated::Punctuated<Expr, syn::token::Comma>,
) -> bool {
    match family {
        "unwrap" | "unwrap_err" => args.len() <= 1,
        "expect" | "expect_err" => args.iter().any(is_panic_message_expr),
        _ => true,
    }
}

fn is_panic_message_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(lit) if matches!(lit.lit, syn::Lit::Str(_)) => true,
        Expr::Macro(mac) => {
            let name = mac
                .mac
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default();
            matches!(name.as_str(), "format" | "concat" | "format_args")
        }
        Expr::Reference(reference) => is_panic_message_expr(&reference.expr),
        Expr::Paren(paren) => is_panic_message_expr(&paren.expr),
        _ => false,
    }
}

/// Multi-segment paths such as `TokenType::Ident` are the custom-method
/// expect shape this inventory must keep out of `clippy::expect_used`.
/// Single-segment idents (`msg`) are treated as Option/Result messages.
fn is_custom_typed_expect_arg(expr: &Expr) -> bool {
    match expr {
        Expr::Path(path) => path.path.segments.len() > 1,
        Expr::Reference(reference) => is_custom_typed_expect_arg(&reference.expr),
        Expr::Paren(paren) => is_custom_typed_expect_arg(&paren.expr),
        _ => false,
    }
}

fn first_feature(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("cfg") && !attr.path().is_ident("cfg_attr") {
            continue;
        }
        let collapsed = collapse(&attr.meta.to_token_string());
        if let Some(feature) = extract_quoted_after(&collapsed, "feature=") {
            return Some(feature);
        }
    }
    None
}

fn first_platform(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("cfg") {
            continue;
        }
        let collapsed = collapse(&attr.meta.to_token_string());
        for platform in ["windows", "unix", "linux", "macos", "target_os"] {
            if collapsed.contains(platform) {
                return Some(platform.to_string());
            }
        }
    }
    None
}

fn extract_quoted_after(text: &str, prefix: &str) -> Option<String> {
    let start = text.find(prefix)? + prefix.len();
    let rest = text.get(start..)?;
    let rest = rest.trim_start_matches(['"', ' ']);
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_owner(collapsed: &str) -> String {
    if let Some(hash) = collapsed.find('#') {
        let rest = collapsed.get(hash..).unwrap_or("");
        let id: String = rest.chars().skip(1).take_while(|ch| ch.is_ascii_digit()).collect();
        if !id.is_empty() {
            return format!("#{id}");
        }
    }
    if collapsed.contains("reason=")
        && let Some(reason) = extract_quoted_after(collapsed, "reason=")
    {
        return reason;
    }
    String::new()
}

fn collapse(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn family_lint(family: &str) -> &'static str {
    match family {
        "unwrap" | "unwrap_err" => "clippy::unwrap_used",
        "expect" | "expect_err" => "clippy::expect_used",
        "panic!" => "clippy::panic",
        "todo!" => "clippy::todo",
        "unimplemented!" => "clippy::unimplemented",
        "dbg!" => "clippy::dbg_macro",
        "unreachable!" => "clippy::unreachable",
        _ => "",
    }
}

fn source_lint_matches(lint: &str, family: &str) -> bool {
    family_lint(family) == lint
}

fn normalized_invocation(lines: &[&str], start: LineColumn) -> String {
    let line_index = start.line.saturating_sub(1);
    let mut text = String::new();
    let mut depth = 0usize;
    let mut started = false;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, line) in lines.iter().enumerate().skip(line_index).take(16) {
        let scan_fragment =
            if offset == line_index { line.get(start.column..).unwrap_or("") } else { line };
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(line);
        for ch in scan_fragment.chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            if ch == '"' {
                in_string = true;
                continue;
            }
            if ch == '(' || ch == '{' {
                started = true;
                depth += 1;
            } else if (ch == ')' || ch == '}') && started {
                depth = depth.saturating_sub(1);
            }
        }
        if started && depth == 0 {
            break;
        }
    }

    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn path_attribute(node: &ItemMod) -> Option<String> {
    for attr in &node.attrs {
        if attr.path().is_ident("path")
            && let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(value), .. }) = &nv.value
        {
            return Some(value.value());
        }
    }
    None
}

fn inline_module_dir(parent_dir: &Path, node: &ItemMod) -> PathBuf {
    if let Some(path) = path_attribute(node) {
        let joined = parent_dir.join(path);
        let resolved = super::lexically_normalize(&joined).unwrap_or(joined);
        if resolved.is_file() {
            return resolved
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| parent_dir.to_path_buf());
        }
        return resolved;
    }
    parent_dir.join(node.ident.to_string())
}

fn outline_module_path(
    file_dir: &Path,
    module_dir: &Path,
    inside_inline: bool,
    node: &ItemMod,
) -> Option<PathBuf> {
    if let Some(path) = path_attribute(node) {
        // rustc: non-inline #[path] is relative to the source file's directory;
        // inside an inline module it follows rustc_child_module_dir + inline components.
        let base = if inside_inline { module_dir } else { file_dir };
        let joined = base.join(path);
        return Some(super::lexically_normalize(&joined).unwrap_or(joined));
    }
    let name = node.ident.to_string();
    let sibling = module_dir.join(format!("{name}.rs"));
    if sibling.is_file() {
        return Some(sibling);
    }
    let nested = module_dir.join(&name).join("mod.rs");
    nested.is_file().then_some(nested)
}

fn test_generating_macro(name: &str) -> bool {
    matches!(name, "proptest")
}

#[cfg(test)]
mod tests {
    use super::super::model::{RawSite, TargetKind};
    use super::*;

    fn module_work(path: &str, treat_as_test: bool) -> ModuleWork {
        ModuleWork {
            path: PathBuf::from(path),
            treat_as_test,
            package: "demo".to_string(),
            target_kind: TargetKind::UnitTest,
            target_name: "demo".to_string(),
            feature: None,
            required_features: Vec::new(),
            platform: None,
            inherited_covering: Vec::new(),
        }
    }

    fn covering(identity: &str) -> Covering {
        Covering {
            identity: identity.to_string(),
            scope: "crate".to_string(),
            owner: "#13397".to_string(),
            lints: ["clippy::unwrap_used".to_string()].into_iter().collect(),
        }
    }

    fn layer_with(identity: &str) -> CoveringLayer {
        CoveringLayer { ops: vec![LintAction::Cover(covering(identity))] }
    }

    fn deny_only(lint: &str) -> CoveringLayer {
        CoveringLayer { ops: vec![LintAction::Deny(std::iter::once(lint.to_string()).collect())] }
    }

    fn forbid_only(lint: &str) -> CoveringLayer {
        CoveringLayer { ops: vec![LintAction::Forbid(std::iter::once(lint.to_string()).collect())] }
    }

    #[test]
    fn rustc_child_module_dir_follows_lib_and_outline_files() {
        assert_eq!(
            rustc_child_module_dir(Path::new("crates/demo/src/lib.rs")),
            PathBuf::from("crates/demo/src")
        );
        assert_eq!(
            rustc_child_module_dir(Path::new("crates/demo/src/foo.rs")),
            PathBuf::from("crates/demo/src/foo")
        );
        assert_eq!(
            rustc_child_module_dir(Path::new("crates/demo/src/foo/mod.rs")),
            PathBuf::from("crates/demo/src/foo")
        );
        let node: ItemMod =
            syn::parse_str("#[path = \"../src/twin.rs\"]\nmod twin;").expect("path attr module");
        let resolved = outline_module_path(
            Path::new("/tmp/a/crates/demo/tests"),
            Path::new("/tmp/a/crates/demo/tests/via_parent"),
            false,
            &node,
        )
        .expect("outline path");
        assert_eq!(resolved, PathBuf::from("/tmp/a/crates/demo/src/twin.rs"));
    }

    #[test]
    fn enqueue_module_or_treat_as_test_and_keeps_first_identity() {
        let mut pending = BTreeMap::new();
        let mut first = module_work("src/foo.rs", false);
        first.inherited_covering = vec![layer_with("lib.rs:1:allow:clippy::unwrap_used")];
        let mut later = module_work("src/bar/../foo.rs", true);
        later.inherited_covering = vec![layer_with("lib.rs:3:allow:clippy::unwrap_used")];
        enqueue_module(&mut pending, first);
        enqueue_module(&mut pending, later);
        let work = pending.get(&PathBuf::from("src/foo.rs"));
        assert!(work.is_some_and(|item| item.treat_as_test && item.package == "demo"));
        assert_eq!(
            work.map(|item| item
                .inherited_covering
                .iter()
                .map(|layer| layer.coverings().count())
                .sum::<usize>()),
            Some(0),
            "distinct outline edges must intersect covering, not union one edge's allowance onto the other"
        );
        assert_eq!(pending.len(), 1);
        let mut scanned = BTreeMap::new();
        scanned.insert(PathBuf::from("src/shared.rs"), false);
        assert!(!already_scanned_with_sufficient_context(
            &scanned,
            Path::new("src/shared.rs"),
            true
        ));
        assert!(already_scanned_with_sufficient_context(
            &scanned,
            Path::new("src/shared.rs"),
            false
        ));
        scanned.insert(PathBuf::from("src/shared.rs"), true);
        assert!(already_scanned_with_sufficient_context(
            &scanned,
            Path::new("src/shared.rs"),
            true
        ));
        let scanned = ScannedFile {
            entrypoints: Vec::new(),
            sites: Vec::new(),
            declarations: Vec::new(),
            instruments: Vec::new(),
            external_modules: vec![module_work("src/bar.rs", true)],
        };
        assert_eq!(scanned.external_modules.len(), 1);
        let covering = Covering {
            identity: "allow:clippy::unwrap_used".to_string(),
            scope: "fn".to_string(),
            owner: "#13397".to_string(),
            lints: ["clippy::unwrap_used".to_string()].into_iter().collect(),
        };
        assert_eq!(covering.owner, "#13397");
        assert!(test_generating_macro("proptest"));
        assert!(!test_generating_macro("test"));
        assert_eq!(family_lint("unwrap"), "clippy::unwrap_used");
        assert_eq!(family_lint("expect"), "clippy::expect_used");
        assert_eq!(family_lint("panic!"), "clippy::panic");
        assert_eq!(family_lint("todo!"), "clippy::todo");
        assert_eq!(family_lint("unimplemented!"), "clippy::unimplemented");
        assert_eq!(family_lint("dbg!"), "clippy::dbg_macro");
        assert_eq!(family_lint("unreachable!"), "clippy::unreachable");
        assert!(family_lint("unknown").is_empty());
    }

    #[test]
    fn cfg_attr_test_item_is_an_entrypoint_and_allow_is_not() -> color_eyre::eyre::Result<()> {
        let test_item = syn::parse_file("#[cfg_attr(test, test)]\nfn t() {}")
            .map_err(|err| color_eyre::eyre::eyre!("parse test item: {err}"))?;
        let allow_item =
            syn::parse_file("#[cfg_attr(test, allow(clippy::unwrap_used))]\nfn t() {}")
                .map_err(|err| color_eyre::eyre::eyre!("parse allow item: {err}"))?;
        let tokio_item = syn::parse_file("#[cfg_attr(test, tokio::test)]\nfn t() {}")
            .map_err(|err| color_eyre::eyre::eyre!("parse tokio item: {err}"))?;
        let any_cfg = syn::parse_file("#[cfg(any(test, feature = \"x\"))]\nfn t() {}")
            .map_err(|err| color_eyre::eyre::eyre!("parse any cfg: {err}"))?;
        let all_cfg = syn::parse_file("#[cfg(all(test, feature = \"x\"))]\nfn t() {}")
            .map_err(|err| color_eyre::eyre::eyre!("parse all cfg: {err}"))?;
        let test_attr = match test_item.items.as_slice() {
            [syn::Item::Fn(func)] => func.attrs.first(),
            _ => None,
        }
        .ok_or_else(|| color_eyre::eyre::eyre!("missing test attr"))?;
        let allow_attr = match allow_item.items.as_slice() {
            [syn::Item::Fn(func)] => func.attrs.first(),
            _ => None,
        }
        .ok_or_else(|| color_eyre::eyre::eyre!("missing allow attr"))?;
        let tokio_attr = match tokio_item.items.as_slice() {
            [syn::Item::Fn(func)] => func.attrs.first(),
            _ => None,
        }
        .ok_or_else(|| color_eyre::eyre::eyre!("missing tokio attr"))?;
        assert!(is_test_attribute(test_attr));
        assert!(!is_test_attribute(allow_attr));
        assert!(is_test_attribute(tokio_attr));
        let any_fn = match any_cfg.items.as_slice() {
            [syn::Item::Fn(func)] => func,
            _ => return Err(color_eyre::eyre::eyre!("missing any-cfg fn")),
        };
        let all_fn = match all_cfg.items.as_slice() {
            [syn::Item::Fn(func)] => func,
            _ => return Err(color_eyre::eyre::eyre!("missing all-cfg fn")),
        };
        assert!(!attrs_have_cfg_test(&any_fn.attrs));
        assert!(attrs_have_cfg_test(&all_fn.attrs));
        let not_test =
            syn::parse_file("#[cfg_attr(not(test), allow(clippy::unwrap_used))]\nfn t() {}")
                .map_err(|err| color_eyre::eyre::eyre!("parse not-test cfg_attr: {err}"))?;
        let not_attr = match not_test.items.as_slice() {
            [syn::Item::Fn(func)] => func.attrs.first(),
            _ => None,
        }
        .ok_or_else(|| color_eyre::eyre::eyre!("missing not-test cfg_attr"))?;
        assert_eq!(cfg_attr_cover_kind(&not_attr.meta), CfgAttrCover::Inactive);
        assert_eq!(cfg_attr_cover_kind(&allow_attr.meta), CfgAttrCover::Effective);
        let feature_item = syn::parse_file(
            "#[cfg_attr(feature = \"need-me\", allow(clippy::unwrap_used))]\nfn t() {}",
        )
        .map_err(|err| color_eyre::eyre::eyre!("parse feature cfg_attr: {err}"))?;
        let feature_attr = match feature_item.items.as_slice() {
            [syn::Item::Fn(func)] => func.attrs.first(),
            _ => None,
        }
        .ok_or_else(|| color_eyre::eyre::eyre!("missing feature cfg_attr"))?;
        assert_eq!(cfg_attr_cover_kind(&feature_attr.meta), CfgAttrCover::NotProven);
        let prefix = syn::parse_file("#[allow(clippy::panic_in_result_fn)]\nfn t() {}")
            .map_err(|err| color_eyre::eyre::eyre!("parse prefix allow: {err}"))?;
        let prefix_attr = match prefix.items.as_slice() {
            [syn::Item::Fn(func)] => func.attrs.first(),
            _ => None,
        }
        .ok_or_else(|| color_eyre::eyre::eyre!("missing prefix allow"))?;
        let panic_item = syn::parse_file("#[allow(clippy::panic)]\nfn t() {}")
            .map_err(|err| color_eyre::eyre::eyre!("parse panic allow: {err}"))?;
        let panic_attr = match panic_item.items.as_slice() {
            [syn::Item::Fn(func)] => func.attrs.first(),
            _ => None,
        }
        .ok_or_else(|| color_eyre::eyre::eyre!("missing panic allow"))?;
        assert_eq!(
            lint_names_from_attr("allow", &prefix_attr.meta),
            vec!["clippy::panic_in_result_fn".to_string()]
        );
        assert_eq!(
            lint_names_from_attr("allow", &panic_attr.meta),
            vec!["clippy::panic".to_string()]
        );
        assert_eq!(
            lint_names_from_attr("cfg_attr", &allow_attr.meta),
            vec!["clippy::unwrap_used".to_string()]
        );
        Ok(())
    }

    #[test]
    fn restrict_site_covering_keeps_local_allow_and_strips_foreign() {
        let local = "src/shared.rs:1:allow:clippy::unwrap_used";
        let foreign = "src/aaa.rs:2:allow:clippy::unwrap_used";
        assert!(covering_identity_is_local(local, "src/shared.rs"));
        assert!(!covering_identity_is_local(foreign, "src/shared.rs"));
        assert!(!covering_identity_is_local(
            "src/shared.rs.bak:1:allow:clippy::unwrap_used",
            "src/shared.rs"
        ));

        let local_site = |identity: &str| RawSite {
            package: "demo".to_string(),
            target_kind: TargetKind::UnitTest,
            path: "src/shared.rs".to_string(),
            entrypoint: "unit".to_string(),
            family: "unwrap".to_string(),
            snippet: "unwrap()".to_string(),
            line: 4,
            column: 1,
            feature: None,
            platform: None,
            covering_declaration: Some(identity.to_string()),
            covering_scope: Some("crate".to_string()),
            covering_owner: Some("#13397".to_string()),
        };
        let mut kept = vec![local_site(local)];
        restrict_site_covering(&mut kept, "src/shared.rs", &[]);
        assert_eq!(kept[0].covering_declaration.as_deref(), Some(local));
        assert_eq!(kept[0].covering_owner.as_deref(), Some("#13397"));

        let mut stripped = vec![local_site(foreign)];
        restrict_site_covering(&mut stripped, "src/shared.rs", &[]);
        assert!(stripped[0].covering_declaration.is_none());

        let mut denied = vec![local_site(local)];
        restrict_site_covering(&mut denied, "src/shared.rs", &[deny_only("clippy::unwrap_used")]);
        assert_eq!(
            denied[0].covering_declaration.as_deref(),
            Some(local),
            "later-edge deny must not erase a shared-file local allow"
        );

        let mut forbidden = vec![local_site(local)];
        restrict_site_covering(
            &mut forbidden,
            "src/shared.rs",
            &[forbid_only("clippy::unwrap_used")],
        );
        assert!(
            forbidden[0].covering_declaration.is_none(),
            "later-edge forbid must still mask local allow"
        );

        let mut inherited_denied = vec![local_site(foreign)];
        restrict_site_covering(
            &mut inherited_denied,
            "src/shared.rs",
            &[CoveringLayer {
                ops: vec![
                    LintAction::Cover(covering(foreign)),
                    LintAction::Deny(std::iter::once("clippy::unwrap_used".to_string()).collect()),
                ],
            }],
        );
        assert!(
            inherited_denied[0].covering_declaration.is_none(),
            "later-edge deny must still strip inherited-only covering"
        );
    }

    fn method_call(src: &str) -> color_eyre::eyre::Result<ExprMethodCall> {
        match syn::parse_str::<Expr>(src)
            .map_err(|err| color_eyre::eyre::eyre!("parse {src}: {err}"))?
        {
            Expr::MethodCall(call) => Ok(call),
            other => Err(color_eyre::eyre::eyre!("expected method call, got {other:?}")),
        }
    }

    #[test]
    fn panic_method_shape_keeps_nonliteral_expect_and_excludes_typed_custom_args()
    -> color_eyre::eyre::Result<()> {
        let literal = method_call(r#"x.expect("msg")"#)?;
        let variable = method_call("x.expect(msg)")?;
        let custom = method_call("x.expect(TokenType::Ident)")?;
        let format_macro = method_call(r#"x.expect(format!("x"))"#)?;
        assert!(panic_method_shape("expect", &literal.args));
        assert!(panic_method_shape("expect", &variable.args));
        assert!(!panic_method_shape("expect", &custom.args));
        assert!(panic_method_shape("expect", &format_macro.args));
        let expect_err = method_call("x.expect_err(msg)")?;
        assert!(panic_method_shape("expect_err", &expect_err.args));
        Ok(())
    }
}
