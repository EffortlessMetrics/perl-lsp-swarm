# Protocol-Type Substrate Matrix

Status: generated (inventory-only; no Cargo/API/protocol behavior change)
Owner: perl-lsp maintainers
Generator: `cargo xtask generate-protocol-type-substrate-matrix`
Check: `cargo xtask generate-protocol-type-substrate-matrix --check`
Authority: issue #11802 as corrected by the maintainer deep-review comment (ls-types archived 2026-08-15; gen-lsp-types 0.11.0 is the live candidate). External evidence pins inspected 2026-08-22.

This matrix freezes the protocol-type denominator for #1421. It records one selected-substrate record and the discriminating capability-patch rows. Stable vs proposed vs project-extension maturity stays repository-owned (#7113 validator, features.toml); generated type availability never implies protocol stability.

Survival-disposition vocabulary (package-neutral):

- `lower_wire_remove_before_switch`
- `adapter_protocol_type`
- `public_api_break_or_migration`
- `selected_substrate_generated_type`
- `selected_substrate_manual_schema_extension`
- `invalid_current_protocol_shape`
- `project_extension`
- `test_fixture_only`
- `compatibility_with_exit`
- `retire`
- `candidate_rejected`
- `not_proven`

## 1. Selected-substrate record: gen-lsp-types 0.11.0

| Field | Value | Evidence |
| --- | --- | --- |
| selection verdict | selected_maintained_substrate | #11802 maintainer deep-review ruling; first-hand 0.11.0 source/package verification below |
| package / version / source | gen-lsp-types 0.11.0; crates.io; repo https://github.com/ribru17/gen-lsp-types | crates.io API v1 crates/gen-lsp-types inspected {EVIDENCE_PIN_DATE}; trust-published from GitHub run 30327548194 @ 1e84ee239d093e4933bf3024cd597255090e5813 |
| checksum | b64887ac3a8083427ae935a7296db876871582cd57eac077564f8bc18fa49116 | SHA-256 of downloaded gen-lsp-types-0.11.0.crate matched the crates.io API checksum exactly ({EVIDENCE_PIN_DATE}) |
| maintenance state | actively maintained successor; predecessor ls-types archived 2026-08-15 naming this crate as replacement | archived ls-types README supersession notice recorded in the #11802 deep review |
| metamodel / spec source identity | generated from the official LSP metamodel | crate description and generator pipeline in ribru17/gen-lsp-types; #11802 deep-review external pin |
| generator identity / reproducibility | metamodel-codegen inside the upstream repo; published artifacts carry a trust-publish provenance (GitHub Actions run id + commit sha) | crates.io trustpub_data for 0.11.0 (run 30327548194, sha 1e84ee2) |
| edition / MSRV / dependency graph | edition 2024; no declared MSRV (rust_version absent from registry metadata); deps serde 1.0.228, serde_json 1.0.150, optional fluent-uri 0.4.1 (serde) or url 2.5.8 (serde) | Cargo.toml.orig inside the verified 0.11.0 .crate payload; crates.io API rust_version=null |
| URI representation feature | default String-backed `pub struct Uri(pub String)`; optional features `url` (url::Url) and `fluent-uri` (fluent_uri::Uri<String>); feature choice deferred to the migration lane with #8156/#8484/public-API proof | src/generated/common.rs lines 28/66/68 of the verified 0.11.0 payload; #11802 URI submatrix ruling (choose for adapter-boundary preservation, not import-edit minimization) |
| null / absent serialization model | `Option<T>` fields serialize as absent (`skip_serializing_if = "Option::is_none"`, 498 occurrences in structures.rs); explicit null appears only where the metamodel demands it - distinct wire states preserved, not flattened | verified 0.11.0 src/generated/structures.rs serde attributes |
| request / notification direction model | dedicated requests.rs / notifications.rs modules encode method direction types; route/method authority remains #8896 - unchanged by this inventory | verified 0.11.0 src/generated/{requests,notifications}.rs; #11802 falsifier 9 |
| stable / proposed representation model | single Cargo surface without a stable/proposed feature split; protocol maturity stays repository-owned per admitted profile (#7113 validator + features.toml); generated availability is not stability | 0.11.0 Cargo.toml.orig features = url\|fluent-uri only; #11802 stable/proposed boundary ruling |
| coverage of current manual patches | typed `ServerCapabilities.type_hierarchy_provider: Option<TypeHierarchyProvider>` (structures.rs:6062), typed top-level `inline_completion_provider: Option<InlineCompletionProvider>` (:6082), typed `DocumentRangeFormattingOptions.ranges_support: Option<bool>` (:7113); `completionItem.insertTextModes` is NOT modeled server-side (only client-capability InsertTextMode enum exists) | first-hand Select-String over the verified 0.11.0 src/generated/*.rs |
| known gaps / limitations | no declared MSRV; zero-ver breaking policy means point releases may break - migration must pin exact version and record an update policy; String Uri default preserves qualifiers but defers parse/serialize semantics to the chosen feature | registry rust_version=null; upstream policy; #11802 URI submatrix requirements |

## 2. Candidate set evaluation

| Candidate | Version | Source | State | Verdict | Rationale |
| --- | --- | --- | --- | --- | --- |
| lsp-types | 0.97.0 (current incumbent) | crates.io; workspace dep at root Cargo.toml [workspace.dependencies] | incumbent | retire | stays selected only until the migration lane switches; unmaintained lineage motivated #1421. What remains useful (DTO coverage in active adapter paths) is recorded row-by-row in the denominator; incumbent snapshots are behavior evidence, not target authority. |
| ls-types | archived (was 0.0.6-era target) | GitHub archive notice 2026-08-15 | rejected_archived_superseded | candidate_rejected | owner named gen-lsp-types as successor; cannot receive selected_maintained_substrate without a new reviewed ruling supplying fork/security/update plan. Issue-body ls-types field vocabulary (typed_ls_types_*) is retired from the canonical schema. |
| gen-lsp-types | 0.11.0 | crates.io checksum b64887ac...; repo ribru17/gen-lsp-types | active_successor_candidate | selected_maintained_substrate | first-hand verified: checksum match, edition 2024, official-metamodel generation, typed typeHierarchyProvider/rangesSupport/inlineCompletionProvider, String-default Uri with optional url\|fluent-uri, explicit null-vs-absent model, request/notification direction types. Limitations recorded in the substrate record; later LT issues may not independently select another package or feature. |

## 3. Discriminating capability-patch rows

| Row ID | Anchor | Current assumption | Protocol identity | Selected-substrate result | Disposition | Owner | Exit rule | First falsifier |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| PATCH-TYPEHIERARCHY| crates/perl-lsp-rs-core/src/protocol/capabilities.rs capabilities_json() typeHierarchyProvider injection (lines 72-77)| lsp-types 0.97 ServerCapabilities lacks type_hierarchy_provider; patched into serialized JSON post-hoc; parallel experimental injection in protocol/capabilities/experimental.rs:10 and detection support in capability_map.rs| initialize result capabilities.typeHierarchyProvider (object form)| typed once: ServerCapabilities.type_hierarchy_provider: Option<TypeHierarchyProvider>| selected_substrate_generated_type| #11803 migration (surviving LT02 row)| patch and experimental workaround removed together when the adapter serializes the typed field| tests/lsp_caps_contract_shapes.rs typeHierarchyProvider shape assertion population |
| PATCH-RANGESSUPPORT| crates/perl-lsp-rs-core/src/protocol/capabilities.rs documentRangeFormattingProvider rangesSupport injection (lines 79-86)| issue-body claim 'rangesSupport missing' is STALE: verified 0.11.0 structures.rs:7113 DocumentRangeFormattingOptions.ranges_support: Option<bool>| capabilities.documentRangeFormattingProvider.rangesSupport (LSP 3.18 multi-range formatting)| typed once: DocumentRangeFormattingOptions.ranges_support: Option<bool>| selected_substrate_generated_type| #11803 migration (surviving LT02 row)| hand-patched object replaced by typed options struct; 3.18 conformance matrix row stays authoritative for advertisement shape| tests/lsp_caps_contract_shapes.rs rangesSupport pointer assertions; lsp_3_17_lifecycle_tests registration payload |
| PATCH-INLINECOMPLETION| crates/perl-lsp-rs-core/src/protocol/capabilities.rs inlineCompletionProvider injection (lines 87-93); runtime dynamic-client removal at crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs (~lines 781-815)| lsp-types 0.97 predates the field; static advertisement patched into JSON, then removed for dynamic-registration clients at initialize time| capabilities.inlineCompletionProvider top-level (LSP 3.18); experimental placement forbidden (negative-claimed)| typed once: ServerCapabilities.inline_completion_provider: Option<InlineCompletionProvider>; runtime dynamic-client removal logic is behavioral and stays out of type migration scope| selected_substrate_generated_type| #11803 migration (surviving LT02 row); runtime removal seam owned by lifecycle code, not the type switch| patch removed when typed field serializes identically; dynamic-client removal branch must keep byte-identical initialize output| tests/lsp_inline_completion_registration_tests.rs; tests/lsp_cap_snap.rs; ripr_seam_proof_* capability negotiation proofs |
| PATCH-INSERTTEXTMODES| crates/perl-lsp-rs-core/src/protocol/capabilities.rs completionItem.insertTextModes injection (lines 95-105)| advertises numeric array [1,2] inside completionProvider.completionItem; that key is NOT a valid server-capability shape (client capability textDocument.completion.insertTextMode is the real negotiation surface) per #2892/#8032| invalid_current_protocol_shape - not a type gap| no substrate equivalent required: verified 0.11.0 models InsertTextMode enum and the client capability but no server-side insertTextModes field| invalid_current_protocol_shape| #8032 single-capability-authority work removes it; explicitly NOT migrated to the selected substrate| remove the injection and its snapshot assertions in the #8032 lane; migration must not carry it forward as parity| tests/lsp_capabilities_contract.rs insertTextModes advertisement assertions (falsifiers flip to removal proofs) |
