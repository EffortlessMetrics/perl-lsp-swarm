//! Validate the `perl_parser::dead_code` API disposition ledger (#9777).
//!
//! The ledger (`policy/dead-code-api-ledger.toml`) freezes, item by item, what
//! the bounded compatibility surface does today, what it cannot represent, and
//! which authority owns the replacement. This check keeps that record honest in
//! both directions: a ledger row cannot survive the item it describes, and a new
//! public item cannot land without a disposition.
//!
//! The check is offline and read-only. It never runs the analyzer, never
//! consults GitHub, and never rewrites the ledger or its projection.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

const POLICY_PATH: &str = "policy/dead-code-api-ledger.toml";

/// Placeholder rendered for an empty projection cell.
const EMPTY_CELL: &str = "—";

/// Trait methods produced by `#[derive(...)]`. They appear in the public-API
/// baseline but are consequences of a type's own disposition rather than
/// separately decided API, so the ledger dispositions the type, not the derive.
const DERIVED_METHODS: &[&str] = &[
    "clone",
    "fmt",
    "eq",
    "ne",
    "default",
    "serialize",
    "deserialize",
    "hash",
    "cmp",
    "partial_cmp",
];

/// Distinctive references that mean a file consumes this surface. Deliberately
/// excludes the bare word `dead_code`, which collides with `#[allow(dead_code)]`
/// across the workspace and would make the scan meaningless.
const CONSUMER_NEEDLES: &[&str] = &[
    // Only explicitly *rooted*, unambiguous spellings belong here; these exist to
    // catch a fully-qualified inline reference that has no `use` item at all.
    //
    // Two things are deliberately absent. A bare type name such as `DeadCodeStats`
    // would match another crate's identically-named type and force a false
    // consumer row. `perl_parser::prelude` is a general-purpose import that says
    // nothing about this surface — it appeared even inside a string literal in
    // `xtask/tests/parser_tdd_facade_consumers.rs`. Both cases are handled
    // structurally instead, by `classify_import_path`, which knows the root.
    "perl_parser::dead_code",
    "perl_parser::dead_code_detector",
];

/// Directories scanned for consumers, relative to the repository root.
const CONSUMER_SCAN_ROOTS: &[&str] = &["crates", "xtask/src", "xtask/tests"];

/// The twelve result states the reachability programme distinguishes. Every one
/// must be dispositioned; the set is fixed here so a state cannot quietly
/// disappear from the ledger.
const REQUIRED_RESULT_STATES: &[&str] = &[
    "complete_canonical_local",
    "complete_canonical_workspace",
    "complete_local_only_workspace_deferred",
    "partial_or_degraded",
    "cancelled",
    "deadline_exceeded",
    "resource_exhausted",
    "stale_or_superseded",
    "product_failure",
    "instrument_failure",
    "complete_semantics_bounded_view",
    "incomplete_semantic_computation",
];

const REQUIRED_ITEM_KINDS: &[&str] =
    &["module", "enum", "variant", "struct", "field", "method", "function"];

#[derive(Debug, Deserialize)]
struct Ledger {
    schema_version: u32,
    policy: String,
    human_ledger: String,
    module_source: String,
    public_api_baseline: String,
    compat_corpus: String,
    canonical_path: String,
    controlling_issue: String,
    compatibility_controller: String,
    semantic_classes: Vec<String>,
    dispositions: Vec<String>,
    producer_states: Vec<String>,
    proof_ceilings: Vec<String>,
    representations: Vec<String>,
    defect_representations: Vec<String>,
    consumer_classes: Vec<String>,
    governance_paths: Vec<String>,
    unparseable_paths: Vec<String>,
    #[serde(default)]
    export_path: Vec<ExportPath>,
    #[serde(default)]
    item: Vec<Item>,
    #[serde(default)]
    result_state: Vec<ResultState>,
    #[serde(default)]
    consumer: Vec<Consumer>,
    #[serde(default)]
    not_proven: Vec<NotProven>,
}

#[derive(Debug, Deserialize)]
struct ExportPath {
    path: String,
    role: String,
    declared_at: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct Item {
    id: String,
    kind: String,
    semantic_class: String,
    disposition: String,
    producer: String,
    proof_ceiling: String,
    replacement_owner: String,
    edit_authority: String,
    lossiness: String,
    removal_condition: String,
    #[serde(default)]
    fixtures: Vec<String>,
    /// Only meaningful for the entry-point API; `false` records that a path
    /// argument is not canonical root identity.
    #[serde(default)]
    root_identity: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ResultState {
    id: String,
    analyze_file: String,
    analyze_workspace: String,
    #[serde(default)]
    defect: String,
    #[serde(default)]
    owner: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct Consumer {
    path: String,
    class: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct NotProven {
    id: String,
    claim: String,
    reason: String,
}

#[derive(Debug)]
struct Stats {
    items: usize,
    never_produced: usize,
    inert: usize,
    defect_states: usize,
    consumers: usize,
    fixtures: usize,
    not_proven: usize,
}

/// Run the ledger check, optionally regenerating the Markdown projection first.
///
/// `write` regenerates `docs/project/status/dead_code_api_ledger.md` from the
/// ledger before validating, which is how that file is maintained; without it
/// the command is entirely read-only.
pub fn run(write: bool) -> Result<()> {
    let root = project_root()?;
    if write {
        let ledger = read_ledger(&root, POLICY_PATH)?;
        let rendered = render_projection(&ledger);
        let path = root.join(&ledger.human_ledger);
        fs::write(&path, &rendered)
            .with_context(|| format!("failed to write {}", path.display()))?;
        println!("wrote {}", ledger.human_ledger);
    }
    let stats = validate(&root)?;
    println!(
        "dead-code API ledger check passed: {} dispositioned items ({} never produced, {} inert), \
         {} result states recorded as defects, {} declared consumers, {} bound fixtures, \
         {} declared NOT_PROVEN boundaries",
        stats.items,
        stats.never_produced,
        stats.inert,
        stats.defect_states,
        stats.consumers,
        stats.fixtures,
        stats.not_proven
    );
    Ok(())
}

/// Run every law against the tree and collect all violations before failing.
///
/// Violations accumulate rather than short-circuiting so one run reports every
/// disagreement, not just the first.
fn validate(root: &Path) -> Result<Stats> {
    let ledger = read_ledger(root, POLICY_PATH)?;
    let module_text = read_text(root, &ledger.module_source)?;
    let baseline_text = read_text(root, &ledger.public_api_baseline)?;
    let corpus_text = read_text(root, &ledger.compat_corpus)?;
    let human_text = read_text(root, &ledger.human_ledger)?;

    let mut violations = Vec::new();

    validate_shape(&ledger, &mut violations);
    let source_items = parse_module_surface(&module_text)
        .with_context(|| format!("failed to parse {}", ledger.module_source))?;
    validate_source_coverage(&ledger, &source_items, &mut violations);
    validate_baseline_coverage(&ledger, &baseline_text, &mut violations);
    validate_export_paths(&ledger, &baseline_text, &mut violations);
    validate_item_laws(&ledger, &mut violations);
    validate_result_states(&ledger, &mut violations);
    validate_fixtures(&ledger, &corpus_text, &mut violations);
    validate_consumers(root, &ledger, &mut violations);
    validate_human_projection(&ledger, &human_text, &mut violations);

    if !violations.is_empty() {
        eprintln!("dead-code API ledger violations:");
        for violation in &violations {
            eprintln!("  - {violation}");
        }
        bail!("dead-code API ledger check failed with {} violation(s)", violations.len());
    }

    Ok(Stats {
        items: ledger.item.len(),
        never_produced: ledger.item.iter().filter(|i| i.producer == "never_produced").count(),
        inert: ledger.item.iter().filter(|i| i.producer == "inert").count(),
        defect_states: ledger
            .result_state
            .iter()
            .filter(|s| {
                ledger.defect_representations.contains(&s.analyze_file)
                    || ledger.defect_representations.contains(&s.analyze_workspace)
            })
            .count(),
        consumers: ledger.consumer.len(),
        fixtures: ledger.item.iter().flat_map(|i| i.fixtures.iter()).collect::<BTreeSet<_>>().len(),
        not_proven: ledger.not_proven.len(),
    })
}

/// Parse the canonical ledger, naming the file when it will not parse.
fn read_ledger(root: &Path, rel: &str) -> Result<Ledger> {
    let text = read_text(root, rel)?;
    toml::from_str(&text).with_context(|| format!("failed to parse {rel}"))
}

/// Read one repository-relative file, naming the absolute path on failure.
fn read_text(root: &Path, rel: &str) -> Result<String> {
    let path = root.join(rel);
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

// ---------------------------------------------------------------------------
// Shape
// ---------------------------------------------------------------------------

/// Check the ledger's own structural invariants: schema version, self-reference,
/// canonical path, export-path roles, and that every declared vocabulary term
/// used elsewhere actually exists.
fn validate_shape(ledger: &Ledger, violations: &mut Vec<String>) {
    if ledger.schema_version != 1 {
        violations.push(format!(
            "schema_version must be 1 while this contract is current, found {}",
            ledger.schema_version
        ));
    }
    if ledger.policy != POLICY_PATH {
        violations
            .push(format!("policy self-reference must be {POLICY_PATH}, found {}", ledger.policy));
    }
    if ledger.canonical_path != "perl_parser::dead_code" {
        violations.push(format!(
            "canonical_path must be perl_parser::dead_code, found {}",
            ledger.canonical_path
        ));
    }
    for representation in &ledger.defect_representations {
        if !ledger.representations.contains(representation) {
            violations.push(format!(
                "defect_representations contains {representation}, which is not a declared representation"
            ));
        }
    }
    if ledger.export_path.is_empty() {
        violations.push("no export paths declared; the surface is reachable somehow".to_string());
    }
    let mut roles = BTreeSet::new();
    for export in &ledger.export_path {
        if export.declared_at.is_empty() {
            violations.push(format!("export path {} has no declared_at", export.path));
        }
        if export.note.trim().is_empty() {
            violations.push(format!(
                "export path {} has no note; each path must say what distinguishes it from the \
                 canonical one",
                export.path
            ));
        }
        if !roles.insert(export.role.clone()) && export.role == "canonical" {
            violations.push("more than one canonical export path declared".to_string());
        }
    }
    if !roles.contains("canonical") {
        violations.push("no canonical export path declared".to_string());
    }
    for np in &ledger.not_proven {
        if np.claim.trim().is_empty() || np.reason.trim().is_empty() {
            violations
                .push(format!("NOT_PROVEN row {} must carry both a claim and a reason", np.id));
        }
    }
}

// ---------------------------------------------------------------------------
// L1 — source coverage
// ---------------------------------------------------------------------------

/// One public item discovered in the module source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SourceItem {
    id: String,
    kind: String,
}

/// Extract the public surface of the module with `syn`, so the item inventory
/// comes from the code rather than from a hand-kept list.
fn parse_module_surface(text: &str) -> Result<BTreeSet<SourceItem>> {
    let file: syn::File = syn::parse_str(text).context("module source is not parseable Rust")?;
    let mut items = BTreeSet::new();
    // The module itself is always dispositioned.
    items.insert(SourceItem { id: "dead_code".to_string(), kind: "module".to_string() });
    collect_public_items(&file.items, "", &mut items)?;
    Ok(items)
}

/// Record every public item in `source`.
///
/// `prefix` qualifies the ids so an item reached through an inline `pub mod`
/// keeps a ledger identity distinct from a same-named item at module level.
/// Recording only the enclosing module would let the governed public surface
/// grow without any row describing the items it gained.
fn collect_public_items(
    source: &[syn::Item],
    prefix: &str,
    items: &mut BTreeSet<SourceItem>,
) -> Result<()> {
    for item in source {
        match item {
            syn::Item::Enum(node) if is_public(&node.vis) => {
                let name = node.ident.to_string();
                items
                    .insert(SourceItem { id: format!("{prefix}{name}"), kind: "enum".to_string() });
                for variant in &node.variants {
                    items.insert(SourceItem {
                        id: format!("{prefix}{name}::{}", variant.ident),
                        kind: "variant".to_string(),
                    });
                }
            }
            syn::Item::Struct(node) if is_public(&node.vis) => {
                let name = node.ident.to_string();
                items.insert(SourceItem {
                    id: format!("{prefix}{name}"),
                    kind: "struct".to_string(),
                });
                for (index, field) in node.fields.iter().enumerate() {
                    if !is_public(&field.vis) {
                        continue;
                    }
                    // Tuple-struct fields are positional and still public API.
                    let field_id = match field.ident.as_ref() {
                        Some(ident) => ident.to_string(),
                        None => index.to_string(),
                    };
                    items.insert(SourceItem {
                        id: format!("{prefix}{name}::{field_id}"),
                        kind: "field".to_string(),
                    });
                }
            }
            syn::Item::Union(node) if is_public(&node.vis) => {
                let name = node.ident.to_string();
                items.insert(SourceItem {
                    id: format!("{prefix}{name}"),
                    kind: "union".to_string(),
                });
                for field in &node.fields.named {
                    if !is_public(&field.vis) {
                        continue;
                    }
                    if let Some(ident) = field.ident.as_ref() {
                        items.insert(SourceItem {
                            id: format!("{prefix}{name}::{ident}"),
                            kind: "field".to_string(),
                        });
                    }
                }
            }
            syn::Item::Fn(node) if is_public(&node.vis) => {
                items.insert(SourceItem {
                    id: format!("{prefix}{}", node.sig.ident),
                    kind: "function".to_string(),
                });
            }
            syn::Item::Type(node) if is_public(&node.vis) => {
                items.insert(SourceItem {
                    id: format!("{prefix}{}", node.ident),
                    kind: "type_alias".to_string(),
                });
            }
            syn::Item::Const(node) if is_public(&node.vis) => {
                items.insert(SourceItem {
                    id: format!("{prefix}{}", node.ident),
                    kind: "constant".to_string(),
                });
            }
            syn::Item::Static(node) if is_public(&node.vis) => {
                items.insert(SourceItem {
                    id: format!("{prefix}{}", node.ident),
                    kind: "static".to_string(),
                });
            }
            syn::Item::Trait(node) if is_public(&node.vis) => {
                let name = node.ident.to_string();
                items.insert(SourceItem {
                    id: format!("{prefix}{name}"),
                    kind: "trait".to_string(),
                });
                for inner in &node.items {
                    let member = match inner {
                        syn::TraitItem::Fn(method) => Some(method.sig.ident.to_string()),
                        syn::TraitItem::Const(konst) => Some(konst.ident.to_string()),
                        syn::TraitItem::Type(ty) => Some(ty.ident.to_string()),
                        _ => None,
                    };
                    if let Some(member) = member {
                        items.insert(SourceItem {
                            id: format!("{prefix}{name}::{member}"),
                            kind: "trait_member".to_string(),
                        });
                    }
                }
            }
            syn::Item::Mod(node) if is_public(&node.vis) => {
                let name = format!("{prefix}{}", node.ident);
                items.insert(SourceItem { id: name.clone(), kind: "submodule".to_string() });
                match node.content.as_ref() {
                    Some((_, inner)) => {
                        collect_public_items(inner, &format!("{name}::"), items)?;
                    }
                    // An out-of-line public submodule puts public API in a file
                    // this ledger does not govern, so L1 could not see it at
                    // all. Refuse rather than report a surface known to be
                    // partial.
                    None => bail!(
                        "public submodule `{name}` is declared out-of-line; this ledger governs \
                         a single file, so the items it exports cannot be inventoried"
                    ),
                }
            }
            syn::Item::Use(node) if is_public(&node.vis) => {
                for name in use_tree_names(&node.tree) {
                    items.insert(SourceItem {
                        id: format!("{prefix}{name}"),
                        kind: "reexport".to_string(),
                    });
                }
            }
            // A `macro_rules!` definition reaches the public surface only with
            // `#[macro_export]`; without it the macro exports nothing. Anything
            // else in item position is an *invocation*, and `syn` sees the call
            // rather than the expansion — it may add public constants, types or
            // functions that nothing here can enumerate, so it fails closed the
            // same way an uninterpreted item does.
            syn::Item::Macro(node) => {
                let is_definition = node.mac.path.is_ident("macro_rules");
                let is_exported = node.attrs.iter().any(is_macro_export);
                match (is_definition, is_exported) {
                    (true, true) => {
                        if let Some(ident) = node.ident.as_ref() {
                            items.insert(SourceItem {
                                id: format!("{prefix}{ident}"),
                                kind: "exported_macro".to_string(),
                            });
                        }
                    }
                    (true, false) => {}
                    (false, _) => {
                        items.insert(SourceItem {
                            id: format!("{prefix}<unexpanded macro item #{}>", items.len()),
                            kind: "unsupported_public_form".to_string(),
                        });
                    }
                }
            }
            syn::Item::Impl(node) if node.trait_.is_none() => {
                let Some(self_name) = type_ident(&node.self_ty) else {
                    continue;
                };
                for inner in &node.items {
                    let member = match inner {
                        syn::ImplItem::Fn(method) if is_public(&method.vis) => {
                            Some((method.sig.ident.to_string(), "method"))
                        }
                        syn::ImplItem::Const(konst) if is_public(&konst.vis) => {
                            Some((konst.ident.to_string(), "associated_constant"))
                        }
                        syn::ImplItem::Type(ty) if is_public(&ty.vis) => {
                            Some((ty.ident.to_string(), "associated_type"))
                        }
                        _ => None,
                    };
                    if let Some((member, kind)) = member {
                        items.insert(SourceItem {
                            id: format!("{prefix}{self_name}::{member}"),
                            kind: kind.to_string(),
                        });
                    }
                }
            }
            // Fail closed: a public item in a form this parser does not model
            // must not pass silently. It is emitted with a kind the ledger
            // vocabulary rejects, so L1 reports it as unclassified rather than
            // letting the surface grow unobserved.
            // Tokens `syn` could not interpret. There is no visibility to
            // inspect, so it cannot be ruled out as public API.
            syn::Item::Verbatim(_) => {
                items.insert(SourceItem {
                    id: format!("{prefix}<uninterpreted item #{}>", items.len()),
                    kind: "unsupported_public_form".to_string(),
                });
            }
            // `extern "C" { … }` carries no visibility of its own, but its
            // foreign items can be public.
            syn::Item::ForeignMod(node) => {
                let has_public_item = node.items.iter().any(|item| match item {
                    syn::ForeignItem::Fn(node) => is_public(&node.vis),
                    syn::ForeignItem::Static(node) => is_public(&node.vis),
                    syn::ForeignItem::Type(node) => is_public(&node.vis),
                    _ => false,
                });
                if has_public_item {
                    items.insert(SourceItem {
                        id: format!("{prefix}<public foreign item #{}>", items.len()),
                        kind: "unsupported_public_form".to_string(),
                    });
                }
            }
            other => {
                if let Some(vis) = item_visibility(other)
                    && is_public(vis)
                {
                    items.insert(SourceItem {
                        id: format!("{prefix}<unmodelled public item #{}>", items.len()),
                        kind: "unsupported_public_form".to_string(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Every name a `pub use` tree introduces into the module's public surface.
fn use_tree_names(tree: &syn::UseTree) -> Vec<String> {
    match tree {
        syn::UseTree::Path(path) => use_tree_names(&path.tree),
        syn::UseTree::Name(name) => vec![name.ident.to_string()],
        syn::UseTree::Rename(rename) => vec![rename.rename.to_string()],
        syn::UseTree::Group(group) => group.items.iter().flat_map(use_tree_names).collect(),
        // A public glob re-export is unbounded: it cannot be enumerated here, so
        // it is surfaced as a single unclassifiable name and fails L1.
        syn::UseTree::Glob(_) => vec!["<public glob re-export>".to_string()],
    }
}

/// Whether an attribute is `#[macro_export]`, which makes a macro public API.
fn is_macro_export(attr: &syn::Attribute) -> bool {
    attr.path().is_ident("macro_export")
}

/// Visibility of the item forms that can carry one, for the fail-closed arm.
fn item_visibility(item: &syn::Item) -> Option<&syn::Visibility> {
    match item {
        syn::Item::Const(node) => Some(&node.vis),
        syn::Item::Enum(node) => Some(&node.vis),
        syn::Item::ExternCrate(node) => Some(&node.vis),
        syn::Item::Fn(node) => Some(&node.vis),
        syn::Item::Mod(node) => Some(&node.vis),
        syn::Item::Static(node) => Some(&node.vis),
        syn::Item::Struct(node) => Some(&node.vis),
        syn::Item::Trait(node) => Some(&node.vis),
        syn::Item::TraitAlias(node) => Some(&node.vis),
        syn::Item::Type(node) => Some(&node.vis),
        syn::Item::Union(node) => Some(&node.vis),
        syn::Item::Use(node) => Some(&node.vis),
        _ => None,
    }
}

/// Whether a visibility marks an item as part of the crate's public API.
fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

/// The final path segment of a type, used to name the `impl` block a method
/// belongs to.
fn type_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// L1 — the public items in the module source and the ledger's rows are the same
/// set, with matching kinds. A new public item fails until dispositioned; a row
/// naming an item that no longer exists fails until retired.
fn validate_source_coverage(
    ledger: &Ledger,
    source_items: &BTreeSet<SourceItem>,
    violations: &mut Vec<String>,
) {
    let ledger_kinds: BTreeMap<&str, &str> =
        ledger.item.iter().map(|i| (i.id.as_str(), i.kind.as_str())).collect();

    if ledger_kinds.len() != ledger.item.len() {
        violations.push("ledger contains duplicate item ids".to_string());
    }

    for source in source_items {
        match ledger_kinds.get(source.id.as_str()) {
            None => violations.push(format!(
                "L1: public item `{}` ({}) has no ledger disposition; every public item must be classified",
                source.id, source.kind
            )),
            Some(kind) if *kind != source.kind => violations.push(format!(
                "L1: ledger classifies `{}` as {} but the source declares a {}",
                source.id, kind, source.kind
            )),
            Some(_) => {}
        }
    }

    let source_ids: BTreeSet<&str> = source_items.iter().map(|s| s.id.as_str()).collect();
    for item in &ledger.item {
        if !source_ids.contains(item.id.as_str()) {
            violations.push(format!(
                "L1: ledger row `{}` names no public item in the module source; a retired item must \
                 leave the ledger with its removal, not outlive it",
                item.id
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// L2 — public-API baseline coverage
// ---------------------------------------------------------------------------

/// Reduce a public-API baseline line to the ledger item id it corresponds to,
/// or `None` when the line does not describe an item this ledger owns.
fn baseline_item_id(line: &str, canonical_path: &str) -> Option<String> {
    let rest = line.strip_prefix("pub ")?;
    // Strip the optional item keyword so what remains starts with the subject path.
    let rest = ["mod ", "enum ", "struct ", "fn ", "trait ", "type ", "const ", "static "]
        .iter()
        .find_map(|kw| rest.strip_prefix(*kw))
        .unwrap_or(rest);

    // The subject path ends at the first `(`, `<`, `:` (field type) or space.
    let subject: String =
        rest.chars().take_while(|c| !matches!(c, '(' | '<' | ' ')).collect::<String>();
    // A field row is `path::name: Type`; trim a trailing `:`.
    let subject = subject.trim_end_matches(':');

    if subject == canonical_path {
        return Some("dead_code".to_string());
    }
    // Require a real path boundary so `perl_parser::dead_code_detector` — the
    // compatibility alias, which duplicates every row — is not matched here.
    let tail = subject.strip_prefix(canonical_path)?.strip_prefix("::")?;
    if tail.is_empty() {
        return None;
    }
    // Drop derive-generated trait methods.
    if let Some((_, last)) = tail.rsplit_once("::")
        && DERIVED_METHODS.contains(&last)
    {
        return None;
    }
    Some(tail.to_string())
}

/// L2 — every `perl_parser::dead_code` declaration in the public-API baseline has
/// a ledger row, with a vacuity guard so a baseline format change cannot leave
/// this law checking nothing.
fn validate_baseline_coverage(ledger: &Ledger, baseline_text: &str, violations: &mut Vec<String>) {
    let ledger_ids: BTreeSet<&str> = ledger.item.iter().map(|i| i.id.as_str()).collect();
    let mut missing = BTreeSet::new();
    let mut matched = BTreeSet::new();
    for line in baseline_text.lines() {
        if let Some(id) = baseline_item_id(line.trim(), &ledger.canonical_path) {
            if !ledger_ids.contains(id.as_str()) {
                missing.insert(id.clone());
            }
            matched.insert(id);
        }
    }
    // Vacuity guard: if the baseline format moves and nothing matches any more,
    // L2 would pass by checking nothing. The surface always has at least the
    // module, the four types and the two free/associated entry points.
    if matched.len() < 6 {
        violations.push(format!(
            "L2: only {} item(s) recognised in {}; the baseline format changed and this law is no \
             longer checking anything",
            matched.len(),
            ledger.public_api_baseline
        ));
    }
    for id in missing {
        violations.push(format!(
            "L2: `{id}` is exported per {} but has no ledger disposition",
            ledger.public_api_baseline
        ));
    }
}

// ---------------------------------------------------------------------------
// L11 — export-path completeness
// ---------------------------------------------------------------------------

/// Every path prefix under which the public-API baseline exposes this surface.
///
/// Two shapes reach it and both are routes a caller can depend on:
///
/// * a module route — `perl_parser::dead_code`, and any alias of it such as
///   `perl_parser::compat::dead_code_detector`;
/// * a type-re-export namespace — `perl_parser::prelude`, which republishes the
///   types without republishing the module.
///
/// A path is reduced at its leftmost surface segment, so
/// `perl_parser::dead_code::DeadCodeType::clone` and
/// `perl_parser::prelude::DeadCode` both collapse to the route that carries
/// them. Deriving the set from the baseline rather than from the ledger is the
/// point: a newly added alias appears here on its own and has to be declared.
fn baseline_export_paths(ledger: &Ledger, baseline_text: &str) -> BTreeSet<String> {
    const MODULE_TAILS: [&str; 2] = ["dead_code", "dead_code_detector"];
    // Type names come from the ledger's own rows, so a type added to the
    // surface widens this derivation automatically.
    let type_names: BTreeSet<&str> = ledger
        .item
        .iter()
        .filter(|item| {
            matches!(item.kind.as_str(), "enum" | "struct" | "union" | "trait" | "type_alias")
        })
        .map(|item| item.id.as_str())
        .collect();

    let mut paths = BTreeSet::new();
    for line in baseline_text.lines() {
        let tokens = line.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'));
        for token in tokens {
            let segments: Vec<&str> = token.split("::").filter(|s| !s.is_empty()).collect();
            if segments.first() != Some(&"perl_parser") {
                continue;
            }
            for (index, segment) in segments.iter().enumerate().skip(1) {
                // A module route includes its own tail; a type route stops
                // before the type, because the namespace is what is exported.
                let route = if MODULE_TAILS.contains(segment) {
                    Some(&segments[..=index])
                } else if type_names.contains(segment) {
                    Some(&segments[..index])
                } else {
                    None
                };
                if let Some(route) = route {
                    if route.len() > 1 {
                        paths.insert(route.join("::"));
                    }
                    break;
                }
            }
        }
    }
    paths
}

/// L11 — every route the public-API baseline exposes has an `export_path` row,
/// and no declared route has stopped existing.
///
/// Without this, `export_path` was a hand-kept list: adding a public alias in
/// the facade left the ledger green while the reachable surface grew.
fn validate_export_paths(ledger: &Ledger, baseline_text: &str, violations: &mut Vec<String>) {
    let derived = baseline_export_paths(ledger, baseline_text);
    // Vacuity guard: the canonical route is always exported. If it is not in
    // the derived set the baseline format has moved and this law is checking
    // nothing, which must fail rather than pass.
    if !derived.contains(&ledger.canonical_path) {
        violations.push(format!(
            "L11: the canonical route {} is not derivable from {}; the baseline format changed \
             and this law is no longer checking anything",
            ledger.canonical_path, ledger.public_api_baseline
        ));
        return;
    }

    let declared: BTreeSet<&str> =
        ledger.export_path.iter().map(|export| export.path.as_str()).collect();
    for route in &derived {
        if !declared.contains(route.as_str()) {
            violations.push(format!(
                "L11: {} exposes `{route}`, which no export_path row declares",
                ledger.public_api_baseline
            ));
        }
    }
    for route in &declared {
        if !derived.contains(*route) {
            violations.push(format!(
                "L11: export_path `{route}` is not exposed by {}; the row is stale",
                ledger.public_api_baseline
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// L3, L4, L6 — per-item laws
// ---------------------------------------------------------------------------

/// L3, L4 and L6 — per-item laws: an unproduced item may not be presented as
/// working and must carry a negative control; no item carries edit authority;
/// inert configuration declares itself and never claims root identity.
fn validate_item_laws(ledger: &Ledger, violations: &mut Vec<String>) {
    for item in &ledger.item {
        let id = &item.id;
        require_member(id, "kind", &item.kind, REQUIRED_ITEM_KINDS, violations);
        require_member_owned(
            id,
            "semantic_class",
            &item.semantic_class,
            &ledger.semantic_classes,
            violations,
        );
        require_member_owned(
            id,
            "disposition",
            &item.disposition,
            &ledger.dispositions,
            violations,
        );
        require_member_owned(id, "producer", &item.producer, &ledger.producer_states, violations);
        require_member_owned(
            id,
            "proof_ceiling",
            &item.proof_ceiling,
            &ledger.proof_ceilings,
            violations,
        );

        if item.lossiness.trim().is_empty() {
            violations
                .push(format!("`{id}` declares no lossiness; \"none\" must be stated explicitly"));
        }
        if item.removal_condition.trim().is_empty() {
            violations.push(format!("`{id}` declares no removal condition or exit"));
        }
        if item.replacement_owner.trim().is_empty() {
            violations.push(format!("`{id}` names no replacement owner"));
        }

        // L4: no item on this surface authorizes an edit. #4205/#4208 own edit
        // authorization and nothing here may acquire it.
        if item.edit_authority != "none" {
            violations.push(format!(
                "L4: `{id}` claims edit_authority `{}`; this compatibility surface authorizes no edit",
                item.edit_authority
            ));
        }

        // L3: a variant nothing constructs may not be presented as working, and
        // must be held down by an executable negative control.
        if item.producer == "never_produced" {
            if !matches!(item.disposition.as_str(), "deprecate" | "remove") {
                violations.push(format!(
                    "L3: `{id}` is never produced but is dispositioned `{}`; an unimplemented item \
                     may only be deprecated or removed, never advertised as retained behavior",
                    item.disposition
                ));
            }
            if item.proof_ceiling != "none" {
                violations.push(format!(
                    "L3: `{id}` is never produced but claims proof ceiling `{}`",
                    item.proof_ceiling
                ));
            }
            if item.fixtures.is_empty() {
                violations.push(format!(
                    "L3: `{id}` is never produced but has no negative control; the claim would be \
                     unfalsifiable"
                ));
            }
        }

        // L6: inert configuration must say so, and must not be mapped to root identity.
        if item.producer == "inert" {
            if item.disposition != "deprecate" && item.disposition != "remove" {
                violations.push(format!(
                    "L6: `{id}` is inert but dispositioned `{}`; configuration nothing reads may \
                     not be retained as if it worked",
                    item.disposition
                ));
            }
            if item.fixtures.is_empty() {
                violations
                    .push(format!("L6: `{id}` is inert but has no fixture proving inertness"));
            }
        }
        // An inert configuration item must state the root-identity disposition
        // explicitly. `root_identity` is optional, so leaving it out would pass
        // the `Some(true)` rejection below while silently dropping the record
        // that a path is not canonical root identity.
        if item.producer == "inert" && item.root_identity.is_none() {
            violations.push(format!(
                "L6: `{id}` is inert but does not declare `root_identity`; the disposition must be \
                 explicit, not absent"
            ));
        }
        if item.root_identity == Some(true) {
            violations.push(format!(
                "L6: `{id}` claims root identity; a path is a configuration candidate, not canonical \
                 root identity (#10871)"
            ));
        }

        // A produced item must be reachable through a proof class.
        if item.producer == "produced" && item.kind == "variant" && item.proof_ceiling == "none" {
            violations.push(format!(
                "`{id}` is produced but claims no proof ceiling; a produced finding class must \
                 record what its value can support"
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// L5 — result-state mapping
// ---------------------------------------------------------------------------

/// L5 — all twelve result states are dispositioned, every collapse names a defect
/// and an owner, and every non-collapsing state explains itself.
fn validate_result_states(ledger: &Ledger, violations: &mut Vec<String>) {
    let present: BTreeSet<&str> = ledger.result_state.iter().map(|s| s.id.as_str()).collect();
    if present.len() != ledger.result_state.len() {
        violations.push("duplicate result-state rows".to_string());
    }
    for required in REQUIRED_RESULT_STATES {
        if !present.contains(required) {
            violations.push(format!(
                "L5: result state `{required}` has no disposition; a state the surface cannot \
                 represent must be recorded, not omitted"
            ));
        }
    }
    for state in &ledger.result_state {
        if !REQUIRED_RESULT_STATES.contains(&state.id.as_str()) {
            violations.push(format!("L5: unknown result state `{}`", state.id));
        }
        require_member_owned(
            &state.id,
            "analyze_file",
            &state.analyze_file,
            &ledger.representations,
            violations,
        );
        require_member_owned(
            &state.id,
            "analyze_workspace",
            &state.analyze_workspace,
            &ledger.representations,
            violations,
        );

        let collapses = ledger.defect_representations.contains(&state.analyze_file)
            || ledger.defect_representations.contains(&state.analyze_workspace);
        if collapses {
            if state.defect.trim().is_empty() {
                violations.push(format!(
                    "L5: `{}` collapses into an ordinary-looking result but names no defect; a \
                     hidden state must be recorded as a defect, never accepted silently",
                    state.id
                ));
            }
            if state.owner.trim().is_empty() {
                violations.push(format!(
                    "L5: `{}` records a defect with no owner to route it to",
                    state.id
                ));
            }
            // A collapse is explained by its defect; `note` stays optional there.
        } else if state.note.trim().is_empty() {
            violations.push(format!(
                "L5: `{}` neither records a defect nor explains itself; a state disposition must \
                 say why it is representable, lossy or unreachable",
                state.id
            ));
        }
        if !collapses && !state.defect.trim().is_empty() {
            violations.push(format!(
                "L5: `{}` names a defect but neither entry point collapses; the defect field would \
                 be unreadable",
                state.id
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// L9, L10 — fixture binding
// ---------------------------------------------------------------------------

/// Names of the `#[test]` functions in the compatibility corpus.
///
/// Derived from parsed Rust, not from comment text: a fixture identity is only
/// real if an executable test carries it. Section banners and doc comments are
/// deliberately non-authoritative, so deleting a test while leaving its banner
/// behind breaks the binding instead of silently preserving it.
fn corpus_test_functions(corpus_text: &str) -> Result<BTreeSet<String>> {
    let file: syn::File =
        syn::parse_str(corpus_text).context("compatibility corpus is not parseable Rust")?;
    let mut names = BTreeSet::new();
    for item in &file.items {
        if let syn::Item::Fn(node) = item
            && node.attrs.iter().any(|attr| attr.path().is_ident("test"))
        {
            names.insert(node.sig.ident.to_string());
        }
    }
    Ok(names)
}

/// The ledger writes fixture ids in kebab case (`dcapi-entry-point-is-inert`);
/// the corpus writes test functions in snake case, optionally with a longer
/// descriptive tail (`dcapi_entry_point_is_inert_and_non_vacuous`). A fixture is
/// bound when some test function equals its snake form or extends it at an
/// identifier boundary.
fn fixture_is_bound(fixture_id: &str, test_functions: &BTreeSet<String>) -> bool {
    let snake = fixture_id.replace('-', "_");
    test_functions.iter().any(|name| name == &snake || name.starts_with(&format!("{snake}_")))
}

/// L9 and L10 — every fixture a row cites names a real `#[test]` function, and
/// every `dcapi_*` test supports at least one recorded claim.
fn validate_fixtures(ledger: &Ledger, corpus_text: &str, violations: &mut Vec<String>) {
    let test_functions = match corpus_test_functions(corpus_text) {
        Ok(names) => names,
        Err(error) => {
            violations.push(format!("L9: cannot parse {}: {error}", ledger.compat_corpus));
            return;
        }
    };
    if test_functions.is_empty() {
        violations.push(format!(
            "L9: no `#[test]` functions found in {}; the ledger would be unbound",
            ledger.compat_corpus
        ));
        return;
    }

    let mut cited = BTreeSet::new();
    for item in &ledger.item {
        for fixture in &item.fixtures {
            cited.insert(fixture.clone());
            if !fixture_is_bound(fixture, &test_functions) {
                violations.push(format!(
                    "L9: `{}` cites fixture `{fixture}`, which names no `#[test]` function in {}; \
                     a comment banner is not proof",
                    item.id, ledger.compat_corpus
                ));
            }
        }
    }

    // L10: every dcapi test must support a recorded claim, so proof cannot drift
    // loose from the ledger it exists to hold down.
    for name in &test_functions {
        if !name.starts_with("dcapi_") {
            continue;
        }
        if !cited.iter().any(|fixture| {
            fixture_is_bound(fixture, &test_functions) && {
                let snake = fixture.replace('-', "_");
                name == &snake || name.starts_with(&format!("{snake}_"))
            }
        }) {
            violations.push(format!(
                "L10: `{name}` is an executable fixture that no ledger row cites; proof that \
                 supports no recorded claim is either mis-filed or the claim is missing"
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// L7 — consumer inventory
// ---------------------------------------------------------------------------

/// L7 — the declared consumer inventory matches the tree.
///
/// Scans the consumer roots for files referencing the surface and fails on any
/// that is undeclared. Traversal, read and parse failures are violations, not
/// skips: a file the scan cannot see has not been shown to be a non-consumer.
fn validate_consumers(root: &Path, ledger: &Ledger, violations: &mut Vec<String>) {
    let declared: BTreeMap<&str, &str> =
        ledger.consumer.iter().map(|c| (c.path.as_str(), c.class.as_str())).collect();
    if declared.len() != ledger.consumer.len() {
        violations.push("duplicate consumer rows".to_string());
    }

    for consumer in &ledger.consumer {
        require_member_owned(
            &consumer.path,
            "class",
            &consumer.class,
            &ledger.consumer_classes,
            violations,
        );
        if !root.join(&consumer.path).exists() {
            violations.push(format!(
                "L7: declared consumer {} does not exist; a stale inventory reads as evidence it is not",
                consumer.path
            ));
        }
        if consumer.note.trim().is_empty() {
            violations
                .push(format!("L7: consumer {} has no note explaining its class", consumer.path));
        }
    }

    let mut found = BTreeSet::new();
    let mut declared_unparseable_seen = BTreeSet::new();
    for scan_root in CONSUMER_SCAN_ROOTS {
        let dir = root.join(scan_root);
        if !dir.exists() {
            violations.push(format!("L7: consumer scan root {scan_root} is missing"));
            continue;
        }
        // Traversal, read and parse failures are recorded as violations rather
        // than skipped: a scan that cannot see a file has not established that
        // the file is not a consumer. Missing or instrument-failed evidence is
        // NOT_PROVEN, never a pass.
        for entry in WalkDir::new(&dir) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    violations.push(format!(
                        "L7: cannot traverse under {scan_root}: {error}; the consumer inventory \
                         would be evidence of nothing"
                    ));
                    continue;
                }
            };
            let path = entry.path();
            if !path.is_file() || path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            // Skip the target directory if a nested one exists.
            if path.components().any(|c| c.as_os_str() == "target") {
                continue;
            }
            let display = path.strip_prefix(root).unwrap_or(path).to_string_lossy().to_string();
            let text = match fs::read_to_string(path) {
                Ok(text) => text,
                Err(error) => {
                    violations.push(format!("L7: cannot read {display}: {error}"));
                    continue;
                }
            };
            // The exemption is conditional on the file *still* failing to parse.
            // A declared path that becomes valid Rust falls through to the normal
            // structural scan and is left out of `declared_unparseable_seen`, so
            // the stale declaration is reported rather than silently continuing
            // to suppress import analysis.
            if ledger.unparseable_paths.contains(&display)
                && syn::parse_str::<syn::File>(&text).is_err()
            {
                // Still text-scanned for direct references: declaring a file is
                // not a way to stop looking at it.
                if CONSUMER_NEEDLES.iter().any(|needle| contains_word(&text, needle)) {
                    found.insert(display.replace('\\', "/"));
                }
                declared_unparseable_seen.insert(display.clone());
                continue;
            }
            let inside_owning_crate = display.starts_with("crates/perl-parser/");
            match references_surface(&text, inside_owning_crate) {
                Ok(true) => {
                    found.insert(display.replace('\\', "/"));
                }
                Ok(false) => {}
                Err(error) => violations.push(format!(
                    "L7: cannot analyse {display}: {error}; a file the scan cannot parse is not \
                     evidence that it is not a consumer"
                )),
            }
        }
    }

    for path in &found {
        // The subject module and this ledger's own tooling name the surface in
        // order to govern it. The exemption list lives in the ledger so it stays
        // reviewable; it is checked for staleness below.
        if ledger.governance_paths.iter().any(|g| g == path) {
            continue;
        }
        if !declared.contains_key(path.as_str()) {
            violations.push(format!(
                "L7: {path} references the dead-code surface but is not in the consumer inventory; \
                 an undeclared consumer — production or otherwise — must be dispositioned before it lands"
            ));
        }
    }
    for declared in &ledger.unparseable_paths {
        if !declared_unparseable_seen.contains(declared) {
            violations.push(format!(
                "L7: {declared} is declared unparseable but the scan did not reach it, or it now \
                 parses as valid Rust; a stale exemption can hide a real consumer"
            ));
        }
    }

    for governance in &ledger.governance_paths {
        if !root.join(governance).exists() {
            violations.push(format!("L7: governance path {governance} does not exist"));
        } else if governance != &ledger.module_source && !found.contains(governance.as_str()) {
            // The module source is the subject of the ledger, not something the
            // scan is expected to detect: it declares the surface rather than
            // importing it. Every *other* governance path is tooling that names
            // the surface, so a stale entry there could hide a real consumer.
            violations.push(format!(
                "L7: governance path {governance} no longer references the surface; a stale \
                 exemption can hide a real consumer"
            ));
        }
        if declared.contains_key(governance.as_str()) {
            violations.push(format!(
                "L7: {governance} is both a governance path and a declared consumer; it must be one \
                 or the other"
            ));
        }
    }

    for consumer in &ledger.consumer {
        // Re-export and dormant rows may live in files the needle scan also
        // finds; a declared row that the scan cannot see is a stale record.
        if !found.contains(consumer.path.as_str()) && consumer.class != "independent_implementation"
        {
            violations.push(format!(
                "L7: declared consumer {} no longer references the surface; retire the row",
                consumer.path
            ));
        }
        if consumer.class == "independent_implementation" && found.contains(consumer.path.as_str())
        {
            violations.push(format!(
                "L7: {} is declared an independent implementation but now references this surface; \
                 that is a duplicate-authority change, not a documentation change",
                consumer.path
            ));
        }
    }
}

/// The surface type names, used to recognise an import that names a type
/// directly rather than the module.
const SURFACE_TYPES: &[&str] =
    &["DeadCode", "DeadCodeType", "DeadCodeAnalysis", "DeadCodeStats", "DeadCodeDetector"];

/// Import-structure facts about one candidate file.
#[derive(Debug, Default)]
struct ImportFacts {
    /// A `use` tree reaches the module by any spelling, grouping or alias.
    reaches_module: bool,
    /// A `use` tree names one of the surface types directly.
    names_surface_type: bool,
    /// A `use` tree reaches `perl_parser::prelude` (or `crate::prelude`).
    reaches_prelude: bool,
}

/// Whether a file consumes the dead-code surface.
///
/// Import *structure* is the primary signal, parsed with `syn`, so grouped and
/// aliased spellings — `use perl_parser::{dead_code as dc};`,
/// `use perl_parser::{prelude as p};` — are recognised even though they contain
/// none of the flat text needles.
///
/// The raw-text needles remain as a second route, because a fully-qualified
/// inline reference (`perl_parser::dead_code::DeadCode`) has no `use` item at
/// all.
///
/// The bare identifier `DeadCode` is never sufficient on its own: `perl-tdd-support`
/// has an unrelated `CodeSmell::DeadCode` variant. It counts only alongside a
/// prelude import, which is the wildcard case.
///
/// Returns `Err` when the file cannot be parsed, so a scan that cannot see a
/// file is recorded as instrument failure rather than silently reading as
/// "not a consumer".
fn references_surface(text: &str, inside_owning_crate: bool) -> std::result::Result<bool, String> {
    if CONSUMER_NEEDLES.iter().any(|needle| contains_word(text, needle)) {
        return Ok(true);
    }
    let file: syn::File =
        syn::parse_str(text).map_err(|error| format!("unparseable Rust: {error}"))?;
    let facts = import_facts(&file, inside_owning_crate);
    Ok(facts.reaches_module
        || facts.names_surface_type
        || (facts.reaches_prelude && contains_word(text, "DeadCode")))
}

/// Extract the import facts that decide whether a file consumes this surface.
///
/// Two passes: the first collects crate aliases so `use perl_parser as pf;` can
/// root a later `pf::dead_code`, the second classifies every flattened import
/// path against the resulting root set.
fn import_facts(file: &syn::File, inside_owning_crate: bool) -> ImportFacts {
    // First pass: collect crate aliases (`use perl_parser as pf;`), so a later
    // `pf::dead_code::…` is recognised as this surface.
    let mut roots: BTreeSet<String> = BTreeSet::new();
    roots.insert("perl_parser".to_string());
    if inside_owning_crate {
        roots.extend(["crate", "self", "super"].iter().map(|s| (*s).to_string()));
    }
    let uses = collect_use_items(file);
    for node in &uses {
        collect_crate_aliases(&node.tree, 0, &mut roots);
    }

    let mut facts = ImportFacts::default();
    for node in &uses {
        for path in flatten_use_tree(&node.tree, &mut Vec::new()) {
            classify_import_path(&path, &roots, &mut facts);
        }
    }
    facts
}

/// Every `use` declaration in a file, wherever it sits.
///
/// Scanning only top-level items missed the ones that matter most: an import
/// inside an inline module, an impl block, or a function body is just as much a
/// consumption of this surface, and those are exactly the shapes a prelude glob
/// or a crate alias hides in.
///
/// Lexical scope is deliberately flattened — an alias introduced in one inner
/// scope roots a path in another. That over-approximates consumption, which is
/// the fail-closed direction: it can only require an inventory row that was not
/// strictly needed, never let a real consumer through unrecorded.
fn collect_use_items(file: &syn::File) -> Vec<&syn::ItemUse> {
    struct Collector<'ast> {
        uses: Vec<&'ast syn::ItemUse>,
    }
    impl<'ast> syn::visit::Visit<'ast> for Collector<'ast> {
        fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
            self.uses.push(node);
            syn::visit::visit_item_use(self, node);
        }
    }

    let mut collector = Collector { uses: Vec::new() };
    syn::visit::Visit::visit_file(&mut collector, file);
    collector.uses
}

/// Classify one import path.
///
/// Rooting matters. `perl_parser::…::dead_code` is this surface from anywhere;
/// `crate::dead_code` is it only from inside `perl-parser` itself. A bare
/// `dead_code::…` is some *other* crate's local module — `perl-lsp-rs-core` has
/// exactly that, an independent dead-code implementation whose `mod dead_code;`
/// must not be read as consuming this surface.
fn classify_import_path(path: &[String], roots: &BTreeSet<String>, facts: &mut ImportFacts) {
    let rooted_at_surface_crate = path.first().is_some_and(|first| roots.contains(first));
    if rooted_at_surface_crate
        && path.iter().any(|seg| seg == "dead_code" || seg == "dead_code_detector")
    {
        facts.reaches_module = true;
    }
    if rooted_at_surface_crate && path.iter().any(|seg| seg == "prelude") {
        facts.reaches_prelude = true;
    }
    // Only a *rooted* type import counts. Another crate may legitimately define
    // its own `DeadCodeStats`; classifying that as consuming this surface would
    // force a false ledger row and block unrelated work.
    if rooted_at_surface_crate
        && path.last().is_some_and(|last| SURFACE_TYPES.contains(&last.as_str()))
    {
        facts.names_surface_type = true;
    }
}

/// Collect crate aliases for `perl_parser`, descending through groups.
///
/// A rename only aliases the *crate* when it sits at depth zero — no path
/// segment precedes it. `use {perl_parser as pf};` and `use ::perl_parser as pf;`
/// both qualify; `use perl_parser::{dead_code as dc};` does not, because there
/// `dc` renames a module inside the crate rather than the crate itself.
fn collect_crate_aliases(tree: &syn::UseTree, depth: usize, roots: &mut BTreeSet<String>) {
    match tree {
        syn::UseTree::Rename(rename) if depth == 0 && rename.ident == "perl_parser" => {
            roots.insert(rename.rename.to_string());
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_crate_aliases(item, depth, roots);
            }
        }
        _ => {}
    }
}

/// Flatten a `use` tree into full segment paths, descending through groups and
/// keeping the *original* name of a renamed leaf (the alias is a local label;
/// the path is what identifies the surface).
fn flatten_use_tree(tree: &syn::UseTree, prefix: &mut Vec<String>) -> Vec<Vec<String>> {
    match tree {
        syn::UseTree::Path(node) => {
            prefix.push(node.ident.to_string());
            let out = flatten_use_tree(&node.tree, prefix);
            prefix.pop();
            out
        }
        syn::UseTree::Name(node) => {
            let mut path = prefix.clone();
            path.push(node.ident.to_string());
            vec![path]
        }
        syn::UseTree::Rename(node) => {
            let mut path = prefix.clone();
            path.push(node.ident.to_string());
            vec![path]
        }
        syn::UseTree::Glob(_) => vec![prefix.clone()],
        syn::UseTree::Group(node) => {
            node.items.iter().flat_map(|item| flatten_use_tree(item, prefix)).collect()
        }
    }
}

/// Substring search with identifier boundaries/// Substring search with identifier boundaries, so `DeadCodeType` does not match
/// inside a longer identifier and `crate::dead_code` does not match
/// `crate::dead_code_detector`.
fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(idx, _)| {
        let before = haystack[..idx].chars().next_back();
        let after = haystack[idx + needle.len()..].chars().next();
        before.is_none_or(|c| !is_ident_char(c)) && after.is_none_or(|c| !is_ident_char(c))
    })
}

/// Whether a character can appear inside a Rust identifier.
fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

// ---------------------------------------------------------------------------
// L8 — Markdown projection agreement
// ---------------------------------------------------------------------------

/// L8 — the checked-in Markdown projection is byte-identical to what
/// [`render_projection`] produces from the ledger.
fn validate_human_projection(ledger: &Ledger, human_text: &str, violations: &mut Vec<String>) {
    let expected = render_projection(ledger);
    if human_text == expected {
        return;
    }
    // Report the first differing line so the failure names a place, not just a
    // mismatch. Comparing the WHOLE document (not only its tables) is the point:
    // every sentence in the projection is derived from the ledger, so prose that
    // drifts is exactly as wrong as a table that drifts.
    let mut expected_lines = expected.lines();
    let mut actual_lines = human_text.lines();
    let mut line_number = 0usize;
    loop {
        line_number += 1;
        match (expected_lines.next(), actual_lines.next()) {
            (None, None) => {
                // Reached only because the whole-document comparison above
                // already failed, so the difference is real but invisible to
                // `str::lines` — a trailing newline, or CRLF versus LF. Report
                // it rather than falling out of the loop with no violation.
                violations.push(format!(
                    "L8: {} differs from the ledger only in line terminators or the trailing \
                     newline; regenerate it with `cargo xtask check-dead-code-api-ledger --write`",
                    ledger.human_ledger
                ));
                return;
            }
            (want, got) if want == got => continue,
            (want, got) => {
                violations.push(format!(
                    "L8: {} line {line_number} disagrees with the ledger; the projection is \
                     generated, not hand-edited — regenerate it with \
                     `cargo xtask check-dead-code-api-ledger --write`\n      ledger:   {:?}\n      \
                     markdown: {:?}",
                    ledger.human_ledger,
                    want.unwrap_or("<end of file>"),
                    got.unwrap_or("<end of file>")
                ));
                return;
            }
        }
    }
}

/// Render the complete Markdown projection from the ledger.
///
/// This is the only generator for `docs/project/status/dead_code_api_ledger.md`.
/// `validate_human_projection` compares the checked-in file against this output
/// byte for byte, so the document cannot be edited by hand and cannot drift in
/// prose, tables, counts or ordering.
fn render_projection(ledger: &Ledger) -> String {
    let mut out = String::new();
    let never_produced: Vec<&Item> =
        ledger.item.iter().filter(|i| i.producer == "never_produced").collect();
    let inert: Vec<&Item> = ledger.item.iter().filter(|i| i.producer == "inert").collect();

    out.push_str("# `perl_parser::dead_code` API disposition ledger\n\n");
    out.push_str("<!-- GENERATED PROJECTION — do not hand-edit.\n");
    out.push_str("     Canonical source: `policy/dead-code-api-ledger.toml`.\n");
    out.push_str("     Regenerate with `cargo xtask check-dead-code-api-ledger --write`. -->\n\n");
    out.push_str(&format!(
        "Controlling issue: {} (C00 under the #8062 reachability programme).\n",
        ledger.controlling_issue
    ));
    out.push_str(&format!("Compatibility controller: {}.\n\n", ledger.compatibility_controller));
    out.push_str(
        "This is a **record of current behavior**, not a specification of desired behavior.\n\
         It says what the bounded compatibility surface does today, what it cannot represent,\n\
         and which authority owns each replacement. It grants no authority of its own: no item\n\
         on this surface authorizes an edit, and no value it produces is proof that removing\n\
         code is safe.\n\n",
    );

    out.push_str("## Export paths\n\n");
    out.push_str(&format!(
        "{} public paths reach one module. The module is dispositioned once; the others\n\
         are projections, not separate authorities.\n\n",
        ledger.export_path.len()
    ));
    out.push_str("| Path | Role | Declared at |\n| --- | --- | --- |\n");
    for export in &ledger.export_path {
        out.push_str(&format!(
            "| `{}` | {} | `{}` |\n",
            export.path, export.role, export.declared_at
        ));
    }
    for export in &ledger.export_path {
        out.push_str(&format!("\n- `{}` — {}", export.path, export.note));
    }
    out.push_str("\n\n## Item dispositions\n\n");
    out.push_str(&format!(
        "All {} public items in the module have exactly one row. A new public item fails the\n\
         check until it is dispositioned here.\n\n",
        ledger.item.len()
    ));
    out.push_str(
        "| Item | Kind | Class | Disposition | Producer | Proof ceiling | Replacement owner |\n\
         | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for item in &ledger.item {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} |\n",
            item.id,
            item.kind,
            item.semantic_class,
            item.disposition,
            item.producer,
            item.proof_ceiling,
            item.replacement_owner
        ));
    }

    out.push_str("\n### What the producer states mean\n\n");
    out.push_str("- `produced` — some current code path constructs it.\n");
    out.push_str(
        "- `never_produced` — advertised by the type and by the serde representation, constructed by nothing.\n",
    );
    out.push_str("- `inert` — accepted and stored, never read by any code path.\n");
    out.push_str("- `structural` — a container or definition that is not itself produced.\n\n");
    out.push_str(&format!(
        "Currently {} items are `never_produced` and {} are `inert`:\n\n",
        never_produced.len(),
        inert.len()
    ));
    for item in never_produced.iter().chain(inert.iter()) {
        out.push_str(&format!("- **`{}`** ({}) — {}\n", item.id, item.producer, item.lossiness));
    }

    out.push_str("\n## Result-state mapping\n\n");
    out.push_str(
        "For each state the reachability programme distinguishes, how the two public entry\n\
         points represent it. A collapse is recorded as a defect with an owner; it is never\n\
         accepted silently.\n\n",
    );
    out.push_str(
        "| Result state | analyze_file | analyze_workspace | Owner |\n| --- | --- | --- | --- |\n",
    );
    for state in &ledger.result_state {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            state.id,
            state.analyze_file,
            state.analyze_workspace,
            if state.owner.is_empty() { EMPTY_CELL } else { state.owner.as_str() }
        ));
    }
    out.push_str("\nRepresentations:\n\n");
    out.push_str("- `represented` — the state is distinctly expressible.\n");
    out.push_str("- `represented_lossy` — expressible, but the row's note names what is lost.\n");
    out.push_str(
        "- `indistinguishable_from_complete` — the state arrives looking like a complete result. **Defect.**\n",
    );
    out.push_str(
        "- `indistinguishable_from_clean_empty` — the state arrives looking like a clean empty result. **Defect.**\n",
    );
    out.push_str(
        "- `not_reachable` — the implementation cannot enter the state at all; it is absent, not hidden.\n\n",
    );
    let defects: Vec<&ResultState> =
        ledger.result_state.iter().filter(|s| !s.defect.trim().is_empty()).collect();
    out.push_str(&format!("### Recorded defects ({})\n\n", defects.len()));
    for state in &defects {
        out.push_str(&format!("- **`{}`** → {} — {}\n", state.id, state.owner, state.defect));
    }
    let notes: Vec<&ResultState> =
        ledger.result_state.iter().filter(|s| !s.note.trim().is_empty()).collect();
    if !notes.is_empty() {
        out.push_str("\n### Notes on the remaining states\n\n");
        for state in &notes {
            out.push_str(&format!("- **`{}`** — {}\n", state.id, state.note));
        }
    }

    out.push_str("\n## Consumer inventory\n\n");
    let roots =
        CONSUMER_SCAN_ROOTS.iter().map(|root| format!("`{root}`")).collect::<Vec<_>>().join(", ");
    out.push_str(&format!(
        "The check scans {roots} and fails on any file that references this surface without a\n\
         row here, so it cannot be wired into a new path silently.\n\n"
    ));
    out.push_str("| Consumer | Class |\n| --- | --- |\n");
    for consumer in &ledger.consumer {
        out.push_str(&format!("| `{}` | {} |\n", consumer.path, consumer.class));
    }
    out.push('\n');
    for consumer in &ledger.consumer {
        out.push_str(&format!("- `{}` — {}\n", consumer.path, consumer.note));
    }
    let production = ledger.consumer.iter().filter(|c| c.class == "production").count();
    out.push_str(&format!(
        "\nProduction consumers: **{production}**.\n\n\
         Paths that name the surface in order to govern it rather than consume it — the module\n\
         itself and this ledger's own tooling — are declared as `governance_paths` and are\n\
         checked for staleness:\n\n"
    ));
    for path in &ledger.governance_paths {
        out.push_str(&format!("- `{path}`\n"));
    }

    out.push_str("\n## Boundaries this ledger does not resolve\n\n");
    for np in &ledger.not_proven {
        out.push_str(&format!(
            "- **{}** — *claim:* {} *Why NOT_PROVEN:* {}\n",
            np.id, np.claim, np.reason
        ));
    }

    out.push_str("\n## Verification\n\n```bash\n");
    out.push_str("cargo xtask check-dead-code-api-ledger\n");
    out.push_str("cargo test -p xtask --locked dead_code_api_ledger\n");
    out.push_str("cargo test -p perl-parser --test dead_code_api_compat --locked\n");
    out.push_str("```\n");
    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Reject a field value outside a fixed vocabulary defined in this module.
fn require_member(
    subject: &str,
    field: &str,
    value: &str,
    allowed: &[&str],
    violations: &mut Vec<String>,
) {
    if !allowed.contains(&value) {
        violations.push(format!("`{subject}` has unknown {field} `{value}`"));
    }
}

/// Reject a field value outside a vocabulary the ledger itself declares.
fn require_member_owned(
    subject: &str,
    field: &str,
    value: &str,
    allowed: &[String],
    violations: &mut Vec<String>,
) {
    if !allowed.iter().any(|candidate| candidate == value) {
        violations.push(format!("`{subject}` has unknown {field} `{value}`"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tracked ledger, projection, module source, baseline and corpus agree
    /// on the current tree. This is the gate: it fails when any of the five
    /// drifts away from the others.
    #[test]
    fn tracked_ledger_is_valid() -> Result<()> {
        let root = project_root()?;
        validate(&root)?;
        Ok(())
    }

    /// A minimal module exercising public and private items of each shape.
    fn sample_source() -> &'static str {
        r#"
        pub enum Kind { A, B }
        pub struct Item { pub field: usize, private: usize }
        pub fn free() {}
        impl Item { pub fn method(&self) {} fn hidden(&self) {} }
        "#
    }

    #[test]
    /// Public items are discovered; private ones are not public API.
    fn source_surface_finds_public_items_and_skips_private_ones() -> Result<()> {
        let items = parse_module_surface(sample_source())?;
        let ids: BTreeSet<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.contains("dead_code"), "the module itself is always dispositioned");
        assert!(ids.contains("Kind"));
        assert!(ids.contains("Kind::A"));
        assert!(ids.contains("Item::field"));
        assert!(ids.contains("Item::method"));
        assert!(ids.contains("free"));
        assert!(!ids.contains("Item::private"), "private fields are not public API");
        assert!(!ids.contains("Item::hidden"), "private methods are not public API");
        Ok(())
    }

    #[test]
    /// Baseline rows map to ledger ids, excluding the compatibility alias's
    /// duplicate rows, derive-generated methods, and unrelated prelude rows.
    fn baseline_ids_reject_the_alias_and_derive_noise() {
        let canonical = "perl_parser::dead_code";
        assert_eq!(
            baseline_item_id("pub mod perl_parser::dead_code", canonical).as_deref(),
            Some("dead_code")
        );
        assert_eq!(
            baseline_item_id("pub enum perl_parser::dead_code::DeadCodeType", canonical).as_deref(),
            Some("DeadCodeType")
        );
        assert_eq!(
            baseline_item_id("pub perl_parser::dead_code::DeadCode::confidence: f32", canonical)
                .as_deref(),
            Some("DeadCode::confidence")
        );
        assert_eq!(
            baseline_item_id(
                "pub fn perl_parser::dead_code::DeadCodeDetector::analyze_file(&self) -> ()",
                canonical
            )
            .as_deref(),
            Some("DeadCodeDetector::analyze_file")
        );
        // The compatibility alias duplicates every row; matching it would double
        // the inventory and make L2 unfalsifiable.
        assert_eq!(
            baseline_item_id(
                "pub fn perl_parser::dead_code_detector::generate_report(x: u8)",
                canonical
            ),
            None
        );
        // Derive-generated trait methods are consequences of the type's row.
        assert_eq!(
            baseline_item_id(
                "pub fn perl_parser::dead_code::DeadCode::clone(&self) -> perl_parser::dead_code::DeadCode",
                canonical
            ),
            None
        );
        // A prelude row whose *type* mentions the module is not a row of it.
        assert_eq!(
            baseline_item_id(
                "pub perl_parser::prelude::DeadCode::code_type: perl_parser::dead_code::DeadCodeType",
                canonical
            ),
            None
        );
    }

    #[test]
    /// Needle matching honours identifier boundaries, so a needle cannot match
    /// inside a longer identifier or an `#[allow(dead_code)]` attribute.
    fn contains_word_respects_identifier_boundaries() {
        assert!(contains_word("use crate::dead_code;", "crate::dead_code"));
        assert!(!contains_word("use crate::dead_code_detector;", "crate::dead_code"));
        assert!(contains_word("let x: DeadCodeType = y;", "DeadCodeType"));
        assert!(!contains_word("struct MyDeadCodeTypeThing;", "DeadCodeType"));
        // The bare attribute must never register as a consumer.
        assert!(!contains_word("#[allow(dead_code)]", "crate::dead_code"));
        assert!(!contains_word("#[allow(dead_code)]", "perl_parser::dead_code"));
    }

    /// `perl-tdd-support` has its own unrelated `CodeSmell::DeadCode` variant.
    /// Scanning for the bare identifier reported it as a consumer of this
    /// surface, which it is not, so the needle set uses the prelude path
    /// instead. This pins the decision so the broad needle is not reintroduced.
    #[test]
    fn consumer_needles_do_not_match_an_unrelated_dead_code_identifier() {
        let unrelated = "    /// Unreachable or unused code\n    DeadCode,\n";
        assert!(
            !CONSUMER_NEEDLES.iter().any(|needle| contains_word(unrelated, needle)),
            "an unrelated `DeadCode` enum variant must not register as a needle match"
        );
        // That import is still detected — structurally, by root-aware import
        // analysis, rather than by a flat needle.
        assert!(
            references_surface("use perl_parser::prelude::DeadCode;", false).expect("parses"),
            "a rooted prelude type import is still a consumer"
        );
    }

    #[test]
    fn fixture_ids_come_from_test_functions_not_comments() -> Result<()> {
        let corpus = r#"
        /// Fixture `dcapi-real-one`.
        #[test]
        fn dcapi_real_one_with_a_longer_tail() {}

        // Fixture `dcapi-only-a-comment` — the test below was deleted.
        fn helper_not_a_test() {}
        "#;
        let names = corpus_test_functions(corpus)?;
        assert!(names.contains("dcapi_real_one_with_a_longer_tail"));
        assert_eq!(names.len(), 1, "only `#[test]` functions count");

        assert!(fixture_is_bound("dcapi-real-one", &names), "snake-prefix binding");
        assert!(
            !fixture_is_bound("dcapi-only-a-comment", &names),
            "a fixture that exists only as comment text must not count as bound"
        );
        Ok(())
    }

    /// Devin review, PR #15086: deleting a test while keeping its banner left
    /// L9/L10 green because fixture ids were scraped from comment text.
    #[test]
    fn falsifier_l9_a_comment_only_fixture_is_not_proof() -> Result<()> {
        let root = project_root()?;
        let ledger = read_ledger(&root, POLICY_PATH)?;
        let corpus = read_text(&root, &ledger.compat_corpus)?;
        // Delete the test function but leave every comment mentioning it.
        let gutted = corpus.replace(
            "fn dcapi_entry_point_is_inert() -> TestResult {",
            "fn deleted_entry_point_fixture_renamed_away() -> TestResult {",
        );
        assert_ne!(gutted, corpus, "the mutation must actually apply");
        let mut violations = Vec::new();
        validate_fixtures(&ledger, &gutted, &mut violations);
        assert!(
            violations
                .iter()
                .any(|v| v.starts_with("L9:") && v.contains("dcapi-entry-point-is-inert")),
            "a deleted test with a surviving banner must break its binding: {violations:?}"
        );
        Ok(())
    }

    /// Devin review, PR #15086: only two tables were compared, so generated
    /// prose outside them could drift without failing L8.
    #[test]
    fn falsifier_l8_prose_outside_the_tables_cannot_drift() -> Result<()> {
        let root = project_root()?;
        let ledger = read_ledger(&root, POLICY_PATH)?;
        let rendered = render_projection(&ledger);

        // A sentence in the consumer-inventory narrative, well outside both tables.
        let drifted =
            rendered.replace("Production consumers: **0**.", "Production consumers: **7**.");
        assert_ne!(drifted, rendered, "the mutation must actually apply");
        let mut violations = Vec::new();
        validate_human_projection(&ledger, &drifted, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L8:")),
            "prose drift outside the tables must fail: {violations:?}"
        );
        Ok(())
    }

    /// CodeRabbit review, PR #15086: `str::lines` drops the final terminator and
    /// strips a trailing `\r`, so a projection differing only in its trailing
    /// newline — or checked out with CRLF — produced identical line sequences,
    /// fell out of the loop, and pushed no violation. L8 passed on a real byte
    /// mismatch.
    #[test]
    fn falsifier_l8_a_byte_difference_invisible_to_lines_is_rejected() -> Result<()> {
        let root = project_root()?;
        let ledger = read_ledger(&root, POLICY_PATH)?;
        let rendered = render_projection(&ledger);

        let truncated = rendered.trim_end_matches('\n').to_string();
        assert_ne!(truncated, rendered, "the mutation must actually apply");
        let mut violations = Vec::new();
        validate_human_projection(&ledger, &truncated, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L8:")),
            "a missing trailing newline must fail: {violations:?}"
        );

        let crlf = rendered.replace('\n', "\r\n");
        assert_ne!(crlf, rendered);
        let mut violations = Vec::new();
        validate_human_projection(&ledger, &crlf, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L8:")),
            "CRLF line endings must fail: {violations:?}"
        );
        Ok(())
    }

    /// CodeRabbit review, PR #15086: `root_identity` is optional and L6 rejected
    /// only `Some(true)`, so deleting the field from the inert row passed.
    #[test]
    fn falsifier_l6_an_inert_row_must_declare_root_identity() -> Result<()> {
        let violations = law_violations(|ledger| {
            for item in &mut ledger.item {
                if item.producer == "inert" {
                    item.root_identity = None;
                }
            }
        })?;
        assert!(
            violations.iter().any(|v| v.starts_with("L6:") && v.contains("does not declare")),
            "an inert row with no root-identity disposition must fail: {violations:?}"
        );
        Ok(())
    }

    /// CodeRabbit review, PR #15086: `Item::Verbatim` and `Item::ForeignMod`
    /// carry no visibility, so the fail-closed arm skipped them entirely.
    #[test]
    fn visibility_less_item_forms_fail_closed() -> Result<()> {
        let foreign = parse_module_surface("unsafe extern \"C\" { pub fn exported(); }")?;
        assert!(
            foreign.iter().any(|i| i.kind == "unsupported_public_form"),
            "a public foreign item must fail L1 rather than pass unseen"
        );
        let private_foreign = parse_module_surface("unsafe extern \"C\" { fn hidden(); }")?;
        assert!(
            !private_foreign.iter().any(|i| i.kind == "unsupported_public_form"),
            "a non-public foreign item is not public API"
        );
        Ok(())
    }

    #[test]
    /// Rendering the projection twice yields identical bytes.
    fn projection_render_is_deterministic() -> Result<()> {
        let root = project_root()?;
        let ledger = read_ledger(&root, POLICY_PATH)?;
        assert_eq!(render_projection(&ledger), render_projection(&ledger));
        Ok(())
    }

    /// Devin review, PR #15086: type aliases, constants, statics, traits,
    /// unions, re-exports and associated constants escaped disposition.
    #[test]
    fn source_surface_covers_every_public_form() -> Result<()> {
        let items = parse_module_surface(
            r#"
            pub type Alias = usize;
            pub const LIMIT: usize = 1;
            pub static TABLE: usize = 2;
            pub trait Shape { fn area(&self) -> usize; const SIDES: usize; }
            pub use other::Reexported;
            pub mod nested {}
            pub struct Tuple(pub usize);
            struct Private;
            impl Tuple { pub const ZERO: usize = 0; pub fn make() -> Self { Self(0) } }
            "#,
        )?;
        let by_id: BTreeMap<&str, &str> =
            items.iter().map(|i| (i.id.as_str(), i.kind.as_str())).collect();

        assert_eq!(by_id.get("Alias"), Some(&"type_alias"));
        assert_eq!(by_id.get("LIMIT"), Some(&"constant"));
        assert_eq!(by_id.get("TABLE"), Some(&"static"));
        assert_eq!(by_id.get("Shape"), Some(&"trait"));
        assert_eq!(by_id.get("Shape::area"), Some(&"trait_member"));
        assert_eq!(by_id.get("Shape::SIDES"), Some(&"trait_member"));
        assert_eq!(by_id.get("Reexported"), Some(&"reexport"));
        assert_eq!(by_id.get("nested"), Some(&"submodule"));
        assert_eq!(by_id.get("Tuple::0"), Some(&"field"), "tuple fields are public API");
        assert_eq!(by_id.get("Tuple::ZERO"), Some(&"associated_constant"));
        assert_eq!(by_id.get("Tuple::make"), Some(&"method"));
        assert!(!by_id.contains_key("Private"), "private items stay out");
        Ok(())
    }

    #[test]
    /// A `pub use other::*;` glob cannot be enumerated, so it must fail L1
    /// rather than silently contributing no items.
    fn public_glob_reexport_fails_closed() -> Result<()> {
        let items = parse_module_surface("pub use other::*;")?;
        assert!(
            items.iter().any(|i| i.id.contains("glob")),
            "an unbounded public glob re-export cannot be enumerated and must fail L1"
        );
        Ok(())
    }

    /// Devin review, PR #15086: `use perl_parser::prelude::*;` followed by a bare
    /// `DeadCode` bypassed the needle scan entirely.
    fn is_consumer(text: &str) -> bool {
        references_surface(text, false).expect("sample parses")
    }

    #[test]
    /// A wildcard prelude import combined with a bare `DeadCode` is a consumer;
    /// neither half alone is.
    fn wildcard_prelude_consumers_are_detected() {
        assert!(
            is_consumer("use perl_parser::prelude::*;\nfn f(d: DeadCode) {}\n"),
            "a wildcard prelude import plus a bare DeadCode is a consumer"
        );
        // The unrelated identifier must not register: no prelude import.
        assert!(!is_consumer("enum CodeSmell { DeadCode, Other }\n"));
        // Nor does a prelude import on its own.
        assert!(!is_consumer("use perl_parser::prelude::*;\nfn f(p: Parser) {}\n"));
    }

    /// Devin review, PR #15086: grouped and aliased imports carry none of the
    /// flat text needles, so import *structure* has to be parsed.
    #[test]
    fn grouped_and_aliased_imports_are_detected() {
        assert!(
            is_consumer(
                "use perl_parser::{dead_code as dc};\nfn f() -> dc::DeadCodeStats { todo!() }\n"
            ),
            "grouped alias of the canonical module"
        );
        assert!(
            is_consumer("use perl_parser::{prelude as p};\nfn f(x: p::DeadCode) {}\n"),
            "grouped alias of the prelude plus a DeadCode mention"
        );
        assert!(
            is_consumer(
                "use perl_parser::{dead_code_detector as legacy};\nfn f(x: legacy::DeadCodeType) {}\n"
            ),
            "grouped alias of the compatibility alias"
        );
        assert!(
            !is_consumer("use other_crate::{dead_code as dc};\nfn f() {}\n"),
            "another crate's module of the same name is not this surface"
        );
    }

    /// `perl-lsp-rs-core` declares its own `mod dead_code;` — an independent
    /// implementation. Matching bare `dead_code` segments reported it as a
    /// consumer; rooting the match fixes that.
    #[test]
    fn a_foreign_local_dead_code_module_is_not_this_surface() {
        let foreign = "mod dead_code;\npub use dead_code::detect_dead_code;\n";
        assert!(!is_consumer(foreign), "a local module of the same name is not this surface");
        // From inside perl-parser, `crate::dead_code` IS this surface.
        let owning = "use crate::dead_code::DeadCodeStats;\n";
        assert!(
            references_surface(owning, true).expect("parses"),
            "crate::dead_code inside perl-parser is the surface"
        );
        assert!(
            !references_surface("use crate::dead_code::Thing;\n", false).expect("parses"),
            "crate::dead_code from another crate is that crate's own module"
        );
    }

    /// Devin review, PR #15086: `use perl_parser as pf;` followed by
    /// `pf::dead_code::…` defeated root matching, so the recurrence guard could
    /// not see a consumer reaching the surface through a crate alias.
    #[test]
    fn crate_aliases_are_resolved() {
        assert!(
            is_consumer("use perl_parser as pf;\nuse pf::dead_code::DeadCodeStats;\n"),
            "a crate alias must still root at this surface"
        );
        assert!(
            is_consumer(
                "use perl_parser as pf;\nuse pf::{prelude as p};\nfn f(x: p::DeadCode) {}\n"
            ),
            "alias plus grouped prelude alias plus a DeadCode mention"
        );
        assert!(
            !is_consumer("use other_crate as pf;\nuse pf::dead_code::Thing;\n"),
            "aliasing an unrelated crate does not create a consumer"
        );
        // Devin review: a *grouped* crate alias left the alias outside `roots`.
        assert!(
            is_consumer("use {perl_parser as pf};\nuse pf::dead_code::DeadCodeStats;\n"),
            "a grouped crate alias must still root at this surface"
        );
        assert!(
            is_consumer("use ::perl_parser as pf;\nuse pf::prelude::DeadCode;\n"),
            "a leading-colon crate alias must still root at this surface"
        );
        // A rename *inside* the crate path is not a crate alias, and must not
        // widen the root set to some unrelated `dc::…` elsewhere in the file.
        assert!(
            !is_consumer("use other_crate::{something as dc};\nuse dc::Thing;\n"),
            "a module rename inside another crate is not a crate alias"
        );
    }

    /// Devin review, PR #15086: an unrelated crate's identically-named type was
    /// classified as consuming this surface, which would force a false ledger
    /// row and block unrelated work.
    #[test]
    fn an_unrelated_crates_same_named_type_is_not_a_consumer() {
        assert!(
            !is_consumer("use other_crate::DeadCodeStats;\nfn f(s: DeadCodeStats) {}\n"),
            "another crate may define its own DeadCodeStats"
        );
        // The rooted spelling of the same import still counts.
        assert!(is_consumer("use perl_parser::prelude::DeadCodeStats;\n"));
        assert!(is_consumer("use perl_parser::dead_code::DeadCodeStats;\n"));
    }

    /// A general-purpose `perl_parser::prelude` mention — even inside a string
    /// literal, as in `xtask/tests/parser_tdd_facade_consumers.rs` — must not by
    /// itself make a file a consumer.
    #[test]
    fn a_bare_prelude_mention_is_not_a_consumer() {
        let in_a_string = r#"fn f() { let s = "use perl_parser::prelude::*;"; }"#;
        assert!(!is_consumer(in_a_string));
        assert!(!is_consumer("use perl_parser::prelude::*;\nfn f(p: Parser) {}\n"));
    }

    /// Devin review, PR #15086: the unparseable exemption was applied on path
    /// membership alone, so a declared file that became valid Rust kept its
    /// exemption *and* was marked current, suppressing import analysis silently.
    #[test]
    fn a_declared_unparseable_path_that_parses_is_not_exempt() -> Result<()> {
        let root = project_root()?;
        let ledger = read_ledger(&root, POLICY_PATH)?;
        assert!(!ledger.unparseable_paths.is_empty(), "need a declared path to reason about");
        for declared in &ledger.unparseable_paths {
            let text = read_text(&root, declared)?;
            assert!(
                syn::parse_str::<syn::File>(&text).is_err(),
                "{declared} is declared unparseable but parses; the declaration is stale"
            );
        }
        Ok(())
    }

    /// Devin review, PR #15086: the scan discarded traversal and read failures,
    /// so incomplete evidence read as a pass. Repository policy makes missing or
    /// instrument-failed evidence NOT_PROVEN.
    #[test]
    fn an_unparseable_file_is_instrument_failure_not_a_pass() {
        let broken = "fn f( {{{ this is not rust";
        let outcome = references_surface(broken, false);
        assert!(
            outcome.is_err(),
            "a file the scan cannot parse must not silently report `not a consumer`"
        );
    }

    // ---- Falsifiers: each mutates a valid ledger and asserts the named law bites ----

    /// Apply a mutation to the tracked ledger in memory and return the per-item
    /// and result-state violations it produces, so a falsifier can assert the
    /// named law fires without touching the checked-in file.
    fn law_violations(mutate: impl FnOnce(&mut Ledger)) -> Result<Vec<String>> {
        let root = project_root()?;
        let mut ledger = read_ledger(&root, POLICY_PATH)?;
        mutate(&mut ledger);
        let mut violations = Vec::new();
        validate_item_laws(&ledger, &mut violations);
        validate_result_states(&ledger, &mut violations);
        Ok(violations)
    }

    #[test]
    /// L3 — an item nothing constructs may not be dispositioned as retained.
    fn falsifier_l3_never_produced_item_cannot_be_retained() -> Result<()> {
        let violations = law_violations(|ledger| {
            for item in &mut ledger.item {
                if item.producer == "never_produced" {
                    item.disposition = "retain_projection".to_string();
                }
            }
        })?;
        assert!(
            violations.iter().any(|v| v.starts_with("L3:")),
            "presenting an unimplemented variant as retained behavior must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    /// L3 — an unproduced item without a fixture is an unfalsifiable claim.
    fn falsifier_l3_never_produced_item_needs_a_negative_control() -> Result<()> {
        let violations = law_violations(|ledger| {
            for item in &mut ledger.item {
                if item.producer == "never_produced" {
                    item.fixtures.clear();
                }
            }
        })?;
        assert!(
            violations.iter().any(|v| v.contains("unfalsifiable")),
            "an unimplemented variant with no negative control must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    /// L4 — nothing on this compatibility surface authorizes an edit.
    fn falsifier_l4_no_item_may_claim_edit_authority() -> Result<()> {
        let violations = law_violations(|ledger| {
            if let Some(item) = ledger.item.iter_mut().find(|i| i.id == "generate_report") {
                item.edit_authority = "safe_delete".to_string();
            }
        })?;
        assert!(
            violations.iter().any(|v| v.starts_with("L4:")),
            "a report gaining deletion authority must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    /// L5 — a state that collapses into an ordinary result must name a defect.
    fn falsifier_l5_a_collapsed_state_cannot_be_accepted_silently() -> Result<()> {
        let violations = law_violations(|ledger| {
            for state in &mut ledger.result_state {
                state.defect.clear();
                state.owner.clear();
            }
        })?;
        assert!(
            violations.iter().any(|v| v.contains("names no defect")),
            "hiding a collapse must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    /// L5 — dropping a result state fails rather than reading as absent.
    fn falsifier_l5_every_state_must_be_dispositioned() -> Result<()> {
        let violations = law_violations(|ledger| {
            ledger.result_state.retain(|s| s.id != "incomplete_semantic_computation");
        })?;
        assert!(
            violations.iter().any(|v| v.contains("incomplete_semantic_computation")),
            "dropping a state must fail rather than read as absent: {violations:?}"
        );
        Ok(())
    }

    #[test]
    /// L6 — a path is a configuration candidate, never canonical root identity.
    fn falsifier_l6_entry_point_cannot_claim_root_identity() -> Result<()> {
        let violations = law_violations(|ledger| {
            if let Some(item) =
                ledger.item.iter_mut().find(|i| i.id == "DeadCodeDetector::add_entry_point")
            {
                item.root_identity = Some(true);
            }
        })?;
        assert!(
            violations.iter().any(|v| v.starts_with("L6:") && v.contains("root identity")),
            "a path masquerading as root identity must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    /// L1 — a public item with no ledger row fails until dispositioned.
    fn falsifier_l1_a_new_public_item_is_unclassified() -> Result<()> {
        let root = project_root()?;
        let ledger = read_ledger(&root, POLICY_PATH)?;
        let mut source = parse_module_surface(&read_text(&root, &ledger.module_source)?)?;
        source.insert(SourceItem {
            id: "DeadCodeType::UnusedLabel".to_string(),
            kind: "variant".to_string(),
        });
        let mut violations = Vec::new();
        validate_source_coverage(&ledger, &source, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L1:") && v.contains("UnusedLabel")),
            "a new public item must fail until it is dispositioned: {violations:?}"
        );
        Ok(())
    }

    #[test]
    /// L1 — a public item nested in an inline module is still public API and
    /// must receive its own row under a qualified id. Recording only the
    /// enclosing module would let the governed surface grow undispositioned.
    fn falsifier_l1_a_public_item_inside_an_inline_module_is_recorded() -> Result<()> {
        let surface = parse_module_surface(
            "pub mod nested { pub fn exposed() {} pub struct Held { pub field: u8 } }",
        )?;
        for (id, kind) in [
            ("nested", "submodule"),
            ("nested::exposed", "function"),
            ("nested::Held", "struct"),
            ("nested::Held::field", "field"),
        ] {
            assert!(
                surface.iter().any(|item| item.id == id && item.kind == kind),
                "nested public item {id} ({kind}) must be recorded: {surface:?}"
            );
        }
        Ok(())
    }

    #[test]
    /// L1 — an out-of-line public submodule puts public API in a file this
    /// ledger does not govern. It is rejected rather than silently uncovered.
    fn falsifier_l1_an_out_of_line_public_submodule_is_rejected() -> Result<()> {
        let error = parse_module_surface("pub mod elsewhere;")
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("elsewhere") && error.contains("out-of-line"),
            "an out-of-line public submodule must be rejected, got: {error:?}"
        );
        // A private out-of-line module exposes nothing and stays acceptable.
        assert!(parse_module_surface("mod helper;").is_ok(), "a private submodule is fine");
        Ok(())
    }

    #[test]
    /// L7 — consumers whose only import sits inside a nested module or a
    /// function body are still consumers. Scanning top-level items alone let
    /// prelude and crate-alias consumers merge without an inventory row.
    fn falsifier_l7_a_nested_import_still_marks_a_consumer() -> Result<()> {
        let cases = [
            (
                "prelude glob inside an inline module",
                "mod inner { use perl_parser::prelude::*; pub fn f() -> Option<DeadCode> { None } }",
            ),
            (
                "crate alias inside an inline module",
                "mod inner { use perl_parser as pf; use pf::dead_code::DeadCodeDetector; \
                 pub fn f() -> DeadCodeDetector { DeadCodeDetector::new() } }",
            ),
            (
                "block-scoped import inside a function",
                "fn f() { use perl_parser::prelude::*; let _: Option<DeadCode> = None; }",
            ),
        ];
        for (label, text) in cases {
            let seen = references_surface(text, false)
                .map_err(|error| color_eyre::eyre::eyre!("{label}: {error}"))?;
            assert!(seen, "{label} must be detected as a consumer");
        }
        // Negative control: the same shapes without a rooted import must stay
        // invisible, so the recursion has not simply made everything a consumer.
        let unrooted = "mod inner { use dead_code::DeadCodeDetector; pub fn f() {} }";
        assert!(
            !references_surface(unrooted, false).map_err(color_eyre::eyre::Report::msg)?,
            "another crate's own `dead_code` module must not be read as this surface"
        );
        Ok(())
    }

    #[test]
    /// L1 — an item-macro invocation can expand to public items that `syn`
    /// cannot see. It fails closed rather than being skipped, while a private
    /// `macro_rules!` definition (which exports nothing) stays acceptable.
    fn falsifier_l1_an_item_macro_invocation_fails_closed() -> Result<()> {
        let expanded = parse_module_surface("declare_surface! { pub fn exposed() {} }")?;
        assert!(
            expanded.iter().any(|item| item.kind == "unsupported_public_form"),
            "an item-macro invocation must fail closed: {expanded:?}"
        );

        let private_definition = parse_module_surface("macro_rules! helper { () => {}; }")?;
        assert!(
            private_definition.iter().all(|item| item.kind != "unsupported_public_form"),
            "a private macro_rules! definition exports nothing: {private_definition:?}"
        );

        let exported = parse_module_surface("#[macro_export]\nmacro_rules! shouted { () => {}; }")?;
        assert!(
            exported.iter().any(|item| item.id == "shouted" && item.kind == "exported_macro"),
            "an exported macro keeps its own row: {exported:?}"
        );
        Ok(())
    }

    #[test]
    /// L11 — a route the baseline exposes must be declared, and a declared
    /// route the baseline no longer exposes must be reported as stale. Before
    /// this law `export_path` was a hand-kept list: a new public alias in the
    /// facade left the ledger green while the reachable surface grew.
    fn falsifier_l11_export_paths_track_the_baseline_in_both_directions() -> Result<()> {
        let root = project_root()?;
        let ledger = read_ledger(&root, POLICY_PATH)?;
        let baseline = read_text(&root, &ledger.public_api_baseline)?;

        // A fourth alias appears in the facade; the ledger is unchanged.
        let widened = format!(
            "{baseline}\npub mod perl_parser::legacy::dead_code_detector\npub enum \
             perl_parser::legacy::dead_code_detector::DeadCodeType\n"
        );
        let mut violations = Vec::new();
        validate_export_paths(&ledger, &widened, &mut violations);
        assert!(
            violations
                .iter()
                .any(|v| v.starts_with("L11:")
                    && v.contains("perl_parser::legacy::dead_code_detector")),
            "an undeclared route must fail: {violations:?}"
        );

        // A declared route that the baseline stops exposing is stale.
        let mut narrowed = ledger;
        narrowed.export_path.push(ExportPath {
            path: "perl_parser::retired::dead_code".to_string(),
            role: "compatibility_alias".to_string(),
            declared_at: "crates/perl-parser/src/lib.rs".to_string(),
            note: "Removed route.".to_string(),
        });
        let mut violations = Vec::new();
        validate_export_paths(&narrowed, &baseline, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L11:")
                && v.contains("perl_parser::retired::dead_code")
                && v.contains("stale")),
            "a route the baseline does not expose must be reported stale: {violations:?}"
        );
        Ok(())
    }

    #[test]
    /// L11 — a baseline whose format has moved must fail rather than pass by
    /// deriving nothing.
    fn falsifier_l11_a_baseline_that_derives_nothing_is_not_a_pass() -> Result<()> {
        let root = project_root()?;
        let ledger = read_ledger(&root, POLICY_PATH)?;
        let mut violations = Vec::new();
        validate_export_paths(&ledger, "# baseline format moved\n", &mut violations);
        assert!(
            violations
                .iter()
                .any(|v| v.starts_with("L11:") && v.contains("no longer checking anything")),
            "an underivable baseline is instrument failure, not a pass: {violations:?}"
        );
        Ok(())
    }

    #[test]
    /// The consumer scan is deliberately mixed, and the ledger's NOT_PROVEN row
    /// must describe it accurately: the two rooted needles are a text match, so
    /// a comment or string naming them *does* force an inventory row, while
    /// every other spelling (prelude globs, crate aliases) is import-structural
    /// and a comment naming it does not.
    fn comment_and_string_references_are_matched_only_by_the_rooted_needles() -> Result<()> {
        let needle_in_a_comment = "// see perl_parser::dead_code for the legacy shape\n";
        assert!(
            references_surface(needle_in_a_comment, false)
                .map_err(color_eyre::eyre::Report::msg)?,
            "a rooted needle inside a comment is matched as text; the ledger says so"
        );
        let needle_in_a_string = "fn f() { let _ = \"perl_parser::dead_code_detector\"; }";
        assert!(
            references_surface(needle_in_a_string, false).map_err(color_eyre::eyre::Report::msg)?,
            "a rooted needle inside a string literal is matched as text"
        );
        let prelude_in_a_comment = "// the prelude re-exports DeadCode\nfn f() {}\n";
        assert!(
            !references_surface(prelude_in_a_comment, false)
                .map_err(color_eyre::eyre::Report::msg)?,
            "a non-needle spelling in a comment stays structural and is not inventoried"
        );
        Ok(())
    }

    #[test]
    /// L7 — a file referencing the surface without an inventory row fails.
    fn falsifier_l7_an_undeclared_consumer_fails() -> Result<()> {
        let root = project_root()?;
        let mut ledger = read_ledger(&root, POLICY_PATH)?;
        let dropped = ledger.consumer.remove(0);
        let mut violations = Vec::new();
        validate_consumers(&root, &ledger, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L7:") && v.contains(dropped.path.as_str())),
            "a consumer that exists but is not inventoried must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    /// L9 — citing proof that does not exist fails.
    fn falsifier_l9_a_ledger_row_cannot_cite_a_missing_fixture() -> Result<()> {
        let root = project_root()?;
        let mut ledger = read_ledger(&root, POLICY_PATH)?;
        let corpus = read_text(&root, &ledger.compat_corpus)?;
        if let Some(item) = ledger.item.first_mut() {
            item.fixtures.push("dcapi-does-not-exist".to_string());
        }
        let mut violations = Vec::new();
        validate_fixtures(&ledger, &corpus, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L9:")),
            "citing proof that does not exist must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    /// L8 — a hand-edited projection fails.
    fn falsifier_l8_markdown_drift_is_rejected() -> Result<()> {
        let root = project_root()?;
        let ledger = read_ledger(&root, POLICY_PATH)?;
        let human = read_text(&root, &ledger.human_ledger)?;
        let drifted = human.replacen("retain_projection", "remove", 1);
        let mut violations = Vec::new();
        validate_human_projection(&ledger, &drifted, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L8:")),
            "a hand-edited projection must fail: {violations:?}"
        );
        Ok(())
    }
}
