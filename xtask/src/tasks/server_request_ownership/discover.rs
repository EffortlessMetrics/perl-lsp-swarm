//! Discovery of the three surfaces the ownership matrix is joined against.
//!
//! Discovery is source-derived rather than hand-listed so a newly added request
//! shows up here without anyone remembering to extend a list. The direction
//! registry stays the classification authority; this module only reads it.
//!
//! Rust source is read with `syn`, not with hand-written scanners. Four review
//! rounds each found a fail-open in the scanners this replaced — a bounded line
//! window, a first-`*/` comment skip, a literal `#[cfg(test)]` match, `fn ` and
//! send sites read out of string literals — and the fifth round would have
//! found the next one. A parser removes that whole class by construction: it
//! never sees a comment, never mistakes a string for code, and reads a `cfg`
//! predicate as a predicate. `syn` is already an `xtask` dependency with
//! `full`/`parsing`/`visit`, used by four other tasks here.
//!
//! What remains a judgement rather than a fact is narrow and stated: which
//! call names count as sending a request, and which helper counts as
//! forwarding one. Both fail closed — a send this module cannot attribute to a
//! resolvable method is a finding, never a silent skip, and a file it cannot
//! parse is an instrument failure rather than an empty file.

use super::model::{CatalogRow, Discovered, RegistryKind, Violation};
use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use syn::visit::Visit;

/// Method names whose call emits a server-initiated request.
const REQUEST_SENDERS: &[&str] = &["send_request", "send_request_internal"];

/// A helper that takes the method from its caller is the only admitted
/// forwarding shape, and only when its own body reaches a sender.
const FORWARDED_METHOD_PARAM: &str = "method";

// ── cfg predicates ───────────────────────────────────────────────────────

/// Whether a `cfg` predicate can only hold under `cfg(test)`.
///
/// `all(test, feature = "x")` is test-only — it cannot compile in production —
/// so its items carry no production emission. `any(test, feature = "x")` can,
/// so its items stay in the denominator. Anything else, `not(..)` and bare
/// feature gates included, is treated as reachable: over-stripping would shrink
/// the denominator silently, which is the failure direction to avoid.
fn meta_is_test_only(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) => {
            let Ok(nested) = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return false;
            };
            if list.path.is_ident("all") {
                nested.iter().any(meta_is_test_only)
            } else if list.path.is_ident("any") {
                !nested.is_empty() && nested.iter().all(meta_is_test_only)
            } else {
                false
            }
        }
        syn::Meta::NameValue(_) => false,
    }
}

/// Whether an item's attributes gate it to `cfg(test)`.
fn is_test_gated(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr.parse_args::<syn::Meta>().is_ok_and(|meta| meta_is_test_only(&meta))
    })
}

// ── Expression shapes ────────────────────────────────────────────────────

/// The string a literal expression is, if it is one.
fn string_literal(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Lit(lit) => match &lit.lit {
            syn::Lit::Str(value) => Some(value.value()),
            _ => None,
        },
        // A borrowed or parenthesised literal is the same literal.
        syn::Expr::Reference(inner) => string_literal(&inner.expr),
        syn::Expr::Paren(inner) => string_literal(&inner.expr),
        syn::Expr::Group(inner) => string_literal(&inner.expr),
        _ => None,
    }
}

/// The final path segment of a path expression, if it is one.
fn path_tail(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(path) => path.path.segments.last().map(|segment| segment.ident.to_string()),
        syn::Expr::Reference(inner) => path_tail(&inner.expr),
        syn::Expr::Paren(inner) => path_tail(&inner.expr),
        syn::Expr::Group(inner) => path_tail(&inner.expr),
        _ => None,
    }
}

/// Whether an expression is exactly the forwarded method parameter.
fn is_forwarded_parameter(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Path(path) if path.path.is_ident(FORWARDED_METHOD_PARAM))
}

/// Whether a signature declares the forwarded method parameter: named
/// `method`, typed `&str`.
///
/// Matched structurally rather than as text. Testing the signature for
/// `"method: &str"` also matched `other_method: &str`, which forwards nothing
/// of the kind.
fn declares_forwarded_method(signature: &syn::Signature) -> bool {
    signature.inputs.iter().any(|input| {
        let syn::FnArg::Typed(typed) = input else { return false };
        let syn::Pat::Ident(ident) = &*typed.pat else { return false };
        if ident.ident != FORWARDED_METHOD_PARAM {
            return false;
        }
        let syn::Type::Reference(reference) = &*typed.ty else { return false };
        matches!(&*reference.elem, syn::Type::Path(path) if path.path.is_ident("str"))
    })
}

// ── Function facts ───────────────────────────────────────────────────────

/// One call site inside a function.
struct CallSite {
    /// The name being invoked: a method name, or a path's final segment.
    callee: String,
    /// The method the call names directly, if any argument is a string literal.
    literal: Option<String>,
    /// Final path segments of the call's arguments, for constant resolution.
    paths: Vec<String>,
    /// Whether an argument is exactly the forwarded method parameter.
    forwards_parameter: bool,
}

/// One function's facts.
struct FnFacts {
    name: String,
    forwards_method: bool,
    sites: Vec<CallSite>,
}

/// Collect every production function in one parsed file, with its call sites.
///
/// Test-gated items are not descended into, so a test-only send is never
/// production emission. Because this walks the parsed tree, a `fn` or a send
/// written inside a string literal or a comment does not exist to be found.
#[derive(Default)]
struct FnCollector {
    functions: Vec<FnFacts>,
    stack: Vec<usize>,
}

impl FnCollector {
    fn enter(&mut self, name: String, signature: &syn::Signature) {
        self.functions.push(FnFacts {
            name,
            forwards_method: declares_forwarded_method(signature),
            sites: Vec::new(),
        });
        self.stack.push(self.functions.len() - 1);
    }

    fn record(&mut self, site: CallSite) {
        if let Some(index) = self.stack.last().copied() {
            self.functions[index].sites.push(site);
        }
    }
}

impl<'ast> Visit<'ast> for FnCollector {
    fn visit_item(&mut self, node: &'ast syn::Item) {
        let attrs: &[syn::Attribute] = match node {
            syn::Item::Fn(item) => &item.attrs,
            syn::Item::Mod(item) => &item.attrs,
            syn::Item::Impl(item) => &item.attrs,
            syn::Item::Trait(item) => &item.attrs,
            syn::Item::Macro(item) => &item.attrs,
            _ => &[],
        };
        if is_test_gated(attrs) {
            return;
        }
        syn::visit::visit_item(self, node);
    }

    fn visit_impl_item(&mut self, node: &'ast syn::ImplItem) {
        let attrs: &[syn::Attribute] = match node {
            syn::ImplItem::Fn(item) => &item.attrs,
            syn::ImplItem::Const(item) => &item.attrs,
            _ => &[],
        };
        if is_test_gated(attrs) {
            return;
        }
        syn::visit::visit_impl_item(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.enter(node.sig.ident.to_string(), &node.sig);
        syn::visit::visit_item_fn(self, node);
        self.stack.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.enter(node.sig.ident.to_string(), &node.sig);
        syn::visit::visit_impl_item_fn(self, node);
        self.stack.pop();
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.enter(node.sig.ident.to_string(), &node.sig);
        syn::visit::visit_trait_item_fn(self, node);
        self.stack.pop();
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let mut literal = None;
        let mut paths = Vec::new();
        let mut forwards_parameter = false;
        for arg in &node.args {
            if literal.is_none() {
                literal = string_literal(arg);
            }
            if let Some(tail) = path_tail(arg) {
                paths.push(tail);
            }
            forwards_parameter |= is_forwarded_parameter(arg);
        }
        self.record(CallSite {
            callee: node.method.to_string(),
            literal,
            paths,
            forwards_parameter,
        });
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        // `Type::send_request(self, ..)` is the same emission as the method
        // call form; matching only the latter let it leave discovery entirely.
        if let Some(callee) = path_tail(&node.func) {
            let mut literal = None;
            let mut paths = Vec::new();
            let mut forwards_parameter = false;
            for arg in &node.args {
                if literal.is_none() {
                    literal = string_literal(arg);
                }
                if let Some(tail) = path_tail(arg) {
                    paths.push(tail);
                }
                forwards_parameter |= is_forwarded_parameter(arg);
            }
            self.record(CallSite { callee, literal, paths, forwards_parameter });
        }
        syn::visit::visit_expr_call(self, node);
    }
}

// ── Registry, constants, catalog ─────────────────────────────────────────

/// Parse `pub const NAME: &str = "value";` declarations into a lookup table.
fn method_constants(source: &str) -> BTreeMap<String, String> {
    let Ok(file) = syn::parse_file(source) else { return BTreeMap::new() };
    file.items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Const(konst) => {
                string_literal(&konst.expr).map(|value| (konst.ident.to_string(), value))
            }
            _ => None,
        })
        .collect()
}

/// Read the `REGISTRY` table out of `method_direction.rs`.
///
/// Entries are `c2s(..)`, `s2c(..)`, or `ext(..)` constructor calls; `s2c` is
/// server-to-client by construction and `ext` names its direction explicitly.
/// Reading the parsed const item rather than the file's text means a fixture
/// or a commented-out entry elsewhere in the file cannot inject phantom rows,
/// and there is no table-literal boundary left to get wrong.
pub(super) fn parse_direction_registry(
    source: &str,
    constants: &BTreeMap<String, String>,
) -> (BTreeMap<String, RegistryKind>, Vec<Violation>) {
    let mut out = BTreeMap::new();
    let mut violations = Vec::new();

    let Ok(file) = syn::parse_file(source) else {
        violations.push(Violation::new(
            "registry-source-unparsable",
            "<registry>",
            "the direction registry could not be parsed; its silence must not read as an empty \
             classification surface",
        ));
        return (out, violations);
    };

    let registry = file.items.iter().find_map(|item| match item {
        syn::Item::Const(konst) if konst.ident == "REGISTRY" => Some(&*konst.expr),
        syn::Item::Static(statik) if statik.ident == "REGISTRY" => Some(&*statik.expr),
        _ => None,
    });
    let Some(registry) = registry else { return (out, violations) };

    let mut entries = RegistryEntries { rows: Vec::new() };
    entries.visit_expr(registry);

    for (constructor, args) in entries.rows {
        let implied = match constructor.as_str() {
            "s2c" => Some(false),
            "c2s" => Some(true),
            "ext" => None,
            _ => continue,
        };

        // A constant-named entry is resolved, not skipped: dropping it would
        // quietly shrink the coverage denominator so a newly classified
        // request would need no row.
        let method = args.iter().find_map(|arg| {
            string_literal(arg)
                .or_else(|| path_tail(arg).and_then(|tail| constants.get(&tail).cloned()))
        });
        let Some(method) = method else {
            violations.push(Violation::new(
                "registry-entry-unresolved",
                "<registry>",
                format!(
                    "a `{constructor}(..)` registry entry names no resolvable method; the \
                     classification denominator is incomplete"
                ),
            ));
            continue;
        };

        let mentions = |needle: &str| {
            args.iter().any(|arg| {
                let mut found = Mentions { needle: needle.to_string(), found: false };
                found.visit_expr(arg);
                found.found
            })
        };
        let notification = mentions("Notification");
        let client_to_server = implied.unwrap_or_else(|| mentions("ClientToServer"));

        let kind = if client_to_server {
            RegistryKind::ClientToServer
        } else if notification {
            RegistryKind::ServerToClientNotification
        } else {
            RegistryKind::ServerToClientRequest
        };
        out.insert(method, kind);
    }
    (out, violations)
}

/// Collect every `name(..)` call inside the registry table expression.
struct RegistryEntries {
    rows: Vec<(String, Vec<syn::Expr>)>,
}

impl<'ast> Visit<'ast> for RegistryEntries {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let Some(name) = path_tail(&node.func) {
            self.rows.push((name, node.args.iter().cloned().collect()));
        }
        syn::visit::visit_expr_call(self, node);
    }
}

/// Whether a path expression anywhere in a subtree ends in `needle`.
struct Mentions {
    needle: String,
    found: bool,
}

impl<'ast> Visit<'ast> for Mentions {
    fn visit_path(&mut self, node: &'ast syn::Path) {
        if node.segments.iter().any(|segment| segment.ident == self.needle) {
            self.found = true;
        }
        syn::visit::visit_path(self, node);
    }
}

// ── Emission scan ────────────────────────────────────────────────────────

/// Scan production runtime sources for server-request emission call sites.
///
/// Two passes. The first parses every file and records its functions; the
/// second resolves send sites once the set of forwarding helpers is known
/// across the whole scan root, because a wrapper and its caller need not share
/// a file. Each site is attributed to the function containing it, so the matrix
/// can be required to cite the exact emitting symbol.
pub(super) fn scan_emission(
    repo_root: &Path,
    scan_root: &str,
    constants: &BTreeMap<String, String>,
) -> Result<(BTreeMap<String, Vec<String>>, BTreeSet<String>, Vec<Violation>)> {
    let mut emitted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut ambiguous: BTreeSet<String> = BTreeSet::new();
    let mut violations = Vec::new();

    let root = repo_root.join(scan_root);
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(&root).sort_by_file_name() {
        // A directory the walker cannot enter is an instrument failure. Dropping
        // the error would silently shrink the scan and read as absence.
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                violations.push(Violation::new(
                    "emission-scan-incomplete",
                    scan_root,
                    format!(
                        "the emission scan could not traverse the source tree ({error}); a \
                         partial scan must not read as a complete one"
                    ),
                ));
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.into_path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            paths.push(path);
        }
    }
    paths.sort();

    // ── Pass one: parse and collect ──────────────────────────────────────
    let mut files: Vec<(String, Vec<FnFacts>)> = Vec::new();
    for path in paths {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        // Whole-file test modules carry no production emission.
        if name.ends_with("_tests.rs") {
            continue;
        }
        let relative = path
            .strip_prefix(repo_root)
            .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
        let source = std::fs::read_to_string(&path)
            .wrap_err_with(|| format!("reading emission source {relative}"))?;

        let parsed = match syn::parse_file(&source) {
            Ok(parsed) => parsed,
            Err(error) => {
                violations.push(Violation::new(
                    "emission-source-unparsable",
                    relative.clone(),
                    format!(
                        "this source could not be parsed ({error}); a file the reader cannot \
                         read is an instrument failure, not an empty one"
                    ),
                ));
                continue;
            }
        };
        let mut collector = FnCollector::default();
        collector.visit_file(&parsed);

        // `path#symbol` cannot distinguish two same-named functions in one
        // file, so record the collision rather than letting one stand for both.
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for facts in &collector.functions {
            if !seen.insert(facts.name.as_str()) {
                ambiguous.insert(format!("{relative}#{}", facts.name));
            }
        }
        files.push((relative, collector.functions));
    }

    // ── Forwarding closure, across the whole scan root ───────────────────
    // A helper forwards a request only when it takes the method from its caller
    // *and* hands that same parameter to a sender. Requiring the parameter to
    // reach the sender — not merely that the body contains one — keeps a helper
    // that inspects or logs a method while sending some unrelated method of its
    // own from making its callers' arguments look like emitted methods.
    // Requiring a body at all keeps every helper that merely inspects a method
    // name — such as `is_lifecycle_method` — from being read as a request
    // emitter. Computing it per file left a wrapper and its caller in different
    // files unconnected.
    let mut senders: BTreeSet<String> =
        REQUEST_SENDERS.iter().map(|name| (*name).to_string()).collect();
    loop {
        let mut grew = false;
        for (_, functions) in &files {
            for facts in functions {
                if !facts.forwards_method || senders.contains(&facts.name) {
                    continue;
                }
                if facts
                    .sites
                    .iter()
                    .any(|site| senders.contains(&site.callee) && site.forwards_parameter)
                {
                    senders.insert(facts.name.clone());
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }

    // A send site names a function, not a definition, so `senders` can only
    // hold bare names. One unrelated function sharing a promoted forwarder's
    // name would therefore make its own callers' arguments read as emitted
    // methods. Telling the two apart needs the types a syntactic reader does
    // not have, so the collision is reported instead of guessed. Base senders
    // are exempt: a trait declaration and its impl legitimately share a name.
    let base: BTreeSet<&str> = REQUEST_SENDERS.iter().copied().collect();
    let mut unattributable: BTreeSet<String> = BTreeSet::new();
    for name in senders.iter().filter(|name| !base.contains(name.as_str())) {
        let defined: Vec<&str> = files
            .iter()
            .flat_map(|(relative, functions)| {
                functions.iter().filter(|facts| &facts.name == name).map(move |_| relative.as_str())
            })
            .collect();
        // Every definition of a promoted name must itself forward, or a call
        // by that name cannot be attributed to the one that does.
        let forwarding = files
            .iter()
            .flat_map(|(_, functions)| functions.iter())
            .filter(|facts| &facts.name == name)
            .filter(|facts| {
                facts.forwards_method
                    && facts
                        .sites
                        .iter()
                        .any(|site| senders.contains(&site.callee) && site.forwards_parameter)
            })
            .count();
        if defined.len() > forwarding {
            unattributable.insert(name.clone());
            violations.push(Violation::new(
                "forwarder-ambiguous",
                name,
                format!(
                    "`{name}` forwards a request in one place and is also defined without \
                     forwarding one in {} ({}); a call naming it cannot be attributed to the \
                     forwarder by name alone, so the send is reported rather than guessed",
                    if defined.len() - forwarding == 1 { "another" } else { "others" },
                    defined.join(", ")
                ),
            ));
        }
    }

    // ── Pass two: resolve send sites ─────────────────────────────────────
    for (relative, functions) in &files {
        for facts in functions {
            for site in facts.sites.iter().filter(|site| {
                senders.contains(&site.callee) && !unattributable.contains(&site.callee)
            }) {
                let resolved = site
                    .literal
                    .clone()
                    .or_else(|| site.paths.iter().find_map(|path| constants.get(path).cloned()));
                match resolved {
                    Some(method) => {
                        let reference = format!("{relative}#{}", facts.name);
                        let entry = emitted.entry(method).or_default();
                        if !entry.contains(&reference) {
                            entry.push(reference);
                        }
                    }
                    // The only admitted unresolved shape is a forwarder passing
                    // along its own caller-supplied parameter. Keying this on
                    // the signature alone let a forwarder send a computed or
                    // remapped method with no row and no finding.
                    None if facts.forwards_method && site.forwards_parameter => {}
                    None => violations.push(Violation::new(
                        "emission-unresolved",
                        relative.clone(),
                        format!(
                            "a server-request send site in `{}` names no resolvable method and \
                             does not pass its declared `{FORWARDED_METHOD_PARAM}: &str` \
                             parameter",
                            facts.name
                        ),
                    )),
                }
            }
        }
    }

    for paths in emitted.values_mut() {
        paths.sort();
        paths.dedup();
    }
    Ok((emitted, ambiguous, violations))
}

// ── Feature catalog ──────────────────────────────────────────────────────

/// Minimal typed view of `features.toml`. Unknown fields are ignored; the
/// catalog carries many columns this join does not consume.
#[derive(Debug, Deserialize)]
struct FeatureCatalog {
    #[serde(default)]
    feature: Vec<FeatureRow>,
}

#[derive(Debug, Deserialize)]
struct FeatureRow {
    id: String,
    #[serde(default)]
    spec: String,
    #[serde(default)]
    direction: String,
    #[serde(default)]
    advertised: bool,
    #[serde(default)]
    maturity: String,
    #[serde(default)]
    state_owner: String,
}

/// Collect `features.toml` rows declaring `direction = "server_to_client"`,
/// mapped to the fields this join consumes.
///
/// Parsed as real TOML: a substring scan would classify a row whose prose
/// merely quotes the direction key, and would drop a real row whose spelling or
/// spacing differs.
pub(super) fn parse_feature_catalog(source: &str) -> Result<BTreeMap<String, CatalogRow>> {
    let catalog: FeatureCatalog =
        toml::from_str(source).wrap_err("parsing the feature catalog as TOML")?;
    Ok(catalog
        .feature
        .into_iter()
        .filter(|row| row.direction == "server_to_client")
        .map(|row| {
            (
                row.id,
                CatalogRow {
                    spec: row.spec,
                    advertised: row.advertised,
                    maturity: row.maturity,
                    state_owner: row.state_owner,
                },
            )
        })
        .collect())
}

/// Join all three surfaces.
pub(super) fn discover(
    repo_root: &Path,
    direction_registry: &str,
    feature_catalog: &str,
    emission_scan_root: &str,
) -> Result<(Discovered, Vec<Violation>)> {
    let registry_source = std::fs::read_to_string(repo_root.join(direction_registry))
        .wrap_err_with(|| format!("reading direction registry {direction_registry}"))?;
    let catalog_source = std::fs::read_to_string(repo_root.join(feature_catalog))
        .wrap_err_with(|| format!("reading feature catalog {feature_catalog}"))?;
    let constants_source =
        std::fs::read_to_string(repo_root.join("crates/perl-lsp-rs-core/src/protocol/methods.rs"))
            .wrap_err("reading protocol method constants")?;

    let constants = method_constants(&constants_source);
    let (emitted, ambiguous_symbols, violations) =
        scan_emission(repo_root, emission_scan_root, &constants)?;

    let (registry, mut registry_findings) = parse_direction_registry(&registry_source, &constants);
    registry_findings.extend(violations);

    Ok((
        Discovered {
            registry,
            emitted,
            catalog_rows: parse_feature_catalog(&catalog_source)?,
            ambiguous_symbols,
        },
        registry_findings,
    ))
}
