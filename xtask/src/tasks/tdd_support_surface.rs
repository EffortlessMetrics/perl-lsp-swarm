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
    "macro", "feature", "method", "field", "variant",
];

/// Kinds that are members of an owning public type rather than free items.
///
/// Their identity is `<Type>::<name>` under the owning type's path, and
/// `--propose` inherits their disposition from the owning type's row so that a
/// member cannot be classified more loosely than the type it hangs off.
const MEMBER_KINDS: &[&str] = &["method", "field", "variant"];

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
    /// For members (`method`, `field`, `variant`), the owning type's path.
    pub(crate) owner: String,
}

impl Discovered {
    fn new(api_kind: &str, path: String, cfg: String, source: String) -> Self {
        Self {
            id: format!("{api_kind}:{path}"),
            api_kind: api_kind.to_string(),
            path,
            cfg,
            source,
            owner: String::new(),
        }
    }

    fn member(api_kind: &str, owner: String, name: &str, cfg: String) -> Self {
        let path = format!("{owner}::{name}");
        Self {
            id: format!("{api_kind}:{path}"),
            api_kind: api_kind.to_string(),
            path,
            cfg,
            source: String::new(),
            owner,
        }
    }

    fn is_member(&self) -> bool {
        MEMBER_KINDS.contains(&self.api_kind.as_str())
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
        if attr.path().is_ident("cfg")
            && let syn::Meta::List(list) = &attr.meta
        {
            let rendered = list.tokens.to_string();
            found.push(normalize_cfg(&rendered));
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

/// AND an enclosing module's `cfg` predicate onto an item's own.
///
/// A `#[cfg(..)]` on a module gates every item inside it, so a consumer must
/// satisfy the module's predicate and the item's together. Recording only the
/// item's local `cfg` would mark, for example, the functions inside the
/// `#[cfg(feature = "lsp-compat")]` `lsp_integration` module as unconditional.
fn combine_cfg(inherited: &str, local: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in inherited.split(" + ").chain(local.split(" + ")) {
        // An item under a `#[cfg(windows)]` module that also carries its own
        // `#[cfg(windows)]` yields one `windows`, not `windows + windows`.
        if !part.is_empty() && !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts.join(" + ")
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
    let mut sink = Sink::default();
    walk_module_file(&src_dir, &[], "", &lib, &mut sink)?;
    let Sink { mut out, shadow, .. } = sink;
    // A type defined in a private module becomes surface when a public `pub use`
    // republishes it. Its definition was walked into the shadow, so lift its
    // members under the re-export's public path; otherwise a private-module
    // type's methods, fields, and variants could change without a ledger row.
    let lifted: Vec<Discovered> = out
        .iter()
        .filter(|item| item.api_kind == "reexport")
        .flat_map(|reexport| {
            let origin = resolve_local_origin(&reexport.source, &reexport.path);
            shadow
                .iter()
                .filter(move |member| member.is_member() && member.owner == origin)
                .map(|member| {
                    let name = member.path.rsplit("::").next().unwrap_or_default();
                    Discovered::member(
                        &member.api_kind,
                        reexport.path.clone(),
                        name,
                        combine_cfg(&reexport.cfg, &member.cfg),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    out.extend(lifted);
    // An inherent method is public surface only when its type is: `impl` blocks
    // are walked wherever they appear, so drop the ones whose owning type was
    // never discovered as a public struct, enum, or union (or a re-export of
    // one) at that path.
    let owning_types: BTreeSet<String> = out
        .iter()
        .filter(|item| matches!(item.api_kind.as_str(), "struct" | "enum" | "union" | "reexport"))
        .map(|item| item.path.clone())
        .collect();
    out.retain(|item| item.api_kind != "method" || owning_types.contains(&item.owner));
    out.extend(discover_features(root)?);
    out.sort();
    out.dedup();
    Ok(out)
}

/// Discovery output split by reachability.
///
/// Items under a private module are not surface, but their definitions are
/// kept in `shadow` so a public re-export can lift their members into `out`.
#[derive(Default)]
struct Sink {
    out: Vec<Discovered>,
    shadow: Vec<Discovered>,
    private_depth: usize,
}

impl Sink {
    fn push(&mut self, item: Discovered) {
        if self.private_depth == 0 {
            self.out.push(item);
        } else {
            self.shadow.push(item);
        }
    }
}

/// Resolve a re-export's origin (as written in `pub use`) to the crate path
/// of the item it names. `crate::a::B` is absolute; `self::`/bare `a::B` is
/// relative to the module the `pub use` sits in.
fn resolve_local_origin(source: &str, reexport_path: &str) -> String {
    if let Some(rest) = source.strip_prefix("crate::") {
        return format!("{SUBJECT_ROOT_PATH}::{rest}");
    }
    let module = reexport_path.rsplit_once("::").map(|(m, _)| m).unwrap_or(SUBJECT_ROOT_PATH);
    let rest = source.strip_prefix("self::").unwrap_or(source);
    format!("{module}::{rest}")
}

fn walk_module_file(
    src_dir: &Path,
    module_path: &[String],
    inherited_cfg: &str,
    file: &Path,
    out: &mut Sink,
) -> Result<()> {
    let source =
        fs::read_to_string(file).wrap_err_with(|| format!("failed to read {}", file.display()))?;
    let parsed = syn::parse_file(&source)
        .wrap_err_with(|| format!("failed to parse {} with syn", file.display()))?;
    walk_items(src_dir, module_path, inherited_cfg, &parsed.items, out)
}

fn walk_items(
    src_dir: &Path,
    module_path: &[String],
    inherited_cfg: &str,
    items: &[Item],
    out: &mut Sink,
) -> Result<()> {
    for item in items {
        walk_item(src_dir, module_path, inherited_cfg, item, out)?;
    }
    Ok(())
}

fn walk_item(
    src_dir: &Path,
    module_path: &[String],
    inherited_cfg: &str,
    item: &Item,
    out: &mut Sink,
) -> Result<()> {
    macro_rules! simple {
        ($node:expr, $kind:literal) => {{
            let node = $node;
            if is_pub(&node.vis) && !is_cfg_test(&node.attrs) {
                out.push(Discovered::new(
                    $kind,
                    qualify(module_path, &node.ident.to_string()),
                    combine_cfg(inherited_cfg, &cfg_of(&node.attrs)),
                    String::new(),
                ));
            }
        }};
    }

    match item {
        Item::Struct(node) => {
            simple!(node, "struct");
            if is_pub(&node.vis) && !is_cfg_test(&node.attrs) {
                let owner = qualify(module_path, &node.ident.to_string());
                let type_cfg = combine_cfg(inherited_cfg, &cfg_of(&node.attrs));
                walk_fields(&owner, &type_cfg, &node.fields, out);
            }
        }
        Item::Enum(node) => {
            simple!(node, "enum");
            if is_pub(&node.vis) && !is_cfg_test(&node.attrs) {
                let owner = qualify(module_path, &node.ident.to_string());
                let type_cfg = combine_cfg(inherited_cfg, &cfg_of(&node.attrs));
                for variant in &node.variants {
                    if is_cfg_test(&variant.attrs) {
                        continue;
                    }
                    out.push(Discovered::member(
                        "variant",
                        owner.clone(),
                        &variant.ident.to_string(),
                        combine_cfg(&type_cfg, &cfg_of(&variant.attrs)),
                    ));
                }
            }
        }
        Item::Impl(node) => walk_impl(module_path, inherited_cfg, node, out),
        Item::Fn(node) if is_pub(&node.vis) && !is_cfg_test(&node.attrs) => {
            out.push(Discovered::new(
                "fn",
                qualify(module_path, &node.sig.ident.to_string()),
                combine_cfg(inherited_cfg, &cfg_of(&node.attrs)),
                String::new(),
            ));
        }
        Item::Const(node) => simple!(node, "const"),
        Item::Static(node) => simple!(node, "static"),
        Item::Type(node) => simple!(node, "type"),
        Item::Trait(node) => simple!(node, "trait"),
        Item::Union(node) => simple!(node, "union"),
        Item::Mod(node) => walk_mod(src_dir, module_path, inherited_cfg, node, out)?,
        Item::Use(node) => walk_use(module_path, inherited_cfg, node, out)?,
        // A `macro_rules!` with `#[macro_export]` is public surface reachable as
        // `perl_tdd_support::<name>`, independent of module visibility. Its
        // identity is the macro name; there is no signature to spell here.
        Item::Macro(node) if !is_cfg_test(&node.attrs) && has_macro_export(&node.attrs) => {
            if let Some(ident) = &node.ident {
                out.push(Discovered::new(
                    "macro",
                    format!("{SUBJECT_ROOT_PATH}::{ident}"),
                    combine_cfg(inherited_cfg, &cfg_of(&node.attrs)),
                    String::new(),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Record the public fields of a struct as `<Type>::<field>` members.
///
/// Tuple-struct fields are named by position (`Type::0`), which is how a
/// consumer spells them. Unit structs contribute nothing.
fn walk_fields(owner: &str, type_cfg: &str, fields: &syn::Fields, out: &mut Sink) {
    match fields {
        syn::Fields::Named(named) => {
            for field in &named.named {
                if !is_pub(&field.vis) || is_cfg_test(&field.attrs) {
                    continue;
                }
                if let Some(ident) = &field.ident {
                    out.push(Discovered::member(
                        "field",
                        owner.to_string(),
                        &ident.to_string(),
                        combine_cfg(type_cfg, &cfg_of(&field.attrs)),
                    ));
                }
            }
        }
        syn::Fields::Unnamed(unnamed) => {
            for (index, field) in unnamed.unnamed.iter().enumerate() {
                if !is_pub(&field.vis) || is_cfg_test(&field.attrs) {
                    continue;
                }
                out.push(Discovered::member(
                    "field",
                    owner.to_string(),
                    &index.to_string(),
                    combine_cfg(type_cfg, &cfg_of(&field.attrs)),
                ));
            }
        }
        syn::Fields::Unit => {}
    }
}

/// Record the public inherent methods of an `impl` block as
/// `<Type>::<method>` members.
///
/// Trait impls are skipped: their methods are governed through the trait row
/// (or the foreign trait's own contract), not as inherent surface of the type.
/// The owning type is spelled under the module the `impl` block lives in;
/// [`discover_surface`] later drops methods whose owner is not a discovered
/// public type at that path, so an `impl` of a private type is never surface.
fn walk_impl(module_path: &[String], inherited_cfg: &str, node: &syn::ItemImpl, out: &mut Sink) {
    if node.trait_.is_some() || is_cfg_test(&node.attrs) {
        return;
    }
    let syn::Type::Path(type_path) = node.self_ty.as_ref() else { return };
    let Some(last) = type_path.path.segments.last() else { return };
    let owner = qualify(module_path, &last.ident.to_string());
    let impl_cfg = combine_cfg(inherited_cfg, &cfg_of(&node.attrs));
    for item in &node.items {
        let syn::ImplItem::Fn(method) = item else { continue };
        if !is_pub(&method.vis) || is_cfg_test(&method.attrs) {
            continue;
        }
        out.push(Discovered::member(
            "method",
            owner.clone(),
            &method.sig.ident.to_string(),
            combine_cfg(&impl_cfg, &cfg_of(&method.attrs)),
        ));
    }
}

/// True when an item carries `#[macro_export]`.
fn has_macro_export(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("macro_export"))
}

fn walk_mod(
    src_dir: &Path,
    module_path: &[String],
    inherited_cfg: &str,
    node: &syn::ItemMod,
    out: &mut Sink,
) -> Result<()> {
    if is_cfg_test(&node.attrs) {
        return Ok(());
    }
    let name = node.ident.to_string();
    let mut child_path = module_path.to_vec();
    child_path.push(name.clone());
    // The module's own gate applies to it and to everything inside it.
    let module_cfg = combine_cfg(inherited_cfg, &cfg_of(&node.attrs));
    // A private module is not surface by itself (the crate denies
    // `unreachable_pub`), but a `pub` type inside it becomes surface through a
    // `pub use`, so its definitions are walked into the shadow rather than
    // skipped.
    let private = !is_pub(&node.vis);
    if private {
        out.private_depth += 1;
    } else {
        out.push(Discovered::new(
            "module",
            qualify(module_path, &name),
            module_cfg.clone(),
            String::new(),
        ));
    }

    let result = if let Some((_, items)) = &node.content {
        walk_items(src_dir, &child_path, &module_cfg, items, out)
    } else if let Some(file) = module_file(src_dir, &child_path) {
        walk_module_file(src_dir, &child_path, &module_cfg, &file, out)
    } else if private {
        // A private module whose file is absent cannot compile either way; it
        // has nothing a re-export could lift.
        Ok(())
    } else {
        Err(eyre!(
            "public module `{}` declared in {SUBJECT_CRATE} has no backing file; expected {}.rs \
             or {}/mod.rs under {}",
            child_path.join("::"),
            child_path.join("/"),
            child_path.join("/"),
            src_dir.display()
        ))
    };
    if private {
        out.private_depth -= 1;
    }
    result
}

fn walk_use(
    module_path: &[String],
    inherited_cfg: &str,
    node: &syn::ItemUse,
    out: &mut Sink,
) -> Result<()> {
    if !is_pub(&node.vis) || is_cfg_test(&node.attrs) {
        return Ok(());
    }
    let cfg = combine_cfg(inherited_cfg, &cfg_of(&node.attrs));
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
        if let Some(entry) = ledger_by_id.get(id)
            && entry.cfg != item.cfg
        {
            problems.push(format!(
                "cfg drift on `{id}`: source declares {:?}, ledger records {:?}",
                item.cfg, entry.cfg
            ));
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
    /// Cargo features of the governed crate this consumer activates, through
    /// its dependency spec (`features = [..]`, `default-features`) or its own
    /// `[features]` table (`"perl-tdd-support/x"`, `"perl-tdd-support?/x"`).
    pub(crate) enabled_features: BTreeSet<String>,
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
        if let Some(&second) = segments.get(1)
            && TDD_REEXPORTED_SUBMODULES.contains(&second)
        {
            out.push(second.to_string());
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

/// The crates that activate a governed Cargo feature.
pub(crate) fn feature_consumers(feature: &str, edges: &[ConsumerEdge]) -> BTreeSet<String> {
    edges
        .iter()
        .filter(|edge| edge.enabled_features.contains(feature))
        .map(|edge| edge.crate_name.clone())
        .collect()
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
        // A feature is consumed by activation, not by path reference, so its
        // consumer set is derived from the manifests rather than the symbol scan.
        let expected = if entry.api_kind == "feature" {
            feature_consumers(&entry.path, edges)
        } else {
            derived_consumers(&entry.path, &index)
        };
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
    let workspace_spec = workspace_dependency_spec(root)?;
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
        let enabled_features = enabled_features(&parsed, workspace_spec.as_ref());
        edges.push(ConsumerEdge {
            crate_name: name.to_string(),
            manifest: relative(root, &manifest),
            dep_kind,
            referenced,
            class,
            enabled_features,
        });
    }
    edges.sort();
    Ok(edges)
}

fn declared_dep_kind(manifest: &toml::Value) -> Option<String> {
    // Production over dev over build: a crate that depends on the subject in
    // more than one table is classified by its strongest edge, so a
    // test-only-import claim can never hide a production dependency.
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if manifest.get(section).and_then(|table| table.get(SUBJECT_CRATE)).is_some() {
            return Some(section.to_string());
        }
        // A platform-gated dependency (`[target.'cfg(..)'.<section>]`) is a real
        // consumer too; a later migration must not drop it just because it is
        // only declared under a target table.
        if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
            for spec in targets.values() {
                if spec.get(section).and_then(|table| table.get(SUBJECT_CRATE)).is_some() {
                    return Some(section.to_string());
                }
            }
        }
    }
    None
}

/// The `[workspace.dependencies]` spec for the governed crate, when the root
/// manifest declares one; `workspace = true` consumers inherit its
/// `features`/`default-features`.
fn workspace_dependency_spec(root: &Path) -> Result<Option<toml::Value>> {
    let path = root.join("Cargo.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let text =
        fs::read_to_string(&path).wrap_err_with(|| format!("failed to read {}", path.display()))?;
    let parsed: toml::Value =
        toml::from_str(&text).wrap_err_with(|| format!("failed to parse {}", path.display()))?;
    Ok(parsed
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|deps| deps.get(SUBJECT_CRATE))
        .cloned())
}

/// Every dependency spec for the governed crate in a manifest, across the
/// plain and `[target.'cfg(..)']` sections.
fn dependency_specs(manifest: &toml::Value) -> Vec<&toml::Value> {
    let mut specs = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(spec) = manifest.get(section).and_then(|table| table.get(SUBJECT_CRATE)) {
            specs.push(spec);
        }
        if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
            for target in targets.values() {
                if let Some(spec) = target.get(section).and_then(|table| table.get(SUBJECT_CRATE)) {
                    specs.push(spec);
                }
            }
        }
    }
    specs
}

fn spec_features(spec: &toml::Value, out: &mut BTreeSet<String>) {
    if let Some(list) = spec.get("features").and_then(toml::Value::as_array) {
        out.extend(list.iter().filter_map(toml::Value::as_str).map(str::to_string));
    }
}

fn spec_disables_default(spec: &toml::Value) -> bool {
    ["default-features", "default_features"]
        .iter()
        .any(|key| spec.get(key).and_then(toml::Value::as_bool) == Some(false))
}

/// The governed crate's features a consumer manifest activates.
///
/// Cargo's rule is followed rather than approximated: `default` is on unless
/// the effective spec says `default-features = false`; a `workspace = true`
/// spec takes `features`/`default-features` from the root; and a consumer's own
/// `[features]` table activates `x` through `"perl-tdd-support/x"` or
/// `"perl-tdd-support?/x"`.
fn enabled_features(
    manifest: &toml::Value,
    workspace_spec: Option<&toml::Value>,
) -> BTreeSet<String> {
    let mut enabled = BTreeSet::new();
    let mut default_on = true;
    for spec in dependency_specs(manifest) {
        if spec.get("workspace").and_then(toml::Value::as_bool) == Some(true)
            && let Some(root_spec) = workspace_spec
        {
            spec_features(root_spec, &mut enabled);
            if spec_disables_default(root_spec) {
                default_on = false;
            }
        }
        spec_features(spec, &mut enabled);
        if spec_disables_default(spec) {
            default_on = false;
        }
    }
    if let Some(features) = manifest.get("features").and_then(toml::Value::as_table) {
        for value in features.values() {
            let Some(list) = value.as_array() else { continue };
            for item in list.iter().filter_map(toml::Value::as_str) {
                for prefix in [format!("{SUBJECT_CRATE}/"), format!("{SUBJECT_CRATE}?/")] {
                    if let Some(feature) = item.strip_prefix(prefix.as_str()) {
                        enabled.insert(feature.to_string());
                    }
                }
            }
        }
    }
    if default_on {
        enabled.insert("default".to_string());
    }
    enabled
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
        if path.extension().is_none_or(|ext| ext != "rs") {
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
        let aliases = crate_aliases(&parsed);
        let mut visitor = ReferenceVisitor { found: &mut found, aliases: &aliases };
        syn::visit::Visit::visit_file(&mut visitor, &parsed);
    }
    Ok(found)
}

/// Names a file binds to the governed crate root besides its own name:
/// `use perl_tdd_support as support;`, `use perl_tdd_support::{self as support};`
/// and `extern crate perl_tdd_support as support;`.
///
/// Aliases are collected file-wide before references are scanned, so a path
/// spelled through the alias counts as a reference to the crate. A file scope
/// is an over-approximation of Rust's module scope; it can only add
/// references, never hide one, which is the safe direction for a consumer
/// inventory.
fn crate_aliases(file: &syn::File) -> BTreeSet<String> {
    let mut aliases = BTreeSet::new();
    for item in &file.items {
        match item {
            Item::Use(node) => collect_root_aliases(&node.tree, false, &mut aliases),
            Item::ExternCrate(node) => {
                if node.ident == SUBJECT_ROOT_PATH
                    && let Some((_, rename)) = &node.rename
                {
                    aliases.insert(rename.to_string());
                }
            }
            _ => {}
        }
    }
    aliases
}

fn collect_root_aliases(tree: &syn::UseTree, under_root: bool, out: &mut BTreeSet<String>) {
    match tree {
        syn::UseTree::Rename(rename) => {
            let renames_root = (!under_root && rename.ident == SUBJECT_ROOT_PATH)
                || (under_root && rename.ident == "self");
            if renames_root {
                out.insert(rename.rename.to_string());
            }
        }
        syn::UseTree::Path(path) if !under_root && path.ident == SUBJECT_ROOT_PATH => {
            collect_root_aliases(&path.tree, true, out);
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_root_aliases(item, under_root, out);
            }
        }
        _ => {}
    }
}

/// Records the first path segment after `perl_tdd_support::` (or after an
/// alias of it).
///
/// The first segment is the crate-root export actually being reached for —
/// `tdd_basic` for `perl_tdd_support::tdd_basic::TestGenerator`, `must` for
/// `perl_tdd_support::must` — which is the granularity edge classification and
/// the migration in #8605 both work at.
struct ReferenceVisitor<'a> {
    found: &'a mut BTreeSet<String>,
    aliases: &'a BTreeSet<String>,
}

impl ReferenceVisitor<'_> {
    fn names_root(&self, ident: &syn::Ident) -> bool {
        *ident == SUBJECT_ROOT_PATH || self.aliases.contains(&ident.to_string())
    }

    fn record_use_tree(&mut self, tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(path) => {
                self.found.insert(path.ident.to_string());
            }
            syn::UseTree::Name(name) => {
                self.found.insert(name.ident.to_string());
            }
            syn::UseTree::Rename(rename) if rename.ident != "self" => {
                self.found.insert(rename.ident.to_string());
            }
            syn::UseTree::Rename(_) => {}
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
        if let syn::UseTree::Path(path) = &node.tree
            && self.names_root(&path.ident)
        {
            self.record_use_tree(&path.tree);
        }
        syn::visit::visit_item_use(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        let mut segments = node.segments.iter();
        if let (Some(first), Some(second)) = (segments.next(), segments.next())
            && self.names_root(&first.ident)
        {
            self.found.insert(second.ident.to_string());
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
///
/// A member (`method`, `field`, `variant`) whose owning type already has a row
/// inherits that row's classification and derived consumers, so the proposal
/// is complete rather than a `TODO` skeleton: a member cannot be reached by a
/// consumer the type is not reached by, and its fate follows the type's.
fn propose(discovered: &[Discovered], ledger: &Ledger, edges: &[ConsumerEdge]) -> Result<()> {
    let governed: BTreeSet<&str> = ledger.entry.iter().map(|e| e.id.as_str()).collect();
    let owners: BTreeMap<&str, &LedgerEntry> =
        ledger.entry.iter().map(|e| (e.path.as_str(), e)).collect();
    let index = reference_index(edges);
    let missing: Vec<&Discovered> =
        discovered.iter().filter(|item| !governed.contains(item.id.as_str())).collect();
    if missing.is_empty() {
        println!("every discovered {SUBJECT_CRATE} public item already has a ledger row");
        return Ok(());
    }
    let missing_count = missing.len();
    println!("# {missing_count} unclassified item(s); add to {LEDGER_PATH}");
    for item in missing {
        let owner = if item.is_member() { owners.get(item.owner.as_str()).copied() } else { None };
        let consumers = if item.api_kind == "feature" {
            feature_consumers(&item.path, edges)
        } else {
            derived_consumers(&item.path, &index)
        };
        let consumers_toml =
            consumers.iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>().join(", ");
        println!();
        println!("[[entry]]");
        println!("id = \"{}\"", item.id);
        println!("api_kind = \"{}\"", item.api_kind);
        println!("path = \"{}\"", item.path);
        println!("cfg = \"{}\"", item.cfg);
        match owner {
            Some(owner_row) => {
                let member_name = item.path.rsplit("::").next().unwrap_or(item.path.as_str());
                let noun = match item.api_kind.as_str() {
                    "method" => "Inherent method",
                    "field" => "Public field",
                    _ => "Variant",
                };
                println!(
                    "behavior = \"{noun} `{member_name}` of `{}`; governed with its owning type.\"",
                    item.owner
                );
                println!("consumers = [{consumers_toml}]");
                println!("consumer_class = \"{}\"", owner_row.consumer_class);
                println!("compatibility = \"{}\"", owner_row.compatibility);
                println!("disposition = \"{}\"", owner_row.disposition);
                println!("replacement_owner = \"{}\"", owner_row.replacement_owner);
                println!("owner_issue = {}", owner_row.owner_issue);
                println!("exit_condition = \"{}\"", owner_row.exit_condition);
                println!("proof_command = \"{}\"", owner_row.proof_command);
            }
            None => {
                println!("behavior = \"TODO: what this item does today\"");
                println!("consumers = [{consumers_toml}]");
                println!("consumer_class = \"not_proven\"");
                println!("compatibility = \"published\"");
                println!("disposition = \"legacy_internal_until_issue\"");
                println!("replacement_owner = \"TODO: replacement API or owning package\"");
                println!("owner_issue = 8418");
                println!("exit_condition = \"TODO: when this row is removed or revisited\"");
                println!("proof_command = \"cargo test -p {SUBJECT_CRATE} --locked\"");
            }
        }
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
        let consumers = if item.api_kind == "feature" {
            feature_consumers(&item.path, edges)
        } else {
            derived_consumers(&item.path, &index)
        };
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
    let edges = discover_consumers(root)?;

    // The receipt describes the ledger and edges as loaded, so it is written
    // before any mode-specific early return: `--propose --json` and
    // `--emit-consumers --json` must not silently drop the requested file.
    if let Some(json_path) = json {
        write_json_receipt(root, json_path, &ledger, &edges)?;
    }

    if propose_rows {
        return propose(&discovered, &ledger, &edges);
    }

    if emit_consumers_rows {
        emit_consumers(&discovered, &edges);
        return Ok(());
    }

    reconcile(&discovered, &ledger)?;
    validate_derived_consumers(&ledger, &edges)?;

    let projection = render_projection(&ledger, &edges);
    let projection_path = root.join(PROJECTION_PATH);
    if write {
        if let Some(parent) = projection_path.parent().filter(|p| !p.as_os_str().is_empty()) {
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
                "enabled_features": edge.enabled_features.iter().collect::<Vec<_>>(),
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
    if let Some(parent) = absolute.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create directory {}", parent.display()))?;
    }
    fs::write(&absolute, serde_json::to_string_pretty(&receipt)?)
        .wrap_err_with(|| format!("failed to write {}", absolute.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests;
