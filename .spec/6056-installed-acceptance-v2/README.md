# Canonical pre-freeze installed acceptance v2 (#6056)

Root-adjudicated bounded v2 implementation packet; not an installed receipt or freeze decision. This slice advances #6056 and does not close its real installed-execution acceptance.

## Authorities and precedence

- [#13768, Gate 3](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13768): full installed Windows x64 and Linux x64; startup/readiness, diagnostics, completion, hover, definition, references, symbols, retained code action, Unicode/CRLF edit-save-requery, whole-document formatting, native-only Critic, DAP preview, restart/repair, clean shutdown. Other retained targets require topology-selected build/archive/version/launch proof.
- [#6056](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6056): same packaged candidate on Linux shipped-manifest minimum and execution-time stable VS Code; isolated profile/config/fixture identities; twelve explicit zero-budget counters; first-ten-minutes observation. Its older Windows preparation-only boundary is superseded by #13768, not inherited into v2.
- [FF02 decision](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13856#issuecomment-5750171875) and [current owner amendment](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6056#issuecomment-5750187184): canonical pre-freeze acceptance bound to frozen source, shared validator, Windows/Linux mandatory, no later installed_public_beta substitution.
- [#5901](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5901): exact_current, bounded_fallback, not_ready, unsupported_or_dynamic, safe_refusal, legitimate_empty, product_or_instrument_error remain distinct. Passing a safe-refusal proposition does not assert successful edit execution.
- [#4346](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4346): installed reachability, freshness, multi-root, conditional retained testing, and explicit controller override of older Windows boundary. Its later prepared/release subject is not a prerequisite identity for this pre-freeze model.

Source inspected at live main a30f10020a36b7db80ec3e825881941fb8920a9c; canonical example blob d6330a1f72c1dc980060d6840d8ea8f10577afe5:
[model](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/a30f10020a36b7db80ec3e825881941fb8920a9c/xtask/examples/pre_freeze_public_beta_acceptance.rs#L74), provenance L100, platform L109, global journey L126, zero counts L136, packet L180, journey validation L266, recommendation L326, CLI L402. v1 has eleven global cells and no host denominator; Windows limited can be ready. CLI validation success alone is not readiness.

## Exact mandatory execution rows

| row_id | platform | architecture | host_role |
| --- | --- | --- | --- |
| linux_x64_minimum_supported | linux | x64 | minimum_supported |
| linux_x64_current_stable | linux | x64 | current_stable |
| windows_x64_current_stable | windows | x64 | current_stable |

Exactly these three semantic rows, each with every mandatory cell below exactly once. No Windows minimum-version row is added. Windows current_stable is the root-adjudicated single host role. Each row records exact vscode_version and extension-host evidence; Linux floor binds the shipped VSIX manifest engine requirement and its resolved minimum host. Stable binds an observation timestamp and retained version-selection evidence. If floor equals stable, both role rows remain explicit; reuse of one execution must be declared and identity-equivalent, not silently missing a role.

## Stable cell IDs and propositions

Every mandatory row carries the 24 cells below. These are required v2 vocabulary, not claims that existing execution producers already emit them.

| cell_id | Required observable proposition / authority |
| --- | --- |
| install_upgrade_identity | Isolated release-shaped install/upgrade selects exact server/DAP/VSIX; no workspace/PATH/cache/profile substitution (#6056/#5903). |
| startup_readiness | Binary launch, initialize, active-document and workspace readiness/generation states; quiet healthy path (#13768/#5901). |
| workspace_multiroot | Finite fixture workspace loads; root isolation and provenance remain intact (#6056/#4346). |
| diagnostics | Current native diagnostics, with exact source/generation and visible failure distinction (#13768). |
| completion | Installed completion request/result and currentness (#13768). |
| hover | Installed hover request/result and currentness (#13768). |
| definition | Installed definition request/result and currentness (#13768). |
| references | Installed references request/result and currentness (#13768). |
| document_symbols | Installed document symbols with source subject (#13768/#6056). |
| workspace_symbols | Installed workspace symbols with workspace/root subject (#13768/#6056). |
| retained_code_action | Representative retained action is reachable and safe/current; refusal does not count as applied edit (#13768). |
| result_state_distinctions | Exact, legitimate empty, not-ready, bounded fallback, unsupported/dynamic, safe refusal, product/instrument error remain distinguishable (#5901/#6056). |
| unicode_crlf_edit_save_requery | FULL/UTF-16, Unicode/surrogate and CRLF/LF edit-save-immediate-requery evidence, stale rejection (#13768/#9386/#9388). |
| file_create_rename_delete | File lifecycle invalidates obsolete indexed results (#6056/#4346). |
| safe_rename_or_refusal | Complete safe current edit set or explicit safe refusal, never partial-success (#6056). |
| whole_document_formatting | Retained whole-document formatting path or contract-permitted actionable refusal; preserves line endings/current subject (#13768/#4346). |
| native_only_critic | Diagnostics/code actions/runCritic share native subject; external Perl::Critic presence/absence does not select runtime behavior (#13768 Gate 1). |
| doctor_optional_tools | Native health does not require optional external tools (#6056/#7212). |
| diagnosis_and_identity_repair | Missing/instrument-failed health does not invent diagnosis; definite managed mismatch gets governed repair (#6056/#6875/#6854). |
| dap_preview | Exact installed adapter, bounded supported preview and honest unsupported outcome; no parity claim (#13768/#6056). |
| retained_test_entry | Retained Test Explorer/test entry executes bounded/cancellable truthfully when claimed; otherwise proves explicit withdrawn/unsupported disposition, never fabricated test success (#6056/#4346). |
| installed_contribution_reachability | Candidate VSIX activation, representative commands, formatter config, status/doctor/restart and claimed contribution paths actually reachable (#6056/#7765). |
| restart_recovery_rollback | Restart current installed subject; rejected corrupt/partial candidate preserves known-good pair; success binds replacement identities (#6056/#5903/#6097). |
| shutdown_cleanup | No candidate server/adapter/debugger/test residue or stale lock/cache readiness claim (#13768/#6056). |

One packet-level first_ten_minutes observation is mandatory, bound to the same artifact set. It records a nonempty unique subset of the three observed row IDs. It never implies human observation on other rows. The denominator is exactly 24 cells x 3 rows plus this one observation.

Cells may reference shared child receipts for genuinely shared propositions, but each row explicitly binds the observations applicable to that exact host/platform. One Linux receipt cannot establish Windows execution.

## Identity and evidence model

Keep existing candidate_id, exact repository_sha, actual source_version, target_release series, topology_digest, artifact_set_id, claim boundary. Add explicit phase=pre_freeze_product. Preserve separation from prepared/release-repository SHA and actual RC/VSIX version mapping.

Replace the single cross-platform binary pair with a closed inventory of artifact identities keyed by stable artifact_id, role, target and SHA-256. Each row selects its exact perllsp/DAP/VSIX artifact IDs. Server and DAP target must match row platform/architecture and inventory; a universal VSIX may be shared only when topology actually selects it. All entries bind the same proposed product SHA/topology/artifact set and release-shaped provenance. Transport path is not identity. A validator cannot infer target from filename or a declared hash alone; topology binding and validated leaf evidence remain required.

Row context: row_id, platform, architecture, host_role, exact vscode_version, clean_profile_id, configuration_identity, fixture IDs/content digests, artifact selections, provenance. Evidence references should bind immutable receipt bytes (digest/reference) and their row/subject provenance, not merely accept nonempty strings as proof. Reuse each existing leaf contract; do not create a generic status envelope that impersonates leaf validation. Mandatory binding checks are deterministic/offline; execution or provenance authenticity is not created by declaring fields.

Keep bounded topology-required other-target preparation as a separate explicit denominator, never a substitute semantic row. FF02 must compare the packet's target membership against the already-pinned topology authority; packet author cannot omit a required target. Avoid adding macOS semantic parity.

## Status and recommendation law

- Structurally invalid: missing/duplicate/unknown row or cell, unknown fields, absent required identities, malformed digest, contradictory target/provenance, inconsistent declared recommendation -> validation error; never ready.
- Cell status remains pass/limited/blocked/not_proven; limited/not_proven require reason. A mandatory limited cell cannot satisfy readiness. An expected safe refusal can pass only the explicitly stated refusal proposition; it does not upgrade unsupported behavior.
- Product blocker, any blocked mandatory evidence, or any of twelve nonzero trust counters -> blocked.
- Otherwise missing execution, instrument failure, mandatory limited/not_proven, or incomplete mandatory topology preparation -> not_proven.
- Ready only when all mandatory semantic cells/rows, packet-level observation, mechanism evidence and topology-required preparation obligations satisfy their accepted contracts, with zero trust counters and no blockers. An explicitly bounded preparation claim may be limited semantically while its required preparation checks all pass; do not conflate bounded claim scope with unexecuted obligation.
- Include all twelve #6056 counters explicitly, including false_repair_diagnosis and optional_tool_false_requirement; no omitted default-zero fields.
- Declared recommendation must equal computed result. Consumer receives typed result; CLI validation exit0 cannot be used as readiness. A --require-ready adapter is optional and must be separately tested if added.

## Compatibility, files, proof and non-goals

Use pre_freeze_public_beta_acceptance.v2; preserve v1 historical parsing/validation under its old semantics, clearly labelled historical and never admitted by current FF02 readiness. Do not reinterpret an old Linux-only ready as Windows/Linux proof. No migration can synthesize missing observations.

Likely files: shared xtask library module exported through xtask/src/lib.rs; thin existing example; focused module/CLI tests; docs/releases/v0.18-public-beta-experience.md and a settled .spec view. Keep legacy test propositions, convert touched assertions to fallible checks. No new crate or runtime product changes.

Controls: complete synthetic denominator; individually remove/duplicate/downgrade each row/cell; equal-version distinct host roles; wrong target pair; cross-candidate/topology/row receipt; missing floor/current evidence; each nonzero trust counter; instrument vs product failure; correct refusal vs false-success; optional testing withdrawal vs claimed unsupported pass; historical v1 refusal at current-consumer boundary. Synthetic positive is validator proof only.

Existing proof command: cargo test -p xtask --example pre_freeze_public_beta_acceptance. Add narrowly selected shared-module tests and scoped Clippy; no commands run in this research packet.


## Settled decision and implementation status

Authority: [root adjudication5750256278](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6056#issuecomment-5750256278). This builder view compiles that decision. The bounded model and CLI are implemented and independently reviewed; focused native and process proof is recorded below. Nothing here closes #6056 installed execution.

## Closed v2 wire schema and shared API

All objects deny unknown fields. All named fields are required, including nullable fields; no omitted default-zero counters. Collections have unique keys and deterministic diagnostic order. Hex identities use lowercase canonical SHA-1 (40 digits) for repository_sha and SHA-256 (64 digits) for content; topology_digest uses sha256: followed by 64 lowercase digits. Nonempty identifiers reject whitespace-only strings. Native paths remain transport strings, never identity authority.

Proposed Rust module: xtask::pre_freeze_public_beta_acceptance. The wire vocabulary below is the implementation contract; Rust names use PascalCase. No new crate or dependency is required.

```text
PacketV2 {
  check: "pre-freeze-public-beta-acceptance",
  schema_version: "pre_freeze_public_beta_acceptance.v2",
  phase: "pre_freeze_product",
  subject: Subject,
  source_version: nonempty string,
  target_release: "0.18.0",
  artifacts: Artifact[],
  rows: JourneyRow[3],
  first_ten_minutes: Observation,
  preparation: PreparationRow[],
  mechanisms: Mechanism[4],
  zero_budget_counts: ZeroBudgetCounts,
  product_blockers: string[],
  expected_beta_limitations: string[],
  friction_findings: string[],
  freeze_recommendation: ready|blocked|not_proven,
  claim_boundary: nonempty string
}
Subject { candidate_id, repository_sha, topology_digest, artifact_set_id }
Artifact {
  id, role: perllsp|perl_dap|vsix, target,
  path, sha256, subject: Subject,
  provenance: release_shaped|workspace_output|unknown
}
JourneyRow {
  id: one of the three fixed row IDs,
  platform: linux|windows, architecture: "x64",
  host_role: minimum_supported|current_stable,
  vscode_version, host_selection: EvidenceRef,
  clean_profile_id, configuration_identity,
  fixtures: Fixture[], artifacts: ArtifactSelection,
  subject: Subject, cells: Cell[24]
}
Fixture { id, content_sha256 }
ArtifactSelection { perllsp: artifact_id, perl_dap: artifact_id, vsix: artifact_id }
Cell {
  id: fixed cell ID, status: pass|limited|blocked|not_proven,
  proposition: executed|safe_refusal|claim_withdrawn,
  evidence: EvidenceRef[], reason: string|null
}
EvidenceRef {
  id, locator, sha256, subject: Subject,
  row_id: fixed row ID|null, artifact_ids: string[]
}
Observation {
  status: pass|limited|blocked|not_proven,
  observed_rows: fixed row ID[], evidence: EvidenceRef[], reason: string|null
}
PreparationRow {
  target, status: pass|limited|blocked|not_proven,
  artifact_ids: string[], evidence: EvidenceRef[], reason: string|null
}
Mechanism {
  issue: "#5900"|"#5901"|"#5902"|"#5903",
  status: pass|limited|blocked|not_proven,
  evidence: EvidenceRef[], reason: string|null
}
```

`ZeroBudgetCounts` has exactly the ten v1 fields plus `false_repair_diagnosis` and `optional_tool_false_requirement`, each u64. Test individual nonzero values without summation overflow. Lists of evidence, fixtures and artifact selections are nonempty where used. Reference IDs are unique within an owner; artifact IDs resolve to inventory. Evidence subject equals packet subject. Row-scoped evidence binds its row and selected artifacts; shared references require explicit per-row binding, not null row_id. Observation references bind one of observed_rows; preparation and packet-wide mechanism references use null row_id and their applicable inventory artifacts. No evidence locator is opened by pure validation.

The exact three row tuples are fixed, not caller-selectable. Both Linux roles select the same artifact triple. Their exact host versions may be equal, but identities and role evidence remain explicit. Different semantic platforms must not select one native binary identity: target membership comes from topology requirements, not filename guessing. A shared universal VSIX is accepted only under supplied topology requirements, which remain an external authority obligation.

`proposition=executed` is the normal cell law. `safe_refusal` is permitted only for safe_rename_or_refusal, whole_document_formatting, retained_test_entry and dap_preview, and requires a nonempty reason plus an accepted-claim evidence obligation. `claim_withdrawn` is permitted only for retained_test_entry, with the same obligation. No other cell can bypass execution using a generic withdrawal. Refusal still must be observed; it is not a missing-execution status.

The library exposes these pure entry points:

```rust
pub fn parse_v2(bytes: &[u8]) -> anyhow::Result<PacketV2>;
pub fn validate_v2(
    packet: &PacketV2,
    requirements: &TopologyRequirements,
) -> anyhow::Result<ValidationReport>;
```

`TopologyRequirements` is an explicit adapter input, not a second persisted producer schema. It contains the expected Subject, a unique sorted required_preparation_targets set, and artifact target admissibility for each semantic row/role (including explicitly allowed universal VSIX). Validate its internal completeness and compare packet membership exactly. Its caller must derive it from the pinned topology; passing arbitrary requirements does not attest that derivation. Missing requirements returns an error, not an empty denominator default.

`ValidationReport` exposes `bundle_recommendation: Recommendation`, `evidence_requirements: Vec<EvidenceRequirement>`, and `installed_qualification: NotProven`. These names are deliberate: this slice computes supplied-bundle consistency, and cannot emit installed qualification. `EvidenceRequirement` identifies owner kind/id, exact Subject, row binding, selected artifact IDs, receipt locator/digest and the required adapter category. Categories are topology_binding, artifact_provenance, host_selection, installed_journey, accepted_claim, first_ten_minutes, preparation and mechanism. Requirements are deterministically ordered and deduplicated only when the entire subject and obligation match.

The report always exposes topology and artifact authority obligations as well as referenced leaves. There is no public constructor or boolean parameter that fabricates verified installed authority. FF02 must fulfill these obligations through concrete accepted adapters and its own subject binding before admitting installed evidence. No caller-asserted generic Verified/Pass wrapper is implemented here.

The declaration `freeze_recommendation` must equal bundle_recommendation. A synthetic fully consistent fixture can compute ready, while installed_qualification stays not_proven. Documentation and CLI output always show both fields together; ready is never printed alone as installed eligibility.

## Adapter boundary and precise remaining prerequisites

This slice does not implement receipt authentication, JSON-pointer extraction from arbitrary producer schemas, runtime execution, artifact unpacking, VS Code version discovery or topology discovery. Existing v1 nonempty strings are not a reusable installed-evidence validator. In particular:

- #6056 execution owns actual installed per-cell evidence, host selection, clean profile/config/fixture identity and provenance.
- #5901/#5902/#5903/#5900 own their mechanism semantics; their identifiers alone do not validate receipt bytes.
- #5889/#6052 own the pinned topology and target denominator supplied to TopologyRequirements.
- #13856 owns FF02 consumption: verify actual bytes and concrete leaf laws, then join frozen subject and topology. Missing adapters keep installed qualification NOT_PROVEN.

An adapter compatibility issue discovered during implementation returns to root with the exact missing interface. It must not be repaired by adding a replacement generic producer or silently omitting an evidence requirement.

## Legacy adapter and proposed file cage

Keep the current v1 example's parser/validator semantics and all ten test propositions. Move legacy code only as needed to make the example a thin adapter; do not expose v1 as accepted by parse_v2. Historical v1 output labels historical semantics explicitly. No automatic upgrade, Windows synthesis or stronger v1 recommendation is permitted.

Proposed files after root accepts this spec:

- xtask/src/pre_freeze_public_beta_acceptance.rs: v2 model, pure validator and report.
- xtask/src/pre_freeze_public_beta_acceptance/tests.rs: fallible v2 discriminators.
- xtask/src/lib.rs: one module export.
- xtask/examples/pre_freeze_public_beta_acceptance.rs: retained historical v1 path and explicit v2 adapter. For v2, require a supplied topology-requirements input; do not derive authority from packet contents. File reading belongs only here.
- docs/releases/v0.18-public-beta-experience.md: historical/current distinction and bounded commands.
- This spec; narrowly required policy registration only after source inventory inspection.

A small private legacy module may be extracted if needed for the thin adapter; it must preserve v1 wire and recommendation behavior. No CI workflow, release producer, dependency, runtime product, version or installed execution changes.

## Exact planned proof commands and review condition

After root grants the heavy slot, use the lane-owned F: target and two jobs:

```text
cargo test -p xtask --lib pre_freeze_public_beta_acceptance --locked --profile agent -j 2
cargo test -p xtask --example pre_freeze_public_beta_acceptance --locked --profile agent -j 2
cargo clippy -p xtask --lib --example pre_freeze_public_beta_acceptance --locked --profile agent -j 2 -- -D warnings
```

Set RUSTC_WRAPPER to an empty string and CARGO_TARGET_DIR to the lane-owned F: cache; use direct Cargo, with no tool-wrapper requirement. Commands above are planned, not executed; compiler diagnostics outside this seam remain separately attributable. Run rustfmt on changed Rust and git diff --check. No broad workspace proof. Tests must preserve all ten v1 propositions and exercise the complete v2 denominator, statuses, subject/target joins, counter fields, refusal constraints, requirements completeness and permanent NOT_PROVEN installed boundary. Root independently reviews spec before implementation and exact source/proof before publication.

### Concrete implementation naming and retained adapter boundary

Rust enum names are Status, Recommendation, Role, Provenance and Proposition; wire
spellings above are unchanged. TopologyRequirements rows are RowTargets
{row_id, perllsp, perl_dap, vsix}, each role containing nonempty unique allowed target
strings. Native server/DAP selections must agree on target; Linux and Windows
native selections cannot reuse ID, target or digest. Preparation evidence and row
evidence name their complete selected artifact set; packet mechanisms bind the
complete inventory. Host-selection leaf evidence owns engine-floor resolution,
exact stable-version observation time and logs; this pure model does not verify
those external facts from the version string.

The existing public_beta_experience example has private validate_source_receipt
and verified_child_receipt.v1 processing. That is useful existing #5900 source
authority, not a reusable validator of this v2 three-host/24-cell denominator.
This slice neither replaces that leaf contract nor treats its wrapper status as
installed authority. Future adapters must reuse the actual leaf laws.

The historical CLI stdout lint exception is removed in favor of fallible writes;
no new source exception is needed.

## Implemented validation and proof

The candidate passed 10 shared v2 tests, all 10 retained historical example tests,
and scoped Clippy with the repository agent-clippy `-A missing_docs` allowance
(`.cargo/config.toml`). A dedicated target verified the compiled library belongs
to this checkout. An earlier shared-target example attempt failed because a stale
normal library from another checkout lacked this module; that instrument result
is retained and is not counted as a source pass.

The just-built normal example passed four external process controls: a complete
synthetic bundle exits 0 with `bundle_recommendation=ready`, permanent
`installed_qualification=not_proven`, and 89 external obligations; absent topology
input, a mismatched subject, and a missing semantic row each exit 1 without a
report. These establish validator and CLI behavior, not installed execution.
Actual installed receipts and accepted evidence adapters remain under the owners
listed above. The earlier command block records the planned proof; scoped Clippy
actually included the repository-authorized missing-docs allowance.

### Preparation inventory completeness correction

PR review identified that preparation evidence could cover a self-selected subset
of same-target inventory artifacts. Preparation now compares its declared IDs
with every artifact in the closed supplied inventory whose target equals the
preparation target. Universal VSIX artifacts remain outside native-target rows;
this preserves the existing explicit universal selection contract.

The added fallible regression removes each server, DAP and target-specific VSIX
from both a preparation row and its evidence, for both native platforms. It failed
against the original implementation with `invalid bundle accepted`; all 11 shared
v2 tests passed after the exact-set repair, including the existing universal-VSIX
positive. This corrects bundle consistency only; installed qualification is still
NOT_PROVEN and independent topology/evidence authority is still required.

### Status-contract correction

`blocked` may carry a null reason in a cell, observation, preparation row, or mechanism. `limited` and `not_proven` still require nonempty reasons. Refusal and claim-withdrawal propositions retain their separate mandatory reason requirement. The focused regression failed against the old shared status helper and passes after narrowing its reason requirement; actual CLI controls preserve `blocked` and installed `not_proven` independently.
