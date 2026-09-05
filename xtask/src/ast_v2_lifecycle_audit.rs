//! The `perl-ast-v2` package lifecycle audit (`ast_v2_package_lifecycle.v1`).
//!
//! I01 of the #9213 AST programme, under package disposition #7403. This module
//! consumes `.spec/8843-ast-v2-lifecycle-audit/ast_v2_package_lifecycle.v1.json`
//! strictly as DATA and provides, offline:
//!
//! * a fail-closed loader: strict schema (`deny_unknown_fields`), closed
//!   vocabularies, unique stable ids, referential integrity between rows, and a
//!   pinned canonical digest;
//! * a **derivation half** that recomputes the denominator from current source
//!   rather than trusting the authored rows — public items and enum variants are
//!   read out of `crates/perl-ast-v2/src/lib.rs` with `syn`, the production
//!   `NodeKind` variant set is read out of `crates/perl-ast/src/ast.rs` the same
//!   way, and the consumer denominator is scanned out of the Rust, crate-manifest
//!   and policy surfaces where an actual API or package consumer can appear;
//! * reconciliation that fails closed in both directions, so a public item, enum
//!   variant, re-export or direct consumer cannot land without moving a row, and
//!   a row cannot outlive the thing it describes;
//! * the ruling laws: a `retain` ruling must name independent-lifecycle evidence,
//!   and download counts are structurally barred from being consumer evidence.
//!
//! Claim ceiling (#8843): inventory and ruling only. No source move, package
//! removal, semantic convergence, parser change, or claim that v2 is
//! parity-complete belongs here — adding one violates the issue's non-goals and
//! the guard test named `no_migration_or_mutation_surface_is_added`.
//!
//! Instrument boundary. `syn` sees the declarations this crate actually compiles,
//! which is what a public-API denominator needs and what a grep cannot promise.
//! It does not see semantic consumers, so the consumer rows stay authored and are
//! checked against a scanned reference set rather than generated from one. The
//! two halves are deliberately different instruments; neither is asked to prove
//! the other's proposition.
//!
//! Shared-mechanics ruling: the landed `lsp_runtime_train.v1` and
//! `import_cleanup_train.v1` loading shape is mirrored deliberately rather than
//! extracted, on the same #10554 reasoning those manifests record. Three
//! manifests sharing a loading shape still does not satisfy that issue's landed
//! duplication gate; this module records itself as the third instance.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use sha2::Digest as ShaDigest;
use sha2::Sha256;

/// Repository-relative location of the audit contract (#8843).
pub const MANIFEST_RELATIVE_PATH: &str =
    ".spec/8843-ast-v2-lifecycle-audit/ast_v2_package_lifecycle.v1.json";

/// Schema identity consumed by this model.
pub const SCHEMA_NAME: &str = "ast_v2_package_lifecycle.v1";
/// Schema major version consumed by this model.
pub const SCHEMA_VERSION: u64 = 1;

/// Repository-relative source of the audited package's whole public surface.
pub const V2_SOURCE_RELATIVE_PATH: &str = "crates/perl-ast-v2/src/lib.rs";

/// Repository-relative source of the production AST the parity rows compare to.
pub const V1_AST_SOURCE_RELATIVE_PATH: &str = "crates/perl-ast/src/ast.rs";

/// Crate root the derived public paths are rendered under.
const V2_CRATE_PATH: &str = "perl_ast_v2";

/// Pinned canonical digest of the current `ast_v2_package_lifecycle.v1` revision.
///
/// Canonicalization: recursive content walk with byte-ordinal ordering (see
/// `canonical_digest`). A semantic revision must move this pin deliberately
/// together with the manifest bytes; patching around it silently is exactly what
/// the pin exists to prevent.
pub const PINNED_CANONICAL_DIGEST: &str =
    "0D7D8CE05C05B0BA8216A234323ECE9C4EF948B12462F7AC1821DC66DD2B7A80";

// ---------------------------------------------------------------------------
// Code-owned v1 vocabularies. A cardinality check lets a repinned manifest
// rename a value and keep the count, so the reviewed sets live here and are
// compared for exact membership.
// ---------------------------------------------------------------------------

/// Item kinds the derivation can emit and a row may claim.
const V1_ITEM_KINDS: [&str; 6] =
    ["type_alias", "struct", "enum", "enum_variant", "associated_fn", "trait_impl"];

/// How one v2 public item relates to the production AST.
///
/// `not_applicable` is for items with no production counterpart concept at all
/// (a generator utility); `unique` is for a proposition the production AST does
/// not express (`ErrorRef`, `MissingKind`). The two are kept distinct so an
/// absent counterpart cannot be read as a deliberate experimental one.
const V1_RELATIONS: [&str; 5] = ["equivalent", "narrower", "divergent", "unique", "not_applicable"];

/// Disposition vocabulary shared by the range/recovery/node-id/serialization/
/// currentness columns. Every column must state one; there is no empty default,
/// because "unstated" is exactly the failure the audit exists to prevent.
const V1_DISPOSITIONS: [&str; 6] = [
    "represented",
    "represented_unproven",
    "not_represented",
    "experimental_only",
    "delegated",
    "not_applicable",
];

/// Consumer roles.
///
/// The first four actually reach the package's API or declare a dependency on
/// it; the last three name it as a string without consuming anything. Keeping
/// them distinct is the whole point: it is what stops a prose mention or a
/// coverage-fixture path from being counted as production use.
const V1_CONSUMER_ROLES: [&str; 7] = [
    "production_implementation",
    "public_reexport",
    "package_dependency",
    "test_fixture",
    "policy_inventory",
    "docs_reference",
    "release_metadata",
];

/// Consumer roles that assert an actual API or package consumer, and are
/// therefore reconciled against the scanned denominator in both directions.
const GATING_CONSUMER_ROLES: [&str; 4] =
    ["production_implementation", "public_reexport", "package_dependency", "test_fixture"];

/// Package-surface classes.
const V1_PACKAGE_SURFACES: [&str; 6] = [
    "workspace_member",
    "workspace_dependency",
    "publish_allowlist",
    "repository_topology",
    "policy_baseline",
    "docs_metadata",
];

/// External-evidence classes. `not_consumer_evidence` exists so download volume
/// can be recorded honestly without being promoted into adoption.
const V1_EXTERNAL_EVIDENCE_CLASSES: [&str; 4] =
    ["registry_publication", "reverse_dependency", "not_consumer_evidence", "unavailable"];

/// The only two rulings `v1` may carry.
const V1_RULINGS: [&str; 2] = ["absorb", "retain"];

/// The claim ceiling `v1` may state. A manifest that promotes itself to a
/// migration authority fails closed here.
const REQUIRED_CLAIM_CEILING: &str = "inventory_and_ruling_only";

// ---------------------------------------------------------------------------
// Strict schema. Every struct denies unknown fields, so a field added to the
// artifact without a model change fails closed rather than being ignored.
// ---------------------------------------------------------------------------

/// The audit contract as authored.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    schema: String,
    schema_version: u64,
    planning_basis: String,
    claim_ceiling: String,
    programme: Programme,
    derivation: Derivation,
    public_items: Vec<PublicItemRow>,
    reexport_paths: Vec<ReexportRow>,
    consumers: Vec<ConsumerRow>,
    package_surfaces: Vec<PackageSurfaceRow>,
    external_evidence: Vec<ExternalEvidenceRow>,
    ruling: LifecycleRuling,
    successor_wake_conditions: Vec<WakeRow>,
    limitations: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Programme {
    audit_issue: u64,
    package_disposition_issue: u64,
    programme_issue: u64,
    package_set_owner_issue: u64,
    method_authority: String,
}

/// Declares which instrument produced which half of the denominator, so a later
/// reader cannot mistake an authored disposition for a derived fact.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Derivation {
    v2_source: String,
    v1_ast_source: String,
    public_item_instrument: String,
    consumer_instrument: String,
    gating_scan_roots: Vec<String>,
    gating_scan_excludes: Vec<String>,
    non_gating_note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicItemRow {
    item_id: String,
    path: String,
    kind: String,
    derived_shape: String,
    v1_relation: String,
    v1_counterpart: Option<String>,
    range_disposition: String,
    recovery_disposition: String,
    node_id_disposition: String,
    serialization_disposition: String,
    currentness_disposition: String,
    parity_note: String,
    consumer_ids: Vec<String>,
    replacement_path: String,
    earliest_removal_owner: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReexportRow {
    reexport_id: String,
    path: String,
    site: String,
    exposes: String,
    consumer_id: String,
    compatibility_obligation: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsumerRow {
    consumer_id: String,
    file: String,
    role: String,
    symbols: Vec<String>,
    proposition: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageSurfaceRow {
    surface_id: String,
    surface: String,
    site: String,
    current_status: String,
    owner: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalEvidenceRow {
    evidence_id: String,
    class: String,
    observed: String,
    observed_at: String,
    instrument: String,
    lifecycle_weight: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleRuling {
    ruling: String,
    rationale: String,
    evidence_ids: Vec<String>,
    compatibility_window: String,
    reversal_condition: String,
    independent_lifecycle_evidence_ids: Vec<String>,
    unknowns: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WakeRow {
    successor_issue: u64,
    wake_event: String,
    blocked_until: String,
}

// ---------------------------------------------------------------------------
// Derivation: current public surface, read with `syn`.
// ---------------------------------------------------------------------------

/// One derived public item: its rendered path and its normalized shape.
///
/// The shape is the load-bearing half. Comparing names alone would let a field
/// change, a derive change, or a signature change land under an unmoved row —
/// which is falsifier 6 in the issue's own list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedItem {
    /// Rendered public path, e.g. `perl_ast_v2::NodeKind::Program`.
    pub path: String,
    /// Item kind, drawn from [`V1_ITEM_KINDS`].
    pub kind: String,
    /// Normalized declaration shape.
    pub shape: String,
}

/// Derive every public item of the audited crate from its source.
pub fn derive_public_items(source: &str) -> Result<Vec<DerivedItem>> {
    let file = syn::parse_file(source)
        .map_err(|err| color_eyre::eyre::eyre!("failed to parse the v2 crate source: {err}"))?;

    let mut derived = Vec::new();
    collect_public_items(&file.items, V2_CRATE_PATH, &mut derived)?;
    derived.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(derived)
}

/// Walk one module's items, recursing into public inline modules.
///
/// The unhandled-kind arm **bails**. Skipping instead would be the single worst
/// defect this instrument could have: a `pub fn`, `pub const`, `pub trait`,
/// `pub union` or `pub use` added to the crate would produce no row, no error
/// and no drift, so the "a public item cannot land without a row" claim would
/// be silently false for every item shape the match does not name. An audit that
/// cannot model a public item must say so, not pass.
fn collect_public_items(
    items: &[syn::Item],
    module_path: &str,
    derived: &mut Vec<DerivedItem>,
) -> Result<()> {
    for item in items {
        // Non-public items are genuinely out of scope, so they are skipped
        // before the unhandled-kind check — otherwise a private helper `fn`
        // would fail the audit for no reason.
        if let Some(vis) = item_visibility(item)
            && !is_public(vis)
        {
            continue;
        }

        match item {
            syn::Item::Type(alias) => {
                let name = alias.ident.to_string();
                derived.push(DerivedItem {
                    path: format!("{module_path}::{name}"),
                    kind: "type_alias".to_string(),
                    shape: format!(
                        "type {name}{} = {}",
                        render_generics(&alias.generics)?,
                        render_type(&alias.ty)?
                    ),
                });
            }
            syn::Item::Struct(item_struct) => {
                let name = item_struct.ident.to_string();
                derived.push(DerivedItem {
                    path: format!("{module_path}::{name}"),
                    kind: "struct".to_string(),
                    shape: format!(
                        "struct {name}{} {}{} {}",
                        render_generics(&item_struct.generics)?,
                        render_derives(&item_struct.attrs)?,
                        render_non_exhaustive(&item_struct.attrs),
                        render_fields(&item_struct.fields, FieldContext::Struct)?
                    ),
                });
            }
            syn::Item::Enum(item_enum) => {
                let name = item_enum.ident.to_string();
                derived.push(DerivedItem {
                    path: format!("{module_path}::{name}"),
                    kind: "enum".to_string(),
                    shape: format!(
                        "enum {name}{} {}{} ({} variants)",
                        render_generics(&item_enum.generics)?,
                        render_derives(&item_enum.attrs)?,
                        render_non_exhaustive(&item_enum.attrs),
                        item_enum.variants.len()
                    ),
                });
                for variant in &item_enum.variants {
                    let variant_name = variant.ident.to_string();
                    derived.push(DerivedItem {
                        path: format!("{module_path}::{name}::{variant_name}"),
                        kind: "enum_variant".to_string(),
                        shape: format!(
                            "variant {variant_name}{} {}",
                            render_non_exhaustive(&variant.attrs),
                            render_fields(&variant.fields, FieldContext::Variant)?
                        ),
                    });
                }
            }
            syn::Item::Impl(item_impl) => {
                let self_name = match item_impl.self_ty.as_ref() {
                    syn::Type::Path(path) => render_path(&path.path)?,
                    other => bail!(
                        "the audit derivation supports inherent and trait impls on a named \
                         type only; found {other:?}"
                    ),
                };
                match &item_impl.trait_ {
                    Some((trait_path, _)) => {
                        let trait_name = render_path(trait_path)?;
                        derived.push(DerivedItem {
                            path: format!("{module_path}::{self_name} as {trait_name}"),
                            kind: "trait_impl".to_string(),
                            shape: format!("impl {trait_name} for {self_name}"),
                        });
                    }
                    None => {
                        for impl_item in &item_impl.items {
                            let syn::ImplItem::Fn(method) = impl_item else {
                                continue;
                            };
                            if !is_public(&method.vis) {
                                continue;
                            }
                            let method_name = method.sig.ident.to_string();
                            derived.push(DerivedItem {
                                path: format!("{module_path}::{self_name}::{method_name}"),
                                kind: "associated_fn".to_string(),
                                shape: render_signature(&method.sig)?,
                            });
                        }
                    }
                }
            }
            syn::Item::Mod(module) => {
                let Some((_, inner)) = &module.content else {
                    bail!(
                        "`pub mod {}` has no inline body, so its public surface lives in another \
                         file that this derivation does not read. The audited crate is a single \
                         file by contract; splitting it requires teaching the derivation to \
                         follow modules.",
                        module.ident
                    );
                };
                let nested = format!("{module_path}::{}", module.ident);
                collect_public_items(inner, &nested, derived)?;
            }
            other => bail!(
                "the audit derivation cannot model this public item ({}) in `{module_path}`. It \
                 must be handled explicitly: silently skipping it would mean a public item had \
                 landed with no inventory row and no error, which is the exact failure this \
                 audit exists to prevent.",
                describe_item(other)
            ),
        }
    }
    Ok(())
}

/// The visibility of an item, where the item kind has one.
///
/// `impl` blocks and macro invocations have no visibility of their own; they
/// return `None` and are dispatched on kind instead.
fn item_visibility(item: &syn::Item) -> Option<&syn::Visibility> {
    match item {
        syn::Item::Const(x) => Some(&x.vis),
        syn::Item::Enum(x) => Some(&x.vis),
        syn::Item::ExternCrate(x) => Some(&x.vis),
        syn::Item::Fn(x) => Some(&x.vis),
        syn::Item::Mod(x) => Some(&x.vis),
        syn::Item::Static(x) => Some(&x.vis),
        syn::Item::Struct(x) => Some(&x.vis),
        syn::Item::Trait(x) => Some(&x.vis),
        syn::Item::TraitAlias(x) => Some(&x.vis),
        syn::Item::Type(x) => Some(&x.vis),
        syn::Item::Union(x) => Some(&x.vis),
        syn::Item::Use(x) => Some(&x.vis),
        _ => None,
    }
}

/// A human-readable label for an item kind, for the fail-closed message.
fn describe_item(item: &syn::Item) -> String {
    match item {
        syn::Item::Const(x) => format!("pub const {}", x.ident),
        syn::Item::Fn(x) => format!("pub fn {}", x.sig.ident),
        syn::Item::Static(x) => format!("pub static {}", x.ident),
        syn::Item::Trait(x) => format!("pub trait {}", x.ident),
        syn::Item::TraitAlias(x) => format!("pub trait alias {}", x.ident),
        syn::Item::Union(x) => format!("pub union {}", x.ident),
        syn::Item::Use(_) => "a pub use re-export".to_string(),
        syn::Item::ExternCrate(x) => format!("pub extern crate {}", x.ident),
        syn::Item::Macro(_) => {
            "a macro invocation, whose expansion is not visible here".to_string()
        }
        syn::Item::ForeignMod(_) => "an extern block".to_string(),
        _ => "an item kind this derivation has no name for".to_string(),
    }
}

/// Render item-level generics, which are public contract.
///
/// Adding a lifetime, a type parameter, or a bound on one narrows or widens the
/// API without touching a single field, so a shape that ignored generics would
/// let a breaking change land under an unmoved row.
fn render_generics(generics: &syn::Generics) -> Result<String> {
    let mut parts: Vec<String> = Vec::new();
    for param in &generics.params {
        match param {
            syn::GenericParam::Lifetime(lifetime) => {
                parts.push(format!("'{}", lifetime.lifetime.ident));
            }
            syn::GenericParam::Type(ty) => {
                let mut rendered = ty.ident.to_string();
                let mut bounds: Vec<String> = Vec::new();
                for bound in &ty.bounds {
                    match bound {
                        syn::TypeParamBound::Trait(trait_bound) => {
                            bounds.push(render_path(&trait_bound.path)?);
                        }
                        syn::TypeParamBound::Lifetime(lifetime) => {
                            bounds.push(format!("'{}", lifetime.ident));
                        }
                        other => bail!(
                            "the audit derivation cannot render this generic bound: {other:?}"
                        ),
                    }
                }
                if !bounds.is_empty() {
                    bounds.sort();
                    rendered.push_str(&format!(": {}", bounds.join(" + ")));
                }
                parts.push(rendered);
            }
            // `GenericParam` is exhaustive in syn 3, so there is no catch-all
            // arm here: adding one would be dead code, and a future variant
            // would surface as a compile error, which is the stronger signal.
            syn::GenericParam::Const(konst) => {
                parts.push(format!("const {}: {}", konst.ident, render_type(&konst.ty)?));
            }
        }
    }

    let where_clause = match &generics.where_clause {
        Some(clause) => format!(" where[{} predicates]", clause.predicates.len()),
        None => String::new(),
    };

    if parts.is_empty() {
        Ok(where_clause)
    } else {
        Ok(format!("<{}>{where_clause}", parts.join(", ")))
    }
}

/// Record `#[non_exhaustive]`, which is public contract: adding it stops
/// downstream exhaustive matching and literal construction, and no field or
/// derive would record the change.
fn render_non_exhaustive(attrs: &[syn::Attribute]) -> &'static str {
    if attrs.iter().any(|attr| attr.path().is_ident("non_exhaustive")) {
        " non_exhaustive"
    } else {
        ""
    }
}

/// Derive the production `NodeKind` variant names the parity rows may cite.
///
/// The parity claim is only as good as this set: without it, `v1_counterpart`
/// could name a variant that no longer exists and the row would still read
/// green.
pub fn derive_v1_node_kind_variants(source: &str) -> Result<BTreeSet<String>> {
    let file = syn::parse_file(source).map_err(|err| {
        color_eyre::eyre::eyre!("failed to parse the production AST source: {err}")
    })?;

    for item in &file.items {
        if let syn::Item::Enum(item_enum) = item
            && item_enum.ident == "NodeKind"
        {
            return Ok(item_enum.variants.iter().map(|v| v.ident.to_string()).collect());
        }
    }

    bail!("the production AST source declares no `NodeKind` enum; the parity denominator is gone")
}

fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

/// Render the derive list, which is public contract: losing `Copy` or `Eq` is a
/// breaking change that no field or signature would record.
///
/// The full path is rendered, not just the last identifier. A path-qualified
/// derive such as `serde::Serialize` is exactly the kind of change these rows
/// must catch — the audit's own `serialization_disposition` columns say
/// serialization is not represented, and a silently dropped `Serialize` would
/// make that claim false while the shape stayed byte-identical.
///
/// A `#[derive(...)]` this cannot parse fails the audit rather than yielding a
/// shorter list that would read as a deliberate removal.
fn render_derives(attrs: &[syn::Attribute]) -> Result<String> {
    let mut names: Vec<String> = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        let mut rendered: Vec<String> = Vec::new();
        let mut render_error: Option<color_eyre::Report> = None;
        attr.parse_nested_meta(|meta| {
            match render_path(&meta.path) {
                Ok(path) => rendered.push(path),
                Err(err) => render_error = Some(err),
            }
            Ok(())
        })
        .map_err(|err| color_eyre::eyre::eyre!("could not parse a derive attribute: {err}"))?;
        if let Some(err) = render_error {
            return Err(err);
        }
        names.extend(rendered);
    }
    names.sort();
    Ok(format!("derives[{}]", names.join(", ")))
}

/// Which declaration the fields belong to.
///
/// `syn` reports enum-variant fields with inherited visibility, exactly like a
/// private struct field, but a variant field is publicly reachable through the
/// variant. Only the caller knows which case it is, so the context is passed in
/// rather than guessed from the field.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldContext {
    /// Struct fields: inherited visibility means genuinely private.
    Struct,
    /// Enum-variant fields: every field is reachable through the variant.
    Variant,
}

fn render_fields(fields: &syn::Fields, context: FieldContext) -> Result<String> {
    match fields {
        syn::Fields::Unit => Ok("unit".to_string()),
        syn::Fields::Named(named) => {
            let mut reachable = Vec::new();
            let mut non_public = 0usize;
            for field in &named.named {
                let Some(ident) = field.ident.as_ref() else {
                    bail!("a named field without an identifier is not renderable");
                };
                if context == FieldContext::Struct && !is_public(&field.vis) {
                    // Counted, never named: a private field is not public
                    // contract, but adding or removing one still moves the
                    // shape so the row cannot silently stay current.
                    non_public += 1;
                    continue;
                }
                reachable.push(format!("{ident}: {}", render_type(&field.ty)?));
            }
            let mut rendered = format!("{{ {} }}", reachable.join(", "));
            if non_public > 0 {
                rendered.push_str(&format!(" +{non_public} non-public"));
            }
            Ok(rendered)
        }
        syn::Fields::Unnamed(unnamed) => {
            let mut parts = Vec::new();
            for field in &unnamed.unnamed {
                parts.push(render_type(&field.ty)?);
            }
            Ok(format!("({})", parts.join(", ")))
        }
    }
}

fn render_signature(sig: &syn::Signature) -> Result<String> {
    let mut inputs = Vec::new();
    for input in &sig.inputs {
        match input {
            syn::FnArg::Receiver(receiver) => {
                // `ReceiverKind` is `#[non_exhaustive]`; an unrecognised
                // shorthand must stop the audit rather than be rendered as a
                // plain `self` it is not.
                let rendered = match &receiver.kind {
                    syn::ReceiverKind::Value => {
                        if receiver.mutability.is_some() {
                            "mut self".to_string()
                        } else {
                            "self".to_string()
                        }
                    }
                    syn::ReceiverKind::Reference(_, lifetime, mutability) => {
                        let mut rendered = String::from("&");
                        if let Some(lifetime) = lifetime {
                            rendered.push_str(&format!("'{} ", lifetime.ident));
                        }
                        if mutability.is_some() {
                            rendered.push_str("mut ");
                        }
                        rendered.push_str("self");
                        rendered
                    }
                    syn::ReceiverKind::Typed(_, ty) => {
                        format!("self: {}", render_type(ty)?)
                    }
                    other => bail!(
                        "the audit derivation cannot render this receiver shape; it must be \
                         handled explicitly rather than approximated: {other:?}"
                    ),
                };
                inputs.push(rendered);
            }
            syn::FnArg::Typed(typed) => {
                let name = match typed.pat.as_ref() {
                    syn::Pat::Ident(ident) => ident.ident.to_string(),
                    _ => "_".to_string(),
                };
                inputs.push(format!("{name}: {}", render_type(&typed.ty)?));
            }
        }
    }
    let output = match &sig.output {
        syn::ReturnType::Default => "()".to_string(),
        syn::ReturnType::Type(_, ty) => render_type(ty)?,
    };
    Ok(format!("fn {}({}) -> {output}", sig.ident, inputs.join(", ")))
}

/// Render a type deterministically.
///
/// Deliberately total over the shapes this audit's sources actually use and
/// fail-closed everywhere else: an unrenderable type must stop the audit rather
/// than collapse into a placeholder that two different types would share.
fn render_type(ty: &syn::Type) -> Result<String> {
    match ty {
        syn::Type::Path(path) => {
            if path.qself.is_some() {
                bail!("qualified-self types are outside the audited surface");
            }
            render_path(&path.path)
        }
        syn::Type::Reference(reference) => {
            let mut rendered = String::from("&");
            if let Some(lifetime) = &reference.lifetime {
                rendered.push_str(&format!("'{} ", lifetime.ident));
            }
            if reference.mutability.is_some() {
                rendered.push_str("mut ");
            }
            rendered.push_str(&render_type(&reference.elem)?);
            Ok(rendered)
        }
        syn::Type::Tuple(tuple) => {
            let mut parts = Vec::new();
            for elem in &tuple.elems {
                parts.push(render_type(elem)?);
            }
            Ok(format!("({})", parts.join(", ")))
        }
        syn::Type::Slice(slice) => Ok(format!("[{}]", render_type(&slice.elem)?)),
        // The length is part of the type. Rendering `[u8; _]` would make
        // `[u8; 4]` and `[u8; 8]` share a shape, so a breaking width change
        // could land under an unmoved row.
        syn::Type::Array(array) => {
            Ok(format!("[{}; {}]", render_type(&array.elem)?, render_const_expr(&array.len)?))
        }
        syn::Type::Paren(paren) => render_type(&paren.elem),
        syn::Type::Group(group) => render_type(&group.elem),
        syn::Type::Never(_) => Ok("!".to_string()),
        other => bail!(
            "the audit derivation cannot render this type shape; it must be handled \
             explicitly rather than approximated: {other:?}"
        ),
    }
}

/// Render a const expression appearing in a type position (an array length).
///
/// Deliberately narrow: a literal or a named const path, and fail closed on
/// anything else rather than reduce a computed length to a placeholder that a
/// different length would share.
fn render_const_expr(expr: &syn::Expr) -> Result<String> {
    match expr {
        syn::Expr::Lit(lit) => match &lit.lit {
            syn::Lit::Int(int) => Ok(int.base10_digits().to_string()),
            other => {
                bail!("the audit derivation cannot render this array length literal: {other:?}")
            }
        },
        syn::Expr::Path(path) => render_path(&path.path),
        other => bail!(
            "the audit derivation cannot render this array length expression; it must be handled \
             explicitly rather than approximated: {other:?}"
        ),
    }
}

fn render_path(path: &syn::Path) -> Result<String> {
    let mut segments = Vec::new();
    for segment in &path.segments {
        let name = segment.ident.to_string();
        match &segment.arguments {
            syn::PathArguments::None => segments.push(name),
            syn::PathArguments::AngleBracketed(args) => {
                let mut rendered = Vec::new();
                for arg in &args.args {
                    match arg {
                        syn::GenericArgument::Type(ty) => rendered.push(render_type(ty)?),
                        syn::GenericArgument::Lifetime(lifetime) => {
                            rendered.push(format!("'{}", lifetime.ident));
                        }
                        other => bail!(
                            "the audit derivation cannot render this generic argument: {other:?}"
                        ),
                    }
                }
                segments.push(format!("{name}<{}>", rendered.join(", ")));
            }
            syn::PathArguments::Parenthesized(_) => {
                bail!("parenthesized (Fn-style) path arguments are outside the audited surface");
            }
        }
    }
    Ok(segments.join("::"))
}

// ---------------------------------------------------------------------------
// Derivation: current reference denominator, scanned from the surfaces where an
// actual API or package consumer can appear.
// ---------------------------------------------------------------------------

/// Tokens that name the audited package in source or manifest form.
///
/// `perl_ast::v2` earns its own token: the canonical re-export path contains
/// none of the other three, so a scan without it silently drops every consumer
/// that reaches the package the documented way.
const REFERENCE_TOKENS: [&str; 4] = ["perl_ast_v2", "perl-ast-v2", "ast_v2", "perl_ast::v2"];

/// This module's own files, which name the audited package in every token form
/// because describing it is their entire job.
///
/// This is an instrument self-exclusion, not a consumer exemption: it is bounded
/// to two named paths, and `instrument_self_exclusion_is_exactly_two_named_files`
/// pins it so it cannot grow into a way of hiding a real consumer.
const INSTRUMENT_SELF_FILES: [&str; 2] =
    ["xtask/src/ast_v2_lifecycle_audit.rs", "xtask/src/ast_v2_lifecycle_audit_tests.rs"];

/// Characters that continue an identifier in Rust source or a TOML package name.
///
/// Both sides matter. Without a trailing boundary `ast_v2` matches
/// `ast_v2_lifecycle_audit`; without a leading one it matches the tail of
/// `perl_ast_v2`, which the dedicated token already covers.
fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

/// Whether `text` contains `token` as a whole word.
fn contains_token(text: &str, token: &str) -> bool {
    let mut from = 0usize;
    while let Some(offset) = text[from..].find(token) {
        let start = from + offset;
        let end = start + token.len();
        let leading_ok = text[..start].chars().next_back().is_none_or(|ch| !is_word_char(ch));
        let trailing_ok = text[end..].chars().next().is_none_or(|ch| !is_word_char(ch));
        if leading_ok && trailing_ok {
            return true;
        }
        // Advance past this occurrence's first character so overlapping
        // candidates are still examined.
        from = start + token.chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// Two v2 types are public API of `perl-parser-core` under unqualified names
/// (`crates/perl-parser-core/src/lib.rs:97`). A consumer reaching them that way
/// writes `perl_parser_core::DiagnosticId` and names none of the four tokens
/// above, so a token scan alone silently drops it.
///
/// This was not a theoretical gap. `crates/perl-parser-core/tests/diagnostic_id_tests.rs`
/// does exactly that and was missing from the first version of this inventory,
/// which had recorded the blind spot and then wrongly asserted it was empty.
/// Detecting the path mechanically is worth more than documenting it.
///
/// Both the direct form and the grouped-import form are matched, because
/// `use perl_parser_core::{DiagnosticId, ParseError};` reaches the same type.
static REEXPORTED_TYPE_PATH: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "a literal pattern that fails to compile is a build-time defect in this module, \
                  not a runtime condition a caller could handle"
    )]
    regex::Regex::new(
        r"perl_parser_core::(?:\{[^}]*\b(?:DiagnosticId|MissingKind)\b|(?:DiagnosticId|MissingKind)\b)",
    )
    .expect("the re-exported-type pattern is a valid literal regex")
});

/// Whether `text` reaches the audited package by any known path.
pub fn mentions_audited_package(text: &str) -> bool {
    REFERENCE_TOKENS.iter().any(|token| contains_token(text, token))
        || REEXPORTED_TYPE_PATH.is_match(text)
}

/// Rust path forms that mean a file actually *reaches the API*, as opposed to
/// naming the crate as a string.
///
/// These are all `::`-joined Rust paths. A policy glob (`"crates/perl-ast-v2/**"`)
/// or a crate-name inventory entry (`"perl-ast-v2",`) is hyphenated and matches
/// none of them, which is exactly the distinction the role vocabulary draws.
static API_USE_FORM: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "a literal pattern that fails to compile is a build-time defect in this module, \
                  not a runtime condition a caller could handle"
    )]
    regex::Regex::new(
        r"\bperl_ast_v2\s*(?:::|as\b)|\bast_v2\s*::|\bperl_ast::v2\b|perl_parser_core::(?:\{[^}]*\b(?:DiagnosticId|MissingKind)\b|(?:DiagnosticId|MissingKind)\b)",
    )
    .expect("the API-use pattern is a valid literal regex")
});

/// Whether a Rust file reaches the package's API from code, ignoring comments.
///
/// This stops the inverse of falsifier 9. The scan knows a file references the
/// package; without this it does not know *how*, so a real code consumer could
/// be relabelled `docs_reference` with its symbols emptied and slip out of every
/// gating check — the non-gating roles are exactly the ones that skip the symbol
/// and scan requirements.
///
/// Deliberately scoped two ways. Only `.rs` files are examined, because a TOML
/// policy row that names the crate is a genuine `policy_inventory` reference and
/// not API use. And only `::`-joined Rust path forms count, so a crate name
/// inside a string literal — a coverage fixture path, an allowlist glob — stays
/// correctly classifiable as inventory rather than being forced to a code role.
pub fn references_package_api_in_code(text: &str, path: &str) -> bool {
    if !path.ends_with(".rs") {
        return false;
    }
    let mut stripped = String::with_capacity(text.len());
    let mut in_block = false;
    for line in text.lines() {
        let mut rest = line;
        loop {
            if in_block {
                match rest.find("*/") {
                    Some(end) => {
                        rest = &rest[end + 2..];
                        in_block = false;
                    }
                    None => {
                        rest = "";
                        break;
                    }
                }
            }
            let block_start = rest.find("/*");
            let line_start = rest.find("//");
            match (block_start, line_start) {
                (Some(b), Some(l)) if b < l => {
                    stripped.push_str(&rest[..b]);
                    rest = &rest[b + 2..];
                    in_block = true;
                }
                (_, Some(l)) => {
                    stripped.push_str(&rest[..l]);
                    rest = "";
                    break;
                }
                (Some(b), None) => {
                    stripped.push_str(&rest[..b]);
                    rest = &rest[b + 2..];
                    in_block = true;
                }
                (None, None) => break,
            }
        }
        stripped.push_str(rest);
        stripped.push('\n');
    }
    API_USE_FORM.is_match(&stripped)
}

/// Roots scanned for the gating consumer denominator.
///
/// Bounded on purpose. Documentation, scripts and release notes mention the
/// package and are inventoried, but a prose mention is not an API consumer and
/// must not be able to fail this check — that is falsifier 9.
const GATING_SCAN_ROOTS: [&str; 4] = ["crates", "xtask", "policy", "Cargo.toml"];

/// Directory names never descended into during the scan.
const GATING_SCAN_EXCLUDES: [&str; 4] = ["target", ".git", "archive", "node_modules"];

/// Scan the gating roots for files that reference the audited package.
///
/// Returns repository-relative paths with `/` separators so the output does not
/// depend on the host path separator or on walk order — falsifier 12.
pub fn derive_reference_files(repo_root: &Path) -> Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();

    for root in GATING_SCAN_ROOTS {
        let absolute = repo_root.join(root);
        if !absolute.exists() {
            bail!(
                "gating scan root `{root}` is missing; the consumer denominator is not derivable"
            );
        }
        for entry in
            walkdir::WalkDir::new(&absolute).sort_by_file_name().into_iter().filter_entry(|entry| {
                // Compared as an OsStr, not through `to_string_lossy`: a lossy
                // conversion can map a non-UTF-8 directory name onto an
                // exclusion and skip a subtree that was never excluded.
                !GATING_SCAN_EXCLUDES.iter().any(|excluded| entry.file_name() == *excluded)
            })
        {
            let entry = entry.with_context(|| format!("failed to walk gating scan root {root}"))?;
            let path = entry.path();

            // `WalkDir` does not follow symlinks by default, so this is not a
            // recursion guard — it is a completeness guard. A symlinked source
            // file is yielded with a symlink file type, so the `is_file` check
            // below skips it silently, and a real consumer could sit behind a
            // link and never enter the denominator.
            //
            // The guard is deliberately narrow rather than "reject every
            // symlink": an unrelated symlinked source file is harmless, and
            // failing on it would block work this audit has no business
            // blocking. It fails only when the link actually hides a reference,
            // or when the link cannot be read at all and therefore cannot be
            // shown to be harmless. A link is not resolved into the denominator
            // under its own path, because the target may already be there under
            // its real one and one file must not occupy two rows.
            if entry.path_is_symlink() && is_scannable(path) {
                let relative = relative_slash_path(repo_root, path)?;
                match std::fs::read_to_string(path) {
                    Ok(text) if mentions_audited_package(&text) => bail!(
                        "`{relative}` is a symbolic link whose target references the audited \
                         package. It would be skipped silently, so the denominator cannot account \
                         for it: replace the link with the file, or exclude it deliberately."
                    ),
                    Ok(_) => {}
                    Err(err) => bail!(
                        "`{relative}` is a symbolic link to scannable source that cannot be read \
                         ({err}), so it cannot be shown not to reference the audited package."
                    ),
                }
            }

            if !entry.file_type().is_file() {
                continue;
            }
            if !is_scannable(path) {
                continue;
            }
            let relative = relative_slash_path(repo_root, path)?;
            if INSTRUMENT_SELF_FILES.contains(&relative.as_str()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                // A non-UTF-8 file under these roots cannot declare a Rust
                // dependency or import; skipping it is not a silent loss.
                continue;
            };
            if mentions_audited_package(&text) {
                found.insert(relative);
            }
        }
    }

    Ok(found)
}

/// Whether a repository-relative path falls inside a gating scan root.
///
/// Extension matters as well as prefix: a `.md` file under `crates/` is never
/// scanned, so its absence from the scan says nothing about staleness.
fn is_under_gating_scan_root(file: &str) -> bool {
    let scannable_extension = file.ends_with(".rs") || file.ends_with(".toml");
    scannable_extension
        && GATING_SCAN_ROOTS
            .iter()
            .any(|root| file == *root || file.starts_with(&format!("{root}/")))
}

fn is_scannable(path: &Path) -> bool {
    matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs") | Some("toml"))
}

fn relative_slash_path(repo_root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(repo_root)
        .with_context(|| format!("scanned path {} escaped the repository root", path.display()))?;
    let mut rendered = String::new();
    for (index, component) in relative.components().enumerate() {
        if index > 0 {
            rendered.push('/');
        }
        rendered.push_str(&component.as_os_str().to_string_lossy());
    }
    Ok(rendered)
}

// ---------------------------------------------------------------------------
// Canonical digest: recursive content walk, order-invariant, byte-ordinal
// sorting, SHA-256 uppercase hex. Same shape family as the landed runtime-train
// and cleanup-train projections so tooling stays comparable without coupling
// pins.
// ---------------------------------------------------------------------------

/// Canonical content digest of the manifest value.
pub fn canonical_digest(value: &Value) -> Result<String> {
    let mut canonical = String::new();
    canonical_walk(value, &mut canonical)?;
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = std::fmt::Write::write_fmt(&mut hex, format_args!("{byte:02X}"));
    }
    Ok(hex)
}

fn canonical_walk(value: &Value, out: &mut String) -> Result<()> {
    match value {
        Value::Null => out.push_str("n;"),
        Value::Bool(flag) => {
            let _ = std::fmt::Write::write_fmt(
                out,
                format_args!("b:{};", if *flag { "True" } else { "False" }),
            );
        }
        Value::Number(number) => {
            if let Some(signed) = number.as_i64() {
                let _ = std::fmt::Write::write_fmt(out, format_args!("i:{signed};"));
            } else if let Some(unsigned) = number.as_u64() {
                let _ = std::fmt::Write::write_fmt(out, format_args!("i:{unsigned};"));
            } else {
                bail!("manifest canonicalization defines integers only; found {number}");
            }
        }
        // Escaping bound: only the backslash and the semicolon that terminates a
        // scalar token are escaped, matching the landed walks. Container
        // delimiters are not escaped inside string content; that stays
        // unambiguous because every scalar token is self-terminating and object
        // keys come from the strict field set rather than from input.
        Value::String(text) => {
            out.push_str("s:");
            for ch in text.chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    ';' => out.push_str("\\;"),
                    _ => out.push(ch),
                }
            }
            out.push(';');
        }
        Value::Array(items) => {
            out.push_str("a[");
            for item in items {
                canonical_walk(item, out)?;
            }
            out.push_str("];");
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push_str("o{");
            for key in keys {
                out.push_str("k:");
                out.push_str(key);
                out.push(';');
                let Some(entry) = map.get(key) else {
                    bail!("canonicalization lost key {key} between enumeration and lookup");
                };
                canonical_walk(entry, out)?;
            }
            out.push_str("};");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Loaded view and bounded read accessors for the successor slices.
// ---------------------------------------------------------------------------

/// A manifest that passed every law, with its digest.
#[derive(Debug)]
pub struct LoadedAudit {
    manifest: Manifest,
    canonical_digest: String,
}

impl LoadedAudit {
    /// The recorded lifecycle ruling: `absorb` or `retain`.
    pub fn ruling(&self) -> &str {
        &self.manifest.ruling.ruling
    }

    /// The compatibility window the ruling binds its successors to.
    pub fn compatibility_window(&self) -> &str {
        &self.manifest.ruling.compatibility_window
    }

    /// The condition that would move the ruling the other way.
    pub fn reversal_condition(&self) -> &str {
        &self.manifest.ruling.reversal_condition
    }

    /// Wake event for one successor issue (#8844, #8845, #8847).
    pub fn wake_event(&self, successor_issue: u64) -> Option<&str> {
        self.manifest
            .successor_wake_conditions
            .iter()
            .find(|row| row.successor_issue == successor_issue)
            .map(|row| row.wake_event.as_str())
    }

    /// Canonical digest of the loaded revision.
    pub fn canonical_digest(&self) -> &str {
        &self.canonical_digest
    }

    /// Number of inventoried public items.
    pub fn public_item_count(&self) -> usize {
        self.manifest.public_items.len()
    }

    /// Number of inventoried consumer rows.
    pub fn consumer_count(&self) -> usize {
        self.manifest.consumers.len()
    }

    /// Number of inventoried public re-export paths.
    pub fn reexport_count(&self) -> usize {
        self.manifest.reexport_paths.len()
    }
}

/// Load and fully validate the workspace audit contract.
pub fn load_audit() -> Result<LoadedAudit> {
    let root = workspace_root()?;
    load_audit_from(&root)
}

/// Load and fully validate the audit contract rooted at `repo_root`.
pub fn load_audit_from(repo_root: &Path) -> Result<LoadedAudit> {
    let manifest_path = repo_root.join(MANIFEST_RELATIVE_PATH);
    let bytes = std::fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("{} is not valid JSON", manifest_path.display()))?;

    let manifest: Manifest = serde_json::from_value(value.clone())
        .with_context(|| "strict deserialization of the audit contract failed")?;

    validate_manifest(&manifest)?;
    reconcile_with_source(&manifest, repo_root)?;

    let digest = canonical_digest(&value)?;
    if digest != PINNED_CANONICAL_DIGEST {
        bail!(
            "audit contract digest drift: pinned {PINNED_CANONICAL_DIGEST}, computed {digest}. \
             A semantic revision must move the pin deliberately."
        );
    }

    Ok(LoadedAudit { manifest, canonical_digest: digest })
}

/// Locate the workspace root from this crate's manifest directory.
pub fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| color_eyre::eyre::eyre!("xtask must live in a workspace subdirectory"))
}

// ---------------------------------------------------------------------------
// Laws.
// ---------------------------------------------------------------------------

/// Every law that needs only the manifest itself.
fn validate_manifest(m: &Manifest) -> Result<()> {
    if m.schema != SCHEMA_NAME {
        bail!("schema identity mismatch: expected {SCHEMA_NAME}, found {}", m.schema);
    }
    if m.schema_version != SCHEMA_VERSION {
        bail!("schema version mismatch: expected {SCHEMA_VERSION}, found {}", m.schema_version);
    }
    if m.claim_ceiling != REQUIRED_CLAIM_CEILING {
        bail!(
            "claim ceiling must remain `{REQUIRED_CLAIM_CEILING}`; found `{}`. #8843 inventories \
             and rules; it does not authorize migration.",
            m.claim_ceiling
        );
    }
    // The programme coordinates are part of the contract, not decoration: a
    // successor that reads this manifest resolves its own place from them.
    for (label, actual, expected) in [
        ("audit issue", m.programme.audit_issue, 8843u64),
        ("package disposition issue", m.programme.package_disposition_issue, 7403),
        ("programme issue", m.programme.programme_issue, 9213),
        ("package set owner issue", m.programme.package_set_owner_issue, 7400),
    ] {
        if actual != expected {
            bail!("{label} must remain {expected}; found {actual}");
        }
    }
    if m.programme.method_authority.trim().is_empty() {
        bail!("the programme block names no method authority");
    }
    if m.planning_basis.trim().is_empty() {
        bail!("planning basis must state the evidence pin it was authored against");
    }
    if m.limitations.is_empty() {
        bail!("an audit that records no limitation is claiming completeness it cannot hold");
    }

    validate_closed_vocabularies(m)?;
    validate_unique_ids(m)?;
    validate_referential_integrity(m)?;
    validate_ruling(m)?;
    Ok(())
}

fn validate_closed_vocabularies(m: &Manifest) -> Result<()> {
    for row in &m.public_items {
        require_member("public item kind", &row.kind, &V1_ITEM_KINDS, &row.item_id)?;
        require_member("v1 relation", &row.v1_relation, &V1_RELATIONS, &row.item_id)?;
        for (column, value) in [
            ("range disposition", &row.range_disposition),
            ("recovery disposition", &row.recovery_disposition),
            ("node id disposition", &row.node_id_disposition),
            ("serialization disposition", &row.serialization_disposition),
            ("currentness disposition", &row.currentness_disposition),
        ] {
            require_member(column, value, &V1_DISPOSITIONS, &row.item_id)?;
        }
        if row.parity_note.trim().is_empty() {
            bail!("public item {} states no parity note", row.item_id);
        }
        if row.replacement_path.trim().is_empty() {
            bail!("public item {} states no replacement path", row.item_id);
        }
        if row.earliest_removal_owner.trim().is_empty() {
            bail!("public item {} names no earliest removal owner", row.item_id);
        }
        // `v1_counterpart` names a production `NodeKind` variant, which only an
        // enum-variant row can have. Scoping the rule this way keeps the parity
        // claim checkable: every counterpart a row names is verified to exist,
        // and no row can assert a relation whose subject cannot be checked.
        if row.kind == "enum_variant" {
            match (row.v1_relation.as_str(), row.v1_counterpart.as_deref()) {
                ("unique" | "not_applicable", Some(counterpart)) => bail!(
                    "public item {} is `{}` yet names production counterpart `{counterpart}`",
                    row.item_id,
                    row.v1_relation
                ),
                ("equivalent" | "narrower" | "divergent", None) => bail!(
                    "public item {} claims relation `{}` without naming a production counterpart; \
                     a parity claim with no subject cannot be checked",
                    row.item_id,
                    row.v1_relation
                ),
                _ => {}
            }
        } else if row.v1_counterpart.is_some() {
            bail!(
                "public item {} is a {} and names a production `NodeKind` counterpart; only an \
                 enum-variant row can, because that is the only comparison this audit checks",
                row.item_id,
                row.kind
            );
        }
    }

    for row in &m.consumers {
        require_member("consumer role", &row.role, &V1_CONSUMER_ROLES, &row.consumer_id)?;
        if row.proposition.trim().is_empty() {
            bail!("consumer {} states no proposition", row.consumer_id);
        }
        if row.file.trim().is_empty() {
            bail!("consumer {} names no file", row.consumer_id);
        }
        if row.file.contains('\\') {
            bail!(
                "consumer {} uses a host path separator; repository-relative `/` paths keep the \
                 denominator host-independent",
                row.consumer_id
            );
        }
    }

    for row in &m.package_surfaces {
        require_member("package surface", &row.surface, &V1_PACKAGE_SURFACES, &row.surface_id)?;
        if row.current_status.trim().is_empty() {
            bail!("package surface {} states no current status", row.surface_id);
        }
        if row.owner.trim().is_empty() {
            bail!("package surface {} names no owner", row.surface_id);
        }
    }

    for row in &m.external_evidence {
        require_member(
            "external evidence class",
            &row.class,
            &V1_EXTERNAL_EVIDENCE_CLASSES,
            &row.evidence_id,
        )?;
        if row.observed.trim().is_empty() {
            bail!("external evidence {} records no observation", row.evidence_id);
        }
        if row.observed_at.trim().is_empty() {
            bail!(
                "external evidence {} records no observation date; undated external evidence \
                 cannot be aged",
                row.evidence_id
            );
        }
        if row.instrument.trim().is_empty() {
            bail!("external evidence {} names no instrument", row.evidence_id);
        }
        // An observation with no stated weight invites a later reader to give it
        // whatever weight the conclusion needs.
        if row.lifecycle_weight.trim().is_empty() {
            bail!(
                "external evidence {} states no lifecycle weight; an observation that does not \
                 say what it does and does not establish is not evidence",
                row.evidence_id
            );
        }
    }

    require_member("ruling", &m.ruling.ruling, &V1_RULINGS, "ruling")?;
    Ok(())
}

fn require_member(column: &str, value: &str, allowed: &[&str], row_id: &str) -> Result<()> {
    if !allowed.contains(&value) {
        bail!("{row_id}: {column} `{value}` is outside the closed v1 vocabulary {allowed:?}");
    }
    Ok(())
}

fn validate_unique_ids(m: &Manifest) -> Result<()> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for row in &m.public_items {
        if !seen.insert(row.item_id.as_str()) {
            bail!("duplicate public item id {}", row.item_id);
        }
    }
    let mut paths: BTreeSet<&str> = BTreeSet::new();
    for row in &m.public_items {
        if !paths.insert(row.path.as_str()) {
            bail!("duplicate public item path {}", row.path);
        }
    }
    let mut consumer_ids: BTreeSet<&str> = BTreeSet::new();
    for row in &m.consumers {
        if !consumer_ids.insert(row.consumer_id.as_str()) {
            bail!("duplicate consumer id {}", row.consumer_id);
        }
    }
    let mut reexport_ids: BTreeSet<&str> = BTreeSet::new();
    for row in &m.reexport_paths {
        if !reexport_ids.insert(row.reexport_id.as_str()) {
            bail!("duplicate re-export id {}", row.reexport_id);
        }
    }
    let mut surface_ids: BTreeSet<&str> = BTreeSet::new();
    for row in &m.package_surfaces {
        if !surface_ids.insert(row.surface_id.as_str()) {
            bail!("duplicate package surface id {}", row.surface_id);
        }
    }
    let mut evidence_ids: BTreeSet<&str> = BTreeSet::new();
    for row in &m.external_evidence {
        if !evidence_ids.insert(row.evidence_id.as_str()) {
            bail!("duplicate external evidence id {}", row.evidence_id);
        }
    }
    let mut successors: BTreeSet<u64> = BTreeSet::new();
    for row in &m.successor_wake_conditions {
        if !successors.insert(row.successor_issue) {
            bail!("duplicate successor wake row for #{}", row.successor_issue);
        }
    }
    Ok(())
}

fn validate_referential_integrity(m: &Manifest) -> Result<()> {
    let consumer_ids: BTreeSet<&str> =
        m.consumers.iter().map(|row| row.consumer_id.as_str()).collect();

    for row in &m.public_items {
        if row.consumer_ids.is_empty() {
            bail!(
                "public item {} names no consumer; an item with genuinely no consumer must say so \
                 with an explicit `consumer:none` row rather than an empty list",
                row.item_id
            );
        }
        for consumer_id in &row.consumer_ids {
            if !consumer_ids.contains(consumer_id.as_str()) {
                bail!("public item {} references unknown consumer {consumer_id}", row.item_id);
            }
        }
    }

    let mut reexport_paths: BTreeSet<&str> = BTreeSet::new();
    for row in &m.reexport_paths {
        if !consumer_ids.contains(row.consumer_id.as_str()) {
            bail!("re-export {} references unknown consumer {}", row.reexport_id, row.consumer_id);
        }
        if !reexport_paths.insert(row.path.as_str()) {
            bail!("duplicate re-export path {}", row.path);
        }
        if row.exposes.trim().is_empty() {
            bail!(
                "re-export {} does not say what it exposes; a path that re-exports an unstated \
                 surface cannot be migrated",
                row.reexport_id
            );
        }
        if row.compatibility_obligation.trim().is_empty() {
            bail!(
                "re-export {} states no compatibility obligation; a public path with no stated \
                 obligation cannot bound a removal",
                row.reexport_id
            );
        }
    }

    let evidence_ids: BTreeSet<&str> =
        m.external_evidence.iter().map(|row| row.evidence_id.as_str()).collect();
    for evidence_id in &m.ruling.evidence_ids {
        if !evidence_ids.contains(evidence_id.as_str()) {
            bail!("ruling references unknown evidence {evidence_id}");
        }
    }
    for evidence_id in &m.ruling.independent_lifecycle_evidence_ids {
        if !evidence_ids.contains(evidence_id.as_str()) {
            bail!("ruling references unknown independent-lifecycle evidence {evidence_id}");
        }
    }
    Ok(())
}

fn validate_ruling(m: &Manifest) -> Result<()> {
    if m.ruling.rationale.trim().is_empty() {
        bail!("the lifecycle ruling states no rationale");
    }
    if m.ruling.evidence_ids.is_empty() {
        bail!("the lifecycle ruling names no evidence");
    }
    if m.ruling.compatibility_window.trim().is_empty() {
        bail!("the lifecycle ruling states no compatibility window");
    }
    if m.ruling.reversal_condition.trim().is_empty() {
        bail!("the lifecycle ruling states no reversal condition");
    }
    // A lifecycle ruling drawn partly from instruments that cannot see external
    // source has unknowns by construction. Claiming none is overclaiming.
    if m.ruling.unknowns.is_empty() {
        bail!(
            "the lifecycle ruling records no unknowns; an audit whose external instruments \
             cannot enumerate non-registry use has unknowns whether or not it states them"
        );
    }
    for unknown in &m.ruling.unknowns {
        if unknown.trim().is_empty() {
            bail!("the lifecycle ruling carries an empty unknown");
        }
    }

    // The issue's threshold, executable: package existence, docs.rs metadata,
    // publish allowlisting or experimental branding do not meet it.
    if m.ruling.ruling == "retain" && m.ruling.independent_lifecycle_evidence_ids.is_empty() {
        bail!(
            "a `retain` ruling must name independent-lifecycle evidence. Package existence, \
             publish allowlisting or docs metadata alone do not meet the threshold."
        );
    }

    // Download volume is consistent with CI, docs.rs and mirror traffic. It may
    // be recorded, but it may never be the thing a ruling rests on.
    for evidence_id in
        m.ruling.evidence_ids.iter().chain(m.ruling.independent_lifecycle_evidence_ids.iter())
    {
        let Some(row) = m.external_evidence.iter().find(|row| &row.evidence_id == evidence_id)
        else {
            continue;
        };
        if row.class == "not_consumer_evidence" {
            bail!(
                "ruling rests on evidence {evidence_id}, which is classified \
                 `not_consumer_evidence`; download volume is not adoption"
            );
        }
    }

    if m.successor_wake_conditions.is_empty() {
        bail!("the ruling defines no successor wake condition; #8844/#8845/#8847 would not know");
    }
    for row in &m.successor_wake_conditions {
        if row.wake_event.trim().is_empty() {
            bail!("successor #{} has no wake event", row.successor_issue);
        }
        if row.blocked_until.trim().is_empty() {
            bail!("successor #{} states no blocking condition", row.successor_issue);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Reconciliation against current source. This is the half that makes the
// inventory checked rather than authored.
// ---------------------------------------------------------------------------

fn reconcile_with_source(m: &Manifest, repo_root: &Path) -> Result<()> {
    if m.derivation.v2_source != V2_SOURCE_RELATIVE_PATH {
        bail!(
            "the manifest declares v2 source `{}` but the derivation reads `{V2_SOURCE_RELATIVE_PATH}`",
            m.derivation.v2_source
        );
    }
    if m.derivation.v1_ast_source != V1_AST_SOURCE_RELATIVE_PATH {
        bail!(
            "the manifest declares production AST source `{}` but the derivation reads \
             `{V1_AST_SOURCE_RELATIVE_PATH}`",
            m.derivation.v1_ast_source
        );
    }
    if m.derivation.gating_scan_roots != GATING_SCAN_ROOTS {
        bail!(
            "the manifest declares scan roots {:?} but the derivation scans {GATING_SCAN_ROOTS:?}",
            m.derivation.gating_scan_roots
        );
    }
    if m.derivation.gating_scan_excludes != GATING_SCAN_EXCLUDES {
        bail!(
            "the manifest declares scan excludes {:?} but the derivation excludes \
             {GATING_SCAN_EXCLUDES:?}",
            m.derivation.gating_scan_excludes
        );
    }
    // The instrument descriptions are what tell a later reader which columns are
    // derived and which are authored. An audit that does not say so is one a
    // reader can only over-trust.
    for (label, text) in [
        ("public item instrument", &m.derivation.public_item_instrument),
        ("consumer instrument", &m.derivation.consumer_instrument),
        ("non-gating note", &m.derivation.non_gating_note),
    ] {
        if text.trim().is_empty() {
            bail!("the derivation block states no {label}");
        }
    }

    reconcile_public_items(m, repo_root)?;
    reconcile_parity_counterparts(m, repo_root)?;
    reconcile_consumers(m, repo_root)?;
    reconcile_reexport_sites(m, repo_root)?;
    reconcile_package_surface_sites(m, repo_root)?;
    Ok(())
}

fn reconcile_public_items(m: &Manifest, repo_root: &Path) -> Result<()> {
    let source = std::fs::read_to_string(repo_root.join(V2_SOURCE_RELATIVE_PATH))
        .with_context(|| format!("failed to read {V2_SOURCE_RELATIVE_PATH}"))?;
    let derived = derive_public_items(&source)?;

    let derived_by_path: BTreeMap<&str, &DerivedItem> =
        derived.iter().map(|item| (item.path.as_str(), item)).collect();
    let row_by_path: BTreeMap<&str, &PublicItemRow> =
        m.public_items.iter().map(|row| (row.path.as_str(), row)).collect();

    for (path, item) in &derived_by_path {
        let Some(row) = row_by_path.get(path) else {
            bail!(
                "public item `{path}` exists in {V2_SOURCE_RELATIVE_PATH} with no inventory row. \
                 A new public item or re-export cannot land without a row."
            );
        };
        if row.kind != item.kind {
            bail!(
                "public item `{path}` is a {} in source but the inventory calls it a {}",
                item.kind,
                row.kind
            );
        }
        if row.derived_shape != item.shape {
            bail!(
                "public item `{path}` changed shape without moving its row.\n  inventory: {}\n  \
                 source:    {}",
                row.derived_shape,
                item.shape
            );
        }
    }

    for path in row_by_path.keys() {
        if !derived_by_path.contains_key(path) {
            bail!(
                "inventory row for `{path}` describes a public item that no longer exists in \
                 {V2_SOURCE_RELATIVE_PATH}; a stale row is not a record, it is a false claim"
            );
        }
    }

    Ok(())
}

fn reconcile_parity_counterparts(m: &Manifest, repo_root: &Path) -> Result<()> {
    let source = std::fs::read_to_string(repo_root.join(V1_AST_SOURCE_RELATIVE_PATH))
        .with_context(|| format!("failed to read {V1_AST_SOURCE_RELATIVE_PATH}"))?;
    let v1_variants = derive_v1_node_kind_variants(&source)?;

    for row in &m.public_items {
        let Some(counterpart) = row.v1_counterpart.as_deref() else {
            continue;
        };
        if !v1_variants.contains(counterpart) {
            bail!(
                "public item {} names production counterpart `{counterpart}`, which is not a \
                 current `perl_ast::NodeKind` variant. A parity row cannot cite a variant that \
                 does not exist.",
                row.item_id
            );
        }
    }
    Ok(())
}

fn reconcile_consumers(m: &Manifest, repo_root: &Path) -> Result<()> {
    let scanned = derive_reference_files(repo_root)?;
    let all_files: BTreeSet<&str> = m.consumers.iter().map(|row| row.file.as_str()).collect();

    // Direction one: nothing may reference the package without being inventoried.
    // The row's *role* then decides what the reference means — classification is
    // the audit's job, and excluding a file from the scan to avoid classifying it
    // would be the audit lying to itself.
    for file in &scanned {
        if !all_files.contains(file.as_str()) {
            bail!(
                "`{file}` references the audited package but has no consumer row. A new direct \
                 consumer or reference must move the inventory."
            );
        }
    }

    for row in &m.consumers {
        let gating = GATING_CONSUMER_ROLES.contains(&row.role.as_str());

        // Direction two: a gating row claims a real API or package consumer, so
        // the scan must still find it. A non-gating row may legitimately live
        // outside the scan roots (docs/, scripts/), so it is checked for
        // existence only — a deleted document must not leave a green row behind.
        if !scanned.contains(&row.file) {
            if gating {
                bail!(
                    "consumer {} claims `{}` is a `{}` consumer, but the current scan of \
                     {GATING_SCAN_ROOTS:?} finds no whole-word reference there",
                    row.consumer_id,
                    row.file,
                    row.role
                );
            }
            // A non-gating row may legitimately sit outside the scan roots
            // (docs/, scripts/), where existence is all that can be checked.
            // Inside them, absence from the scan means the reference is gone and
            // the row is stale — a green row describing nothing.
            if is_under_gating_scan_root(&row.file) {
                bail!(
                    "consumer {} records `{}`, which is inside the scanned roots but no longer \
                     references the audited package; the row is stale",
                    row.consumer_id,
                    row.file
                );
            }
            if !repo_root.join(&row.file).exists() {
                bail!(
                    "consumer {} records `{}`, which no longer exists",
                    row.consumer_id,
                    row.file
                );
            }
        }

        // A dependency whose exact symbol use is unknown is not an inventoried
        // consumer — that is falsifier 4. Only reference roles may be symbol-free,
        // and they must say what the reference is instead.
        if gating && row.symbols.is_empty() {
            bail!(
                "consumer {} has role `{}` but names no symbols; a direct dependency whose exact \
                 symbol use is unknown is not an inventoried consumer",
                row.consumer_id,
                row.role
            );
        }

        // The inverse of falsifier 9, and the one the first version missed: a
        // real code consumer relabelled `docs_reference` or `policy_inventory`
        // escapes every gating check, because those roles are exactly the ones
        // that skip the symbol and scan requirements. Classification must answer
        // to the file, so a non-gating role is only available to a file that
        // does not reach the package from code.
        if !gating
            && let Ok(text) = std::fs::read_to_string(repo_root.join(&row.file))
            && references_package_api_in_code(&text, &row.file)
        {
            bail!(
                "consumer {} claims the non-gating role `{}`, but `{}` reaches the audited \
                 package's API from Rust code, not from a comment or a crate-name string. A real \
                 consumer cannot be downgraded to a prose mention.",
                row.consumer_id,
                row.role,
                row.file
            );
        }
    }

    Ok(())
}

fn reconcile_reexport_sites(m: &Manifest, repo_root: &Path) -> Result<()> {
    for row in &m.reexport_paths {
        let (file, line) = split_site(&row.site, &row.reexport_id)?;
        let text = std::fs::read_to_string(repo_root.join(&file)).with_context(|| {
            format!("re-export {} names site {} which cannot be read", row.reexport_id, row.site)
        })?;
        let Some(actual) = text.lines().nth(line - 1) else {
            bail!("re-export {} names {}:{line}, past the end of that file", row.reexport_id, file);
        };
        if !mentions_audited_package(actual) {
            bail!(
                "re-export {} names {}:{line}, but that line no longer mentions the audited \
                 package:\n  {}",
                row.reexport_id,
                file,
                actual.trim()
            );
        }
    }
    Ok(())
}

fn reconcile_package_surface_sites(m: &Manifest, repo_root: &Path) -> Result<()> {
    for row in &m.package_surfaces {
        let (file, line) = split_site(&row.site, &row.surface_id)?;
        let text = std::fs::read_to_string(repo_root.join(&file)).with_context(|| {
            format!(
                "package surface {} names site {} which cannot be read",
                row.surface_id, row.site
            )
        })?;
        let Some(actual) = text.lines().nth(line - 1) else {
            bail!(
                "package surface {} names {}:{line}, past the end of that file",
                row.surface_id,
                file
            );
        };
        if !mentions_audited_package(actual) {
            bail!(
                "package surface {} names {}:{line}, but that line no longer mentions the audited \
                 package:\n  {}",
                row.surface_id,
                file,
                actual.trim()
            );
        }
    }
    Ok(())
}

/// Split a `path/to/file.rs:123` site reference.
fn split_site(site: &str, row_id: &str) -> Result<(String, usize)> {
    let Some((file, line)) = site.rsplit_once(':') else {
        bail!("{row_id}: site `{site}` is not in `path:line` form");
    };
    let line: usize =
        line.parse().with_context(|| format!("{row_id}: site `{site}` has no numeric line"))?;
    if line == 0 {
        bail!("{row_id}: site `{site}` uses line 0; lines are 1-based");
    }
    Ok((file.to_string(), line))
}

#[cfg(test)]
#[path = "ast_v2_lifecycle_audit_tests.rs"]
mod tests;
