# Acceptance Criteria: #1715 — Rename Protocol

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| Valid plain identifier with capability value `1` | `{ "defaultBehavior": true }` | No `range` or `placeholder` fields |
| Sigiled variable | `{ range, placeholder }` | Includes the sigil; excludes `defaultBehavior` |
| Reserved Perl keyword | `null` | Keyword is not a valid rename target |
| Capability value `257` | Range/placeholder response | Out-of-range value does not wrap into `1` |
| Empty rename branch | Negotiated empty WorkspaceEdit | Uses `documentChanges` when advertised |
| Open document in `changes` | `documentChanges` entry with tracked version | Reads `DocumentState.version` |
| Non-open document in `changes` | `documentChanges` entry with `version: null` | Disk/index content is authoritative |
| WorkspaceEdit metadata | Preserved during conversion | Includes `changeAnnotations` and future top-level fields |
| Legacy client capability | Object-valued `changes` map | Omits `documentChanges` |

Local proof: `cargo test -p perl-lsp-rs --locked --test lsp_rename_tests` (24 passed).
Hosted required checks remain the merge authority.

## §Hazards

| Class | Invariant | Surface | Required adversarial test |
|---|---|---|---|
| Bounds/overflow | Unknown capability integers never wrap into a supported enum value | `capabilities.rs:prepare_support_default_behavior` | `test_prepare_rename_ignores_out_of_range_default_behavior` |
| Protocol-safety | Prepare result variants are mutually exclusive and valid for the token kind | `rename.rs:handle_prepare_rename` | `test_prepare_rename_sigiled_variable_always_returns_range_placeholder` |
| State/lock ordering | Version lookup does not reacquire the document mutex while held | `rename.rs:handle_rename_workspace_inner` | `test_rename_respects_documentchanges_client_capability` |
| Test-encodes-the-bug | Legacy and documentChanges assertions cannot pass on `null` or the wrong shape | `tests/lsp_rename_tests.rs` | `test_rename_uses_legacy_changes_without_documentchanges_capability` |
| Coverage/measurement integrity | Focused tests exercise both negotiated formats and metadata/version conversion | rename unit/integration tests | `document_changes_preserve_versions_and_workspace_edit_metadata` |
| ID/ref-space collision | N/A — no numeric ID or reference space is introduced | N/A | N/A — no new ID space |
| Scanner literal/comment blindness | N/A — no scanner or source classification logic is changed | N/A | N/A — not applicable |

Subsystem-specific defaults consulted: LSP rows in `docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md`.

## §Contracts

| Contract | Source document + section | How this change satisfies it |
|---|---|---|
| PrepareRenameResult variants | LSP 3.17, `textDocument/prepareRename` | Uses `defaultBehavior` only for valid plain targets; otherwise emits the range/placeholder variant or `null` |
| PrepareSupportDefaultBehavior value set | LSP 3.17, rename client capabilities | Accepts defined value `1` without numeric wrapping |
| WorkspaceEdit documentChanges | LSP 3.17, `WorkspaceEdit` and `TextDocumentEdit` | Converts `changes` while retaining URI, edits, versions, and metadata |
| Parser contract index | N/A — no parser contract is changed | Explicitly out of scope |

## §API-Shape

N/A — this change has no new public API surface. The conversion helper remains
private to `LspServer`, and the capability storage type is unchanged.

## §Test-Grid

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| Default behavior happy path | positive | `test_prepare_rename_returns_default_behavior_variant` | Valid plain target gets the negotiated variant |
| Default behavior shape | negative | same test | No range/placeholder fields are mixed in |
| Out-of-range capability | negative | `test_prepare_rename_ignores_out_of_range_default_behavior` | `257` does not enable default behavior |
| Keyword target | negative | `test_prepare_rename_rejects_keyword_with_default_behavior` | Reserved keyword returns `null` |
| Sigiled target | positive | `test_prepare_rename_sigiled_variable_always_returns_range_placeholder` | Exact sigil-inclusive range and placeholder |
| Legacy WorkspaceEdit | positive | `test_rename_uses_legacy_changes_without_documentchanges_capability` | Object-valued `changes` is required |
| documentChanges WorkspaceEdit | positive | `test_rename_respects_documentchanges_client_capability` | URI, version, range, and newText are retained |
| Empty edit routing | negative | existing rename empty-path coverage | Negotiated format is used for empty edits |
| Metadata and closed-document version | adversarial | `document_changes_preserve_versions_and_workspace_edit_metadata` | Metadata survives; open/closed version semantics hold |

## §Blast-Radius

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| Rename request handler | `perl-lsp-rs` | direct call | WorkspaceEdit serialization changes by capability | Covered by rename integration tests |
| Prepare-rename clients | LSP clients | protocol response | Variant and keyword behavior becomes precise | No client update required |
| Existing document state | `perl-lsp-rs` | internal state read | Open-document versions are surfaced in edits | No schema change |
| Parser/index consumers | workspace/index code | indirect | No rename discovery algorithm change | None |

Must-not-touch boundary: change-annotation generation, unrelated providers, parser
grammar, and `doctor.rs`.
