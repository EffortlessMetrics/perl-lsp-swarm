//! Checked public-surface and consumer disposition ledger for `perl-tdd-support`.
//!
//! The crate is the umbrella that later train slots deprecate, migrate, or
//! delete piece by piece. Before any of that is safe, the repository has to be
//! able to answer three questions mechanically rather than by re-reading the
//! source each time:
//!
//! 1. what does `perl-tdd-support` actually export today;
//! 2. who consumes each item, and through which dependency kind;
//! 3. what is supposed to happen to it, who owns the replacement, and when the
//!    row expires.
//!
//! This module owns the answer. It parses the crate with `syn`, reconciles the
//! discovered surface against `policy/tdd-support-surface.toml`, validates the
//! ledger's own schema, derives the workspace consumer edges from manifests and
//! source, and renders one generated review projection from the same data.
//!
//! Reachability note: `crates/perl-tdd-support/src/lib.rs` carries
//! `#![deny(unreachable_pub)]`. Any `pub` item that were not reachable from the
//! crate root would fail to compile, so "declared `pub` outside `cfg(test)`" and
//! "public API surface" are the same set for this crate. Discovery relies on
//! that invariant instead of re-implementing rustc's reachability rules, and the
//! lint is the negative control that keeps the assumption true.
//!
//! Granularity: one row governs one named public export — an item, a module, a
//! re-export, or a Cargo feature (explicit or implicit-from-an-optional-dep).
//! Associated-item signatures — a type's method signatures, its public field
//! types, its trait impls — are governed only through the owning type's row, not
//! individually. That is the right granularity for the disposition train, which
//! moves, retires, or deletes whole items rather than methods; the umbrella
//! #8144 tracks method- and field-level governance as a later step, and this
//! module's `api_kind` vocabulary can carry it when that lands.
//!
//! Consumers are attributed at crate-root entry-segment granularity: the scanner
//! records the first path segment a consumer names after `perl_tdd_support::`,
//! so an item inside a module is attributed to the crates that reach that
//! module, not necessarily that exact item. This is what a lexical scan can
//! honestly prove without a full type resolver, and it is enough for the
//! `must*` migration #8605 acts on, where the entry segment is the symbol.
//!
//! This checker classifies. It deliberately does not decide: rows whose fate a
//! later slot owns carry that slot's issue number, not a disposition invented
//! here.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Deserialize;
use syn::{Item, Visibility};

/// Crate whose surface this ledger governs, relative to the workspace root.
const SUBJECT_CRATE_DIR: &str = "crates/perl-tdd-support";
/// Cargo package name of the governed crate.
const SUBJECT_CRATE: &str = "perl-tdd-support";
/// Rust path prefix every governed identity is spelled under.
const SUBJECT_ROOT_PATH: &str = "perl_tdd_support";
/// Canonical ledger location.
const LEDGER_PATH: &str = "policy/tdd-support-surface.toml";
/// Generated human review projection rendered from the ledger.
const PROJECTION_PATH: &str = "docs/policy/TDD_SUPPORT_SURFACE.md";
/// Schema version this checker understands.
const SCHEMA_VERSION: u32 = 1;
/// Value the ledger's `policy` key must carry.
const POLICY_NAME: &str = "tdd-support-surface";

/// Dispositions a ledger row may carry.
///
/// The set is closed on purpose: a row that cannot be expressed here is a row
/// whose meaning has not been decided, and an undecided row must not be able to
/// masquerade as a settled one.
const DISPOSITIONS: &[&str] = &[
    "retain_pure",
    "compatibility_with_expiry",
    "legacy_internal_until_issue",
    "move_to_owner",
    "remove",
];

/// How a governed item is consumed today.
///
/// `not_proven` exists so that "we could not establish the consumers" never has
/// to be recorded as "there are none".
const CONSUMER_CLASSES: &[&str] = &[
    "production_workspace_consumer",
    "test_dev_workspace_consumer",
    "example_or_doc",
    "published_compatibility_surface",
    "self_only",
    "not_proven",
];

/// Downstream compatibility classes.
const COMPATIBILITY_CLASSES: &[&str] = &["published", "internal"];

/// API kinds discovery can produce; a ledger row must declare one of these.
const API_KINDS: &[&str] = &[
    "struct", "enum", "fn", "const", "static", "type", "trait", "union", "module", "reexport",
    "feature",
];

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// One public entity found in the governed crate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Discovered {
    /// Stable identity key: `<api_kind>:<path>`.
    pub(crate) id: String,
    /// One of [`API_KINDS`].
    pub(crate) api_kind: String,
    /// Fully qualified path, or the feature name for `feature` rows.
    pub(crate) path: String,
    /// Stringified `#[cfg(..)]` predicate, empty when unconditional.
    pub(crate) cfg: String,
    /// For re-exports, the origin path the name is imported from.
    pub(crate) source: String,
}

impl Discovered {
    fn new(api_kind: &str, path: String, cfg: String, source: String) -> Self {
        Self { id: format!("{api_kind}:{path}"), api_kind: api_kind.to_string(), path, cfg, source }
    }
}

/// Render a `#[cfg(..)]` predicate into a stable string, empty when absent.
///
/// Discovery parses source rather than compiling it, so a platform- or
/// feature-gated item is found on every host. Recording the predicate keeps the
/// ledger identical on Linux, macOS, and Windows instead of making the check
/// depend on where it runs.
fn cfg_of(attrs: &[syn::Attribute]) -> String {
    let mut found = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("cfg") {
            if let syn::Meta::List(list) = &attr.meta {
                let rendered = list.tokens.to_string();
                found.push(normalize_cfg(&rendered));
            }
        }
    }
    found.join(" + ")
}

/// Collapse token-stream spacing so `feature = "x"` and `feature="x"` agree.
fn normalize_cfg(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_was_space = false;
    for ch in raw.chars() {
        if ch.is_whitespace() {
            last_was_space = true;
            continue;
        }
        if last_was_space && !out.is_empty() && needs_space(&out, ch) {
            out.push(' ');
        }
        out.push(ch);
        last_was_space = false;
    }
    out
}

fn needs_space(out: &str, next: char) -> bool {
    let prev = out.chars().next_back().unwrap_or(' ');
    let joinable = |c: char| c.is_alphanumeric() || c == '_' || c == '"';
    joinable(prev) && joinable(next)
}

/// True when the item is compiled out of every non-test build.
///
/// Multiple `#[cfg(..)]` attributes on one item are ANDed, so the item is
/// test-only if any single attribute forces `test`. Within one predicate the
/// forcing rule is: `test` forces test; `all(..)` forces test if any member
/// does (every member must hold); `any(..)` forces test only if every member
/// does (one non-test member would satisfy it without test); `not(..)` and
/// everything else do not force test. This is why a naive
/// `contains("test")` is wrong — it would wrongly treat
/// `any(test, feature = "x")` (reachable via the feature alone) as test-only.
fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Meta>()
                .map(|meta| cfg_meta_forces_test(&meta))
                .unwrap_or(false)
    })
}

/// Whether a single `cfg` predicate can only be true when `test` is true.
fn cfg_meta_forces_test(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) => {
            let nested = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            );
            let Ok(items) = nested else { return false };
            if list.path.is_ident("all") {
                items.iter().any(cfg_meta_forces_test)
            } else if list.path.is_ident("any") {
                !items.is_empty() && items.iter().all(cfg_meta_forces_test)
            } else {
                // `not(..)` requires test to be false, not true; unknown
                // predicates are treated as not forcing test.
                false
            }
        }
        syn::Meta::NameValue(_) => false,
    }
}

fn is_pub(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// Join a module path into the crate-qualified prefix.
fn qualify(module_path: &[String], ident: &str) -> String {
    let mut parts = vec![SUBJECT_ROOT_PATH.to_string()];
    parts.extend(module_path.iter().cloned());
    parts.push(ident.to_string());
    parts.join("::")
}

/// Resolve `mod name;` to the file that backs it.
fn module_file(src_dir: &Path, module_path: &[String]) -> Option<PathBuf> {
    let mut base = src_dir.to_path_buf();
    for part in module_path {
        base.push(part);
    }
    let as_file = base.with_extension("rs");
    if as_file.is_file() {
        return Some(as_file);
    }
    let as_dir = base.join("mod.rs");
    if as_dir.is_file() {
        return Some(as_dir);
    }
    None
}

/// Walk the public module tree of the governed crate.
pub(crate) fn discover_surface(root: &Path) -> Result<Vec<Discovered>> {
    let src_dir = root.join(SUBJECT_CRATE_DIR).join("src");
    let lib = src_dir.join("lib.rs");
    if !lib.is_file() {
        bail!("cannot discover {SUBJECT_CRATE} surface: {} does not exist", lib.display());
    }
    let mut out = Vec::new();
    walk_module_file(&src_dir, &[], &lib, &mut out)?;
    out.extend(discover_features(root)?);
    out.sort();
    out.dedup();
    Ok(out)
}

fn walk_module_file(
    src_dir: &Path,
    module_path: &[String],
    file: &Path,
    out: &mut Vec<Discovered>,
) -> Result<()> {
    let source =
        fs::read_to_string(file).wrap_err_with(|| format!("failed to read {}", file.display()))?;
    let parsed = syn::parse_file(&source)
        .wrap_err_with(|| format!("failed to parse {} with syn", file.display()))?;
    walk_items(src_dir, module_path, &parsed.items, out)
}

fn walk_items(
    src_dir: &Path,
    module_path: &[String],
    items: &[Item],
    out: &mut Vec<Discovered>,
) -> Result<()> {
    for item in items {
        walk_item(src_dir, module_path, item, out)?;
    }
    Ok(())
}

fn walk_item(
    src_dir: &Path,
    module_path: &[String],
    item: &Item,
    out: &mut Vec<Discovered>,
) -> Result<()> {
    macro_rules! simple {
        ($node:expr, $kind:literal) => {{
            let node = $node;
            if is_pub(&node.vis) && !is_cfg_test(&node.attrs) {
                out.push(Discovered::new(
                    $kind,
                    qualify(module_path, &node.ident.to_string()),
                    cfg_of(&node.attrs),
                    String::new(),
                ));
            }
        }};
    }

    match item {
        Item::Struct(node) => simple!(node, "struct"),
        Item::Enum(node) => simple!(node, "enum"),
        Item::Fn(node) => {
            if is_pub(&node.vis) && !is_cfg_test(&node.attrs) {
                out.push(Discovered::new(
                    "fn",
                    qualify(module_path, &node.sig.ident.to_string()),
                    cfg_of(&node.attrs),
                    String::new(),
                ));
            }
        }
        Item::Const(node) => simple!(node, "const"),
        Item::Static(node) => simple!(node, "static"),
        Item::Type(node) => simple!(node, "type"),
        Item::Trait(node) => simple!(node, "trait"),
        Item::Union(node) => simple!(node, "union"),
        Item::Mod(node) => walk_mod(src_dir, module_path, node, out)?,
        Item::Use(node) => walk_use(module_path, node, out)?,
        _ => {}
    }
    Ok(())
}

fn walk_mod(
    src_dir: &Path,
    module_path: &[String],
    node: &syn::ItemMod,
    out: &mut Vec<Discovered>,
) -> Result<()> {
    if is_cfg_test(&node.attrs) {
        return Ok(());
    }
    // A private module cannot contribute public surface here: the crate denies
    // `unreachable_pub`, so a `pub` item behind a private module would not
    // compile. Skipping it keeps discovery aligned with what consumers can name.
    if !is_pub(&node.vis) {
        return Ok(());
    }
    let name = node.ident.to_string();
    let mut child_path = module_path.to_vec();
    child_path.push(name.clone());
    out.push(Discovered::new(
        "module",
        qualify(module_path, &name),
        cfg_of(&node.attrs),
        String::new(),
    ));

    if let Some((_, items)) = &node.content {
        return walk_items(src_dir, &child_path, items, out);
    }
    let Some(file) = module_file(src_dir, &child_path) else {
        bail!(
            "public module `{}` declared in {SUBJECT_CRATE} has no backing file; expected {}.rs \
             or {}/mod.rs under {}",
            child_path.join("::"),
            child_path.join("/"),
            child_path.join("/"),
            src_dir.display()
        );
    };
    walk_module_file(src_dir, &child_path, &file, out)
}

fn walk_use(module_path: &[String], node: &syn::ItemUse, out: &mut Vec<Discovered>) -> Result<()> {
    if !is_pub(&node.vis) || is_cfg_test(&node.attrs) {
        return Ok(());
    }
    let cfg = cfg_of(&node.attrs);
    let mut leaves = Vec::new();
    collect_use_tree(&node.tree, Vec::new(), &mut leaves)?;
    for (source, name) in leaves {
        out.push(Discovered::new("reexport", qualify(module_path, &name), cfg.clone(), source));
    }
    Ok(())
}

/// Flatten a `use` tree into `(origin path, exported name)` leaves.
///
/// A glob re-export is refused rather than skipped. Discarding `pub use api::*`
/// would silently drop every symbol it republishes, and an inventory that
/// under-counts public surface is worse than no inventory: later slots would
/// delete or move items nobody recorded as exported.
fn collect_use_tree(
    tree: &syn::UseTree,
    prefix: Vec<String>,
    out: &mut Vec<(String, String)>,
) -> Result<()> {
    match tree {
        syn::UseTree::Path(path) => {
            let mut next = prefix;
            next.push(path.ident.to_string());
            collect_use_tree(&path.tree, next, out)
        }
        syn::UseTree::Name(name) => {
            out.push((origin_of(&prefix, &name.ident.to_string()), name.ident.to_string()));
            Ok(())
        }
        syn::UseTree::Rename(rename) => {
            out.push((origin_of(&prefix, &rename.ident.to_string()), rename.rename.to_string()));
            Ok(())
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_tree(item, prefix.clone(), out)?;
            }
            Ok(())
        }
        syn::UseTree::Glob(_) => {
            let mut shown = prefix.join("::");
            if shown.is_empty() {
                shown = "crate".to_string();
            }
            Err(eyre!(
                "unsupported glob re-export `pub use {shown}::*` in {SUBJECT_CRATE}: expand it \
                 into named re-exports so every published symbol has a ledger identity"
            ))
        }
    }
}

fn origin_of(prefix: &[String], name: &str) -> String {
    let mut parts: Vec<String> =
        prefix.iter().filter(|part| part.as_str() != "self").cloned().collect();
    parts.push(name.to_string());
    parts.join("::")
}

/// Read the Cargo features of the governed crate, explicit and implicit.
///
/// A downstream crate can activate `--features <dep>` for any `optional = true`
/// dependency, so those implicit features are public surface too — Cargo
/// synthesizes a feature named after each optional dependency unless some
/// `[features]` value already references it through the `dep:<name>` form.
/// Parsing only the `[features]` table would miss them and under-count the
/// surface (this crate's `lsp-types` and `url` optional deps each create one).
fn discover_features(root: &Path) -> Result<Vec<Discovered>> {
    let manifest_path = root.join(SUBJECT_CRATE_DIR).join("Cargo.toml");
    let text = fs::read_to_string(&manifest_path)
        .wrap_err_with(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: toml::Value = toml::from_str(&text)
        .wrap_err_with(|| format!("failed to parse {}", manifest_path.display()))?;

    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut suppressed: BTreeSet<String> = BTreeSet::new();

    if let Some(features) = manifest.get("features").and_then(toml::Value::as_table) {
        for (name, value) in features {
            names.insert(name.clone());
            if let Some(list) = value.as_array() {
                for item in list {
                    if let Some(reference) = item.as_str() {
                        // `dep:foo` in a feature value suppresses the implicit
                        // `foo` feature; a bare `foo` reference does not.
                        if let Some(dep) = reference.strip_prefix("dep:") {
                            suppressed.insert(dep.to_string());
                        }
                    }
                }
            }
        }
    }

    for dep in optional_dependencies(&manifest) {
        if !suppressed.contains(&dep) {
            names.insert(dep);
        }
    }

    Ok(names
        .into_iter()
        .map(|name| Discovered::new("feature", name, String::new(), String::new()))
        .collect())
}

/// Names of every `optional = true` dependency, across the normal and
/// target-specific dependency tables (dev/build deps do not create features).
fn optional_dependencies(manifest: &toml::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_optional(manifest.get("dependencies"), &mut out);
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for spec in targets.values() {
            collect_optional(spec.get("dependencies"), &mut out);
        }
    }
    out
}

fn collect_optional(table: Option<&toml::Value>, out: &mut BTreeSet<String>) {
    let Some(table) = table.and_then(toml::Value::as_table) else { return };
    for (name, spec) in table {
        let optional =
            spec.as_table().and_then(|t| t.get("optional")).and_then(toml::Value::as_bool);
        if optional == Some(true) {
            // A renamed optional dep still creates the feature under its table
            // key, which is what `--features <key>` and downstream activation use.
            out.insert(name.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Ledger
// ---------------------------------------------------------------------------

/// Deserialized `policy/tdd-support-surface.toml`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Ledger {
    pub(crate) schema_version: u32,
    pub(crate) policy: String,
    pub(crate) subject_crate: String,
    #[serde(default)]
    pub(crate) entry: Vec<LedgerEntry>,
}

/// One governed row.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LedgerEntry {
    pub(crate) id: String,
    pub(crate) api_kind: String,
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) cfg: String,
    pub(crate) behavior: String,
    #[serde(default)]
    pub(crate) consumers: Vec<String>,
    pub(crate) consumer_class: String,
    pub(crate) compatibility: String,
    pub(crate) disposition: String,
    pub(crate) replacement_owner: String,
    pub(crate) owner_issue: u64,
    pub(crate) exit_condition: String,
    pub(crate) proof_command: String,
}

pub(crate) fn load_ledger(path: &Path) -> Result<Ledger> {
    let text =
        fs::read_to_string(path).wrap_err_with(|| format!("failed to read {}", path.display()))?;
    let ledger: Ledger = toml::from_str(&text)
        .wrap_err_with(|| format!("failed to parse {} as the surface ledger", path.display()))?;
    Ok(ledger)
}

/// Schema-level validation that does not depend on the crate source.
pub(crate) fn validate_ledger(ledger: &Ledger, path: &Path) -> Result<()> {
    let shown = path.display();
    if ledger.schema_version != SCHEMA_VERSION {
        bail!(
            "{shown}: unsupported schema_version {} (this checker understands {SCHEMA_VERSION})",
            ledger.schema_version
        );
    }
    if ledger.policy != POLICY_NAME {
        bail!("{shown}: expected policy = \"{POLICY_NAME}\", found {:?}", ledger.policy);
    }
    if ledger.subject_crate != SUBJECT_CRATE {
        bail!(
            "{shown}: expected subject_crate = \"{SUBJECT_CRATE}\", found {:?}",
            ledger.subject_crate
        );
    }
    if ledger.entry.is_empty() {
        bail!("{shown}: ledger has no rows; an empty ledger cannot govern a public surface");
    }

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for entry in &ledger.entry {
        if !seen.insert(entry.id.as_str()) {
            bail!("{shown}: duplicate row id `{}`", entry.id);
        }
        validate_entry(entry, &shown.to_string())?;
    }
    Ok(())
}

fn validate_entry(entry: &LedgerEntry, shown: &str) -> Result<()> {
    let id = &entry.id;
    let expected_id = format!("{}:{}", entry.api_kind, entry.path);
    if *id != expected_id {
        bail!(
            "{shown}: row `{id}` has inconsistent identity; api_kind/path spell `{expected_id}`. \
             The id is the identity key, so it cannot drift from the fields it encodes"
        );
    }
    for (field, value) in [
        ("path", &entry.path),
        ("behavior", &entry.behavior),
        ("replacement_owner", &entry.replacement_owner),
        ("exit_condition", &entry.exit_condition),
        ("proof_command", &entry.proof_command),
    ] {
        if value.trim().is_empty() {
            bail!("{shown}: row `{id}` has empty required field `{field}`");
        }
    }
    if !API_KINDS.contains(&entry.api_kind.as_str()) {
        bail!(
            "{shown}: row `{id}` has unknown api_kind {:?}; expected one of {API_KINDS:?}",
            entry.api_kind
        );
    }
    if !DISPOSITIONS.contains(&entry.disposition.as_str()) {
        bail!(
            "{shown}: row `{id}` has unknown disposition {:?}; expected one of {DISPOSITIONS:?}",
            entry.disposition
        );
    }
    if !CONSUMER_CLASSES.contains(&entry.consumer_class.as_str()) {
        bail!(
            "{shown}: row `{id}` has unknown consumer_class {:?}; expected one of \
             {CONSUMER_CLASSES:?}",
            entry.consumer_class
        );
    }
    if !COMPATIBILITY_CLASSES.contains(&entry.compatibility.as_str()) {
        bail!(
            "{shown}: row `{id}` has unknown compatibility {:?}; expected one of \
             {COMPATIBILITY_CLASSES:?}",
            entry.compatibility
        );
    }
    if entry.owner_issue == 0 {
        bail!(
            "{shown}: row `{id}` has no owner_issue; a disposition without an owner is an \
             intention, not a tracked exit"
        );
    }
    // A wildcard row would let future public additions land pre-classified,
    // which is exactly the drift this ledger exists to catch.
    if entry.path.contains('*') || entry.path.contains('?') {
        bail!(
            "{shown}: row `{id}` uses a wildcard path; every governed item needs its own exact \
             row so a new symbol cannot inherit an existing classification"
        );
    }
    // A row that says "nobody uses this" must say so through the vocabulary,
    // not by leaving the evidence field blank. `published_compatibility_surface`
    // is the honest exception: a published crate can carry a symbol whose only
    // consumers are downstream and therefore unnameable from inside this repo.
    if entry.consumers.is_empty()
        && !matches!(
            entry.consumer_class.as_str(),
            "self_only" | "not_proven" | "published_compatibility_surface"
        )
    {
        bail!(
            "{shown}: row `{id}` lists no consumers but claims consumer_class {:?}; use \
             `self_only`, `not_proven`, or `published_compatibility_surface` when there is no \
             in-repository consumer to name",
            entry.consumer_class
        );
    }
    Ok(())
}

/// Reconcile the ledger against the surface discovered from source.
pub(crate) fn reconcile(discovered: &[Discovered], ledger: &Ledger) -> Result<()> {
    let discovered_by_id: BTreeMap<&str, &Discovered> =
        discovered.iter().map(|item| (item.id.as_str(), item)).collect();
    let ledger_by_id: BTreeMap<&str, &LedgerEntry> =
        ledger.entry.iter().map(|entry| (entry.id.as_str(), entry)).collect();

    let mut problems: Vec<String> = Vec::new();

    for (id, item) in &discovered_by_id {
        if !ledger_by_id.contains_key(id) {
            problems.push(format!(
                "unclassified public surface: `{id}` exists in {SUBJECT_CRATE} but has no ledger \
                 row (run `cargo xtask tdd-support-surface --propose` for a row skeleton)"
            ));
            continue;
        }
        // `cfg` is part of what a consumer must satisfy to name the item, so a
        // silent gate change is a contract change even when the path is stable.
        if let Some(entry) = ledger_by_id.get(id) {
            if entry.cfg != item.cfg {
                problems.push(format!(
                    "cfg drift on `{id}`: source declares {:?}, ledger records {:?}",
                    item.cfg, entry.cfg
                ));
            }
        }
    }

    for id in ledger_by_id.keys() {
        if !discovered_by_id.contains_key(id) {
            problems.push(format!(
                "stale ledger row: `{id}` is governed but no longer exists in {SUBJECT_CRATE}; \
                 remove the row in the same change that removed the symbol"
            ));
        }
    }

    if problems.is_empty() {
        return Ok(());
    }
    bail!(
        "{SUBJECT_CRATE} surface ledger is out of date ({} problem(s)):\n  - {}",
        problems.len(),
        problems.join("\n  - ")
    );
}

// ---------------------------------------------------------------------------
// Consumers
// ---------------------------------------------------------------------------

/// One workspace crate that declares a dependency on the governed crate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ConsumerEdge {
    pub(crate) crate_name: String,
    pub(crate) manifest: String,
    pub(crate) dep_kind: String,
    pub(crate) referenced: BTreeSet<String>,
    pub(crate) class: String,
}

/// Names re-exported from `perl-test-must`; an edge that touches only these is
/// the migration target #8605 acts on.
const MUST_FAMILY: &[&str] =
    &["must", "must_err", "must_err_with", "must_some", "must_some_with", "must_with"];

/// The `tdd` submodules that are also re-exported at the crate root, so an item
/// under `tdd::<m>` is reachable both as `perl_tdd_support::tdd::<m>::X` and as
/// `perl_tdd_support::<m>::X`.
const TDD_REEXPORTED_SUBMODULES: &[&str] =
    &["tdd_basic", "tdd_workflow", "test_generator", "test_runner"];

/// Invert the consumer edges into `first-path-segment -> consuming crates`.
///
/// The edge scanner records the first path segment a consumer names after
/// `perl_tdd_support::`, which is the granularity at which consumption can be
/// attributed without a full type resolver: `must` for `perl_tdd_support::must`,
/// `governance` for `perl_tdd_support::governance::IgnoredTestGuardian`.
pub(crate) fn reference_index(edges: &[ConsumerEdge]) -> BTreeMap<String, BTreeSet<String>> {
    let mut index: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in edges {
        for name in &edge.referenced {
            index.entry(name.clone()).or_default().insert(edge.crate_name.clone());
        }
    }
    index
}

/// The crate-root entry segments through which an item's row path is reachable.
///
/// One item can have more than one, because the crate re-exports the `tdd`
/// submodules at the root: an item under `tdd::tdd_basic` answers to both
/// `tdd` (full path) and `tdd_basic` (re-export path).
fn entry_segments(path: &str) -> Vec<String> {
    let rest = path.strip_prefix(SUBJECT_ROOT_PATH).and_then(|s| s.strip_prefix("::"));
    let Some(rest) = rest else { return Vec::new() };
    let segments: Vec<&str> = rest.split("::").collect();
    let Some(&first) = segments.first() else { return Vec::new() };
    let mut out = vec![first.to_string()];
    if first == "tdd" {
        if let Some(&second) = segments.get(1) {
            if TDD_REEXPORTED_SUBMODULES.contains(&second) {
                out.push(second.to_string());
            }
        }
    } else if TDD_REEXPORTED_SUBMODULES.contains(&first) {
        out.push("tdd".to_string());
    }
    out
}

/// The crates that reference an item, derived from the edge index.
pub(crate) fn derived_consumers(
    path: &str,
    index: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for segment in entry_segments(path) {
        if let Some(crates) = index.get(&segment) {
            out.extend(crates.iter().cloned());
        }
    }
    out
}

/// Validate that each row's `consumers` matches the crates that actually
/// reference it, and that the `consumer_class` is consistent with that set.
///
/// This is what stops `consumers` from decaying into prose. A hand-authored
/// list that names a crate not referencing the symbol — or omits one that does —
/// fails here, so the classification #8605 consumes cannot silently drift.
pub(crate) fn validate_derived_consumers(ledger: &Ledger, edges: &[ConsumerEdge]) -> Result<()> {
    let index = reference_index(edges);
    let mut problems: Vec<String> = Vec::new();

    for entry in &ledger.entry {
        if entry.api_kind == "feature" {
            continue;
        }
        let expected = derived_consumers(&entry.path, &index);
        let listed: BTreeSet<String> = entry
            .consumers
            .iter()
            .filter_map(|value| value.split_whitespace().next())
            .map(str::to_string)
            .collect();

        if listed != expected {
            let missing: Vec<&String> = expected.difference(&listed).collect();
            let extra: Vec<&String> = listed.difference(&expected).collect();
            problems.push(format!(
                "row `{}` consumers are stale: derived {:?}; ledger lists {:?} (missing {:?}, \
                 unexpected {:?})",
                entry.id,
                expected.iter().collect::<Vec<_>>(),
                listed.iter().collect::<Vec<_>>(),
                missing,
                extra
            ));
            continue;
        }

        let class = entry.consumer_class.as_str();
        if expected.is_empty() {
            if !matches!(class, "self_only" | "not_proven" | "published_compatibility_surface") {
                problems.push(format!(
                    "row `{}` has no in-repository consumer but claims consumer_class {:?}; use \
                     `self_only`, `not_proven`, or `published_compatibility_surface`",
                    entry.id, class
                ));
            }
        } else if matches!(class, "self_only" | "not_proven") {
            problems.push(format!(
                "row `{}` has proven consumers {:?} but claims consumer_class {:?}",
                entry.id,
                expected.iter().collect::<Vec<_>>(),
                class
            ));
        }
    }

    if problems.is_empty() {
        return Ok(());
    }
    bail!(
        "{SUBJECT_CRATE} consumer classification is out of date ({} problem(s)):\n  - {}",
        problems.len(),
        problems.join("\n  - ")
    );
}

/// Locate every workspace manifest that declares a dependency on the crate.
pub(crate) fn discover_consumers(root: &Path) -> Result<Vec<ConsumerEdge>> {
    let mut edges = Vec::new();
    for manifest in workspace_manifests(root)? {
        let text = fs::read_to_string(&manifest)
            .wrap_err_with(|| format!("failed to read {}", manifest.display()))?;
        let parsed: toml::Value = toml::from_str(&text)
            .wrap_err_with(|| format!("failed to parse {}", manifest.display()))?;
        let Some(package) = parsed.get("package").and_then(|p| p.get("name")) else {
            continue;
        };
        let Some(name) = package.as_str() else { continue };
        if name == SUBJECT_CRATE {
            continue;
        }
        let Some(dep_kind) = declared_dep_kind(&parsed) else { continue };
        let Some(crate_dir) = manifest.parent() else { continue };
        let referenced = referenced_symbols(crate_dir)?;
        let class = classify_edge(&referenced);
        edges.push(ConsumerEdge {
            crate_name: name.to_string(),
            manifest: relative(root, &manifest),
            dep_kind,
            referenced,
            class,
        });
    }
    edges.sort();
    Ok(edges)
}

fn declared_dep_kind(manifest: &toml::Value) -> Option<String> {
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if manifest.get(section).and_then(|table| table.get(SUBJECT_CRATE)).is_some() {
            return Some(section.to_string());
        }
    }
    None
}

/// Collect every top-level `perl_tdd_support::<name>` reference under a crate.
///
/// The scan parses each file with `syn` rather than matching text. A lexical
/// scan reports any mention of the crate path, including the ones inside doc
/// comments and string literals — and this repository has files whose whole
/// purpose is to name these symbols in prose (the `perl-parser` TDD facade
/// guard, for one). Those produced consumer edges for crates that import
/// nothing, which is exactly the kind of wrong denominator later slots would
/// act on.
fn referenced_symbols(crate_dir: &Path) -> Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    for entry in walkdir::WalkDir::new(crate_dir)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry.path()))
        .filter_map(std::result::Result::ok)
    {
        let path = entry.path();
        if !path.extension().is_some_and(|ext| ext == "rs") {
            continue;
        }
        let Ok(source) = fs::read_to_string(path) else { continue };
        if !source.contains(SUBJECT_ROOT_PATH) {
            continue;
        }
        // A file this checker cannot parse contributes nothing rather than
        // aborting the run: the manifest edge, not this scan, is the blocking
        // consumer assertion.
        let Ok(parsed) = syn::parse_file(&source) else { continue };
        let mut visitor = ReferenceVisitor { found: &mut found };
        syn::visit::Visit::visit_file(&mut visitor, &parsed);
    }
    Ok(found)
}

/// Records the first path segment after `perl_tdd_support::`.
///
/// The first segment is the crate-root export actually being reached for —
/// `tdd_basic` for `perl_tdd_support::tdd_basic::TestGenerator`, `must` for
/// `perl_tdd_support::must` — which is the granularity edge classification and
/// the migration in #8605 both work at.
struct ReferenceVisitor<'a> {
    found: &'a mut BTreeSet<String>,
}

impl ReferenceVisitor<'_> {
    fn record_use_tree(&mut self, tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(path) => {
                self.found.insert(path.ident.to_string());
            }
            syn::UseTree::Name(name) => {
                self.found.insert(name.ident.to_string());
            }
            syn::UseTree::Rename(rename) => {
                self.found.insert(rename.ident.to_string());
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.record_use_tree(item);
                }
            }
            syn::UseTree::Glob(_) => {
                self.found.insert("*".to_string());
            }
        }
    }
}

impl<'ast> syn::visit::Visit<'ast> for ReferenceVisitor<'_> {
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        if let syn::UseTree::Path(path) = &node.tree {
            if path.ident == SUBJECT_ROOT_PATH {
                self.record_use_tree(&path.tree);
            }
        }
        syn::visit::visit_item_use(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        let mut segments = node.segments.iter();
        if let (Some(first), Some(second)) = (segments.next(), segments.next()) {
            if first.ident == SUBJECT_ROOT_PATH {
                self.found.insert(second.ident.to_string());
            }
        }
        syn::visit::visit_path(self, node);
    }
}

fn classify_edge(referenced: &BTreeSet<String>) -> String {
    if referenced.is_empty() {
        return "declared_unused".to_string();
    }
    let must: BTreeSet<&str> = MUST_FAMILY.iter().copied().collect();
    let uses_must = referenced.iter().any(|name| must.contains(name.as_str()));
    let uses_other = referenced.iter().any(|name| !must.contains(name.as_str()));
    match (uses_must, uses_other) {
        (true, false) => "must_only".to_string(),
        (true, true) => "mixed".to_string(),
        (false, true) => "other_only".to_string(),
        (false, false) => "declared_unused".to_string(),
    }
}

fn workspace_manifests(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .max_depth(3)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry.path()))
        .filter_map(std::result::Result::ok)
    {
        if entry.file_name() == "Cargo.toml" {
            out.push(entry.path().to_path_buf());
        }
    }
    out.sort();
    Ok(out)
}

fn is_ignored_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "target" | ".git" | "node_modules"))
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string().replace('\\', "/")
}

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// Render the generated Markdown review projection from ledger + edges.
pub(crate) fn render_projection(ledger: &Ledger, edges: &[ConsumerEdge]) -> String {
    let mut out = String::new();
    out.push_str("# `perl-tdd-support` public surface and consumer dispositions\n\n");
    out.push_str(
        "> Generated by `cargo xtask tdd-support-surface --write`. Do not edit by hand.\n\n",
    );
    out.push_str(
        "This projection is rendered from `policy/tdd-support-surface.toml`. The ledger is the \
         authority; this file exists so the contract can be reviewed without reading TOML.\n\n",
    );

    out.push_str("## Disposition census\n\n");
    let mut census: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in &ledger.entry {
        *census.entry(entry.disposition.as_str()).or_default() += 1;
    }
    out.push_str("| Disposition | Rows |\n| --- | ---: |\n");
    for (disposition, count) in &census {
        out.push_str(&format!("| `{disposition}` | {count} |\n"));
    }
    out.push_str(&format!("| **total** | **{}** |\n\n", ledger.entry.len()));

    out.push_str("## Governed surface\n\n");
    out.push_str(
        "| Identity | Kind | cfg | Consumer class | Compatibility | Disposition | Owner | Exit \
         condition |\n| --- | --- | --- | --- | --- | --- | ---: | --- |\n",
    );
    for entry in &ledger.entry {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | `{}` | #{} | {} |\n",
            entry.path,
            entry.api_kind,
            if entry.cfg.is_empty() { "—".to_string() } else { format!("`{}`", entry.cfg) },
            entry.consumer_class,
            entry.compatibility,
            entry.disposition,
            entry.owner_issue,
            entry.exit_condition,
        ));
    }

    out.push_str("\n## Workspace consumer edges\n\n");
    out.push_str(
        "Edge classification is the input #8605 consumes when it migrates `must*` imports to \
         `perl-test-must` directly. `must_only` edges are the ones that can move wholesale; \
         `mixed` edges keep their dependency for a real second reason.\n\n",
    );
    out.push_str("| Crate | Dependency kind | Edge class | Referenced names |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for edge in edges {
        let names = if edge.referenced.is_empty() {
            "—".to_string()
        } else {
            edge.referenced.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            edge.crate_name, edge.dep_kind, edge.class, names
        ));
    }

    let mut class_census: BTreeMap<&str, usize> = BTreeMap::new();
    for edge in edges {
        *class_census.entry(edge.class.as_str()).or_default() += 1;
    }
    out.push_str("\n| Edge class | Crates |\n| --- | ---: |\n");
    for (class, count) in &class_census {
        out.push_str(&format!("| {class} | {count} |\n"));
    }
    out.push_str(&format!("| **total** | **{}** |\n", edges.len()));

    out
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Print row skeletons for public items that have no ledger row yet.
fn propose(discovered: &[Discovered], ledger: &Ledger) -> Result<()> {
    let governed: BTreeSet<&str> = ledger.entry.iter().map(|e| e.id.as_str()).collect();
    let missing: Vec<&Discovered> =
        discovered.iter().filter(|item| !governed.contains(item.id.as_str())).collect();
    if missing.is_empty() {
        println!("every discovered {SUBJECT_CRATE} public item already has a ledger row");
        return Ok(());
    }
    let missing_count = missing.len();
    println!("# {missing_count} unclassified item(s); add to {LEDGER_PATH}");
    for item in missing {
        println!();
        println!("[[entry]]");
        println!("id = \"{}\"", item.id);
        println!("api_kind = \"{}\"", item.api_kind);
        println!("path = \"{}\"", item.path);
        println!("cfg = \"{}\"", item.cfg);
        println!("behavior = \"TODO: what this item does today\"");
        println!("consumers = []");
        println!("consumer_class = \"not_proven\"");
        println!("compatibility = \"published\"");
        println!("disposition = \"legacy_internal_until_issue\"");
        println!("replacement_owner = \"TODO: replacement API or owning package\"");
        println!("owner_issue = 8418");
        println!("exit_condition = \"TODO: when this row is removed or revisited\"");
        println!("proof_command = \"cargo test -p {SUBJECT_CRATE} --locked\"");
    }
    bail!("{missing_count} unclassified public item(s) in {SUBJECT_CRATE}");
}

/// Print `<row id>\t<crate,crate,...>` for every discovered item, using the
/// same edge-derived attribution the checker enforces.
///
/// This is the authoring aid for the `consumers` field: because the derivation
/// lives here, the ledger is populated from the checker's own output rather than
/// a second, drift-prone implementation.
fn emit_consumers(discovered: &[Discovered], edges: &[ConsumerEdge]) {
    let index = reference_index(edges);
    for item in discovered {
        if item.api_kind == "feature" {
            continue;
        }
        let consumers = derived_consumers(&item.path, &index);
        println!("{}\t{}", item.id, consumers.iter().cloned().collect::<Vec<_>>().join(","));
    }
}

/// `cargo xtask tdd-support-surface`.
pub fn run(
    root: &Path,
    write: bool,
    propose_rows: bool,
    emit_consumers_rows: bool,
    json: Option<&Path>,
) -> Result<()> {
    let ledger_path = root.join(LEDGER_PATH);
    let ledger = load_ledger(&ledger_path)?;
    validate_ledger(&ledger, &ledger_path)?;

    let discovered = discover_surface(root)?;

    if propose_rows {
        return propose(&discovered, &ledger);
    }

    let edges = discover_consumers(root)?;

    if emit_consumers_rows {
        emit_consumers(&discovered, &edges);
        return Ok(());
    }

    reconcile(&discovered, &ledger)?;
    validate_derived_consumers(&ledger, &edges)?;

    let projection = render_projection(&ledger, &edges);
    let projection_path = root.join(PROJECTION_PATH);
    if write {
        if let Some(parent) = projection_path.parent() {
            fs::create_dir_all(parent)
                .wrap_err_with(|| format!("failed to create directory {}", parent.display()))?;
        }
        fs::write(&projection_path, &projection)
            .wrap_err_with(|| format!("failed to write {}", projection_path.display()))?;
        println!("wrote {}", relative(root, &projection_path));
    } else {
        let current = fs::read_to_string(&projection_path).unwrap_or_default();
        if current != projection {
            bail!(
                "{} is stale; regenerate it with `cargo xtask tdd-support-surface --write`",
                relative(root, &projection_path)
            );
        }
    }

    if let Some(json_path) = json {
        write_json_receipt(root, json_path, &ledger, &edges)?;
    }

    println!(
        "{SUBJECT_CRATE} surface ledger current: {} governed row(s), {} consumer edge(s)",
        ledger.entry.len(),
        edges.len()
    );
    Ok(())
}

fn write_json_receipt(
    root: &Path,
    json_path: &Path,
    ledger: &Ledger,
    edges: &[ConsumerEdge],
) -> Result<()> {
    let rows: Vec<serde_json::Value> = ledger
        .entry
        .iter()
        .map(|entry| {
            serde_json::json!({
                "id": entry.id,
                "api_kind": entry.api_kind,
                "path": entry.path,
                "cfg": entry.cfg,
                "consumer_class": entry.consumer_class,
                "compatibility": entry.compatibility,
                "disposition": entry.disposition,
                "owner_issue": entry.owner_issue,
            })
        })
        .collect();
    let edge_rows: Vec<serde_json::Value> = edges
        .iter()
        .map(|edge| {
            serde_json::json!({
                "crate": edge.crate_name,
                "manifest": edge.manifest,
                "dep_kind": edge.dep_kind,
                "class": edge.class,
                "referenced": edge.referenced.iter().collect::<Vec<_>>(),
            })
        })
        .collect();
    let receipt = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "policy": POLICY_NAME,
        "subject_crate": SUBJECT_CRATE,
        "entries": rows,
        "consumer_edges": edge_rows,
    });
    let absolute =
        if json_path.is_absolute() { json_path.to_path_buf() } else { root.join(json_path) };
    if let Some(parent) = absolute.parent() {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create directory {}", parent.display()))?;
    }
    fs::write(&absolute, serde_json::to_string_pretty(&receipt)?)
        .wrap_err_with(|| format!("failed to write {}", absolute.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests;
