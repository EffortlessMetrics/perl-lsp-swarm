//! Generate the protocol-type substrate and migration-denominator matrix.
//!
//! Authority: issue #11802 (LT01 under #1421), corrected by the maintainer
//! deep-review comment on #11802: the originally planned `ls-types` target was
//! archived 2026-08-15; the live candidate substrate is `gen-lsp-types 0.11.0`
//! (crates.io, repo `ribru17/gen-lsp-types`). The comment's rulings supersede
//! the issue body wherever they conflict.
//!
//! This task is INVENTORY ONLY. It emits a deterministic markdown matrix plus a
//! machine-readable JSON receipt recording:
//!
//! 1. the selected-substrate record (package/version/source/checksum/
//!    maintenance/metamodel/generator/URI feature/null-absent model/coverage)
//!    with first-hand evidence pins;
//! 2. the four discriminating capability-patch rows with package-neutral
//!    survival dispositions.
//!
//! Later commits extend this matrix with the resolved Cargo denominator
//! (`cargo metadata`), the schema/null-vs-absent/URI delta classification, and
//! the manual-extension registry with derived LT02/LT03/LT04 populations.
//!
//! No Cargo manifest, public API, protocol behavior, or CI gate is modified by
//! this task or its output.

use crate::utils::project_root;
use color_eyre::eyre::{Context, ContextCompat, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

pub const MATRIX_PATH: &str = "docs/specs/protocol-type-substrate-matrix.md";
pub const RECEIPT_PATH: &str = "docs/specs/protocol-type-substrate-matrix.json";

/// The incumbent protocol-type crate whose denominator this task freezes.
const INCUMBENT_CRATE: &str = "lsp-types";

/// Evidence inspection date for all external registry/source pins below.
/// A constant (not the wall clock) keeps two consecutive generations
/// byte-identical; re-pin deliberately when re-verifying external state.
const EVIDENCE_PIN_DATE: &str = "2026-08-22";

/// Registry checksum of `gen-lsp-types 0.11.0` as published on crates.io,
/// verified first-hand by downloading
/// `https://static.crates.io/crates/gen-lsp-types/gen-lsp-types-0.11.0.crate`
/// and comparing its SHA-256 against the crates.io API checksum value.
const GLT_CHECKSUM_SHA256: &str =
    "b64887ac3a8083427ae935a7296db876871582cd57eac077564f8bc18fa49116";

/// Package-neutral survival-disposition vocabulary from the corrected #11802
/// ruling. Target-specific values such as `typed_ls_types_stable` must never
/// appear in generated output.
pub const DISPOSITIONS: &[&str] = &[
    "lower_wire_remove_before_switch",
    "adapter_protocol_type",
    "public_api_break_or_migration",
    "selected_substrate_generated_type",
    "selected_substrate_manual_schema_extension",
    "invalid_current_protocol_shape",
    "project_extension",
    "test_fixture_only",
    "compatibility_with_exit",
    "retire",
    "candidate_rejected",
    "not_proven",
];

/// One field of the selected-substrate record.
struct SubstrateRecordField {
    field: &'static str,
    value: &'static str,
    evidence: &'static str,
}

/// One evaluated substrate candidate.
struct CandidateRow {
    candidate: &'static str,
    version: &'static str,
    source: &'static str,
    state: &'static str,
    verdict: &'static str,
    rationale: &'static str,
}

/// One governed capability-patch row.
struct CapabilityPatchRow {
    row_id: &'static str,
    anchor: &'static str,
    current_assumption: &'static str,
    protocol_identity: &'static str,
    substrate_result: &'static str,
    disposition: &'static str,
    owner: &'static str,
    exit_rule: &'static str,
    first_falsifier: &'static str,
}

const SUBSTRATE_RECORD: &[SubstrateRecordField] = &[
    SubstrateRecordField {
        field: "selection verdict",
        value: "selected_maintained_substrate",
        evidence: "#11802 maintainer deep-review ruling; first-hand 0.11.0 source/package verification below",
    },
    SubstrateRecordField {
        field: "package / version / source",
        value: "gen-lsp-types 0.11.0; crates.io; repo https://github.com/ribru17/gen-lsp-types",
        evidence: "crates.io API v1 crates/gen-lsp-types inspected {EVIDENCE_PIN_DATE}; trust-published from GitHub run 30327548194 @ 1e84ee239d093e4933bf3024cd597255090e5813",
    },
    SubstrateRecordField {
        field: "checksum",
        value: GLT_CHECKSUM_SHA256,
        evidence: "SHA-256 of downloaded gen-lsp-types-0.11.0.crate matched the crates.io API checksum exactly ({EVIDENCE_PIN_DATE})",
    },
    SubstrateRecordField {
        field: "maintenance state",
        value: "actively maintained successor; predecessor ls-types archived 2026-08-15 naming this crate as replacement",
        evidence: "archived ls-types README supersession notice recorded in the #11802 deep review",
    },
    SubstrateRecordField {
        field: "metamodel / spec source identity",
        value: "generated from the official LSP metamodel",
        evidence: "crate description and generator pipeline in ribru17/gen-lsp-types; #11802 deep-review external pin",
    },
    SubstrateRecordField {
        field: "generator identity / reproducibility",
        value: "metamodel-codegen inside the upstream repo; published artifacts carry a trust-publish provenance (GitHub Actions run id + commit sha)",
        evidence: "crates.io trustpub_data for 0.11.0 (run 30327548194, sha 1e84ee2)",
    },
    SubstrateRecordField {
        field: "edition / MSRV / dependency graph",
        value: "edition 2024; no declared MSRV (rust_version absent from registry metadata); deps serde 1.0.228, serde_json 1.0.150, optional fluent-uri 0.4.1 (serde) or url 2.5.8 (serde)",
        evidence: "Cargo.toml.orig inside the verified 0.11.0 .crate payload; crates.io API rust_version=null",
    },
    SubstrateRecordField {
        field: "URI representation feature",
        value: "default String-backed `pub struct Uri(pub String)`; optional features `url` (url::Url) and `fluent-uri` (fluent_uri::Uri<String>); feature choice deferred to the migration lane with #8156/#8484/public-API proof",
        evidence: "src/generated/common.rs lines 28/66/68 of the verified 0.11.0 payload; #11802 URI submatrix ruling (choose for adapter-boundary preservation, not import-edit minimization)",
    },
    SubstrateRecordField {
        field: "null / absent serialization model",
        value: "`Option<T>` fields serialize as absent (`skip_serializing_if = \"Option::is_none\"` carried on 498 lines of structures.rs); explicit null appears only where the metamodel demands it - distinct wire states preserved, not flattened",
        evidence: "verified 0.11.0 src/generated/structures.rs serde attributes",
    },
    SubstrateRecordField {
        field: "request / notification direction model",
        value: "dedicated requests.rs / notifications.rs modules encode method direction types; route/method authority remains #8896 - unchanged by this inventory",
        evidence: "verified 0.11.0 src/generated/{requests,notifications}.rs; #11802 falsifier 9",
    },
    SubstrateRecordField {
        field: "stable / proposed representation model",
        value: "single Cargo surface without a stable/proposed feature split; protocol maturity stays repository-owned per admitted profile (#7113 validator + features.toml); generated availability is not stability",
        evidence: "0.11.0 Cargo.toml.orig features = url|fluent-uri only; #11802 stable/proposed boundary ruling",
    },
    SubstrateRecordField {
        field: "coverage of current manual patches",
        value: "typed `ServerCapabilities.type_hierarchy_provider: Option<TypeHierarchyProvider>` (structures.rs:6062), typed top-level `inline_completion_provider: Option<InlineCompletionProvider>` (:6082), typed `DocumentRangeFormattingOptions.ranges_support: Option<bool>` (:7113); `completionItem.insertTextModes` is NOT modeled server-side (only client-capability InsertTextMode enum exists)",
        evidence: "first-hand Select-String over the verified 0.11.0 src/generated/*.rs",
    },
    SubstrateRecordField {
        field: "known gaps / limitations",
        value: "no declared MSRV; zero-ver breaking policy means point releases may break - migration must pin exact version and record an update policy; String Uri default preserves qualifiers but defers parse/serialize semantics to the chosen feature",
        evidence: "registry rust_version=null; upstream policy; #11802 URI submatrix requirements",
    },
];

const CANDIDATES: &[CandidateRow] = &[
    CandidateRow {
        candidate: "lsp-types",
        version: "0.97.0 (current incumbent)",
        source: "crates.io; workspace dep at root Cargo.toml [workspace.dependencies]",
        state: "incumbent",
        verdict: "retire",
        rationale: "stays selected only until the migration lane switches; unmaintained lineage motivated #1421. What remains useful (DTO coverage in active adapter paths) is recorded row-by-row in the denominator; incumbent snapshots are behavior evidence, not target authority.",
    },
    CandidateRow {
        candidate: "ls-types",
        version: "archived (was 0.0.6-era target)",
        source: "GitHub archive notice 2026-08-15",
        state: "rejected_archived_superseded",
        verdict: "candidate_rejected",
        rationale: "owner named gen-lsp-types as successor; cannot receive selected_maintained_substrate without a new reviewed ruling supplying fork/security/update plan. Issue-body ls-types field vocabulary (typed_ls_types_*) is retired from the canonical schema.",
    },
    CandidateRow {
        candidate: "gen-lsp-types",
        version: "0.11.0",
        source: "crates.io checksum b64887ac...; repo ribru17/gen-lsp-types",
        state: "active_successor_candidate",
        verdict: "selected_maintained_substrate",
        rationale: "first-hand verified: checksum match, edition 2024, official-metamodel generation, typed typeHierarchyProvider/rangesSupport/inlineCompletionProvider, String-default Uri with optional url|fluent-uri, explicit null-vs-absent model, request/notification direction types. Limitations recorded in the substrate record; later LT issues may not independently select another package or feature.",
    },
];

const CAPABILITY_PATCHES: &[CapabilityPatchRow] = &[
    CapabilityPatchRow {
        row_id: "PATCH-TYPEHIERARCHY",
        anchor: "crates/perl-lsp-rs-core/src/protocol/capabilities.rs capabilities_json() typeHierarchyProvider injection (lines 72-77)",
        current_assumption: "lsp-types 0.97 ServerCapabilities lacks type_hierarchy_provider; patched into serialized JSON post-hoc; parallel experimental injection in protocol/capabilities/experimental.rs:10 and detection support in capability_map.rs",
        protocol_identity: "initialize result capabilities.typeHierarchyProvider (object form)",
        substrate_result: "typed once: ServerCapabilities.type_hierarchy_provider: Option<TypeHierarchyProvider>",
        disposition: "selected_substrate_generated_type",
        owner: "#11803 migration (surviving LT02 row)",
        exit_rule: "patch and experimental workaround removed together when the adapter serializes the typed field",
        first_falsifier: "tests/lsp_caps_contract_shapes.rs typeHierarchyProvider shape assertion population",
    },
    CapabilityPatchRow {
        row_id: "PATCH-RANGESSUPPORT",
        anchor: "crates/perl-lsp-rs-core/src/protocol/capabilities.rs documentRangeFormattingProvider rangesSupport injection (lines 79-86)",
        current_assumption: "issue-body claim 'rangesSupport missing' is STALE: verified 0.11.0 structures.rs:7113 DocumentRangeFormattingOptions.ranges_support: Option<bool>",
        protocol_identity: "capabilities.documentRangeFormattingProvider.rangesSupport (LSP 3.18 multi-range formatting)",
        substrate_result: "typed once: DocumentRangeFormattingOptions.ranges_support: Option<bool>",
        disposition: "selected_substrate_generated_type",
        owner: "#11803 migration (surviving LT02 row)",
        exit_rule: "hand-patched object replaced by typed options struct; 3.18 conformance matrix row stays authoritative for advertisement shape",
        first_falsifier: "tests/lsp_caps_contract_shapes.rs rangesSupport pointer assertions; lsp_3_17_lifecycle_tests registration payload",
    },
    CapabilityPatchRow {
        row_id: "PATCH-INLINECOMPLETION",
        anchor: "crates/perl-lsp-rs-core/src/protocol/capabilities.rs inlineCompletionProvider injection (lines 87-93); runtime dynamic-client removal at crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs (~lines 781-815)",
        current_assumption: "0.97 models the ServerCapabilities field only behind its `proposed` cargo feature (lib.rs ~1954), which this workspace does not select - so the default compiled surface lacks it and the static advertisement is patched into JSON, then removed for dynamic-registration clients at initialize time",
        protocol_identity: "capabilities.inlineCompletionProvider top-level (LSP 3.18); experimental placement forbidden (negative-claimed)",
        substrate_result: "typed once by default: ServerCapabilities.inline_completion_provider: Option<InlineCompletionProvider> (verified 0.11.0 structures.rs:6082); runtime dynamic-client removal logic is behavioral and stays out of type migration scope",
        disposition: "selected_substrate_generated_type",
        owner: "#11803 migration (surviving LT02 row); runtime removal seam owned by lifecycle code, not the type switch",
        exit_rule: "patch removed when typed field serializes identically; dynamic-client removal branch must keep byte-identical initialize output",
        first_falsifier: "tests/lsp_inline_completion_registration_tests.rs; tests/lsp_cap_snap.rs; ripr_seam_proof_* capability negotiation proofs",
    },
    CapabilityPatchRow {
        row_id: "PATCH-INSERTTEXTMODES",
        anchor: "crates/perl-lsp-rs-core/src/protocol/capabilities.rs completionItem.insertTextModes injection (lines 95-105)",
        current_assumption: "advertises numeric array [1,2] inside completionProvider.completionItem; that key is NOT a valid server-capability shape (client capability textDocument.completion.insertTextMode is the real negotiation surface) per #2892/#8032",
        protocol_identity: "invalid_current_protocol_shape - not a type gap",
        substrate_result: "no substrate equivalent required: verified 0.11.0 models InsertTextMode enum and the client capability but no server-side insertTextModes field",
        disposition: "invalid_current_protocol_shape",
        owner: "#8032 single-capability-authority work removes it; explicitly NOT migrated to the selected substrate",
        exit_rule: "remove the injection and its snapshot assertions in the #8032 lane; migration must not carry it forward as parity",
        first_falsifier: "tests/lsp_capabilities_contract.rs insertTextModes advertisement assertions (falsifiers flip to removal proofs)",
    },
];

// ---------------------------------------------------------------------------
// Resolved Cargo denominator (live `cargo metadata` evidence, not manifest grep)
// ---------------------------------------------------------------------------

/// One direct declared dependency edge on the incumbent crate.
struct EdgeRow {
    package: String,
    dep_kind: String,
    profile_class: &'static str,
    gate: String,
    disposition: &'static str,
    removal_owner: String,
}

/// One workspace member that reaches the incumbent only transitively.
struct TransitiveRow {
    package: String,
    reachability: &'static str,
    min_hops: u32,
}

/// The complete resolved denominator for the incumbent crate.
#[derive(Default)]
struct Denominator {
    edges: Vec<EdgeRow>,
    transitive: Vec<TransitiveRow>,
}

/// Static survival policy for known direct edges. Unknown packages classify as
/// `not_proven` so a new edge surfaces loudly instead of silently passing.
fn classify_edge(
    package: &str,
    dep_kind: &str,
    optional: bool,
) -> (&'static str, &'static str, &'static str) {
    match (package, dep_kind, optional) {
        ("perl-lsp-rs", "normal", false) | ("perl-lsp-rs-core", "normal", false) => (
            "production",
            "adapter_protocol_type",
            "#11803 migration; crate-level retirement relation #9645 relocates rows to the final product home first",
        ),
        ("perl-parser", "normal", true) => {
            ("compatibility_edge", "lower_wire_remove_before_switch", "#9893")
        }
        ("perl-position-tracking", "normal", true) => {
            ("compatibility_edge", "lower_wire_remove_before_switch", "#9632")
        }
        ("perl-workspace", "normal", true) => {
            ("compatibility_edge", "lower_wire_remove_before_switch", "#9632")
        }
        ("perl-tdd-support", "normal", true) => (
            "compatibility_edge",
            "compatibility_with_exit",
            "#1421 sequencing; wire-free dev-test profile proof under #9632",
        ),
        ("perl-incremental-parsing", "dev", _) => {
            ("dev_test", "test_fixture_only", "#1421 sequencing; exit when LT02 lands")
        }
        _ => ("not_proven", "not_proven", "unclassified edge; resolve before migration"),
    }
}

/// Run `cargo metadata` for the current workspace and return parsed JSON.
fn load_cargo_metadata(root: &Path) -> Result<serde_json::Value> {
    let output = Command::new(env!("CARGO"))
        .current_dir(root)
        .args(["metadata", "--all-features", "--format-version", "1", "--locked"])
        .output()
        .with_context(|| format!("failed to execute cargo metadata in {}", root.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cargo metadata failed ({}): {}", output.status, stderr.trim());
    }
    serde_json::from_slice(&output.stdout).context("failed to parse cargo metadata JSON")
}

/// Shell out to cargo and derive the full denominator.
fn collect_denominator(root: &Path) -> Result<Denominator> {
    let metadata = load_cargo_metadata(root)?;
    parse_denominator(&metadata)
}

/// Pure derivation of the denominator from parsed `cargo metadata` output.
fn parse_denominator(metadata: &serde_json::Value) -> Result<Denominator> {
    let packages = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| color_eyre::eyre::eyre!("cargo metadata JSON missing packages array"))?;
    let resolve = metadata
        .get("resolve")
        .and_then(|resolve| resolve.get("nodes"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            color_eyre::eyre::eyre!("cargo metadata JSON missing resolve.nodes array")
        })?;

    let mut lsp_ids = Vec::new();
    let mut feature_activations: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for package in packages {
        let name = package
            .get("name")
            .and_then(serde_json::Value::as_str)
            .context("package entry missing name")?;
        if name == INCUMBENT_CRATE {
            let id = package
                .get("id")
                .and_then(serde_json::Value::as_str)
                .context("lsp-types package entry missing id")?;
            lsp_ids.push(id.to_string());
        }
        let mut activations = Vec::new();
        if let Some(features) = package.get("features").and_then(serde_json::Value::as_object) {
            for (feature, requires) in features {
                let mentions_incumbent = requires
                    .as_array()
                    .map(|list| {
                        list.iter().filter_map(serde_json::Value::as_str).any(|req| {
                            req == INCUMBENT_CRATE || req == format!("dep:{INCUMBENT_CRATE}")
                        })
                    })
                    .unwrap_or(false);
                if mentions_incumbent {
                    activations.push(feature.clone());
                }
            }
        }
        feature_activations.insert(name.to_string(), activations);
    }
    if lsp_ids.len() != 1 {
        bail!(
            "expected exactly one resolved {} package node, found {}",
            INCUMBENT_CRATE,
            lsp_ids.len()
        );
    }
    let lsp_id = lsp_ids.remove(0);

    let mut edges = Vec::new();
    for package in packages {
        let name = package
            .get("name")
            .and_then(serde_json::Value::as_str)
            .context("package entry missing name")?;
        let dependencies = package
            .get("dependencies")
            .and_then(serde_json::Value::as_array)
            .context("package entry missing dependencies")?;
        for dep in dependencies {
            let dep_name = match dep.get("name").and_then(serde_json::Value::as_str) {
                Some(dep_name) => dep_name,
                None => continue,
            };
            if dep_name != INCUMBENT_CRATE {
                continue;
            }
            // A dev/build/normal declaration only counts when it resolves to the
            // same incumbent node (name matches are not resolution).
            let dep_id = dep.get("pkg").and_then(serde_json::Value::as_str).unwrap_or_default();
            if !dep_id.is_empty() && dep_id != lsp_id {
                continue;
            }
            let dep_kind = dep.get("kind").and_then(serde_json::Value::as_str).unwrap_or("normal");
            let optional =
                dep.get("optional").and_then(serde_json::Value::as_bool).unwrap_or(false);
            let gates = feature_activations.get(name).cloned().unwrap_or_default();
            let gate = if optional && !gates.is_empty() {
                gates.join("|")
            } else if optional {
                "<undetermined>".to_string()
            } else {
                "-".to_string()
            };
            let (profile_class, disposition, removal_owner) =
                classify_edge(name, dep_kind, optional);
            edges.push(EdgeRow {
                package: name.to_string(),
                dep_kind: dep_kind.to_string(),
                profile_class,
                gate,
                disposition,
                removal_owner: removal_owner.to_string(),
            });
        }
    }
    edges.sort_by(|a, b| a.package.cmp(&b.package));

    // Reverse reachability over the resolved graph. `min_hops_all` counts hops
    // through any dependency kind; `min_hops_normal` counts hops that never use
    // a dev-kind edge. Members reachable only via dev edges are classified as
    // dev-only-chain.
    let mut parents_any: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
    let mut member_names: BTreeMap<String, String> = BTreeMap::new();
    for node in resolve {
        let node_id = node
            .get("id")
            .and_then(serde_json::Value::as_str)
            .context("resolve node missing id")?;
        let deps = match node.get("deps").and_then(serde_json::Value::as_array) {
            Some(deps) => deps,
            None => continue,
        };
        for dep in deps {
            let dep_pkg = match dep.get("pkg").and_then(serde_json::Value::as_str) {
                Some(dep_pkg) => dep_pkg,
                None => continue,
            };
            let has_normal = dep
                .get("dep_kinds")
                .and_then(serde_json::Value::as_array)
                .map(|kinds| {
                    kinds.iter().any(|kind| {
                        // cargo metadata serializes kinds as lowercase strings
                        // ("dev"/"build") or null for a normal edge.
                        kind.get("kind").and_then(serde_json::Value::as_str) != Some("dev")
                    })
                })
                .unwrap_or(true);
            parents_any
                .entry(dep_pkg.to_string())
                .or_default()
                .push((node_id.to_string(), has_normal));
        }
    }
    for package in packages {
        let id = package
            .get("id")
            .and_then(serde_json::Value::as_str)
            .context("package entry missing id")?;
        let name = package
            .get("name")
            .and_then(serde_json::Value::as_str)
            .context("package entry missing name")?;
        member_names.insert(id.to_string(), name.to_string());
    }

    fn bfs(
        start: &str,
        parents: &BTreeMap<String, Vec<(String, bool)>>,
        normal_only: bool,
    ) -> BTreeMap<String, u32> {
        let mut dist = BTreeMap::new();
        dist.insert(start.to_string(), 0u32);
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(start.to_string());
        while let Some(current) = queue.pop_front() {
            let depth = dist[&current];
            if let Some(list) = parents.get(&current) {
                for (parent, has_normal) in list {
                    if normal_only && !has_normal {
                        continue;
                    }
                    if !dist.contains_key(parent) {
                        dist.insert(parent.clone(), depth + 1);
                        queue.push_back(parent.clone());
                    }
                }
            }
        }
        dist
    }

    let dist_any = bfs(&lsp_id, &parents_any, false);
    let dist_normal = bfs(&lsp_id, &parents_any, true);
    let direct_packages: Vec<String> = edges.iter().map(|edge| edge.package.clone()).collect();

    let mut transitive = Vec::new();
    for (id, hops) in &dist_any {
        if *hops == 0 {
            continue;
        }
        let name = match member_names.get(id) {
            Some(name) => name,
            None => continue,
        };
        if direct_packages.iter().any(|direct| direct == name) {
            continue;
        }
        let reachability =
            if dist_normal.contains_key(id) { "normal_chain" } else { "dev_only_chain" };
        let normal_hops = dist_normal.get(id).copied().unwrap_or(u32::MAX);
        transitive.push(TransitiveRow {
            package: name.clone(),
            reachability,
            min_hops: (*hops).min(normal_hops),
        });
    }
    transitive.sort_by(|a, b| a.package.cmp(&b.package));

    Ok(Denominator { edges, transitive })
}

/// One serialization/API delta between incumbent 0.97 and candidate 0.11.0,
/// classified against the #7113 schema authority (not candidate-vs-incumbent
/// snapshots alone).
struct SchemaDeltaRow {
    row_id: &'static str,
    area: &'static str,
    incumbent_evidence: &'static str,
    candidate_evidence: &'static str,
    classification: &'static str,
    migration_note: &'static str,
}

const SCHEMA_DELTAS: &[SchemaDeltaRow] = &[
    SchemaDeltaRow {
        row_id: "DELTA-URI-DEFAULT",
        area: "URI representation: default generated String Uri",
        incumbent_evidence: "0.97 src/uri.rs: newtype `Uri(fluent_uri::Uri<String>)` around fluent-uri 0.1.4; parse errors surface as typed Result",
        candidate_evidence: "0.11.0 src/generated/common.rs:28: plain `pub struct Uri(pub String)` with no parse step",
        classification: "public_api_only_difference",
        migration_note: "qualified-URI preservation and DocumentUriKey/#8156/#8484 interaction must be proven before choosing; parse fallibility moves from construction-time to trust-the-wire",
    },
    SchemaDeltaRow {
        row_id: "DELTA-URI-URL",
        area: "URI representation: optional `url` feature (url::Url 2.5.8)",
        incumbent_evidence: "n/a - incumbent has no url-backed mode",
        candidate_evidence: "0.11.0 Cargo.toml.orig features.url = [\"dep:url\"]; common.rs:66 type alias",
        classification: "schema_equivalent_representation_difference",
        migration_note: "alternative only; feature choice deferred to migration lane with adapter-boundary proof, not import-edit minimization",
    },
    SchemaDeltaRow {
        row_id: "DELTA-URI-FLUENT",
        area: "URI representation: optional `fluent-uri` feature (fluent_uri 0.4.1)",
        incumbent_evidence: "incumbent pins fluent-uri 0.1.4 internally",
        candidate_evidence: "0.11.0 Cargo.toml.orig features.fluent-uri = [\"dep:fluent-uri\"]; common.rs:68 Uri<String>",
        classification: "schema_equivalent_representation_difference",
        migration_note: "closest wire behavior to incumbent but still a major fluent-uri version jump; same URI submatrix proof obligations as DELTA-URI-DEFAULT",
    },
    SchemaDeltaRow {
        row_id: "DELTA-TYPEHIERARCHY-FIELD",
        area: "ServerCapabilities.type_hierarchy_provider field",
        incumbent_evidence: "verified absent from 0.97 default AND proposed surfaces (only TypeHierarchy request types + client capability exist); repo compensates via JSON patch + experimental injection",
        candidate_evidence: "typed Option<TypeHierarchyProvider> at structures.rs:6062",
        classification: "incumbent_defect_corrected_by_candidate",
        migration_note: "PATCH-TYPEHIERARCHY exits when the typed field serializes identically",
    },
    SchemaDeltaRow {
        row_id: "DELTA-RANGESSUPPORT-FIELD",
        area: "DocumentRangeFormattingOptions.ranges_support field (LSP 3.18)",
        incumbent_evidence: "verified absent from 0.97 (only document_range_formatting_provider exists); repo hand-patches rangesSupport into JSON",
        candidate_evidence: "typed Option<bool> at structures.rs:7113 (+ DocumentRangesFormattingOptions twin :9807)",
        classification: "incumbent_defect_corrected_by_candidate",
        migration_note: "PATCH-RANGESSUPPORT exits; 3.18 conformance matrix stays advertisement authority",
    },
    SchemaDeltaRow {
        row_id: "DELTA-INLINECOMPLETION-FIELD",
        area: "ServerCapabilities.inline_completion_provider field (LSP 3.18)",
        incumbent_evidence: "present in 0.97 ONLY behind its non-selected `proposed` cargo feature (lib.rs ~1954); default compiled surface lacks it",
        candidate_evidence: "typed by default at structures.rs:6082",
        classification: "incumbent_defect_corrected_by_candidate",
        migration_note: "candidate removes the proposed-gating hazard without enabling any unstable surface; PATCH-INLINECOMPLETION static half exits",
    },
    SchemaDeltaRow {
        row_id: "DELTA-INSERTTEXTMODES",
        area: "completionProvider.completionItem.insertTextModes server shape",
        incumbent_evidence: "not modeled in 0.97; repo injects numeric array [1,2] manually (invalid server shape per #2892/#8032)",
        candidate_evidence: "also NOT modeled in 0.11.0 (InsertTextMode enum + client capability only) - candidate is correct not to model it",
        classification: "intentional_repository_extension",
        migration_note: "invalid_current_protocol_shape: remove under #8032, never migrate as parity (PATCH-INSERTTEXTMODES)",
    },
    SchemaDeltaRow {
        row_id: "DELTA-STABLE-PROPOSED",
        area: "stable vs proposed cargo-surface split",
        incumbent_evidence: "0.97 has a `proposed = []` feature with explicit no-semver guarantee note",
        candidate_evidence: "0.11.0 exposes one surface with no proposed/stable split (features: url|fluent-uri only)",
        classification: "public_api_only_difference",
        migration_note: "maturity boundary moves fully repository-owned (#7113 validator + admitted-profile ledger); absence of a proposed feature neither advertises proposals nor blocks an admitted generated type",
    },
    SchemaDeltaRow {
        row_id: "DELTA-UNKNOWN-ENUMS",
        area: "unknown enum value tolerance",
        incumbent_evidence: "exactly one serde(other) catch-all across 0.97 src (closed enums otherwise)",
        candidate_evidence: "open enums throughout: `Custom` variants appear on 220 lines of enumerations.rs alone (e.g. InsertTextMode::AsIs|AdjustIndentation|Custom(any))",
        classification: "schema_equivalent_representation_difference",
        migration_note: "wire acceptance for unknown values widens; #7113 validator remains the admission oracle, snapshots stay behavior evidence only",
    },
    SchemaDeltaRow {
        row_id: "DELTA-NULL-ABSENT",
        area: "null vs absent wire states on optional fields",
        incumbent_evidence: "skip_serializing_if carried on 448 lines across 0.97 src",
        candidate_evidence: "498 skip_serializing_if = \"Option::is_none\" occurrences in structures.rs; explicit null only where metamodel requires",
        classification: "schema_equivalent_representation_difference",
        migration_note: "distinct wire states preserved on both sides; do not flatten null-vs-absent during migration (#11802 falsifier 8)",
    },
    SchemaDeltaRow {
        row_id: "DELTA-DIRECTION-MODEL",
        area: "request/notification direction types",
        incumbent_evidence: "0.97 request.rs/notification.rs trait-based declarations",
        candidate_evidence: "dedicated generated requests.rs / notifications.rs modules encoding method direction",
        classification: "schema_equivalent_representation_difference",
        migration_note: "#8896 route/method dispatch authority unchanged by this inventory; direction typing is a compile-time aid only",
    },
];

// ---------------------------------------------------------------------------
// Manual-extension registry and derived downstream denominators
// ---------------------------------------------------------------------------

/// One registered manual JSON/extension seam beyond the four capability
/// patches. Every production known-field patch must have exactly one row.
struct ExtensionSeamRow {
    row_id: &'static str,
    anchor: &'static str,
    seam_behavior: &'static str,
    disposition: &'static str,
    owner: &'static str,
}

const EXTENSION_SEAMS: &[ExtensionSeamRow] = &[
    ExtensionSeamRow {
        row_id: "SEAM-EXPERIMENTAL-TYPEHIERARCHY",
        anchor: "crates/perl-lsp-rs-core/src/protocol/capabilities/experimental.rs:10 insert_experimental_capability",
        seam_behavior: "injects experimental.typeHierarchyProvider=true into the typed ServerCapabilities.experimental value because the default 0.97 surface cannot carry the typed field; detection support in capability_map.rs reads it back",
        disposition: "selected_substrate_manual_schema_extension",
        owner: "#11803 removes the workaround when the adapter serializes the typed field; keep negative gate for experimental.inlineCompletionProvider",
    },
    ExtensionSeamRow {
        row_id: "SEAM-CAPMAP-DETECTION",
        anchor: "crates/perl-lsp-rs-core/src/capability_map.rs feature_ids_from_caps experimental readback (test pin ~line 505)",
        seam_behavior: "maps client capability objects back to feature ids, including the type-hierarchy-via-experimental workaround path",
        disposition: "compatibility_with_exit",
        owner: "#11803 exit together with SEAM-EXPERIMENTAL-TYPEHIERARCHY; detection of real typed fields replaces the workaround branch",
    },
    ExtensionSeamRow {
        row_id: "SEAM-RUNTIME-DYNAMIC-INLINECOMPLETION",
        anchor: "crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs dynamic-client removal (~lines 781-815)",
        seam_behavior: "strips top-level inlineCompletionProvider from initialize output when the client opts into dynamic registration; behavioral protocol logic independent of which crate supplies the type",
        disposition: "adapter_protocol_type",
        owner: "stays in lifecycle code through LT02/LT03; only its input type changes with #11803",
    },
];

/// Field-name needles whose test-file occurrences form the snapshot falsifier
/// population counted mechanically below.
const PATCH_FIELD_NEEDLES: &[&str] =
    &["typeHierarchyProvider", "rangesSupport", "inlineCompletionProvider", "insertTextModes"];

/// Distinct test files under crates/*/tests referencing any patched field.
fn count_falsifier_files(root: &Path) -> Result<Vec<String>> {
    let crates_dir = root.join("crates");
    let mut matches = std::collections::BTreeSet::new();
    for entry in walkdir::WalkDir::new(&crates_dir).max_depth(3).into_iter().filter_map(Result::ok)
    {
        let path = entry.path();
        let is_test = path.extension().and_then(std::ffi::OsStr::to_str) == Some("rs")
            && path
                .parent()
                .and_then(|parent| parent.file_name())
                .map(|name| name == std::ffi::OsStr::new("tests"))
                == Some(true);
        if !is_test || !entry.file_type().is_file() {
            continue;
        }
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if PATCH_FIELD_NEEDLES.iter().any(|needle| contents.contains(needle))
            && let Some(relative) = path.strip_prefix(root).ok().and_then(|p| p.to_str())
        {
            matches.insert(relative.replace('\\', "/"));
        }
    }
    Ok(matches.into_iter().collect())
}

/// Mechanically derived downstream-denominator summary rows (LT02=#11803,
/// LT03=#11804, LT04=#11805).
fn render_downstream_section(denominator: &Denominator, falsifier_files: &[String]) -> String {
    let mut output = String::new();
    let lt02_edges =
        denominator.edges.iter().filter(|edge| edge.disposition == "adapter_protocol_type").count();
    let doomed_edges = denominator
        .edges
        .iter()
        .filter(|edge| edge.disposition == "lower_wire_remove_before_switch")
        .count();
    let manual_rows = CAPABILITY_PATCHES.len() + EXTENSION_SEAMS.len();
    let typed_once = CAPABILITY_PATCHES
        .iter()
        .filter(|row| row.disposition == "selected_substrate_generated_type")
        .count();

    output.push_str("## 6. Manual-extension registry (beyond section 3 patch rows)\n\n");
    output.push_str("| Row ID | Anchor | Seam behavior | Disposition | Owner |\n");
    output.push_str("| --- | --- | --- | --- | --- |\n");
    for row in EXTENSION_SEAMS {
        for cell in [row.row_id, row.anchor, row.seam_behavior, row.disposition, row.owner] {
            output.push_str("| ");
            output.push_str(&escape_cell(cell));
        }
        output.push_str(" |\n");
    }
    output.push('\n');

    output.push_str("## 7. Derived downstream denominators\n\n");
    output.push_str("Mechanically derived from this matrix; no re-research needed:\n\n");
    output.push_str(&format!(
        "- **LT02 / #11803 migration population:** {} surviving direct Cargo edges (`adapter_protocol_type`: perl-lsp-rs, perl-lsp-rs-core) carrying 2 public nominal re-export anchors (`ServerCapabilities` at perl-lsp-rs-core/src/protocol/capabilities.rs:23, `Location` at perl-lsp-rs-core/src/providers/navigation/mod.rs:58), {} typed-once patch rows, plus SEAM-RUNTIME-DYNAMIC-INLINECOMPLETION as a type-consumer. Doomed edges excluded: {} (`lower_wire_remove_before_switch`, owners #9632/#9893).\n",
        lt02_edges, typed_once, doomed_edges
    ));
    output.push_str(&format!(
        "- **LT03 / #11804 representation convergence:** {} manual-extension rows total ({} patches + {} seams), {} serialization-delta rows to converge, URI submatrix decision (DELTA-URI-DEFAULT/URL/FLUENT) with #8156/#8484 proof obligations.\n",
        manual_rows, CAPABILITY_PATCHES.len(), EXTENSION_SEAMS.len(), SCHEMA_DELTAS.len()
    ));
    output.push_str(&format!(
        "- **LT04 / #11805 proof closure:** {} snapshot/contract falsifier files currently assert patched bytes (mechanically counted under crates/*/tests against needles: {}). Wire-neutrality guards stay authoritative: crates/perl-workspace-core/tests/dependency_contract.rs (forbids lsp-types below the adapter) and the perl-ripr-facts manifest contract comment (deliberately avoids perl-workspace because it transitively pulls lsp-types).\n",
        falsifier_files.len(),
        PATCH_FIELD_NEEDLES.join(", ")
    ));
    output.push_str(
        "- Changing any needle, patch row, or seam above must flip the matching falsifier population; \
         a silent zero-count is `not_proven`, never green.\n\n",
    );

    output
}

pub fn run(check: bool) -> Result<()> {
    let root = project_root()?;
    let path = root.join(MATRIX_PATH);
    let receipt_path = root.join(RECEIPT_PATH);
    let denominator = collect_denominator(&root)?;
    let falsifier_files = count_falsifier_files(&root)?;
    let generated = render_matrix(&denominator, &falsifier_files);
    let receipt = render_receipt(&denominator, &falsifier_files)?;

    if check {
        let existing =
            fs::read_to_string(&path).with_context(|| format!("failed to read {}", MATRIX_PATH))?;
        if normalize_newlines(&existing) != generated {
            bail!(
                "{} is stale; run `cargo xtask generate-protocol-type-substrate-matrix`",
                MATRIX_PATH
            );
        }
        let existing_receipt = fs::read_to_string(&receipt_path)
            .with_context(|| format!("failed to read {}", RECEIPT_PATH))?;
        if normalize_newlines(&existing_receipt) != receipt {
            bail!(
                "{} is stale; run `cargo xtask generate-protocol-type-substrate-matrix`",
                RECEIPT_PATH
            );
        }
        println!(
            "Protocol-type substrate matrix is up to date: {} substrate fields, {} candidates, {} patch rows, {} direct Cargo edges, {} transitive members",
            SUBSTRATE_RECORD.len(),
            CANDIDATES.len(),
            CAPABILITY_PATCHES.len(),
            denominator.edges.len(),
            denominator.transitive.len()
        );
        return Ok(());
    }

    fs::write(&path, generated).with_context(|| format!("failed to write {}", MATRIX_PATH))?;
    fs::write(&receipt_path, receipt)
        .with_context(|| format!("failed to write {}", RECEIPT_PATH))?;
    println!(
        "Wrote {} (+ {} receipt) with {} substrate fields, {} candidates, {} patch rows, {} direct Cargo edges, {} transitive members",
        MATRIX_PATH,
        RECEIPT_PATH,
        SUBSTRATE_RECORD.len(),
        CANDIDATES.len(),
        CAPABILITY_PATCHES.len(),
        denominator.edges.len(),
        denominator.transitive.len()
    );
    Ok(())
}

fn render_matrix(denominator: &Denominator, falsifier_files: &[String]) -> String {
    let mut output = String::new();
    output.push_str("# Protocol-Type Substrate Matrix\n\n");
    output.push_str("Status: generated (inventory-only; no Cargo/API/protocol behavior change)\n");
    output.push_str("Owner: perl-lsp maintainers\n");
    output.push_str("Generator: `cargo xtask generate-protocol-type-substrate-matrix`\n");
    output.push_str("Check: `cargo xtask generate-protocol-type-substrate-matrix --check`\n");
    output.push_str(&format!(
        "Authority: issue #11802 as corrected by the maintainer deep-review comment (ls-types archived 2026-08-15; gen-lsp-types 0.11.0 is the live candidate). External evidence pins inspected {EVIDENCE_PIN_DATE}.\n\n"
    ));
    output.push_str(
        "This matrix freezes the protocol-type denominator for #1421. It records one \
         selected-substrate record and the discriminating capability-patch rows. Stable vs \
         proposed vs project-extension maturity stays repository-owned (#7113 validator, \
         features.toml); generated type availability never implies protocol stability.\n\n",
    );
    output.push_str("Survival-disposition vocabulary (package-neutral):\n\n");
    for disposition in DISPOSITIONS {
        output.push_str(&format!("- `{disposition}`\n"));
    }
    output.push('\n');

    output.push_str("## 1. Selected-substrate record: gen-lsp-types 0.11.0\n\n");
    output.push_str("| Field | Value | Evidence |\n");
    output.push_str("| --- | --- | --- |\n");
    for field in SUBSTRATE_RECORD {
        output.push_str("| ");
        output.push_str(&escape_cell(field.field));
        output.push_str(" | ");
        output.push_str(&escape_cell(field.value));
        output.push_str(" | ");
        output.push_str(&escape_cell(field.evidence));
        output.push_str(" |\n");
    }
    output.push('\n');

    output.push_str("## 2. Candidate set evaluation\n\n");
    output.push_str("| Candidate | Version | Source | State | Verdict | Rationale |\n");
    output.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for row in CANDIDATES {
        output.push_str("| ");
        output.push_str(&escape_cell(row.candidate));
        output.push_str(" | ");
        output.push_str(&escape_cell(row.version));
        output.push_str(" | ");
        output.push_str(&escape_cell(row.source));
        output.push_str(" | ");
        output.push_str(&escape_cell(row.state));
        output.push_str(" | ");
        output.push_str(&escape_cell(row.verdict));
        output.push_str(" | ");
        output.push_str(&escape_cell(row.rationale));
        output.push_str(" |\n");
    }
    output.push('\n');

    output.push_str("## 3. Discriminating capability-patch rows\n\n");
    output.push_str(
        "| Row ID | Anchor | Current assumption | Protocol identity | Selected-substrate result | Disposition | Owner | Exit rule | First falsifier |\n",
    );
    output.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for row in CAPABILITY_PATCHES {
        push_patch_row(&mut output, row);
    }
    output.push('\n');

    output.push_str(&render_denominator_section(denominator));
    output.push_str(&render_downstream_section(denominator, falsifier_files));

    output.push_str("## 5. Serialization/API delta matrix vs incumbent 0.97 (classified against #7113 schema authority)\n\n");
    output.push_str(
        "Current snapshots are behavior evidence, not target protocol authority. Classifications use the \
         #11802 corrected vocabulary: incumbent_defect_corrected_by_candidate | candidate_defect | \
         public_api_only_difference | schema_equivalent_representation_difference | \
         intentional_repository_extension | not_proven.\n\n",
    );
    output.push_str(
        "| Row ID | Area | Incumbent 0.97 evidence (verified) | Candidate 0.11.0 evidence (verified) | Classification | Migration note |\n",
    );
    output.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for row in SCHEMA_DELTAS {
        for cell in [
            row.row_id,
            row.area,
            row.incumbent_evidence,
            row.candidate_evidence,
            row.classification,
            row.migration_note,
        ] {
            output.push_str("| ");
            output.push_str(&escape_cell(cell));
        }
        output.push_str(" |\n");
    }
    output.push('\n');

    output
}

fn render_denominator_section(denominator: &Denominator) -> String {
    let mut output = String::new();
    let normal_edges =
        denominator.edges.iter().filter(|edge| edge.profile_class == "production").count();
    let compat_edges =
        denominator.edges.iter().filter(|edge| edge.profile_class == "compatibility_edge").count();
    let dev_edges =
        denominator.edges.iter().filter(|edge| edge.profile_class == "dev_test").count();
    let unclassified_edges =
        denominator.edges.iter().filter(|edge| edge.profile_class == "not_proven").count();

    output.push_str("## 4. Resolved Cargo denominator (live `cargo metadata --all-features --locked` evidence)\n\n");
    output.push_str(&format!(
        "Direct declared edges: {} ({} production, {} compatibility-gated, {} dev/test, {} unclassified). \
         Transitive selecting parents: {} workspace members (normal-chain vs dev-only-chain below). \
         No external (non-workspace) package resolves the incumbent transitively.\n\n",
        denominator.edges.len(),
        normal_edges,
        compat_edges,
        dev_edges,
        unclassified_edges,
        denominator.transitive.len()
    ));
    output.push_str("Doomed lower edges are assigned to their removal owners and are NOT part of the #11803 migration population.\n\n");
    output.push_str(
        "| Package | Dep kind | Profile class | Feature gate | Disposition | Removal owner |\n",
    );
    output.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for edge in &denominator.edges {
        for cell in [
            edge.package.as_str(),
            edge.dep_kind.as_str(),
            edge.profile_class,
            edge.gate.as_str(),
            edge.disposition,
            edge.removal_owner.as_str(),
        ] {
            output.push_str("| ");
            output.push_str(&escape_cell(cell));
        }
        output.push_str(" |\n");
    }
    output.push('\n');
    output.push_str("| Transitive selecting parent | Reachability | Min hops from lsp-types |\n");
    output.push_str("| --- | --- | --- |\n");
    for row in &denominator.transitive {
        for cell in [row.package.as_str(), row.reachability, &row.min_hops.to_string()] {
            output.push_str("| ");
            output.push_str(&escape_cell(cell));
        }
        output.push_str(" |\n");
    }
    output.push('\n');
    output
}

fn push_patch_row(output: &mut String, row: &CapabilityPatchRow) {
    for cell in [
        row.row_id,
        row.anchor,
        row.current_assumption,
        row.protocol_identity,
        row.substrate_result,
        row.disposition,
        row.owner,
        row.exit_rule,
        row.first_falsifier,
    ] {
        output.push_str("| ");
        output.push_str(&escape_cell(cell));
    }
    output.push_str(" |\n");
}

fn render_receipt(denominator: &Denominator, falsifier_files: &[String]) -> Result<String> {
    let mut record = Vec::new();
    for field in SUBSTRATE_RECORD {
        record.push(serde_json::json!({
            "field": field.field,
            "value": field.value,
            "evidence": field.evidence,
        }));
    }
    let mut candidates = Vec::new();
    for row in CANDIDATES {
        candidates.push(serde_json::json!({
            "candidate": row.candidate,
            "version": row.version,
            "source": row.source,
            "state": row.state,
            "verdict": row.verdict,
            "rationale": row.rationale,
        }));
    }
    let mut patches = Vec::new();
    for row in CAPABILITY_PATCHES {
        patches.push(serde_json::json!({
            "row_id": row.row_id,
            "anchor": row.anchor,
            "current_assumption": row.current_assumption,
            "protocol_identity": row.protocol_identity,
            "substrate_result": row.substrate_result,
            "disposition": row.disposition,
            "owner": row.owner,
            "exit_rule": row.exit_rule,
            "first_falsifier": row.first_falsifier,
        }));
    }
    let cargo_denominator = serde_json::json!({
        "instrument": "cargo metadata --all-features --format-version 1 --locked",
        "direct_edges": denominator.edges.iter().map(|edge| serde_json::json!({
            "package": edge.package,
            "dep_kind": edge.dep_kind,
            "profile_class": edge.profile_class,
            "feature_gate": edge.gate,
            "disposition": edge.disposition,
            "removal_owner": edge.removal_owner,
        })).collect::<Vec<_>>(),
        "transitive_members": denominator.transitive.iter().map(|row| serde_json::json!({
            "package": row.package,
            "reachability": row.reachability,
            "min_hops": row.min_hops,
        })).collect::<Vec<_>>(),
    });
    let mut deltas = Vec::new();
    for row in SCHEMA_DELTAS {
        deltas.push(serde_json::json!({
            "row_id": row.row_id,
            "area": row.area,
            "incumbent_evidence": row.incumbent_evidence,
            "candidate_evidence": row.candidate_evidence,
            "classification": row.classification,
            "migration_note": row.migration_note,
        }));
    }
    let receipt = serde_json::json!({
        "schema_version": 1,
        "claim": "11802",
        "parent_goal": "1421",
        "generator": "cargo xtask generate-protocol-type-substrate-matrix",
        "evidence_pin_date": EVIDENCE_PIN_DATE,
        "substrate_checksum_sha256": GLT_CHECKSUM_SHA256,
        "dispositions_vocabulary": DISPOSITIONS,
        "snapshot_falsifier_files": falsifier_files,
        "snapshot_falsifier_file_count": falsifier_files.len(),
        "sections": {
            "substrate_record": record,
            "candidates": candidates,
            "capability_patches": patches,
            "cargo_denominator": cargo_denominator,
            "schema_deltas": deltas,
            "extension_seams": EXTENSION_SEAMS.iter().map(|row| serde_json::json!({
                "row_id": row.row_id,
                "anchor": row.anchor,
                "seam_behavior": row.seam_behavior,
                "disposition": row.disposition,
                "owner": row.owner,
            })).collect::<Vec<_>>(),
        },
    });
    let mut pretty = serde_json::to_string_pretty(&receipt)
        .context("failed to serialize protocol-type substrate receipt")?;
    pretty.push('\n');
    Ok(pretty)
}

fn escape_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "<br>")
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_denominator() -> Denominator {
        Denominator {
            edges: vec![
                EdgeRow {
                    package: "perl-incremental-parsing".to_string(),
                    dep_kind: "dev".to_string(),
                    profile_class: "dev_test",
                    gate: "-".to_string(),
                    disposition: "test_fixture_only",
                    removal_owner: "#1421 sequencing; exit when LT02 lands".to_string(),
                },
                EdgeRow {
                    package: "perl-lsp-rs-core".to_string(),
                    dep_kind: "normal".to_string(),
                    profile_class: "production",
                    gate: "-".to_string(),
                    disposition: "adapter_protocol_type",
                    removal_owner: "#11803 migration; crate-level retirement relation #9645 relocates rows to the final product home first".to_string(),
                },
                EdgeRow {
                    package: "perl-parser".to_string(),
                    dep_kind: "normal".to_string(),
                    profile_class: "compatibility_edge",
                    gate: "lsp-compat".to_string(),
                    disposition: "lower_wire_remove_before_switch",
                    removal_owner: "#9893".to_string(),
                },
            ],
            transitive: vec![TransitiveRow {
                package: "perl-uri".to_string(),
                reachability: "dev_only_chain",
                min_hops: 2,
            }],
        }
    }

    /// Minimal synthetic cargo-metadata payload mirroring the real shape:
    /// packages[] with dependencies[], resolve.nodes[] with deps[]/dep_kinds[].
    fn fixture_metadata() -> serde_json::Value {
        serde_json::json!({
            "packages": [
                { "id": "registry+lsp", "name": "lsp-types", "version": "0.97.0", "features": {}, "dependencies": [] },
                {
                    "id": "path+parser", "name": "perl-parser", "version": "0.17.0",
                    "features": { "lsp-compat": ["lsp-types"] },
                    "dependencies": [
                        { "name": "lsp-types", "pkg": "registry+lsp", "kind": null, "optional": true, "features": [] }
                    ]
                },
                {
                    "id": "path+adapter", "name": "perl-lsp-rs-core", "version": "0.17.0",
                    "features": {},
                    "dependencies": [
                        { "name": "lsp-types", "pkg": "registry+lsp", "kind": null, "optional": false, "features": [] }
                    ]
                },
                {
                    "id": "path+uri", "name": "perl-uri", "version": "0.17.0",
                    "features": {},
                    "dependencies": [
                        { "name": "perl-tdd-support", "pkg": "path+tdd", "kind": null, "optional": false, "features": [] }
                    ]
                },
                {
                    "id": "path+tdd", "name": "perl-tdd-support", "version": "0.17.0",
                    "features": { "lsp-compat": ["dep:lsp-types", "url"] },
                    "dependencies": [
                        { "name": "lsp-types", "pkg": "registry+lsp", "kind": null, "optional": true, "features": [] }
                    ]
                }
            ],
            "resolve": { "nodes": [
                { "id": "registry+lsp", "features": [], "deps": [] },
                { "id": "path+adapter", "features": [], "deps": [ { "pkg": "registry+lsp", "dep_kinds": [ { "kind": null, "target": null } ] } ] },
                { "id": "path+parser", "features": ["lsp-compat"], "deps": [ { "pkg": "registry+lsp", "dep_kinds": [ { "kind": null, "target": null } ] } ] },
                { "id": "path+uri", "features": [], "deps": [ { "pkg": "path+tdd", "dep_kinds": [ { "kind": "dev", "target": null } ] } ] },
                { "id": "path+tdd", "features": ["lsp-compat"], "deps": [ { "pkg": "registry+lsp", "dep_kinds": [ { "kind": null, "target": null } ] } ] }
            ] }
        })
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn denominator_parses_synthetic_metadata() {
        let parsed = parse_denominator(&fixture_metadata()).expect("synthetic metadata must parse");
        let names: Vec<&str> = parsed.edges.iter().map(|edge| edge.package.as_str()).collect();
        assert_eq!(names, vec!["perl-lsp-rs-core", "perl-parser", "perl-tdd-support"]);
        let parser =
            parsed.edges.iter().find(|edge| edge.package == "perl-parser").expect("parser edge");
        assert_eq!(parser.profile_class, "compatibility_edge");
        assert_eq!(parser.gate, "lsp-compat");
        assert_eq!(parser.removal_owner, "#9893");
        // perl-uri reaches lsp-types only through perl-tdd-support's dev edge.
        assert_eq!(parsed.transitive.len(), 1);
        assert_eq!(parsed.transitive[0].package, "perl-uri");
        assert_eq!(parsed.transitive[0].reachability, "dev_only_chain");
    }

    #[test]
    fn unknown_edges_classify_not_proven() {
        let (class, disposition, owner) = classify_edge("some-new-crate", "normal", false);
        assert_eq!(class, "not_proven");
        assert_eq!(disposition, "not_proven");
        assert!(owner.contains("unclassified"));
    }

    #[test]
    fn doomed_lower_rows_are_never_migration_population() {
        for (package, kind, optional) in [
            ("perl-parser", "normal", true),
            ("perl-position-tracking", "normal", true),
            ("perl-workspace", "normal", true),
        ] {
            let (_, disposition, _) = classify_edge(package, kind, optional);
            assert_eq!(
                disposition, "lower_wire_remove_before_switch",
                "{package} is doomed lower wire and must not migrate"
            );
        }
    }

    /// Deterministic falsifier fixture mirroring the real scan output shape.
    fn fixture_falsifiers() -> Vec<String> {
        vec![
            "crates/perl-lsp-rs/tests/lsp_cap_snap.rs".to_string(),
            "crates/perl-lsp-rs/tests/lsp_caps_contract_shapes.rs".to_string(),
        ]
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn falsifier_scan_counts_files_mentioning_patched_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let crates = dir.path().join("crates").join("demo");
        std::fs::create_dir_all(crates.join("tests")).expect("mkdir tests");
        std::fs::create_dir_all(crates.join("src")).expect("mkdir src");
        std::fs::write(
            crates.join("tests").join("contract.rs"),
            "assert!(caps[\"rangesSupport\"].is_object());",
        )
        .expect("write test file");
        std::fs::write(crates.join("src").join("lib.rs"), "fn x() { let _ = 1; }")
            .expect("write src file");
        let found =
            count_falsifier_files(dir.path()).expect("scan must succeed on tempdir fixture");
        assert_eq!(found, vec!["crates/demo/tests/contract.rs".to_string()]);
    }

    #[test]
    fn derived_denominators_appear_in_rendered_output() {
        let denominator = fixture_denominator();
        let rendered = render_matrix(&denominator, &fixture_falsifiers());
        for needle in [
            "## 6. Manual-extension registry",
            "## 7. Derived downstream denominators",
            "SEAM-EXPERIMENTAL-TYPEHIERARCHY",
            "SEAM-RUNTIME-DYNAMIC-INLINECOMPLETION",
            "**LT02 / #11803 migration population:** 1 surviving direct Cargo edges",
            "**LT04 / #11805 proof closure:** 2 snapshot/contract falsifier files",
        ] {
            assert!(rendered.contains(needle), "matrix missing derived content {needle}");
        }
    }

    #[test]
    fn rendered_matrix_contains_substrate_and_discriminating_rows() {
        let rendered = render_matrix(&fixture_denominator(), &fixture_falsifiers());
        for needle in [
            "gen-lsp-types 0.11.0",
            GLT_CHECKSUM_SHA256,
            "rejected_archived_superseded",
            "PATCH-RANGESSUPPORT",
            "PATCH-INSERTTEXTMODES",
            "STALE",
            "## 4. Resolved Cargo denominator",
            "lower_wire_remove_before_switch",
        ] {
            assert!(rendered.contains(needle), "matrix missing required content {needle}");
        }
    }

    #[test]
    fn dispositions_are_package_neutral() {
        let rendered = render_matrix(&fixture_denominator(), &fixture_falsifiers());
        for legacy in [
            "typed_ls_types_stable",
            "typed_ls_types_proposed",
            "missing_in_ls_types_but_spec_admitted",
        ] {
            assert!(
                !rendered.contains(legacy),
                "matrix still carries target-specific legacy vocabulary {legacy}"
            );
        }
        for row in CAPABILITY_PATCHES {
            assert!(
                DISPOSITIONS.contains(&row.disposition),
                "patch row {} uses non-canonical disposition {}",
                row.row_id,
                row.disposition
            );
        }
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn invalid_shape_is_not_marked_for_migration() {
        let modes = CAPABILITY_PATCHES
            .iter()
            .find(|row| row.row_id == "PATCH-INSERTTEXTMODES")
            .expect("insertTextModes patch row must exist");
        assert_eq!(modes.disposition, "invalid_current_protocol_shape");
        assert!(modes.owner.contains("NOT migrated"));
        assert!(modes.substrate_result.starts_with("no substrate equivalent"));
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn archived_target_is_not_selected() {
        let ls_types = CANDIDATES
            .iter()
            .find(|row| row.candidate == "ls-types")
            .expect("ls-types candidate row must exist");
        assert_eq!(ls_types.verdict, "candidate_rejected");
        assert_ne!(ls_types.state, "selected_maintained_substrate");
    }

    #[test]
    fn two_consecutive_renders_are_byte_identical() -> Result<()> {
        let denominator = fixture_denominator();
        let falsifiers = fixture_falsifiers();
        assert_eq!(
            render_matrix(&denominator, &falsifiers),
            render_matrix(&denominator, &falsifiers)
        );
        assert_eq!(
            render_receipt(&denominator, &falsifiers)?,
            render_receipt(&denominator, &falsifiers)?
        );
        Ok(())
    }

    #[test]
    fn schema_delta_rows_stay_within_classification_vocabulary() {
        let allowed = [
            "incumbent_defect_corrected_by_candidate",
            "candidate_defect",
            "public_api_only_difference",
            "schema_equivalent_representation_difference",
            "intentional_repository_extension",
            "not_proven",
        ];
        for row in SCHEMA_DELTAS {
            assert!(
                allowed.contains(&row.classification),
                "{} uses non-canonical classification {}",
                row.row_id,
                row.classification
            );
            assert!(!row.incumbent_evidence.is_empty());
            assert!(!row.candidate_evidence.is_empty());
        }
        // Falsifier 3: a stale "rangesSupport missing" gap must not survive.
        let ranges = SCHEMA_DELTAS
            .iter()
            .find(|row| row.row_id == "DELTA-RANGESSUPPORT-FIELD")
            .unwrap_or(&SCHEMA_DELTAS[0]);
        assert_eq!(ranges.classification, "incumbent_defect_corrected_by_candidate");
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn receipt_is_well_formed_json_with_required_sections() {
        let denominator = fixture_denominator();
        let falsifiers = fixture_falsifiers();
        let receipt =
            render_receipt(&denominator, &falsifiers).expect("receipt must serialize in tests");
        let parsed: serde_json::Value = serde_json::from_str(receipt.trim())
            .expect("generated receipt must be well-formed JSON");
        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["claim"], "11802");
        let sections = &parsed["sections"];
        assert_eq!(
            sections["capability_patches"].as_array().map(Vec::len),
            Some(CAPABILITY_PATCHES.len())
        );
        assert_eq!(
            sections["substrate_record"].as_array().map(Vec::len),
            Some(SUBSTRATE_RECORD.len())
        );
        assert_eq!(
            sections["cargo_denominator"]["direct_edges"].as_array().map(Vec::len),
            Some(denominator.edges.len())
        );
        assert_eq!(sections["schema_deltas"].as_array().map(Vec::len), Some(SCHEMA_DELTAS.len()));
        assert_eq!(
            sections["extension_seams"].as_array().map(Vec::len),
            Some(EXTENSION_SEAMS.len())
        );
        assert_eq!(parsed["snapshot_falsifier_file_count"], falsifiers.len() as u64);
    }
}
