# `perl_parser::dead_code` API disposition ledger

<!-- GENERATED PROJECTION — do not hand-edit.
     Canonical source: `policy/dead-code-api-ledger.toml`.
     Regenerate with `cargo xtask check-dead-code-api-ledger --write`. -->

Controlling issue: #9777 (C00 under the #8062 reachability programme).
Compatibility controller: #8135.

This is a **record of current behavior**, not a specification of desired behavior.
It says what the bounded compatibility surface does today, what it cannot represent,
and which authority owns each replacement. It grants no authority of its own: no item
on this surface authorizes an edit, and no value it produces is proof that removing
code is safe.

## Export paths

3 public paths reach one module. The module is dispositioned once; the others
are projections, not separate authorities.

| Path | Role | Declared at |
| --- | --- | --- |
| `perl_parser::dead_code` | canonical | `crates/perl-parser/src/lib.rs` |
| `perl_parser::dead_code_detector` | compatibility_alias | `crates/perl-parser/src/lib.rs` |
| `perl_parser::prelude` | prelude_reexport | `crates/perl-parser/src/prelude.rs` |

- `perl_parser::dead_code` — Gated `#[cfg(not(target_arch = "wasm32"))]`; absent on wasm32.
- `perl_parser::dead_code_detector` — Backwards-compatibility alias. `crates/perl-parser/src/compat.rs` re-exports the same alias.
- `perl_parser::prelude` — Re-exports the five public types, not the two free/associated functions.

## Item dispositions

All 38 public items in the module have exactly one row. A new public item fails the
check until it is dispositioned here.

| Item | Kind | Class | Disposition | Producer | Proof ceiling | Replacement owner |
| --- | --- | --- | --- | --- | --- | --- |
| `dead_code` | module | compatibility_heuristic | retain_projection | structural | none | #8062 |
| `DeadCodeType` | enum | compatibility_heuristic | retain_projection | structural | none | #8142 |
| `DeadCodeType::UnusedSubroutine` | variant | workspace_liveness | retain_projection | produced | index_reference_count | #10935 |
| `DeadCodeType::UnusedVariable` | variant | workspace_liveness | retain_projection | produced | index_reference_count | #10935 |
| `DeadCodeType::UnusedConstant` | variant | workspace_liveness | retain_projection | produced | index_reference_count | #10935 |
| `DeadCodeType::UnusedPackage` | variant | workspace_liveness | retain_projection | produced | index_reference_count | #10935 |
| `DeadCodeType::UnreachableCode` | variant | local_cfg | retain_projection | produced | heuristic_text_scan | #8118 |
| `DeadCodeType::DeadBranch` | variant | local_cfg | retain_projection | produced | heuristic_text_scan | #8118 |
| `DeadCodeType::UnusedImport` | variant | obsolete | deprecate | never_produced | none | #8142 |
| `DeadCodeType::UnusedExport` | variant | obsolete | deprecate | never_produced | none | #10905 |
| `DeadCode` | struct | compatibility_heuristic | retain_projection | structural | none | #8142 |
| `DeadCode::code_type` | field | compatibility_heuristic | retain_projection | produced | none | #8142 |
| `DeadCode::name` | field | workspace_liveness | retain_projection | produced | none | #10860 |
| `DeadCode::file_path` | field | configuration | typed_replacement | produced | none | #10871 |
| `DeadCode::start_line` | field | compatibility_heuristic | retain_projection | produced | none | #8142 |
| `DeadCode::end_line` | field | compatibility_heuristic | retain_projection | produced | none | #8142 |
| `DeadCode::reason` | field | presentation_report | retain_projection | produced | none | #8142 |
| `DeadCode::confidence` | field | presentation_report | deprecate | produced | none | #8142 |
| `DeadCode::suggestion` | field | presentation_report | retain_projection | produced | none | #8142 |
| `DeadCodeAnalysis` | struct | compatibility_heuristic | typed_replacement | structural | none | #10935 |
| `DeadCodeAnalysis::dead_code` | field | compatibility_heuristic | retain_projection | produced | none | #10935 |
| `DeadCodeAnalysis::stats` | field | presentation_report | retain_projection | produced | none | #10935 |
| `DeadCodeAnalysis::files_analyzed` | field | compatibility_heuristic | deprecate | produced | none | #11553 |
| `DeadCodeAnalysis::total_lines` | field | presentation_report | retain_projection | produced | none | #11553 |
| `DeadCodeStats` | struct | presentation_report | retain_projection | structural | none | #10935 |
| `DeadCodeStats::unused_subroutines` | field | presentation_report | retain_projection | produced | index_reference_count | #10935 |
| `DeadCodeStats::unused_variables` | field | presentation_report | retain_projection | produced | index_reference_count | #10935 |
| `DeadCodeStats::unused_constants` | field | presentation_report | retain_projection | produced | index_reference_count | #10935 |
| `DeadCodeStats::unused_packages` | field | presentation_report | retain_projection | produced | index_reference_count | #10935 |
| `DeadCodeStats::unreachable_statements` | field | presentation_report | retain_projection | produced | heuristic_text_scan | #8118 |
| `DeadCodeStats::dead_branches` | field | presentation_report | retain_projection | produced | heuristic_text_scan | #8118 |
| `DeadCodeStats::total_dead_lines` | field | presentation_report | deprecate | produced | none | #10935 |
| `DeadCodeDetector` | struct | configuration | retain_projection | structural | none | #10935 |
| `DeadCodeDetector::new` | method | configuration | retain_projection | produced | none | #10935 |
| `DeadCodeDetector::add_entry_point` | method | configuration | deprecate | inert | none | #10871 |
| `DeadCodeDetector::analyze_file` | method | local_cfg | retain_projection | produced | heuristic_text_scan | #8118 |
| `DeadCodeDetector::analyze_workspace` | method | workspace_liveness | typed_replacement | produced | index_reference_count | #10935 |
| `generate_report` | function | presentation_report | retain_projection | produced | none | #8142 |

### What the producer states mean

- `produced` — some current code path constructs it.
- `never_produced` — advertised by the type and by the serde representation, constructed by nothing.
- `inert` — accepted and stored, never read by any code path.
- `structural` — a container or definition that is not itself produced.

Currently 2 items are `never_produced` and 1 are `inert`:

- **`DeadCodeType::UnusedImport`** (never_produced) — Advertised by the enum and by the serde representation, constructed by nothing in the workspace. `DeadCodeStats` has no counter for it and `generate_report` cannot render it, so even a hand-constructed value is invisible in the report body. Documenting this variant as implemented would be an overclaim.
- **`DeadCodeType::UnusedExport`** (never_produced) — Advertised and never constructed, exactly as `UnusedImport`. Interface exposure is a separate proposition owned by #10905; this variant cannot stand in for it.
- **`DeadCodeDetector::add_entry_point`** (inert) — The stored set is never read by any code path, so declaring entry points cannot change a single finding. The method also performs no validation: a path in no root, and a bare relative path that cannot identify a root at all, are both accepted silently. It is configuration the surface advertises and does not honour.

## Result-state mapping

For each state the reachability programme distinguishes, how the two public entry
points represent it. A collapse is recorded as a defect with an owner; it is never
accepted silently.

| Result state | analyze_file | analyze_workspace | Owner |
| --- | --- | --- | --- |
| `complete_canonical_local` | represented_lossy | represented_lossy | — |
| `complete_canonical_workspace` | not_reachable | represented_lossy | — |
| `complete_local_only_workspace_deferred` | indistinguishable_from_complete | not_reachable | #10941 |
| `partial_or_degraded` | not_reachable | indistinguishable_from_clean_empty | #10935 |
| `cancelled` | not_reachable | not_reachable | — |
| `deadline_exceeded` | not_reachable | not_reachable | — |
| `resource_exhausted` | not_reachable | not_reachable | — |
| `stale_or_superseded` | indistinguishable_from_complete | indistinguishable_from_complete | #10957 |
| `product_failure` | represented_lossy | indistinguishable_from_clean_empty | #10935 |
| `instrument_failure` | represented_lossy | indistinguishable_from_clean_empty | #10935 |
| `complete_semantics_bounded_view` | not_reachable | not_reachable | — |
| `incomplete_semantic_computation` | indistinguishable_from_complete | indistinguishable_from_complete | #10935 |

Representations:

- `represented` — the state is distinctly expressible.
- `represented_lossy` — expressible, but the row's note names what is lost.
- `indistinguishable_from_complete` — the state arrives looking like a complete result. **Defect.**
- `indistinguishable_from_clean_empty` — the state arrives looking like a clean empty result. **Defect.**
- `not_reachable` — the implementation cannot enter the state at all; it is absent, not hidden.

### Recorded defects (6)

- **`complete_local_only_workspace_deferred`** → #10941 — `analyze_file` returns a local-only result with no marker saying the workspace tier was not consulted, so a caller cannot tell a local-only answer from a complete one.
- **`partial_or_degraded`** → #10935 — Per-file errors are discarded by `analyze_workspace`, so a partially analysed workspace is byte-identical to a fully analysed one with fewer findings.
- **`stale_or_superseded`** → #10957 — The detector owns one `WorkspaceIndex` snapshot taken at construction, and the returned values carry no generation or version, so a result computed from a stale snapshot is indistinguishable from a current one.
- **`product_failure`** → #10935 — `analyze_workspace` drops the error entirely. `analyze_file` reports one, but as an untyped `String` with no typed reason.
- **`instrument_failure`** → #10935 — Nothing separates an instrument failure from a product failure: both arrive as the same untyped `String` from `analyze_file`, and as silence from `analyze_workspace`.
- **`incomplete_semantic_computation`** → #10935 — An incomplete computation returns the same shape as a complete one. This is the collapse the programme forbids: a consumer cannot refuse an incomplete result because it cannot see that it is incomplete.

### Notes on the remaining states

- **`complete_canonical_local`** — Findings are returned, but they are text-scan heuristics rather than canonical local flow facts, and carry no generation or currentness identity.
- **`complete_canonical_workspace`** — `analyze_workspace` returns a whole-workspace value, but with no root, component, SCC, production/test or interface identity. `analyze_file` has no workspace tier at all.
- **`complete_local_only_workspace_deferred`** — Complete result identity is #10941's; this surface has no tier field to carry it.
- **`partial_or_degraded`** — `files_analyzed` counts visited documents, not successful ones, so it cannot recover the difference.
- **`cancelled`** — The API is synchronous and takes no cancellation token, so the state cannot be entered. It is absent, not hidden.
- **`deadline_exceeded`** — No deadline input exists. A host-level timeout kills the caller without producing a value.
- **`resource_exhausted`** — No limit or profile input exists; `analyze_workspace` walks every document unbounded. Finite product profiles are #11590's.
- **`stale_or_superseded`** — Currentness and result-ID eligibility are #10957's.
- **`instrument_failure`** — Missing instrument evidence must not read as zero findings; today it does.
- **`complete_semantics_bounded_view`** — No truncation or bounded-view mechanism exists; output is unbounded. Bounded views are #10935's.
- **`incomplete_semantic_computation`** — Complete bounded view and incomplete semantics must stay distinct; this surface distinguishes neither.

## Consumer inventory

The check scans `crates`, `xtask/src`, `xtask/tests` and fails on any file that references this surface without a
row here, so it cannot be wired into a new path silently.

| Consumer | Class |
| --- | --- |
| `crates/perl-parser/src/lib.rs` | reexport |
| `crates/perl-parser/src/prelude.rs` | reexport |
| `crates/perl-parser/src/compat.rs` | reexport |
| `crates/perl-parser/tests/dead_code_detector.rs` | test |
| `crates/perl-parser/tests/dead_code_api_compat.rs` | test |
| `crates/perl-parser/tests/wave4_completion_absorption_tests.rs` | test |
| `crates/perl-parser/examples/workspace_refactor_demo.rs` | dormant |
| `crates/perl-lsp-rs-core/src/providers/diagnostics/dead_code.rs` | independent_implementation |

- `crates/perl-parser/src/lib.rs` — Declares `pub mod dead_code` and the `dead_code_detector` alias, both gated off wasm32.
- `crates/perl-parser/src/prelude.rs` — Re-exports the five public types.
- `crates/perl-parser/src/compat.rs` — Re-exports the module under the `dead_code_detector` compatibility alias.
- `crates/perl-parser/tests/dead_code_detector.rs` — Pre-existing behavioral tests reached through the compatibility alias.
- `crates/perl-parser/tests/dead_code_api_compat.rs` — The compatibility corpus that binds this ledger's claims to executable proof.
- `crates/perl-parser/tests/wave4_completion_absorption_tests.rs` — Absorption tests that assert the canonical path, the `dead_code_detector` alias and the alias identity. They pin the export surface itself, so retiring an export path breaks them deliberately.
- `crates/perl-parser/examples/workspace_refactor_demo.rs` — References `dead_code_detector` only inside a block comment; the example's `main` is a disabled stub. Not a live consumer and not evidence of demand.
- `crates/perl-lsp-rs-core/src/providers/diagnostics/dead_code.rs` — The LSP's user-visible dead-code diagnostics do NOT flow through this API. `detect_dead_code` reaches `WorkspaceIndex::find_unused_symbols` directly and shares no type with this module, so retiring this surface does not by itself change any diagnostic a user sees — and improving this surface does not improve that one.

Production consumers: **0**.

Paths that name the surface in order to govern it rather than consume it — the module
itself and this ledger's own tooling — are declared as `governance_paths` and are
checked for staleness:

- `crates/perl-parser/src/dead_code/mod.rs`
- `xtask/src/tasks/dead_code_api_ledger.rs`
- `xtask/src/main.rs`

## Boundaries this ledger does not resolve

- **downstream-external-consumers** — *claim:* No consumer outside this repository depends on the surface. *Why NOT_PROVEN:* The check inventories this workspace only. `perl-parser` is published, so an external dependant cannot be excluded from repository evidence alone; semver policy, not this ledger, decides whether that permits removal.
- **file-path-non-path-fallback** — *claim:* `DeadCode::file_path` can hold a raw URI string rather than a filesystem path. *Why NOT_PROVEN:* Read from source: `uri_to_fs_path(&sym.uri).unwrap_or_else(|| PathBuf::from(&sym.uri))`. No portable fixture drives the fallback branch, so the row is recorded as a source-read limitation rather than an executable control.
- **serialized-payloads-in-circulation** — *claim:* Whether any stored or transmitted payload carries `UnusedImport` / `UnusedExport`. *Why NOT_PROVEN:* Both variants derive `Serialize`/`Deserialize`, so a payload could exist outside the tree. Removal conditions for the two variants depend on this and it is not established here.

## Verification

```bash
cargo xtask check-dead-code-api-ledger
cargo test -p xtask --locked dead_code_api_ledger
cargo test -p perl-parser --test dead_code_api_compat --locked
```
