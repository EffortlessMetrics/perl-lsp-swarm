use super::model::{
    Discovered, Entrypoint, FileRecord, Instrument, InstrumentStatus, RawDeclaration, RawSite,
    TargetKind, Topology, Vocabulary,
};
use super::normalize_path;
use super::topology::is_complete_test_file;
use super::vocabulary::{macro_family, method_family};
use proc_macro2::LineColumn;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Attribute, Expr, ExprCall, ExprMethodCall, ItemFn, ItemMod, Macro, Meta};

pub(crate) fn scan(
    root: &Path,
    topology: &Topology,
    vocabulary: &Vocabulary,
) -> color_eyre::eyre::Result<Discovered> {
    let mut entrypoints = Vec::new();
    let mut sites = Vec::new();
    let mut declarations = Vec::new();
    let mut instruments = Vec::new();
    let mut extra_files = BTreeSet::new();

    for file in &topology.files {
        match scan_file(root, file, vocabulary, true, false) {
            Ok(mut scanned) => {
                extra_files.extend(scanned.external_modules);
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
    let mut scanned_extra = BTreeSet::new();
    while let Some(path) = pending.pop_first() {
        if !scanned_extra.insert(path.clone()) {
            continue;
        }
        let relative = normalize_path(&path, root);
        let already = topology.files.iter().find(|file| file.path == relative);
        if already.is_some_and(|file| is_complete_test_file(file.target_kind, &file.path)) {
            continue;
        }
        let file = already.cloned().unwrap_or_else(|| FileRecord {
            package: package_from_path(root, &path).unwrap_or_else(|| "unknown".to_string()),
            target_kind: TargetKind::UnitTest,
            path: relative,
            feature: None,
            platform: None,
        });
        match scan_file(root, &file, vocabulary, true, true) {
            Ok(mut scanned) => {
                pending.extend(scanned.external_modules);
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
    Ok(Discovered { entrypoints, sites, declarations, instruments })
}

struct ScannedFile {
    entrypoints: Vec<Entrypoint>,
    sites: Vec<RawSite>,
    declarations: Vec<RawDeclaration>,
    instruments: Vec<Instrument>,
    external_modules: Vec<PathBuf>,
}

fn scan_file(
    root: &Path,
    file: &FileRecord,
    vocabulary: &Vocabulary,
    follow_modules: bool,
    treat_as_test: bool,
) -> Result<ScannedFile, String> {
    let abs = root.join(&file.path);
    let source = std::fs::read_to_string(&abs).map_err(|err| err.to_string())?;
    let lines: Vec<&str> = source.lines().collect();
    let parsed = syn::parse_file(&source).map_err(|err| err.to_string())?;
    let complete = is_complete_test_file(file.target_kind, &file.path);
    let module_dir = abs.parent().map(Path::to_path_buf).unwrap_or_else(|| abs.clone());
    let mut visitor = DebtVisitor {
        file,
        lines: &lines,
        vocabulary,
        follow_modules,
        module_dir,
        in_test: treat_as_test || complete,
        current_fn: "<file>".to_string(),
        current_feature: file.feature.clone(),
        current_platform: file.platform.clone(),
        declaration_stack: Vec::new(),
        entrypoints: Vec::new(),
        sites: Vec::new(),
        declarations: Vec::new(),
        external_modules: Vec::new(),
    };
    visitor.push_attrs(&parsed.attrs, if complete { "file" } else { "crate" });
    visitor.visit_file(&parsed);
    visitor.pop_attrs();
    Ok(ScannedFile {
        entrypoints: visitor.entrypoints,
        sites: visitor.sites,
        declarations: visitor.declarations,
        instruments: Vec::new(),
        external_modules: visitor.external_modules,
    })
}

struct Covering {
    identity: String,
    scope: String,
    owner: String,
    lints: BTreeSet<String>,
}

struct DebtVisitor<'a> {
    file: &'a FileRecord,
    lines: &'a [&'a str],
    vocabulary: &'a Vocabulary,
    follow_modules: bool,
    module_dir: PathBuf,
    in_test: bool,
    current_fn: String,
    current_feature: Option<String>,
    current_platform: Option<String>,
    declaration_stack: Vec<Vec<Covering>>,
    entrypoints: Vec<Entrypoint>,
    sites: Vec<RawSite>,
    declarations: Vec<RawDeclaration>,
    external_modules: Vec<PathBuf>,
}

impl DebtVisitor<'_> {
    fn push_attrs(&mut self, attrs: &[Attribute], scope: &str) {
        let covering =
            attrs.iter().filter_map(|attr| self.declaration_from_attr(attr, scope)).collect();
        self.declaration_stack.push(covering);
    }

    fn pop_attrs(&mut self) {
        self.declaration_stack.pop();
    }

    fn covering_for(&self, family: &str) -> Option<&Covering> {
        let lint = family_lint(family);
        self.declaration_stack.iter().rev().flatten().find(|covering| {
            covering.lints.contains(lint)
                || covering.lints.iter().any(|name| source_lint_matches(name, family))
        })
    }

    fn declaration_from_attr(&mut self, attr: &Attribute, scope: &str) -> Option<Covering> {
        let ident = attr.path().segments.last()?.ident.to_string();
        if ident != "allow" && ident != "expect" && ident != "cfg_attr" {
            return None;
        }
        let Meta::List(list) = &attr.meta else {
            return None;
        };
        let collapsed = collapse(&list.tokens.to_string());
        let cfg_test = ident == "cfg_attr" && is_cfg_test_prefix(&collapsed);
        if ident == "cfg_attr" && !cfg_test && !self.in_test {
            return None;
        }
        let mut lints = BTreeSet::new();
        for lint in &self.vocabulary.lints {
            let needle = collapse(lint);
            if collapsed.contains(&needle) {
                lints.insert(lint.clone());
            }
        }
        if lints.is_empty() {
            return None;
        }
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
        Some(Covering { identity, scope: scope.to_string(), owner, lints })
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
        if cfg_test {
            self.in_test = true;
        }
        self.current_feature = feature;
        self.current_platform = platform;
        self.push_attrs(&node.attrs, "module");
        if node.content.is_some() {
            self.module_dir = inline_module_dir(&self.module_dir, node);
        } else if self.follow_modules
            && self.in_test
            && let Some(path) = outline_module_path(&self.module_dir, node)
        {
            self.external_modules.push(path);
        }
        syn::visit::visit_item_mod(self, node);
        self.pop_attrs();
        self.in_test = previous_test;
        self.current_feature = previous_feature;
        self.current_platform = previous_platform;
        self.module_dir = previous_dir;
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let is_test = node.attrs.iter().any(is_test_attribute);
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
        if is_test {
            self.in_test = true;
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

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if let Some(family) = method_family(&node.method.to_string()) {
            self.record_site(family, node.method.span());
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path) = &*node.func
            && let Some(ident) = path.path.segments.last()
            && let Some(family) = method_family(&ident.ident.to_string())
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
        if let Some(family) = macro_family(&ident.ident.to_string()) {
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
    attr.path().segments.last().is_some_and(|segment| segment.ident == "test")
}

fn attrs_have_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg") && collapse(&attr.meta.to_token_string()).contains("cfg(test")
            || attr.path().is_ident("cfg_attr")
                && is_cfg_test_prefix(&collapse(&attr.meta.to_token_string()))
    })
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

fn is_cfg_test_prefix(collapsed: &str) -> bool {
    collapsed.contains("cfg_attr(test,") || collapsed.contains("cfg_attr(test)")
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
        let resolved = parent_dir.join(path);
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

fn outline_module_path(module_dir: &Path, node: &ItemMod) -> Option<PathBuf> {
    if let Some(path) = path_attribute(node) {
        return Some(module_dir.join(path));
    }
    let name = node.ident.to_string();
    let sibling = module_dir.join(format!("{name}.rs"));
    if sibling.is_file() {
        return Some(sibling);
    }
    let nested = module_dir.join(&name).join("mod.rs");
    nested.is_file().then_some(nested)
}

fn package_from_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut components = relative.components();
    if components.next()?.as_os_str() == "crates" {
        return components.next()?.as_os_str().to_str().map(str::to_string);
    }
    None
}
