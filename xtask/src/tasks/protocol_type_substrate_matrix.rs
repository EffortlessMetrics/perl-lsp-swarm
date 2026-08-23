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
use color_eyre::eyre::{Context, Result, bail};
use std::fs;

pub const MATRIX_PATH: &str = "docs/specs/protocol-type-substrate-matrix.md";
pub const RECEIPT_PATH: &str = "docs/specs/protocol-type-substrate-matrix.json";

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
        value: "`Option<T>` fields serialize as absent (`skip_serializing_if = \"Option::is_none\"`, 498 occurrences in structures.rs); explicit null appears only where the metamodel demands it - distinct wire states preserved, not flattened",
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
        rationale:
            "stays selected only until the migration lane switches; unmaintained lineage motivated #1421. What remains useful (DTO coverage in active adapter paths) is recorded row-by-row in the denominator; incumbent snapshots are behavior evidence, not target authority.",
    },
    CandidateRow {
        candidate: "ls-types",
        version: "archived (was 0.0.6-era target)",
        source: "GitHub archive notice 2026-08-15",
        state: "rejected_archived_superseded",
        verdict: "candidate_rejected",
        rationale:
            "owner named gen-lsp-types as successor; cannot receive selected_maintained_substrate without a new reviewed ruling supplying fork/security/update plan. Issue-body ls-types field vocabulary (typed_ls_types_*) is retired from the canonical schema.",
    },
    CandidateRow {
        candidate: "gen-lsp-types",
        version: "0.11.0",
        source: "crates.io checksum b64887ac...; repo ribru17/gen-lsp-types",
        state: "active_successor_candidate",
        verdict: "selected_maintained_substrate",
        rationale:
            "first-hand verified: checksum match, edition 2024, official-metamodel generation, typed typeHierarchyProvider/rangesSupport/inlineCompletionProvider, String-default Uri with optional url|fluent-uri, explicit null-vs-absent model, request/notification direction types. Limitations recorded in the substrate record; later LT issues may not independently select another package or feature.",
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
        current_assumption: "lsp-types 0.97 predates the field; static advertisement patched into JSON, then removed for dynamic-registration clients at initialize time",
        protocol_identity: "capabilities.inlineCompletionProvider top-level (LSP 3.18); experimental placement forbidden (negative-claimed)",
        substrate_result: "typed once: ServerCapabilities.inline_completion_provider: Option<InlineCompletionProvider>; runtime dynamic-client removal logic is behavioral and stays out of type migration scope",
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

pub fn run(check: bool) -> Result<()> {
    let root = project_root()?;
    let path = root.join(MATRIX_PATH);
    let receipt_path = root.join(RECEIPT_PATH);
    let generated = render_matrix();
    let receipt = render_receipt()?;

    if check {
        let existing = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", MATRIX_PATH))?;
        if normalize_newlines(&existing) != generated {
            bail!("{} is stale; run `cargo xtask generate-protocol-type-substrate-matrix`", MATRIX_PATH);
        }
        let existing_receipt = fs::read_to_string(&receipt_path)
            .with_context(|| format!("failed to read {}", RECEIPT_PATH))?;
        if normalize_newlines(&existing_receipt) != receipt {
            bail!("{} is stale; run `cargo xtask generate-protocol-type-substrate-matrix`", RECEIPT_PATH);
        }
        println!(
            "Protocol-type substrate matrix is up to date: {} substrate fields, {} candidates, {} patch rows",
            SUBSTRATE_RECORD.len(),
            CANDIDATES.len(),
            CAPABILITY_PATCHES.len()
        );
        return Ok(());
    }

    fs::write(&path, generated).with_context(|| format!("failed to write {}", MATRIX_PATH))?;
    fs::write(&receipt_path, receipt)
        .with_context(|| format!("failed to write {}", RECEIPT_PATH))?;
    println!(
        "Wrote {} (+ {} receipt) with {} substrate fields, {} candidates, {} patch rows",
        MATRIX_PATH,
        RECEIPT_PATH,
        SUBSTRATE_RECORD.len(),
        CANDIDATES.len(),
        CAPABILITY_PATCHES.len()
    );
    Ok(())
}

fn render_matrix() -> String {
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

fn render_receipt() -> Result<String> {
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
    let receipt = serde_json::json!({
        "schema_version": 1,
        "claim": "11802",
        "parent_goal": "1421",
        "generator": "cargo xtask generate-protocol-type-substrate-matrix",
        "evidence_pin_date": EVIDENCE_PIN_DATE,
        "substrate_checksum_sha256": GLT_CHECKSUM_SHA256,
        "dispositions_vocabulary": DISPOSITIONS,
        "sections": {
            "substrate_record": record,
            "candidates": candidates,
            "capability_patches": patches,
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

    #[test]
    fn rendered_matrix_contains_substrate_and_discriminating_rows() {
        let rendered = render_matrix();
        for needle in [
            "gen-lsp-types 0.11.0",
            GLT_CHECKSUM_SHA256,
            "rejected_archived_superseded",
            "PATCH-RANGESSUPPORT",
            "PATCH-INSERTTEXTMODES",
            "STALE",
        ] {
            assert!(rendered.contains(needle), "matrix missing required content {needle}");
        }
    }

    #[test]
    fn dispositions_are_package_neutral() {
        let rendered = render_matrix();
        for legacy in
            ["typed_ls_types_stable", "typed_ls_types_proposed", "missing_in_ls_types_but_spec_admitted"]
        {
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
        assert_eq!(render_matrix(), render_matrix());
        assert_eq!(render_receipt()?, render_receipt()?);
        Ok(())
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn receipt_is_well_formed_json_with_required_sections() {
        let receipt = render_receipt().expect("receipt must serialize in tests");
        let parsed: serde_json::Value = serde_json::from_str(receipt.trim())
            .expect("generated receipt must be well-formed JSON");
        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["claim"], "11802");
        assert_eq!(
            parsed["sections"]["capability_patches"].as_array().map(Vec::len),
            Some(CAPABILITY_PATCHES.len())
        );
        assert_eq!(
            parsed["sections"]["substrate_record"].as_array().map(Vec::len),
            Some(SUBSTRATE_RECORD.len())
        );
    }
}
