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
//!
//! What "both directions" covers, precisely, because the strength differs by
//! row and an unqualified claim would read as stronger than the instrument is:
//!
//! | rows | forward | backward |
//! |---|---|---|
//! | public items | derived from source; an unmodelled declaration stops the audit | shape compared exactly |
//! | re-exports | derived from every scanned file, module paths carried | each row's public path must still be bound |
//! | consumers | derived from a reference scan of the gating roots | file presence, role, and — on Rust gating rows — each named identifier |
//! | package surfaces | **not derived**; rows are checked, unrecorded surfaces are not found | the named line must still mention the package |
//! | parity, dispositions, ruling | authored | vocabulary, referential integrity, and the ruling laws |
//!
//! The package-surface row is the honest weak one: a new `[workspace]` entry or
//! a changed publish setting is not discovered, and the recorded settings are
//! compared as a line mention rather than as values. Deriving them needs a
//! manifest and policy model this issue's ceiling does not authorize; it is
//! recorded as a limitation rather than implied away by the sentence above.
//! * the ruling laws: a `retain` ruling must name independent-lifecycle evidence,
//!   and download counts are structurally barred from being consumer evidence.
//!
//! Claim ceiling (#8843): inventory and ruling only. No source move, package
//! removal, semantic convergence, parser change, or claim that v2 is
//! parity-complete belongs here — adding one violates the issue's non-goals and
//! the guard test named `no_migration_or_mutation_surface_is_added`.
//!
//! Recorded shape vs. breaking change. The shape is the declaration as written,
//! which is deliberately broader than the semver-breaking surface: renaming a
//! public function's parameter moves a row although Rust has no named arguments
//! and no consumer can depend on that name. The rule deciding what gets elided
//! is what eliding costs. Sorting named field order is free — no fact leaves the
//! shape — so a cosmetic reordering does not fire. Dropping a parameter name, or
//! a private field's type, is not free: the first deletes a fact a person reads
//! when reviewing what #8845 must move, the second hides a retype that changes
//! the struct's auto traits. So field order is relaxed, a private field's *name*
//! is elided while its *type* is kept, and parameter names are recorded. This is
//! a decision, not an oversight; a rename costs one mechanical row edit against
//! an error that prints both shapes.
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
    "3EA626CA2093F4E4967CF56B57E321510248520041B2CF16D51F31AE685F726B";

// ---------------------------------------------------------------------------
// Code-owned v1 vocabularies. A cardinality check lets a repinned manifest
// rename a value and keep the count, so the reviewed sets live here and are
// compared for exact membership.
// ---------------------------------------------------------------------------

/// Item kinds the derivation can emit and a row may claim.
const V1_ITEM_KINDS: [&str; 8] = [
    "type_alias",
    "struct",
    "enum",
    "enum_variant",
    "associated_fn",
    "associated_const",
    "associated_type",
    "trait_impl",
];

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
///
/// `release_cadence` and `package_only_proposition` exist because the ruling's
/// own reversal condition names three ways the package could earn an
/// independent lifecycle, and an evidence model carrying only the first would
/// make two of them unrepresentable: a real divergence in release cadence could
/// be observed and still have nowhere to be recorded, let alone authorize the
/// `retain` the reversal condition promises.
const V1_EXTERNAL_EVIDENCE_CLASSES: [&str; 6] = [
    "registry_publication",
    "reverse_dependency",
    "release_cadence",
    "package_only_proposition",
    "not_consumer_evidence",
    "unavailable",
];

/// The evidence classes that can carry the independent-lifecycle threshold, one
/// per clause of the recorded reversal condition.
///
/// Every other class is explicitly below the bar: package existence and
/// publication say only that the package was released, download volume is
/// consistent with CI and mirror traffic, and an unavailable instrument is an
/// unknown rather than a finding.
const QUALIFYING_EVIDENCE_CLASSES: [&str; 3] =
    ["reverse_dependency", "release_cadence", "package_only_proposition"];

/// The only two rulings `v1` may carry.
const V1_RULINGS: [&str; 2] = ["absorb", "retain"];

/// The claim ceiling `v1` may state. A manifest that promotes itself to a
/// migration authority fails closed here.
const REQUIRED_CLAIM_CEILING: &str = "inventory_and_ruling_only";

/// The successors this ruling binds, and which must therefore each carry a wake
/// condition: #8844 (move under `perl_ast::v2`), #8845 (cut consumers over) and
/// #8847 (close the compatibility window). Each is a gate the ruling sets; a
/// manifest that drops one silently un-gates that successor.
const REQUIRED_SUCCESSORS: [u64; 3] = [8844, 8845, 8847];

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
    /// Whether this observation actually clears the issue's independent-lifecycle
    /// bar. Authored, but constrained: only a `reverse_dependency` row may set
    /// it, so a ruling cannot be authorized by listing a below-threshold row.
    meets_independent_lifecycle_threshold: bool,
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
    // Types declared here and *not* public. An `impl` carries no visibility of
    // its own, so without this an inherent `pub fn` on a private helper — or
    // any trait impl for one — produced a public-inventory row for something no
    // consumer can name. That is a false positive in the direction that hurts:
    // it demands a manifest row for internal code and fails reconciliation on
    // an honest change.
    //
    // Collected rather than assumed: a type this list does not mention is
    // treated as public, so an impl for an imported or generic type keeps the
    // fail-closed behaviour rather than being silently skipped.
    let mut private_types: BTreeSet<String> = BTreeSet::new();
    for item in items {
        let declared = match item {
            syn::Item::Struct(node) => Some((node.ident.to_string(), &node.vis)),
            syn::Item::Enum(node) => Some((node.ident.to_string(), &node.vis)),
            syn::Item::Type(node) => Some((node.ident.to_string(), &node.vis)),
            syn::Item::Union(node) => Some((node.ident.to_string(), &node.vis)),
            _ => None,
        };
        if let Some((name, vis)) = declared
            && !is_public(vis)
        {
            private_types.insert(name);
        }
    }

    for item in items {
        // Non-public items are genuinely out of scope, so they are skipped
        // before the unhandled-kind check — otherwise a private helper `fn`
        // would fail the audit for no reason.
        if let Some(vis) = item_visibility(item)
            && !is_public(vis)
        {
            continue;
        }

        // An impl inherits the reachability of the type it is on — but only an
        // *unqualified, single-segment* path can name one of this module's own
        // private declarations. Matching on the last segment alone meant that
        // `impl exported::Helper`, where the module also holds a private
        // `Helper`, was skipped: a public type's methods and trait impls
        // vanished from the inventory with no error. That is the audit losing
        // public API, the failure this instrument is least permitted, and it is
        // silent — so the match is narrowed rather than the skip removed.
        if let syn::Item::Impl(item_impl) = item
            && let syn::Type::Path(path) = item_impl.self_ty.as_ref()
            && path.qself.is_none()
            && path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
            && let Some(only) = path.path.segments.first()
            && private_types.contains(&only.ident.to_string())
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
                        "type {name}{}{} = {}",
                        render_generics(&alias.generics)?,
                        render_contract_attrs(&alias.attrs)?,
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
                        "struct {name}{} {}{}{} {}",
                        render_generics(&item_struct.generics)?,
                        render_derives(&item_struct.attrs)?,
                        render_non_exhaustive(&item_struct.attrs),
                        render_contract_attrs(&item_struct.attrs)?,
                        render_fields(
                            &item_struct.fields,
                            FieldContext::Struct,
                            field_order_is_contract(&item_struct.attrs),
                        )?
                    ),
                });
            }
            syn::Item::Enum(item_enum) => {
                let name = item_enum.ident.to_string();
                derived.push(DerivedItem {
                    path: format!("{module_path}::{name}"),
                    kind: "enum".to_string(),
                    shape: format!(
                        "enum {name}{} {}{}{} ({} variants)",
                        render_generics(&item_enum.generics)?,
                        render_derives(&item_enum.attrs)?,
                        render_non_exhaustive(&item_enum.attrs),
                        render_contract_attrs(&item_enum.attrs)?,
                        item_enum.variants.len()
                    ),
                });
                for variant in &item_enum.variants {
                    let variant_name = variant.ident.to_string();
                    derived.push(DerivedItem {
                        path: format!("{module_path}::{name}::{variant_name}"),
                        kind: "enum_variant".to_string(),
                        shape: format!(
                            "variant {variant_name}{}{}{} {}",
                            render_non_exhaustive(&variant.attrs),
                            render_contract_attrs(&variant.attrs)?,
                            render_discriminant(variant)?,
                            render_fields(
                                &variant.fields,
                                FieldContext::Variant,
                                field_order_is_contract(&item_enum.attrs),
                            )?
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
                        // `impl Trait for X` and `impl<T: Clone> Trait for X<T>`
                        // are different propositions about when the trait is
                        // available, and a `where` bound added later narrows
                        // that availability for every downstream consumer. The
                        // first version rendered neither, so those changes moved
                        // no shape. Safety, polarity and defaultness are the
                        // same kind of contract: `impl !Send for X` is the
                        // opposite claim from `impl Send for X`.
                        let modifiers = &item_impl.modifiers;
                        // `ImplModifiers` is `#[non_exhaustive]`, so a future
                        // syn could carry a modifier this render has never seen.
                        // The crate's own emptiness check is the hook that makes
                        // that fail closed rather than vanish: when neither
                        // modelled modifier is present it must agree the set is
                        // empty, and if it does not, something unmodelled is.
                        if modifiers.defaultness.is_none()
                            && modifiers.polarity.is_none()
                            && modifiers.require_empty().is_err()
                        {
                            bail!(
                                "`impl {trait_name} for {self_name}` carries an impl modifier this \
                                 derivation does not model. It must be rendered explicitly rather \
                                 than dropped from the recorded shape."
                            );
                        }
                        let mut shape = String::new();
                        if modifiers.defaultness.is_some() {
                            shape.push_str("default ");
                        }
                        if item_impl.unsafety.is_some() {
                            shape.push_str("unsafe ");
                        }
                        shape.push_str("impl");
                        shape.push_str(&render_generics(&item_impl.generics)?);
                        shape.push(' ');
                        if modifiers.polarity.is_some() {
                            shape.push('!');
                        }
                        shape.push_str(&trait_name);
                        shape.push_str(" for ");
                        shape.push_str(&render_type(item_impl.self_ty.as_ref())?);
                        shape.push_str(&render_contract_attrs(&item_impl.attrs)?);
                        // A trait impl's associated assignments are its public
                        // contract as much as the header is: `type Item = u8`
                        // becoming `= u16` changes what every consumer of the
                        // trait gets back, and the header does not move. The
                        // impl produces one row, so the assignments belong in
                        // its shape rather than as rows of their own.
                        shape.push_str(&render_trait_impl_items(
                            &item_impl.items,
                            &trait_name,
                            &self_name,
                        )?);
                        derived.push(DerivedItem {
                            path: format!("{module_path}::{self_name} as {trait_name}"),
                            kind: "trait_impl".to_string(),
                            shape,
                        });
                    }
                    None => {
                        for impl_item in &item_impl.items {
                            // Associated consts and types are public API too. The
                            // first version matched only `Fn` and skipped the rest,
                            // so `pub const LIMIT` inside an impl produced no row
                            // and no error — the same silent vanish the item-level
                            // walk was fixed for, one level down.
                            match impl_item {
                                syn::ImplItem::Fn(method) => {
                                    if !is_public(&method.vis) {
                                        continue;
                                    }
                                    derived.push(DerivedItem {
                                        path: format!(
                                            "{module_path}::{self_name}::{}",
                                            method.sig.ident
                                        ),
                                        kind: "associated_fn".to_string(),
                                        shape: format!(
                                            "{}{}",
                                            render_signature(&method.sig)?,
                                            render_contract_attrs(&method.attrs)?
                                        ),
                                    });
                                }
                                syn::ImplItem::Const(konst) => {
                                    if !is_public(&konst.vis) {
                                        continue;
                                    }
                                    derived.push(DerivedItem {
                                        path: format!(
                                            "{module_path}::{self_name}::{}",
                                            konst.ident
                                        ),
                                        kind: "associated_const".to_string(),
                                        shape: format!(
                                            // The value is contract: a public
                                            // `const LIMIT: usize` changing
                                            // from 128 to 64 changes what every
                                            // consumer reads, and the name and
                                            // type do not move.
                                            "const {}: {} = {}{}",
                                            konst.ident,
                                            render_type(&konst.ty)?,
                                            render_const_expr(&konst.expr)?,
                                            render_contract_attrs(&konst.attrs)?
                                        ),
                                    });
                                }
                                syn::ImplItem::Type(assoc) => {
                                    if !is_public(&assoc.vis) {
                                        continue;
                                    }
                                    derived.push(DerivedItem {
                                        path: format!(
                                            "{module_path}::{self_name}::{}",
                                            assoc.ident
                                        ),
                                        kind: "associated_type".to_string(),
                                        shape: format!(
                                            "type {}{}{} = {}",
                                            assoc.ident,
                                            render_generics(&assoc.generics)?,
                                            render_contract_attrs(&assoc.attrs)?,
                                            render_type(&assoc.ty)?
                                        ),
                                    });
                                }
                                syn::ImplItem::Macro(_) | syn::ImplItem::Verbatim(_) => bail!(
                                    "`{module_path}::{self_name}` contains an impl item this                                      derivation cannot model; it must be handled explicitly                                      rather than skipped."
                                ),
                                _ => {}
                            }
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
            // Macros and extern blocks carry no visibility of their own, so the
            // private-item skip above cannot reach them. Bailing on every one
            // would fail an ordinary refactor that adds an internal helper
            // macro, for a reason unrelated to the public API — so privacy is
            // decided here instead, and only genuinely exported surface bails.
            syn::Item::Macro(item_macro) => {
                if item_macro.attrs.iter().any(|attr| attr.path().is_ident("macro_export")) {
                    bail!(
                        "`{module_path}` exports a macro with `#[macro_export]`, which is public \
                         API this derivation cannot model. It must be handled explicitly rather \
                         than skipped."
                    );
                }
                // `ident: None` means this is an *invocation* at item position,
                // not a `macro_rules!` definition. Its expansion can contain
                // public structs, enums, functions, impls or re-exports, and
                // `syn` does not expand macros — so skipping it would let a
                // whole public surface exist with no row and no error, which is
                // the one outcome this derivation refuses. The privacy of the
                // macro says nothing about the privacy of what it emits.
                if item_macro.ident.is_none() {
                    let name = item_macro
                        .mac
                        .path
                        .segments
                        .last()
                        .map_or_else(|| "?".to_string(), |segment| segment.ident.to_string());
                    bail!(
                        "`{module_path}` invokes the macro `{name}!` at item position. Its \
                         expansion may declare public API this derivation cannot see, so the \
                         inventory cannot claim to be complete: expand it, or handle the \
                         invocation explicitly."
                    );
                }
            }
            syn::Item::ForeignMod(foreign) => {
                let exports_public_item = foreign.items.iter().any(|item| match item {
                    syn::ForeignItem::Fn(f) => is_public(&f.vis),
                    syn::ForeignItem::Static(st) => is_public(&st.vis),
                    syn::ForeignItem::Type(t) => is_public(&t.vis),
                    _ => false,
                });
                if exports_public_item {
                    bail!(
                        "`{module_path}` contains an extern block with public items, which are \
                         public API this derivation cannot model."
                    );
                }
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
                // Outlives bounds are contract: `<'a, 'b>` and `<'a: 'b, 'b>`
                // are different APIs and must not share a shape.
                let mut rendered = format!("'{}", lifetime.lifetime.ident);
                let mut bounds: Vec<String> =
                    lifetime.bounds.iter().map(|b| format!("'{}", b.ident)).collect();
                if !bounds.is_empty() {
                    bounds.sort();
                    rendered.push_str(&format!(": {}", bounds.join(" + ")));
                }
                parts.push(rendered);
            }
            syn::GenericParam::Type(ty) => {
                let mut rendered = ty.ident.to_string();
                let mut bounds: Vec<String> = Vec::new();
                for bound in &ty.bounds {
                    match bound {
                        syn::TypeParamBound::Trait(trait_bound) => {
                            bounds.push(render_trait_bound(trait_bound)?);
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
                // A default is contract too: changing `<T = u8>` to `<T = i32>`
                // silently changes what every unparameterised use resolves to.
                if let Some((_, default)) = &ty.default {
                    rendered.push_str(&format!(" = {}", render_type(default)?));
                }
                parts.push(rendered);
            }
            // `GenericParam` is exhaustive in syn 3, so there is no catch-all
            // arm here: adding one would be dead code, and a future variant
            // would surface as a compile error, which is the stronger signal.
            syn::GenericParam::Const(konst) => {
                let mut rendered = format!("const {}: {}", konst.ident, render_type(&konst.ty)?);
                if let Some((_, default)) = &konst.default {
                    rendered.push_str(&format!(" = {}", render_const_expr(default)?));
                }
                parts.push(rendered);
            }
        }
    }

    // Predicate *content*, not a count. Counting alone let `where T: Clone` and
    // `where T: Send` share a shape, which is a breaking difference.
    let where_clause = match &generics.where_clause {
        Some(clause) => {
            let mut predicates: Vec<String> = Vec::new();
            for predicate in &clause.predicates {
                predicates.push(render_where_predicate(predicate)?);
            }
            predicates.sort();
            format!(" where[{}]", predicates.join("; "))
        }
        None => String::new(),
    };

    if parts.is_empty() {
        Ok(where_clause)
    } else {
        Ok(format!("<{}>{where_clause}", parts.join(", ")))
    }
}

/// Render one trait bound, with the parts that change what the bound means.
///
/// The path alone is not the bound. `T: Sized` and `T: ?Sized` differ only in
/// the `?`, and that difference decides whether a caller may pass an unsized
/// type — a widening in one direction and a breaking narrowing in the other.
/// Rendering only the path gave both the same shape. `for<'a>` is contract for
/// the same reason: a higher-ranked bound accepts strictly more than a bound
/// over one named lifetime.
///
/// `TraitBoundModifiers` is `#[non_exhaustive]` and currently has no fields, so
/// there is nothing to render — but a future syn could add one, and dropping it
/// would be the silent vanish this derivation refuses. The crate's own emptiness
/// check is the hook that turns that into a stop.
fn render_trait_bound(trait_bound: &syn::TraitBound) -> Result<String> {
    if trait_bound.modifiers.require_empty().is_err() {
        bail!(
            "the audit derivation cannot render a trait bound carrying an unmodelled modifier; it \
             must be handled explicitly rather than dropped from the recorded shape: {:?}",
            trait_bound.path
        );
    }
    let mut rendered = String::new();
    if let Some(lifetimes) = &trait_bound.lifetimes {
        let mut names: Vec<String> = Vec::new();
        for param in &lifetimes.lifetimes {
            match param {
                syn::GenericParam::Lifetime(lifetime) => {
                    names.push(format!("'{}", lifetime.lifetime.ident));
                }
                other => bail!(
                    "the audit derivation cannot render this higher-ranked bound parameter: \
                     {other:?}"
                ),
            }
        }
        rendered.push_str(&format!("for<{}> ", names.join(", ")));
    }
    if trait_bound.maybe.is_some() {
        rendered.push('?');
    }
    rendered.push_str(&render_path(&trait_bound.path)?);
    Ok(rendered)
}

/// Render one `where` predicate, so two different clauses cannot share a shape.
fn render_where_predicate(predicate: &syn::WherePredicate) -> Result<String> {
    let render_bounds = |bounds: &syn::punctuated::Punctuated<
        syn::TypeParamBound,
        syn::Token![+],
    >|
     -> Result<String> {
        let mut rendered: Vec<String> = Vec::new();
        for bound in bounds {
            match bound {
                syn::TypeParamBound::Trait(trait_bound) => {
                    rendered.push(render_trait_bound(trait_bound)?);
                }
                syn::TypeParamBound::Lifetime(lifetime) => {
                    rendered.push(format!("'{}", lifetime.ident));
                }
                other => bail!("the audit derivation cannot render this where bound: {other:?}"),
            }
        }
        rendered.sort();
        Ok(rendered.join(" + "))
    };

    match predicate {
        syn::WherePredicate::Type(ty) => {
            Ok(format!("{}: {}", render_type(&ty.bounded_ty)?, render_bounds(&ty.bounds)?))
        }
        syn::WherePredicate::Lifetime(lifetime) => {
            let mut bounds: Vec<String> =
                lifetime.bounds.iter().map(|b| format!("'{}", b.ident)).collect();
            bounds.sort();
            Ok(format!("'{}: {}", lifetime.lifetime.ident, bounds.join(" + ")))
        }
        other => bail!(
            "the audit derivation cannot render this where predicate; it must be handled \
             explicitly rather than approximated: {other:?}"
        ),
    }
}

/// Record `#[non_exhaustive]`, which is public contract: adding it stops
/// downstream exhaustive matching and literal construction, and no field or
/// derive would record the change.
/// Attributes that change what a declaration means to a consumer.
///
/// `#[cfg]` decides whether the item exists at all on a given target, `#[repr]`
/// fixes the memory layout an FFI or transmuting consumer depends on, and
/// `#[deprecated]` is the documented removal signal. None of them touch the name,
/// the fields or the types, so a shape built from those alone read identically
/// before and after — a target-specific removal or a layout change landed under
/// an unmoved row.
const CONTRACT_ATTRIBUTES: [&str; 4] = ["cfg", "cfg_attr", "repr", "deprecated"];

/// Whether a rendered `use` target names the audited package itself, rather
/// than forwarding through a local path that happens to be called `ast_v2`.
fn names_package_directly(rendered: &str) -> bool {
    // Anchored at the root, not matched anywhere in the path. A substring test
    // accepted `other::perl_ast_v2` — a nested module of some other package
    // that happens to share the name — as the package itself, which let the
    // chaining rule below wave through the very lookalike it exists to catch.
    // A `use` path names the crate in its first segment or it does not name it.
    let segments: Vec<&str> = rendered.split("::").map(str::trim).collect();
    match segments.as_slice() {
        ["perl_ast_v2", ..] => true,
        ["perl_ast", "v2", ..] => true,
        ["perl_parser_core", second, ..] => *second == "DiagnosticId" || *second == "MissingKind",
        _ => false,
    }
}

/// Render the contract-bearing items of a trait implementation.
///
/// Sorted, so source order does not move the shape. Fails closed on an item
/// kind this derivation has not modelled, for the same reason the item walk
/// does: a trait item that silently contributes nothing is a contract change
/// with no row behind it.
fn render_trait_impl_items(
    items: &[syn::ImplItem],
    trait_name: &str,
    self_name: &str,
) -> Result<String> {
    let mut rendered: Vec<String> = Vec::new();
    for item in items {
        match item {
            syn::ImplItem::Type(assoc) => rendered.push(format!(
                "type {}{} = {}{}",
                assoc.ident,
                render_generics(&assoc.generics)?,
                render_type(&assoc.ty)?,
                render_contract_attrs(&assoc.attrs)?
            )),
            syn::ImplItem::Const(konst) => rendered.push(format!(
                "const {}: {} = {}{}",
                konst.ident,
                render_type(&konst.ty)?,
                render_const_expr(&konst.expr)?,
                render_contract_attrs(&konst.attrs)?
            )),
            // A trait method's signature is fixed by the trait, but its
            // qualifiers are not: `fn f` and `const fn f` are different
            // contracts under the same trait.
            syn::ImplItem::Fn(method) => rendered.push(format!(
                "{}{}",
                render_signature(&method.sig)?,
                render_contract_attrs(&method.attrs)?
            )),
            other => bail!(
                "`impl {trait_name} for {self_name}` carries a trait item this derivation does \
                 not model; it must be handled explicitly rather than dropped from the recorded \
                 shape: {other:?}"
            ),
        }
    }
    if rendered.is_empty() {
        return Ok(String::new());
    }
    rendered.sort();
    Ok(format!(" {{ {} }}", rendered.join("; ")))
}

/// The crate a repository-relative source path belongs to, as a Rust path
/// segment (`crates/perl-parser-core/src/lib.rs` → `perl_parser_core`).
fn crate_root_of(file: &str) -> Option<String> {
    let mut parts = file.split('/');
    if parts.next()? != "crates" {
        return None;
    }
    Some(parts.next()?.replace('-', "_"))
}

/// The module path at which a source file's top-level items live, derived from
/// the standard Cargo layout.
///
/// ```text
/// crates/perl-ast/src/lib.rs                 -> perl_ast
/// crates/perl-parser/src/compat.rs           -> perl_parser::compat
/// crates/perl-parser-core/src/engine/mod.rs  -> perl_parser_core::engine
/// crates/perl-parser-core/src/tokens/trivia.rs
///                                            -> perl_parser_core::tokens::trivia
/// ```
///
/// This is what lets a re-export row be compared as a whole path rather than a
/// suffix. Anchoring only the crate root still let a row name any module it
/// liked — `c::b::ast_v2` was satisfied by a binding derived in
/// `crates/c/src/a.rs` — so the inventory could attach a compatibility
/// obligation to a path that does not exist.
///
/// `None` for anything outside `crates/<name>/src/`. A `#[path]` attribute can
/// also put a file somewhere this does not describe; the caller fails closed on
/// `None` rather than guessing a namespace, which is the conservative direction
/// for an instrument whose rows are compatibility promises.
fn module_path_of(file: &str) -> Option<String> {
    let root = crate_root_of(file)?;
    let mut parts = file.split('/');
    parts.next()?; // "crates"
    parts.next()?; // the crate directory
    if parts.next()? != "src" {
        return None;
    }
    let rest: Vec<&str> = parts.collect();
    let (file_name, directories) = rest.split_last()?;
    let mut segments = vec![root];
    for directory in directories {
        segments.push(directory.replace('-', "_"));
    }
    let stem = file_name.strip_suffix(".rs")?;
    // `lib.rs`, `main.rs` and `mod.rs` *are* the module their location names;
    // any other stem adds a segment.
    if !matches!(stem, "lib" | "main" | "mod") {
        segments.push(stem.replace('-', "_"));
    }
    Some(segments.join("::"))
}

/// Resolve one candidate module path to the inventoried path it lands on.
///
/// Fixed order, because a bare `ast_v2` is a suffix of several inventoried
/// paths and picking the first made the answer depend on map iteration order:
///
/// 1. the site's own crate root joined to the candidate, exactly;
/// 2. a unique match within the site's crate;
/// 3. a unique match anywhere in the inventory.
///
/// More than one match at the last step is reported rather than guessed. Fully
/// resolving a cross-crate forward needs the module graph, which is the name
/// resolution this instrument records as a limitation — so where it cannot be
/// certain it says so instead of choosing.
fn resolve_candidate(
    candidate: &str,
    crate_root: Option<&str>,
    target_of: &BTreeMap<String, String>,
    file: &str,
    binding: &str,
) -> Result<Option<String>> {
    let matches =
        |path: &String| -> bool { path == candidate || path.ends_with(&format!("::{candidate}")) };

    if let Some(root) = crate_root {
        let exact = format!("{root}::{candidate}");
        if target_of.contains_key(&exact) {
            return Ok(Some(exact));
        }
        let prefix = format!("{root}::");
        let own_crate: Vec<&String> =
            target_of.keys().filter(|path| path.starts_with(&prefix) && matches(path)).collect();
        if own_crate.len() == 1 {
            return Ok(Some(own_crate[0].clone()));
        }
        if own_crate.len() > 1 {
            bail!(
                "`{file}` re-exports `{binding}`, whose forwarding target `{candidate}` matches \
                 {} inventoried paths inside `{root}`: {:?}. Resolving that needs the module \
                 graph, so the audit reports the ambiguity rather than choosing one.",
                own_crate.len(),
                own_crate
            );
        }
    }

    let anywhere: Vec<&String> = target_of.keys().filter(|path| matches(path)).collect();
    match anywhere.len() {
        0 => Ok(None),
        1 => Ok(Some(anywhere[0].clone())),
        _ => bail!(
            "`{file}` re-exports `{binding}`, whose forwarding target `{candidate}` matches {} \
             inventoried paths across crates: {:?}. Taking the first would let a local module \
             validate through another crate's chain, so the audit reports the ambiguity instead.",
            anywhere.len(),
            anywhere
        ),
    }
}

/// Follow a forwarding re-export until it reaches a direct package export.
///
/// Stepping once was not enough. A target that merely matches *some* inventoried
/// path is satisfied by another forwarding row, so two rows pointing at each
/// other passed with no direct export anywhere in the chain — the inventory
/// vouching for itself. Each step now moves to the path the target lands on and
/// asks the same question again, with a visited set so a cycle is reported
/// rather than walked forever.
///
/// This still proves reachability *within the inventory*, not that the far end
/// resolves to the package: that needs the name resolution recorded as a
/// limitation.
fn resolve_forwarding(
    rendered: &str,
    file: &str,
    binding: &str,
    target_of: &BTreeMap<String, String>,
) -> Result<()> {
    let mut current = rendered.to_string();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    // The crate a relative target is resolved against is the crate of the path
    // the chain is standing on, not the crate it started in. Deriving it once
    // from the origin file meant that after a hop into another crate, that
    // crate's own relative target was resolved against the origin — accepting
    // or rejecting on a namespace neither path belongs to.
    let mut crate_root = crate_root_of(file);
    loop {
        if names_package_directly(&current) {
            return Ok(());
        }
        let target = current
            .trim_start_matches("crate::")
            .trim_start_matches("self::")
            .trim_start_matches("super::")
            .to_string();
        // The target may name an item *through* an inventoried path —
        // `ast_v2::DiagnosticId` reaches the package through the inventoried
        // `perl_parser_core::ast_v2` — so each module prefix is a candidate,
        // longest first.
        //
        // Which inventoried path a candidate lands on is resolved in a fixed
        // order rather than by taking the first suffix match anywhere. A bare
        // `ast_v2` is a suffix of four inventoried paths across three crates,
        // so "first match wins" made resolution depend on map order and let a
        // local module validate through another crate's genuine chain.
        let mut segments: Vec<&str> = target.split("::").collect();
        let mut landed: Option<String> = None;
        while !segments.is_empty() {
            let candidate = segments.join("::");
            landed =
                resolve_candidate(&candidate, crate_root.as_deref(), target_of, file, binding)?;
            if landed.is_some() {
                break;
            }
            segments.pop();
        }
        let Some(path) = landed else {
            bail!(
                "`{file}` re-exports `{binding}` from `{rendered}`, which names neither the \
                 audited package directly nor any inventoried public path. A forwarding \
                 re-export must terminate in the inventory, or the row describes a path to \
                 something else."
            );
        };
        if !visited.insert(path.clone()) {
            bail!(
                "`{file}` re-exports `{binding}` from `{rendered}`, whose forwarding chain \
                 cycles through `{path}` without ever reaching a direct export of the audited \
                 package. A compatibility path that only forwards to another inventoried path \
                 documents nothing."
            );
        }
        // Standing on `path` now: its own crate is the namespace the next
        // relative target belongs to.
        crate_root = path.split("::").next().map(str::to_string);
        let Some(next) = target_of.get(&path) else {
            bail!(
                "`{file}` re-exports `{binding}` from `{rendered}`, whose chain reaches `{path}` \
                 and stops: no public re-export there says what that path forwards to."
            );
        };
        if *next == current {
            bail!(
                "`{file}` re-exports `{binding}` from `{rendered}`, whose forwarding chain does \
                 not advance past `{path}`."
            );
        }
        current = next.clone();
    }
}

/// Render an explicit enum discriminant.
///
/// The discriminant is the value a `repr` enum crosses an FFI or serialization
/// boundary as, so changing `Missing = 3` to `Missing = 4` is a wire-format
/// change that touches no name, field or type. Nothing in the previous shape
/// recorded it.
fn render_discriminant(variant: &syn::Variant) -> Result<String> {
    match &variant.discriminant {
        Some((_, expr)) => Ok(format!(" = {}", render_const_expr(expr)?)),
        None => Ok(String::new()),
    }
}

/// Render the contract-bearing attributes of one declaration, normalized and
/// ordered so the shape does not depend on source order.
fn render_contract_attrs(attrs: &[syn::Attribute]) -> Result<String> {
    let mut rendered: Vec<String> = Vec::new();
    for attr in attrs {
        let Some(name) = CONTRACT_ATTRIBUTES.iter().find(|name| attr.path().is_ident(name)) else {
            continue;
        };
        match &attr.meta {
            syn::Meta::Path(_) => rendered.push(format!("#[{name}]")),
            syn::Meta::List(list) => {
                // The token text is normalized by `proc_macro2`'s own
                // formatting, so whitespace and line breaks in the source do
                // not move the shape while a changed predicate does.
                rendered.push(format!("#[{name}({})]", list.tokens));
            }
            syn::Meta::NameValue(value) => {
                rendered.push(format!("#[{name} = {}]", render_const_expr(&value.value)?));
            }
        }
    }
    if rendered.is_empty() {
        return Ok(String::new());
    }
    rendered.sort();
    Ok(format!(" attrs[{}]", rendered.join(", ")))
}

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

/// Whether a declaration's own attributes fix its field order.
///
/// Named-field order is not contract in a plain Rust type: construction and
/// pattern matching are by name, so a reordering changes nothing a consumer can
/// observe, and recording it would make the audit fire on a cosmetic edit — the
/// kind of churn that gets a check switched off. Under a layout-fixing `repr`
/// it is the opposite: order *is* the ABI, and a reordering is a breaking
/// change no name or type would record. The attribute decides which rule
/// applies.
/// A `cfg_attr` is read as *possibly* applying its representation, because this
/// audit spans every configuration the source can be built in rather than one
/// host's. `#[cfg_attr(unix, repr(C))]` fixes the layout on unix, so sorting the
/// fields away would hide an ABI reordering from every unix consumer. Taking the
/// conservative branch costs only that a reordering under such a type moves a
/// row; missing it loses a breaking change entirely.
///
/// `render_contract_attrs` already carries `cfg_attr` for exactly this reason.
/// This function looked only at a bare `repr`, so the same rule was syntax-aware
/// in one place and blind in its sibling — the defect class this candidate has
/// hit repeatedly.
fn field_order_is_contract(attrs: &[syn::Attribute]) -> bool {
    fn fixes_layout(spelled: &str) -> bool {
        // `transparent` and the primitive representations do not fix an order
        // over several fields; `C` and `packed` do.
        spelled.contains('C') || spelled.contains("packed")
    }
    attrs.iter().any(|attr| {
        let is_repr = attr.path().is_ident("repr");
        let is_cfg_attr = attr.path().is_ident("cfg_attr");
        if !is_repr && !is_cfg_attr {
            return false;
        }
        let syn::Meta::List(list) = &attr.meta else {
            return false;
        };
        let spelled = list.tokens.to_string();
        if is_repr {
            return fixes_layout(&spelled);
        }
        // Inside a `cfg_attr` the predicate comes first and the attributes
        // follow, so only the part after the first comma can carry a `repr`.
        // Requiring the word `repr` there keeps `#[cfg_attr(feature = "c", ...)]`
        // — a predicate that merely contains a `C` — from reading as layout.
        match spelled.split_once(',') {
            Some((_, applied)) => applied.contains("repr") && fixes_layout(applied),
            None => false,
        }
    })
}

fn render_fields(
    fields: &syn::Fields,
    context: FieldContext,
    order_is_contract: bool,
) -> Result<String> {
    match fields {
        syn::Fields::Unit => Ok("unit".to_string()),
        syn::Fields::Named(named) => {
            let mut reachable = Vec::new();
            let mut non_public = Vec::new();
            for field in &named.named {
                let Some(ident) = field.ident.as_ref() else {
                    bail!("a named field without an identifier is not renderable");
                };
                let ty = render_type(&field.ty)?;
                let attrs = render_contract_attrs(&field.attrs)?;
                if context == FieldContext::Struct && !is_public(&field.vis) {
                    // Typed, never named. A private field's *name* is not
                    // public contract, so a rename must not move the shape.
                    // Its *type* is: the auto traits a struct offers —
                    // `Send`, `Sync`, `Unpin`, `RefUnwindSafe` — are decided
                    // by its fields whatever their visibility, and every
                    // consumer can observe them. Recording only a count let a
                    // `usize` become an `Rc` with the row still reconciling.
                    // Under a layout-fixing `repr` the position is contract
                    // too, so the field stays in place rather than being
                    // lifted out of the sequence.
                    if order_is_contract {
                        reachable.push(format!("_: {ty}{attrs}"));
                    } else {
                        non_public.push(format!("{ty}{attrs}"));
                    }
                    continue;
                }
                reachable.push(format!("{ident}: {ty}{attrs}"));
            }
            // Sorted unless the representation makes order observable, so a
            // cosmetic reordering does not move the shape while an ABI-visible
            // one does.
            if !order_is_contract {
                reachable.sort();
                non_public.sort();
            }
            let mut rendered = format!("{{ {} }}", reachable.join(", "));
            if !non_public.is_empty() {
                rendered.push_str(&format!(" +non-public({})", non_public.join(", ")));
            }
            Ok(rendered)
        }
        syn::Fields::Unnamed(unnamed) => {
            let mut parts = Vec::new();
            for field in &unnamed.unnamed {
                parts.push(format!(
                    "{}{}",
                    render_type(&field.ty)?,
                    render_contract_attrs(&field.attrs)?
                ));
            }
            Ok(format!("({})", parts.join(", ")))
        }
    }
}

fn render_signature(sig: &syn::Signature) -> Result<String> {
    // Qualifiers, ABI and generics are all public contract. Rendering only the
    // name and arguments made `fn f()`, `const fn f()`, `unsafe fn f()` and
    // `fn f<T: Clone>()` share one shape, so any of those changes could land
    // under an unmoved row.
    let mut qualifiers = String::new();
    if sig.constness.is_some() {
        qualifiers.push_str("const ");
    }
    if sig.asyncness.is_some() {
        qualifiers.push_str("async ");
    }
    // syn 3 models this as a three-state `Safety`, not an Option, so `safe` and
    // `unsafe` are distinguished from an unqualified fn rather than collapsed.
    match &sig.safety {
        syn::Safety::Safe(_) => qualifiers.push_str("safe "),
        syn::Safety::Unsafe(_) => qualifiers.push_str("unsafe "),
        syn::Safety::Default => {}
    }
    if let Some(abi) = &sig.abi {
        let name =
            abi.name.as_ref().map_or_else(|| "\"C\"".to_string(), |n| format!("{:?}", n.value()));
        qualifiers.push_str(&format!("extern {name} "));
    }
    let generics = render_generics(&sig.generics)?;
    let variadic = if sig.variadic.is_some() { ", ..." } else { "" };

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
    Ok(format!(
        "{qualifiers}fn {}{generics}({}{variadic}) -> {output}",
        sig.ident,
        inputs.join(", ")
    ))
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

/// Whether a Rust file reaches the package's API from code.
///
/// This stops the inverse of falsifier 9. The scan knows a file references the
/// package; without this it does not know *how*, so a real code consumer could
/// be relabelled `docs_reference` with its symbols emptied and slip out of every
/// gating check — the non-gating roles are exactly the ones that skip the symbol
/// and scan requirements.
///
/// Implemented by parsing with `syn` rather than by stripping comments from the
/// text. The hand-rolled stripper this replaces was wrong in both directions and
/// the failures were ordinary, not exotic: a glob string such as
/// `"crates/perl-ast-v2/*.rs"` contains `/*`, which opened a block comment that
/// never closed and silently discarded the rest of the file — including any real
/// `use perl_ast_v2::Node;` below it — and a `//` inside a string literal ate
/// the rest of its line. Both defeated the guard outright. Parsing removes the
/// whole class: comments and string literals simply are not paths.
///
/// Doc comments survive parsing as `#[doc]` attributes and are still inspected,
/// because a doctest inside one is compiled and run and is therefore real use.
pub fn references_package_api_in_code(text: &str, path: &str) -> bool {
    if !path.ends_with(".rs") {
        return false;
    }
    // Rust this crate cannot parse cannot be shown *not* to use the API. In the
    // classification direction the file is already known to reference the
    // package, so erring toward "this is a consumer" forces a human decision
    // rather than letting an unreadable file take a non-gating role.
    parsed_api_use(text).unwrap_or(true)
}

/// Whether a Rust file reaches the package's API, or `None` if it cannot be
/// parsed.
///
/// This returns `Option` rather than picking a default because the two callers
/// want opposite ones. Discovery asks "does this file reference the package at
/// all", and an unparseable file that names it nowhere is not a consumer —
/// defaulting to `true` there flagged a 590-line Perl fixture with zero
/// mentions of the package. Classification asks "is a reference we already
/// found actually code", where not knowing must not let a real consumer hide.
pub fn parsed_api_use(text: &str) -> Option<bool> {
    let file = syn::parse_file(text).ok()?;
    let mut visitor = ApiUseVisitor { found: false, doc_block: Vec::new() };
    syn::visit::visit_file(&mut visitor, &file);
    // Inner docs (`//!`) belong to the file, not to any item, so nothing has
    // flushed them.
    visitor.flush_doc_block();
    Some(visitor.found)
}

/// Extract the Rust `rustdoc` would compile from one doc comment.
///
/// Only fenced blocks count, and only fences `rustdoc` treats as Rust: a bare
/// ``` ``` ``` opens one, as do the Rust attribute words, while `text`, `json`,
/// `bash` and friends do not. Hidden lines (`# `) are part of the compiled
/// doctest and are kept, with the marker removed.
///
/// Prose is deliberately excluded. An API path written in a sentence is a
/// documentation reference, not compiled use, and classifying it as code forced
/// a gating consumer role onto documentation-only files.
fn rust_doctest_code(block: &str) -> String {
    const RUST_FENCE_WORDS: [&str; 7] =
        ["rust", "ignore", "should_panic", "no_run", "compile_fail", "edition2018", "edition2021"];
    let mut code: Vec<String> = Vec::new();
    let mut inside_rust = false;
    let mut inside_other = false;
    for line in block.lines() {
        let trimmed = line.trim_start();
        if let Some(info) = trimmed.strip_prefix("```") {
            if inside_rust || inside_other {
                inside_rust = false;
                inside_other = false;
                continue;
            }
            let info = info.trim();
            // An empty info string is Rust; otherwise every comma-separated
            // word must be one `rustdoc` recognises as Rust.
            let rust = info.is_empty()
                || info
                    .split(',')
                    .map(str::trim)
                    .filter(|word| !word.is_empty())
                    .all(|word| RUST_FENCE_WORDS.contains(&word));
            if rust {
                inside_rust = true;
            } else {
                inside_other = true;
            }
            continue;
        }
        if inside_rust {
            // `# ` hides a line from the rendered docs but not from the
            // compiler, so it is code.
            let line = trimmed
                .strip_prefix("# ")
                .unwrap_or_else(|| if trimmed == "#" { "" } else { line });
            code.push(line.to_string());
            continue;
        }
        // An indented block is a Markdown code block, and `rustdoc` compiles
        // those as doctests too. Dropping them would be a false negative in the
        // one direction classification may not take: a real compiled consumer
        // downgraded to a prose mention.
        if !inside_other && line.starts_with("    ") {
            code.push(trimmed.strip_prefix("# ").unwrap_or(trimmed).to_string());
        }
    }
    code.join("\n")
}

/// Whether one doctest body reaches the package's API.
///
/// A doctest is usually statements rather than items, and `syn::parse_file`
/// wants items — so a bare `let n = perl_ast::v2::Node::new();` parses as a
/// file only by accident. It is wrapped in a function before parsing, with the
/// unwrapped parse kept for a doctest that really is items (`use` declarations,
/// which is the grouped-import case this exists for).
fn doctest_reaches_package(code: &str) -> bool {
    if parsed_api_use(code).unwrap_or(false) {
        return true;
    }
    parsed_api_use(&format!("fn __doctest() {{\n{code}\n}}")).unwrap_or(false)
}

/// Collects any path, `use` tree, or doc attribute that reaches the package.
struct ApiUseVisitor {
    found: bool,
    /// Doc lines of the item currently being visited, in order.
    ///
    /// A doctest is written one `#[doc = "..."]` attribute per line, so judging
    /// each line on its own splits every construct that spans lines: a grouped
    /// `use perl_ast::{v2,` / `Node};` parses as neither half. The lines are
    /// accumulated and analysed as one block at each item boundary, which is
    /// also what `rustdoc` compiles.
    doc_block: Vec<String>,
}

impl ApiUseVisitor {
    /// Analyse the accumulated doc lines as one block and start a new one.
    ///
    /// `rustdoc` compiles a doc comment as a unit, so that is the unit judged
    /// here. Fenced-code markers and prose lines are left in: they make the
    /// block fail to parse rather than pass wrongly, and the per-line text
    /// match still covers the forms a parse is not needed for.
    fn flush_doc_block(&mut self) {
        if self.doc_block.is_empty() {
            return;
        }
        let block = self.doc_block.join("\n");
        self.doc_block.clear();
        if self.found {
            return;
        }
        // Only what `rustdoc` compiles counts. Judging the whole doc comment
        // classified an API path written in ordinary prose as executable use,
        // which forced a gating consumer role onto a documentation-only
        // reference — the same false-positive direction as an impl on a
        // private type, and just as costly in a check that gates other
        // people's PRs.
        let code = rust_doctest_code(&block);
        if code.trim().is_empty() {
            return;
        }
        if API_USE_FORM.is_match(&code) || doctest_reaches_package(&code) {
            self.found = true;
        }
    }

    /// Match a rendered path. The trailing `::` lets a bare `use perl_ast_v2;`
    /// satisfy the same pattern as a qualified `perl_ast_v2::Node`.
    fn consider(&mut self, rendered: &str) {
        if self.found {
            return;
        }
        if API_USE_FORM.is_match(&format!("{rendered}::")) {
            self.found = true;
        }
    }
}

/// Whether a macro's token stream writes a path into the audited package.
///
/// Token-level, deliberately. `syn` hands a macro body over as an unparsed
/// stream, and rendering it back to text would put string literals — which is
/// what the instrument's own fixtures and assertion messages are made of — back
/// in scope. A `Literal` token is skipped whatever it spells, so
/// `"use perl_ast_v2 as v2;"` does not register while
/// `some_macro!(perl_ast_v2::Node)` does.
///
/// Path-shaped sequences are rebuilt and handed to `names_package_directly`, so
/// what counts as "into the package" has one owner rather than a second opinion
/// living here.
fn macro_tokens_name_package(tokens: &proc_macro2::TokenStream) -> bool {
    scan_macro_path_tokens(tokens, &[])
}

/// The recursive half, carrying the path a group hangs off.
///
/// `prefix` is what preceded the enclosing group, so `perl_ast::{v2, Node}`
/// tests `perl_ast::v2` and `perl_ast::Node` rather than a bare `v2` and `Node`.
/// Descending into a group without it dropped the crate name at the brace and
/// made every grouped import invisible — the same prefix-lost-at-a-boundary
/// mistake this candidate made in re-export matching, in a different shape.
fn scan_macro_path_tokens(tokens: &proc_macro2::TokenStream, prefix: &[String]) -> bool {
    // A comma, or anything else that is not a path, restarts the path — back to
    // the enclosing prefix rather than to nothing, since inside `{a, b}` each
    // element hangs off the same one.
    let restart = |segments: &mut Vec<String>| {
        segments.clear();
        segments.extend_from_slice(prefix);
    };
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut segments: Vec<String> = prefix.to_vec();
    let mut after_ident = false;
    // `as` renames what was just imported, so the identifier following it is a
    // new local name rather than another segment. Without this the generic
    // restart-on-adjacent-identifier rule rebuilt the path from the prefix at
    // both `as` and the alias, synthesising `perl_ast::v2` out of
    // `perl_ast::{Node as v2}` — inventing a consumer from an alias that never
    // touches the package.
    let mut alias_name_next = false;
    for (index, tree) in trees.iter().enumerate() {
        match tree {
            proc_macro2::TokenTree::Group(group) => {
                // A group straight after `::` continues the path; one anywhere
                // else is an unrelated block and starts clean.
                let matched = if !after_ident && !segments.is_empty() {
                    scan_macro_path_tokens(&group.stream(), &segments)
                } else {
                    scan_macro_path_tokens(&group.stream(), &[])
                };
                if matched {
                    return true;
                }
                restart(&mut segments);
                after_ident = false;
            }
            proc_macro2::TokenTree::Ident(ident) => {
                let spelled = ident.to_string();
                if alias_name_next {
                    // The local name an import was renamed to. It is not part of
                    // any path into the package.
                    alias_name_next = false;
                    restart(&mut segments);
                    after_ident = true;
                    continue;
                }
                if spelled == "as" {
                    alias_name_next = true;
                    restart(&mut segments);
                    after_ident = false;
                    continue;
                }
                // Two idents in a row are two paths, not one: `use perl_ast_v2`
                // must not compose into `use::perl_ast_v2`.
                if after_ident {
                    restart(&mut segments);
                }
                segments.push(spelled);
                if names_package_directly(&segments.join("::"))
                    && single_ident_is_a_path(&segments, prefix, &trees, index)
                {
                    return true;
                }
                after_ident = true;
            }
            proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ':' => {
                after_ident = false;
            }
            _ => {
                restart(&mut segments);
                after_ident = false;
            }
        }
    }
    false
}

/// Whether a lone package identifier in a macro is being *used* rather than
/// merely mentioned.
///
/// Inside a macro body an identifier is not yet syntax: `stringify!(perl_ast_v2)`
/// and `some_macro!(name = perl_ast_v2)` pass the crate's name as data and bind
/// nothing. Accepting the bare ident invented a consumer out of them — the audit
/// reporting API use where there is none, which is the direction that fires on
/// other people's PRs rather than merely missing something.
///
/// A single identifier therefore has to look like a path or a binding:
/// `perl_ast_v2::…` traverses it, and `use perl_ast_v2` / `extern crate
/// perl_ast_v2` bind it. Anything with more than one segment already contains a
/// `::` and needs no extra evidence.
fn single_ident_is_a_path(
    segments: &[String],
    prefix: &[String],
    trees: &[proc_macro2::TokenTree],
    index: usize,
) -> bool {
    if segments.len() > 1 || !prefix.is_empty() {
        return true;
    }
    let traverses = matches!(
        (trees.get(index + 1), trees.get(index + 2)),
        (
            Some(proc_macro2::TokenTree::Punct(first)),
            Some(proc_macro2::TokenTree::Punct(second))
        ) if first.as_char() == ':' && second.as_char() == ':'
    );
    let binds =
        index.checked_sub(1).and_then(|previous| trees.get(previous)).is_some_and(
            |tree| match tree {
                proc_macro2::TokenTree::Ident(ident) => {
                    let spelled = ident.to_string();
                    spelled == "use" || spelled == "crate"
                }
                _ => false,
            },
        );
    traverses || binds
}

impl<'ast> syn::visit::Visit<'ast> for ApiUseVisitor {
    fn visit_path(&mut self, node: &'ast syn::Path) {
        let rendered =
            node.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join("::");
        self.consider(&rendered);
        syn::visit::visit_path(self, node);
    }

    fn visit_item(&mut self, node: &'ast syn::Item) {
        // Each item's doc lines are one block. Flushing at the boundary keeps
        // two items' doc comments from being concatenated into a construct
        // neither of them wrote.
        self.flush_doc_block();
        syn::visit::visit_item(self, node);
        self.flush_doc_block();
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let mut paths = Vec::new();
        flatten_use_tree(&node.tree, "", &mut paths);
        for rendered in &paths {
            self.consider(rendered);
        }
        syn::visit::visit_item_use(self, node);
    }

    fn visit_item_extern_crate(&mut self, node: &'ast syn::ItemExternCrate) {
        // `extern crate perl_ast_v2;` binds the crate without ever writing a
        // path, so no `visit_path` sees it. It is a dependency declaration in
        // source form, and on the parser-only branch — this module's own two
        // files — it was the difference between a consumer and an invisible one.
        self.consider(&node.ident.to_string());
        syn::visit::visit_item_extern_crate(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        // `syn` does not expand macros, so a package path inside one is invisible
        // to every other arm here. Scanned at *token* level rather than
        // textually: a string literal is one `Literal` token, so the fixtures and
        // assertion messages these files are full of — `"use perl_ast_v2 as v2;"`
        // — do not register, while `some_macro!(perl_ast_v2::Node)` does.
        if !self.found && macro_tokens_name_package(&node.tokens) {
            self.found = true;
        }
        syn::visit::visit_macro(self, node);
    }

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        // `///` and `//!` arrive here as `#[doc = "..."]`. A doctest in one is
        // compiled and run, so its content counts as code.
        if node.path().is_ident("doc")
            && let syn::Meta::NameValue(pair) = &node.meta
            && let syn::Expr::Lit(lit) = &pair.value
            && let syn::Lit::Str(text) = &lit.lit
        {
            // Everything is deferred to the block: a construct split across
            // doc lines is not judgeable one line at a time, and a line cannot
            // be told from prose without knowing whether a fence is open.
            self.doc_block.push(text.value());
        }
        syn::visit::visit_attribute(self, node);
    }
}

/// Every identifier the file's syntax actually contains.
///
/// Used instead of a text scan when checking a consumer row's symbols: a name
/// left in a comment after the code stopped using it satisfied `contains_token`
/// and kept a stale row alive, which is precisely the case that row check exists
/// to catch. Comments and string literals contribute no identifiers to a parse,
/// so the question becomes "does the code name this" rather than "does the file
/// contain these letters".
fn source_identifiers(text: &str) -> Option<BTreeSet<String>> {
    let file = syn::parse_file(text).ok()?;
    let mut visitor = IdentVisitor { found: BTreeSet::new() };
    syn::visit::visit_file(&mut visitor, &file);
    Some(visitor.found)
}

struct IdentVisitor {
    found: BTreeSet<String>,
}

impl<'ast> syn::visit::Visit<'ast> for IdentVisitor {
    fn visit_ident(&mut self, node: &'ast syn::Ident) {
        self.found.insert(node.to_string());
    }
}

/// Flatten a `use` tree into fully qualified path strings.
fn flatten_use_tree(tree: &syn::UseTree, prefix: &str, out: &mut Vec<String>) {
    let join = |prefix: &str, ident: String| {
        if prefix.is_empty() { ident } else { format!("{prefix}::{ident}") }
    };
    match tree {
        syn::UseTree::Path(node) => {
            let next = join(prefix, node.ident.to_string());
            flatten_use_tree(&node.tree, &next, out);
        }
        syn::UseTree::Name(node) => out.push(join(prefix, node.ident.to_string())),
        syn::UseTree::Rename(node) => out.push(join(prefix, node.ident.to_string())),
        syn::UseTree::Glob(_) => out.push(join(prefix, "*".to_string())),
        syn::UseTree::Group(node) => {
            for item in &node.items {
                flatten_use_tree(item, prefix, out);
            }
        }
    }
}

/// Roots scanned for the gating consumer denominator.
///
/// Bounded on purpose. Documentation, scripts and release notes mention the
/// package and are inventoried, but a prose mention is not an API consumer and
/// must not be able to fail this check — that is falsifier 9.
const GATING_SCAN_ROOTS: [&str; 4] = ["crates", "xtask", "policy", "Cargo.toml"];

/// Directory names never descended into during the scan.
const GATING_SCAN_EXCLUDES: [&str; 4] = ["target", ".git", "archive", "node_modules"];

/// Whether one file's contents reach the audited package, by either route.
///
/// The single discovery classifier, owned here so the symlink guard and the
/// ordinary file branch cannot drift apart. They did: the link branch checked
/// the token scan alone, so a link whose target reached the package through a
/// grouped `use perl_ast::{v2, Node};` named none of the four tokens, was
/// called harmless, and was then skipped as a non-file — precisely the silent
/// loss the guard exists to prevent.
///
/// The text scan alone is not a sufficient filter for Rust: a grouped canonical
/// import contains none of the tokens, so a file reaching the package the
/// documented way would be decided irrelevant before the syntax-aware visitor
/// ever saw it. Rust files are judged by the union — the token scan catches
/// crate-name strings, the parser catches real imports.
///
/// Discovery takes the opposite default from classification on a parse failure:
/// an unparseable file that names the package nowhere is not a consumer.
/// Failing closed here flagged a 590-line Perl fixture with zero mentions of the
/// package, which would have forced a meaningless inventory row.
///
/// Parsing is only needed for the forms the token scan cannot see, and any such
/// import must open a brace directly after one of two crate names, so the
/// substring prefilter is a sound narrowing rather than a second guess at the
/// answer. It matters: parsing every Rust file under the scan roots took this
/// suite from ~1.2s to ~40s, a cost the whole repository's
/// `cargo test -p xtask --lib` lane would have paid.
pub fn reaches_audited_package(text: &str, relative_path: &str) -> bool {
    // This module's own two files name the package in every token form because
    // describing it is their whole job, so the token scan cannot classify them.
    // The caller used to skip them entirely, which made those two the one place
    // in the repository a real consumer could hide: an actual
    // `use perl_ast_v2::Node;` here would never enter the denominator and #8845
    // would migrate without it.
    //
    // They are classified by the parser branch instead, which reads *declared
    // paths* rather than text, so the strings, fixtures and prose that name the
    // package do not register while a genuine import does. The decision lives
    // here rather than at the walk so that one function owns "does this file
    // reach the package" for every file, and a control can exercise it directly
    // instead of testing a helper the walk might not even call.
    if INSTRUMENT_SELF_FILES.contains(&relative_path) {
        return parsed_api_use(text).unwrap_or(false);
    }
    if mentions_audited_package(text) {
        return true;
    }
    let worth_parsing = relative_path.ends_with(".rs")
        && (text.contains("perl_ast::{") || text.contains("perl_parser_core::{"));
    worth_parsing && parsed_api_use(text).unwrap_or(false)
}

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
                // The same union the ordinary file branch uses. Checking only
                // the token scan here left the link branch weaker than the path
                // beside it: a target reaching the package through a grouped
                // `use perl_ast::{v2, Node};` names none of the four tokens, so
                // the link was called harmless and then skipped as a non-file —
                // the exact hole this guard exists to close.
                match std::fs::read_to_string(path) {
                    Ok(text) if reaches_audited_package(&text, &relative) => bail!(
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
            // Only invalid UTF-8 is skipped. A non-UTF-8 file under these roots
            // cannot declare a Rust dependency or import, so skipping it loses
            // nothing. Every other read failure — a permission denial, a file
            // that vanished mid-walk, a transient I/O error — is a file whose
            // contents are *unknown*, and treating unknown as "does not
            // reference the package" is exactly the silent fail-open this
            // denominator exists to prevent. Those propagate.
            let text = match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(err) if err.kind() == std::io::ErrorKind::InvalidData => continue,
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!(
                            "failed to read `{relative}` while scanning for consumers of the \
                             audited package; the denominator cannot skip a file it could not \
                             read"
                        )
                    });
                }
            };
            if reaches_audited_package(&text, &relative) {
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

    // The issue's threshold, executable. A non-empty list was not enough: any
    // row could be made to authorize `retain` merely by being placed in the
    // array, including `ev:registry-publication`, which the ruling's own text
    // calls below the threshold. Qualification is a property of the evidence,
    // and only the classes the reversal condition actually names can carry it.
    //
    // That set is the three reversal clauses, not one of them. Restricting it
    // to `reverse_dependency` made the executable law narrower than the ruling
    // it enforces: an observed divergence in release cadence, or a public
    // proposition reachable only under the package's own path, is a stated
    // ground for `retain` that no evidence row could then express.
    for row in &m.external_evidence {
        if row.meets_independent_lifecycle_threshold
            && !QUALIFYING_EVIDENCE_CLASSES.contains(&row.class.as_str())
        {
            bail!(
                "external evidence {} claims to meet the independent-lifecycle threshold, but its \
                 class is `{}`. Package existence, publication, download volume and an \
                 unavailable instrument are all below that threshold; only \
                 {QUALIFYING_EVIDENCE_CLASSES:?} can carry it.",
                row.evidence_id,
                row.class
            );
        }
    }

    let qualifying: BTreeSet<&str> = m
        .external_evidence
        .iter()
        .filter(|row| row.meets_independent_lifecycle_threshold)
        .map(|row| row.evidence_id.as_str())
        .collect();

    for evidence_id in &m.ruling.independent_lifecycle_evidence_ids {
        if !qualifying.contains(evidence_id.as_str()) {
            bail!(
                "the ruling names {evidence_id} as independent-lifecycle evidence, but that row \
                 does not meet the threshold. Placing a row in this list does not make it qualify."
            );
        }
    }

    match m.ruling.ruling.as_str() {
        "retain" if m.ruling.independent_lifecycle_evidence_ids.is_empty() => bail!(
            "a `retain` ruling must name independent-lifecycle evidence. Package existence, \
             publish allowlisting or docs metadata alone do not meet the threshold."
        ),
        // The consistency rule in the other direction: if qualifying evidence
        // existed, `absorb` would be contradicting the evidence it carries.
        "absorb" if !qualifying.is_empty() => bail!(
            "the ruling is `absorb`, but {:?} meet(s) the independent-lifecycle threshold. An \
             absorb ruling cannot stand while its own evidence says the package earns an \
             independent lifecycle.",
            qualifying
        ),
        _ => {}
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

    // Non-emptiness was not the property that matters. A revision could replace
    // all three successors with one unrelated row and stay green, and
    // `wake_event` would then answer `None` for #8844, #8845 and #8847 —
    // silently removing the migration and compatibility gates this ruling
    // exists to set. The three the ruling actually binds must each be present.
    //
    // Presence, not exact equality: a later revision may legitimately bind a
    // fourth successor, and a check that forbade that would have to be edited
    // to permit ordinary work. Requiring these three refuses the substitution
    // without freezing the set.
    for required in REQUIRED_SUCCESSORS {
        if !m.successor_wake_conditions.iter().any(|row| row.successor_issue == required) {
            bail!(
                "the ruling binds successors {REQUIRED_SUCCESSORS:?}, but #{required} has no wake \
                 condition; that successor would not know when it may start"
            );
        }
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

    // One scan feeds both the consumer denominator and the re-export
    // derivation. They ask different questions of the same file set, and
    // walking the roots twice would double the cost of the cheapest check in
    // the suite for no additional evidence.
    let scanned = derive_reference_files(repo_root)?;

    reconcile_public_items(m, repo_root)?;
    reconcile_parity_counterparts(m, repo_root)?;
    reconcile_consumers(m, repo_root, &scanned)?;
    reconcile_reexport_sites(m, repo_root)?;
    reconcile_derived_reexports(m, repo_root, &scanned)?;
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

/// Whether a consumer row's declared role contradicts what its own path — and,
/// for `public_reexport`, its content — already settle.
///
/// The role was a closed vocabulary and nothing more, so the four gating roles
/// were interchangeable: a `package_dependency` relabelled
/// `production_implementation` reconciled green while #8845's migration
/// breakdown described the wrong work.
///
/// Only what the source actually settles is asserted here. A crate manifest is
/// a manifest; an integration-test target is not production code; a
/// `public_reexport` must really publish one.
///
/// Deliberately **not** asserted: that a `test_fixture` must live under
/// `tests/`. A `#[cfg(test)]` module inside `src/` is a real and legitimate
/// shape for that role, and the path alone cannot tell such a file from a
/// production one. The reverse direction *is* certain — a file under `tests/`
/// is not production — so that is the direction taken. Guessing the other way
/// would fire on other people's PRs, which is the failure mode this instrument
/// is most obliged to avoid.
fn role_contradiction(role: &str, file: &str, publishes_reexport: Option<bool>) -> Option<String> {
    let is_manifest = file == "Cargo.toml" || file.ends_with("/Cargo.toml");
    let is_rust = file.ends_with(".rs");
    let is_test_target = file.contains("/tests/") || file.contains("/benches/");
    let rust_role =
        matches!(role, "production_implementation" | "test_fixture" | "public_reexport");

    if role == "package_dependency" && !is_manifest {
        return Some(format!(
            "role `package_dependency` describes a crate manifest, but `{file}` is not a \
             `Cargo.toml`"
        ));
    }
    if rust_role && is_manifest {
        return Some(format!(
            "`{file}` is a crate manifest, so its role can only be `package_dependency`, not \
             `{role}`"
        ));
    }
    if rust_role && !is_rust {
        return Some(format!(
            "role `{role}` describes Rust source, but `{file}` is not a `.rs` file"
        ));
    }
    if role == "production_implementation" && is_test_target {
        return Some(format!(
            "`{file}` is an integration-test target, so its role is not \
             `production_implementation`"
        ));
    }
    if role == "public_reexport" && publishes_reexport == Some(false) {
        return Some(format!(
            "role `public_reexport` claims `{file}` publishes a public path to the package, but \
             the re-export derivation finds none there"
        ));
    }
    None
}

fn reconcile_consumers(m: &Manifest, repo_root: &Path, scanned: &BTreeSet<String>) -> Result<()> {
    let all_files: BTreeSet<&str> = m.consumers.iter().map(|row| row.file.as_str()).collect();

    // Direction one: nothing may reference the package without being inventoried.
    // The row's *role* then decides what the reference means — classification is
    // the audit's job, and excluding a file from the scan to avoid classifying it
    // would be the audit lying to itself.
    for file in scanned {
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

        // The role is what #8845 reads to size each part of the migration, so a
        // role the source contradicts is a wrong instruction to a successor, not
        // a cosmetic mislabel.
        //
        // Deliberately *after* the scan and existence checks. Those answer a
        // prior question — whether the row describes anything at all — and a row
        // that fails them should say so in their terms. Checking the role first
        // preempted `a_gating_row_naming_a_file_that_does_not_reference_the_package_is_rejected`,
        // which promotes a prose row to a production role: it must still be
        // rejected for the scan finding no reference, not merely for the file
        // not ending in `.rs`.
        let publishes_reexport = if row.role == "public_reexport" {
            match std::fs::read_to_string(repo_root.join(&row.file)) {
                Ok(text) => Some(!derive_public_reexports(&text).is_empty()),
                // Unreadable is not evidence of absence; the checks above own
                // that case and report it in their own terms.
                Err(_) => None,
            }
        } else {
            None
        };
        if let Some(contradiction) = role_contradiction(&row.role, &row.file, publishes_reexport) {
            bail!("consumer {}: {contradiction}", row.consumer_id);
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

        // A row's symbols are the migration set #8845 inherits, and nothing
        // checked them: the loader required only that the list be non-empty, so
        // a stale name left behind by a refactor, or one that was never in the
        // file, read exactly like a real one.
        //
        // Scoped to Rust gating rows on purpose. Those symbols are identifiers,
        // so requiring each to appear as a whole word in the file is a real
        // check the instrument can make. A `package_dependency` row names TOML
        // table paths and a `policy_inventory` row names glob and cohort
        // entries; those are descriptions of where the mention lives, not
        // identifiers, and demanding them verbatim would reject honest rows.
        // The boundary is stated rather than stretched: this proves the named
        // identifier is present, not that the consumer depends on its meaning.
        if gating
            && row.file.ends_with(".rs")
            && let Ok(text) = std::fs::read_to_string(repo_root.join(&row.file))
        {
            // Identifiers from the parse when the file parses, text otherwise.
            // A text scan let a name that only survives in a comment keep a
            // stale row alive — the exact case this check exists to reject. A
            // gating Rust consumer that does not parse falls back to the text
            // scan rather than being failed, since a parse failure says nothing
            // about the row's honesty.
            let identifiers = source_identifiers(&text);
            for symbol in &row.symbols {
                for token in symbol.split("::").flat_map(str::split_whitespace) {
                    // `as` is the `use ... as ...` keyword joining two real
                    // names, not a name of its own.
                    if token.is_empty() || token == "as" {
                        continue;
                    }
                    let named = match &identifiers {
                        Some(idents) => idents.contains(token),
                        None => contains_token(&text, token),
                    };
                    if !named {
                        bail!(
                            "consumer {} lists symbol `{symbol}` for `{}`, but the code there does \
                             not name `{token}`. A symbol list that outlives the code it names is \
                             the migration set #8845 would inherit.",
                            row.consumer_id,
                            row.file
                        );
                    }
                }
            }
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

/// Derive every public re-export of the audited package from one Rust file, as
/// `(alias, rendered target)` pairs.
///
/// `reconcile_reexport_sites` alone only proved that authored rows still point
/// at a live line. It never asked the opposite question, so a second
/// `pub use perl_ast_v2 as other;` added to an already-inventoried file changed
/// no checked set and passed silently.
pub fn derive_public_reexports(text: &str) -> Vec<(String, String)> {
    let Ok(file) = syn::parse_file(text) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    collect_public_reexports(&file.items, "", &mut found);
    found
}

/// Walk one item list for public re-exports of the audited package, descending
/// into public inline modules and carrying the module namespace.
///
/// The descent matters because a public path is a public path: `pub mod compat {
/// pub use perl_ast_v2 as ast_v2; }` publishes the package exactly as a
/// top-level `pub use` does, and a top-level-only walk would have called that
/// file re-export-free. A private inline module is skipped, because nothing
/// outside the crate can name what it re-exports.
///
/// The namespace is carried rather than discarded because the alias alone is
/// not the path. Two public modules in one file can both export `ast_v2`, and
/// dropping their module names collapsed those into one indistinguishable
/// binding, so a single row covered both — leaving a real compatibility path
/// with no obligation recorded against it, exactly what these rows exist to
/// hold.
fn collect_public_reexports(
    items: &[syn::Item],
    namespace: &str,
    found: &mut Vec<(String, String)>,
) {
    for item in items {
        match item {
            syn::Item::Use(item_use) => {
                if !is_public(&item_use.vis) {
                    continue;
                }
                let mut paths = Vec::new();
                flatten_use_tree(&item_use.tree, "", &mut paths);
                let mut aliases = Vec::new();
                collect_use_aliases(&item_use.tree, "", &mut aliases);
                for (alias, rendered) in aliases.into_iter().zip(paths) {
                    if API_USE_FORM.is_match(&format!("{rendered}::")) {
                        let qualified = if namespace.is_empty() {
                            alias
                        } else {
                            format!("{namespace}::{alias}")
                        };
                        found.push((qualified, rendered));
                    }
                }
            }
            // `pub extern crate perl_ast_v2 as ast_v2;` publishes the package
            // under that alias to every downstream consumer, exactly as a
            // `pub use` does. It is a different item kind, so it fell through
            // the catch-all and published a path with no row behind it.
            syn::Item::ExternCrate(item) if is_public(&item.vis) => {
                let rendered = item.ident.to_string();
                if API_USE_FORM.is_match(&format!("{rendered}::")) {
                    let alias = item
                        .rename
                        .as_ref()
                        .map_or_else(|| rendered.clone(), |(_, to)| to.to_string());
                    let qualified =
                        if namespace.is_empty() { alias } else { format!("{namespace}::{alias}") };
                    found.push((qualified, rendered));
                }
            }
            syn::Item::Mod(item_mod) if is_public(&item_mod.vis) => {
                if let Some((_, inner)) = &item_mod.content {
                    let inner_namespace = if namespace.is_empty() {
                        item_mod.ident.to_string()
                    } else {
                        format!("{namespace}::{}", item_mod.ident)
                    };
                    collect_public_reexports(inner, &inner_namespace, found);
                }
            }
            _ => {}
        }
    }
}

/// The full public paths one authored re-export row claims to publish.
///
/// The row's `path` is the path a consumer writes, so a grouped
/// `perl_parser_core::{DiagnosticId, MissingKind}` claims two of them. Returning
/// whole paths rather than bare leaves is what keeps two same-named bindings in
/// one file distinguishable: `a::ast_v2` and `b::ast_v2` are different
/// compatibility obligations, and a leaf-only comparison collapsed them.
fn row_reexport_paths(row: &ReexportRow) -> Result<BTreeSet<String>> {
    let path = row.path.trim();
    let aliases: BTreeSet<String> = if let Some(open) = path.find('{') {
        let Some(close) = path.rfind('}') else {
            bail!(
                "re-export {} declares path `{}`, which opens a group it never closes",
                row.reexport_id,
                row.path
            );
        };
        if close < open {
            bail!(
                "re-export {} declares path `{}`, whose group braces are inverted",
                row.reexport_id,
                row.path
            );
        }
        let prefix = path[..open].trim().trim_end_matches("::");
        path[open + 1..close]
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(
                |name| {
                    if prefix.is_empty() { name.to_string() } else { format!("{prefix}::{name}") }
                },
            )
            .collect()
    } else {
        std::iter::once(path).filter(|path| !path.is_empty()).map(str::to_string).collect()
    };

    if aliases.is_empty() {
        bail!(
            "re-export {} declares path `{}`, from which no public name can be read",
            row.reexport_id,
            row.path
        );
    }
    Ok(aliases)
}

/// Reconcile the authored re-export inventory against what the sources actually
/// publish, in both directions.
///
/// Kept pure over an in-memory `path -> text` map so the two directions can be
/// falsified directly, without a fixture repository the read-only proof
/// contract would not allow this module's tests to build.
fn reconcile_reexport_inventory(
    rows: &[ReexportRow],
    sources: &BTreeMap<String, String>,
) -> Result<()> {
    let mut derived_by_file: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    for (file, text) in sources {
        if !file.ends_with(".rs") {
            continue;
        }
        derived_by_file.insert(file.as_str(), derive_public_reexports(text));
    }

    let mut claimed_by_file: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in rows {
        let (file, _) = split_site(&row.site, &row.reexport_id)?;
        claimed_by_file.entry(file).or_default().extend(row_reexport_paths(row)?);
    }

    // Whether one claimed row path is the public path of one derived binding in
    // one file, compared as a whole path.
    //
    // The row records the path from the crate root
    // (`perl_parser::compat::ast_v2`) while the derivation sees only inside one
    // file, rendering `ast_v2` at file top level and `compat::ast_v2` inside
    // `pub mod compat`. `module_path_of` supplies the missing middle from the
    // file's own location, so the two can be compared exactly.
    //
    // Suffix matching was tried twice and was wrong twice, in widening ways: a
    // bare `::`-segment suffix accepted any crate at all, and anchoring only the
    // crate root still accepted any module within it — `c::b::ast_v2` satisfied
    // by a binding in `crates/c/src/a.rs`. Either way the inventory could attach
    // a compatibility obligation to a path nobody can write.
    let binds = |file: &str, path: &str, binding: &str| -> bool {
        module_path_of(file).is_some_and(|module| path == format!("{module}::{binding}"))
    };
    let claims = |file: &str, claimed: &BTreeSet<String>, binding: &str| -> bool {
        claimed.iter().any(|path| binds(file, path, binding))
    };

    // Every public path the inventory knows about, for chaining forwarding
    // re-exports back to a direct one.
    // What each inventoried public path forwards to, so a chain can be walked
    // rather than merely stepped once. A row's target is the rendered `use`
    // path of the derived binding at its own site.
    let mut target_of: BTreeMap<String, String> = BTreeMap::new();
    for (file, derived) in &derived_by_file {
        let Some(claimed) = claimed_by_file.get(*file) else {
            continue;
        };
        for (binding, rendered) in derived {
            for path in claimed {
                if binds(file, path, binding) {
                    target_of.insert(path.clone(), rendered.clone());
                }
            }
        }
    }

    // Direction one: nothing may publish the package publicly without a row.
    // The earlier form only looked inside files a row already named, so a first
    // public re-export in any other file — a new crate forwarding the package,
    // or a compatibility shim added during absorption — moved no checked set.
    for (file, derived) in &derived_by_file {
        for (binding, rendered) in derived {
            let covered =
                claimed_by_file.get(*file).is_some_and(|claimed| claims(file, claimed, binding));
            if !covered {
                bail!(
                    "`{file}` publicly re-exports the audited package as `{binding}` (from \
                     `{rendered}`) with no matching re-export row. A new public path to the \
                     package must move the compatibility inventory."
                );
            }
            // Half of these rows forward through a local path rather than
            // naming the package: `pub use engine::ast_v2;`. The pattern that
            // recognizes them cannot tell that from `pub use unrelated::ast_v2;`
            // — a same-named module of some other package — because telling
            // them apart needs name resolution, which this instrument does not
            // do and this issue's ceiling does not authorize.
            //
            // What it can require is that a forwarding target terminate in the
            // inventory rather than anywhere: swapping one to an unrelated
            // `ast_v2` then names a path no row describes, and fails. The
            // residual is recorded rather than implied away — this chains rows
            // to rows, it does not prove the far end resolves to the package.
            if !names_package_directly(rendered) {
                resolve_forwarding(rendered, file, binding, &target_of)?;
            }
        }
    }

    // Direction two: a row claims a live public path, and the compatibility
    // obligations attached to these rows are the whole point of the ruling. A
    // `pub use` that became `pub(crate)`, was renamed, or was deleted still
    // leaves `reconcile_reexport_sites` green whenever the line it names merely
    // still mentions the package, so the inventory could outlive the path.
    for row in rows {
        let (file, _) = split_site(&row.site, &row.reexport_id)?;
        let Some(derived) = derived_by_file.get(file.as_str()) else {
            bail!(
                "re-export {} names site {}, whose source is not available to the re-export \
                 derivation",
                row.reexport_id,
                row.site
            );
        };
        for path in row_reexport_paths(row)? {
            let live = derived.iter().any(|(binding, _)| binds(&file, &path, binding));
            if !live {
                bail!(
                    "re-export {} claims `{file}` publishes the audited package at `{path}`, but \
                     no public re-export there binds that path any more. A path that became \
                     private, was renamed, or was removed is a compatibility change and must move \
                     the inventory. The path must also start at the crate that owns the site — \
                     `{}` here.",
                    row.reexport_id,
                    crate_root_of(&file).unwrap_or_else(|| "<not a crate source path>".to_string())
                );
            }
        }
    }

    Ok(())
}

/// Collect the name each leaf of a `use` tree binds, in the same order as
/// [`flatten_use_tree`] renders their paths.
fn collect_use_aliases(tree: &syn::UseTree, prefix: &str, out: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(node) => collect_use_aliases(&node.tree, &node.ident.to_string(), out),
        syn::UseTree::Name(node) => out.push(node.ident.to_string()),
        syn::UseTree::Rename(node) => out.push(node.rename.to_string()),
        syn::UseTree::Glob(_) => out.push(format!("{prefix}::*")),
        syn::UseTree::Group(node) => {
            for item in &node.items {
                collect_use_aliases(item, prefix, out);
            }
        }
    }
}

/// Every public re-export the sources expose must have a row, and every row must
/// still describe a live one.
///
/// The candidate file set is the whole reference scan, not the files the rows
/// already name. Restricting it to inventoried files made the check circular:
/// it could only ever find a re-export next to one already recorded.
fn reconcile_derived_reexports(
    m: &Manifest,
    repo_root: &Path,
    scanned: &BTreeSet<String>,
) -> Result<()> {
    let mut sources: BTreeMap<String, String> = BTreeMap::new();
    for file in scanned {
        if !file.ends_with(".rs") {
            continue;
        }
        // Read failures here are not silent losses: the scan already read this
        // path successfully, and a row whose own file cannot be read is caught
        // below by its absence from the map.
        if let Ok(text) = std::fs::read_to_string(repo_root.join(file)) {
            sources.insert(file.clone(), text);
        }
    }
    // A row's file that no longer reaches the package at all is not in the scan,
    // and it is exactly the case direction two has to report.
    for row in &m.reexport_paths {
        let (file, _) = split_site(&row.site, &row.reexport_id)?;
        if sources.contains_key(&file) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(repo_root.join(&file)) {
            sources.insert(file, text);
        }
    }

    reconcile_reexport_inventory(&m.reexport_paths, &sources)
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
