# Acceptance Criteria: #3002 - Module::Runtime document links

## §Behavior

| Input / condition | Expected result | Notes |
|---|---|---|
| `use_module('Foo::Bar')` | One deferred module link | Exact module metadata and literal-content range |
| `require_module("Baz::Qux")` | One deferred module link | Double quotes supported |
| Qualified `Module::Runtime::use_module` / `require_module` | One deferred module link | Qualified spelling supported |
| Whitespace around call and argument | One link for `use_module ( 'Foo::Bar' )` and qualified equivalents | Range remains exact |
| Two static calls on one line | Two links | No deduplication or truncation |
| Full-line or trailing comment call | No link | Comment text is not code |
| Variable, concatenated, interpolated, or malformed argument | No guessed module link | Conservative static-only boundary |
| A call-looking sequence inside a quoted string | No link | Scanner must not match inside ordinary strings |
| Receiver, other qualification, or longer identifier | No link | Only the four named call forms are supported |
| Existing `use`, bare `require`, `.pm` require, non-`.pm` require, POD, and pragma | Existing result unchanged | Regression contract |

Required proof: focused provider tests, routed LSP document-link integration test,
relevant parser/import regression test, formatting, clippy, and package checks.

## §Hazards

| Class | Invariant | Surface | Required adversarial test |
|---|---|---|---|
| Scanner literal/comment blindness | Only code literals produce links; comments do not | `compute_links` and private matcher | `module_runtime_calls_in_comments_are_ignored` |
| Delimiter pairing | A closing quote must belong to the selected literal; malformed input fails closed | private matcher | `malformed_module_runtime_literals_fail_closed` |
| Range/encoding safety | Link ranges use UTF-16 columns and do not split invalid boundaries | `make_deferred_module_link` call site | `module_runtime_link_range_is_utf16_safe` with a non-ASCII prefix |
| Test-encodes-the-bug | Tests invoke the active provider and routed request, not only parser rejection or semantic analysis | provider and LSP tests | `text_document_document_link_returns_module_runtime_link` |
| Protocol safety | Deferred link shape remains `data.type=module`, `data.module`, and existing range metadata | JSON link output | `module_runtime_link_uses_existing_deferred_shape` |
| ID/ref-space collision | N/A - no IDs, handles, or new protocol references introduced | N/A | N/A - no new ID space |
| Bounds/overflow | N/A - no numeric counters or arithmetic contract introduced | N/A | N/A - existing bounded offsets reused |
| Coverage/measurement integrity | N/A - no coverage tool or metric changed | N/A | N/A - ordinary focused proof |

Subsystem-specific defaults consulted: LSP hazards in
`docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md`; scanner hazards are included because
the implementation is a line-oriented literal matcher.

## §Contracts

| Contract | Source | Satisfaction |
|---|---|---|
| `textDocument/documentLink` response shape | Existing LSP handler and provider contract | Reuse deferred module link JSON and existing resolve path |
| UTF-16 document ranges | Existing `compute_links` range conventions | The new matcher converts byte offsets with `byte_to_utf16_col`; existing import-range debt is out of scope |
| Import-head parser boundary | `crates/perl-module/src/import/mod.rs` | No change; function-call rejection remains intact |
| Dynamic resolution boundary | Issue #3002 corrected claim | Variables/computed names produce no guessed link |

## §API-Shape

N/A - this change adds no public type, function, enum variant, dependency, protocol
field, or ID range. The matcher is private to the existing provider module.

## §Test-Grid

| Scenario | Kind | Test name | Invariant |
|---|---|---|---|
| Unqualified single quote | positive | `module_runtime_literal_calls_emit_module_links` | Basic static behavior |
| Qualified double quote | positive | `qualified_module_runtime_calls_emit_module_links` | Qualified spellings |
| Whitespace | positive | `module_runtime_calls_accept_valid_whitespace` | Spaces before `(` and around literal are valid |
| Multiple calls | positive | `module_runtime_calls_on_one_line_are_not_dropped` | Independent ranges |
| Comment | negative | `module_runtime_calls_in_comments_are_ignored` | No false positives |
| Quoted source text | negative | `module_runtime_call_inside_string_is_ignored` | No scanner matches inside strings |
| Boundary names/methods | negative | `module_runtime_call_boundaries_are_exact` | No receiver, other namespace, or longer identifier |
| Dynamic variable/concatenation | negative | `dynamic_module_runtime_calls_are_ignored` | No guessed resolution |
| Unterminated/escaped literal | adversarial | `malformed_module_runtime_literals_fail_closed` | Delimiter safety |
| Existing imports and POD | regression | `existing_document_link_forms_remain_unchanged` | No behavior regression |
| Routed request | integration | `text_document_document_link_returns_module_runtime_link` | Correct production path |

## §Blast-Radius

| Consumer | Crate | Impact | Required update |
|---|---|---|---|
| `handle_document_links` | `perl-lsp-rs` | Existing provider output gains static links | Integration proof only |
| `documentLink/resolve` | `perl-lsp-rs` | Existing module metadata consumed | No implementation change |
| `perl-module` import parser | `perl-module` | Must remain unchanged | Regression proof |
| Semantic analysis/completion | `perl-semantic-analyzer`, `perl-lsp-rs-core` | No behavior change | Existing tests remain green |
| Legacy alternate scanner | `perl-lsp-rs` | Must not gain duplicate logic | Explicit non-touch boundary |

Must-not-touch boundary: parser/import contracts, semantic/index contracts, the
legacy scanner, protocol capability declarations, and unrelated document-link forms.

## Cargo-allow/source exceptions

N/A - no source exception is needed or authorized. The lane-level proof is
`rtk cargo allow diff --base origin/main`, which must report no new exception for
this change. The full `rtk cargo allow check` is also run and its repository-wide
baseline result is recorded separately; pre-existing ledger debt is not attributed
to this issue.
