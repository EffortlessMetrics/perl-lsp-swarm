# Code-action provider-generation ledger

Machine-readable source of truth: `policy/code-action-generation-ledger.toml`.
Executable parity corpus: `crates/perl-lsp-rs/tests/code_action_generation_parity_corpus.rs`.
Drift check: `cargo xtask check-code-action-generation-ledger`.

This page freezes what production code actions do **today**, per generation and
per user-visible action family, so the convergence train can change routing
against recorded behavior instead of against assumption.

- Controlling issue: #9188 — this ledger and its parity corpus. No routing change.
- Cutover consumer: #9189 — make one generation the single publisher per family.
- Retirement: #9190 — delete retired generations and convert parity fixtures into
  canonical-route regression proof.

The ledger joins, it does not replace: the provider contract remains #8068, the
provider-selection policy remains #8392, and the remediation contract remains
#4205. Nothing here promotes a support tier or advertises a capability.

## Production order

Every `textDocument/codeAction` request enters at
`crates/perl-lsp-rs/src/runtime/dispatch/routing.rs` and is answered by
`handle_code_action` in `crates/perl-lsp-rs/src/runtime/language/code_actions.rs`.
When the document has an AST, that function invokes ten producers in a fixed
order; when it does not, it falls through to a single degraded text generation.

| Stage | Generation | Path | Reachability |
| --- | --- | --- | --- |
| 1 | `explain_diagnostic` | ast | production |
| 2 | `missing_pragmas` | ast | production |
| 3 | `native_critic` | ast | production |
| 4 | `legacy_critic` | ast | production |
| 5 | `provider_v2` | ast | production |
| 6 | `provider_original` | ast | production |
| 7 | `provider_enhanced` | ast | production |
| 8 | `disabled_extract_placeholder` | ast | production |
| 9 | `test_generator` | ast | production |
| 10 | `source_fix_all_aggregate` | ast | production |
| 11 | `text_fallback` | no_ast | production |
| — | `lsp_compat_stub` | none | unreachable_stub |

Stages 3 and 4 are mutually exclusive: the configured critic engine selects one
of them, so they never publish together.

## Disposition ledger

`Disposition` is what #9190 may do with the row, not a quality judgement.

| Generation | Family | Disposition | Retirement blockers |
| --- | --- | --- | --- |
| explain_diagnostic | quickfix:explain_diagnostic | canonical_candidate | — |
| missing_pragmas | quickfix:pragma | canonical_candidate | — |
| provider_original | quickfix:pragma | redundant_behavior | — |
| text_fallback | quickfix:pragma | compatibility_only | canonical_route_has_no_degraded_path_equivalent |
| provider_original | quickfix:diagnostic_routed | canonical_candidate | — |
| provider_v2 | quickfix:diagnostic_routed | unique_behavior | canonical_route_omits_diagnostic_association |
| native_critic | quickfix:critic_finding | canonical_candidate | — |
| legacy_critic | quickfix:critic_finding | compatibility_only | opt_in_engine_has_no_canonical_equivalent |
| provider_original | quickfix:hardcoded_shebang | canonical_candidate | — |
| provider_enhanced | refactor.extract:variable | canonical_candidate | — |
| provider_original | refactor.extract:variable | redundant_behavior | — |
| provider_enhanced | refactor.extract:subroutine | canonical_candidate | — |
| provider_original | refactor.extract:basic_fallback | shadow_only_candidate | — |
| disabled_extract_placeholder | refactor.extract:disabled_placeholder | unique_behavior | capability_gated_disabled_state_has_no_other_producer |
| provider_enhanced | refactor.rewrite:enhanced_transforms | canonical_candidate | — |
| provider_original | refactor.rewrite:enhanced_transforms | redundant_behavior | — |
| text_fallback | refactor.rewrite:text_fallback | compatibility_only | canonical_route_has_no_degraded_path_equivalent |
| provider_original | source.modernize:modernize | canonical_candidate | — |
| test_generator | source:test_generation | canonical_candidate | — |
| source_fix_all_aggregate | source.fixAll:aggregate | canonical_candidate | — |
| lsp_compat_stub | none:unreachable_stub | retire_candidate | — |

## What the inventory found

Four findings decide how much freedom #9189 and #9190 actually have.

### The enhanced generation runs twice per request

`provider_original` does not implement extraction or rewriting itself.
`CodeActionsProvider::get_code_actions` calls
`refactors::get_refactoring_actions`, which constructs its own
`EnhancedCodeActionsProvider` and makes the same
`get_enhanced_refactoring_actions(ast, range)` call the orchestrator makes
independently at stage 7. Both results are pushed, and `dedupe_code_actions`
collapses the byte-identical pair before aggregation.

So the overlap between stages 6 and 7 for the extract and rewrite families is
not two implementations that happen to agree — it is one implementation invoked
twice. Those rows are `redundant_behavior` rather than `unique_behavior`, and
retiring the stage-6 path for those families cannot change the published result.

The literal extract arms that do live in `refactors.rs` are guarded by
`actions.is_empty()`, so they only run when the nested enhanced call returned
nothing. That is `shadow_only_candidate`, not a second authority.

### Only the V2 generation associates a diagnostic with its fix

`provider_v2` and `provider_original` route an overlapping set of diagnostic
codes, so both can answer `quickfix:diagnostic_routed`. They are not
interchangeable: the V2 mapping matches each action back to the published
diagnostic by code and range and attaches it as `CodeAction.diagnostics`, while
the `provider_original` mapping emits `title`, `kind` and `edit` only.

That is why the V2 row is `unique_behavior` with a named blocker rather than
`redundant_behavior`. Cutting production to `provider_original` before it emits
the diagnostic association would silently remove the diagnostic-to-fix link that
#4205 consumers depend on.

### The degraded text generation is not a malformed-source path

`text_fallback` runs only when the request finds no AST for the current
document generation. Malformed source does not get it there: the v3
recursive-descent parser recovers, so `ParsedSnapshot::ast()` stays `Some` and
the AST-path generations answer normally. The repository already records this at
the snapshot layer — no malformed input reliably forces `ast: None`, and tests
that guarded on it were vacuous (#3760). `cac-parity-parse-error-recovery-keeps-ast-path`
pins the same fact at the protocol surface.

What remains is `current_parsed()` returning `None` because the current
generation has no published parse snapshot yet — a timing window, not a source
shape. No corpus fixture can pin that without racing the parser, so both
`text_fallback` rows carry a `proof_gap` instead of a fixture. #9189 must not
treat them as parity-proven, and #9190 should decide whether a generation
reachable only inside that window is worth keeping at all.

### Pragma authority is ordered, not resolved

`missing_pragmas` and the PL100/PL101 arms of `provider_original` produce the
same pragma quick fixes on the same request. Production relies on ordering —
the canonical generation runs first so `source.fixAll` prefers its source-aware
insertion point — and on `dedupe_code_actions` to collapse the rest. The
duplicate authority is real; only its user-visible symptom is suppressed.

## Parity corpus

`crates/perl-lsp-rs/tests/code_action_generation_parity_corpus.rs` drives the
real server through `LspHarness` and asserts hand-written expectations against
the published protocol payload. It never calls a provider directly, so it
remains valid proof after #9190 deletes a generation.

Every fixture id named by a ledger row exists in the corpus, and every fixture
in the corpus is claimed by a ledger row; `cargo xtask
check-code-action-generation-ledger` fails otherwise.

A row that the corpus cannot reach declares a `proof_gap` instead of a fixture.
A gap is a recorded `NOT_PROVEN` boundary, not coverage: the check rejects a row
that names neither, and rejects a row that claims both.

The corpus covers the outcome classes #9188 requires:

| Class | Fixtures |
| --- | --- |
| successful | `cac-parity-diagnostic-routed-quickfix-edit`, `cac-parity-pragma-quickfix-single-edit`, `cac-parity-critic-quickfix-safe-only`, `cac-parity-source-fixall-aggregates-after-dedupe` |
| disabled / refused | `cac-parity-disabled-extract-requires-selection`, `cac-parity-refused-without-disabled-support` |
| stale | `cac-parity-stale-superseded-document-version` |
| ambiguous | `cac-parity-extract-variable-requires-selection`, `cac-parity-duplicate-authority-collapsed` |
| malformed | `cac-parity-parse-error-recovery-keeps-ast-path` |
| legitimate empty | `cac-parity-legitimate-empty-out-of-range-source-action`, `cac-parity-kind-filter-excludes-other-families`, `cac-parity-unknown-document-is-empty-not-error` |
| identity without edit | `cac-parity-explain-diagnostic-command-only`, `cac-parity-test-generation-command-only`, `cac-parity-v2-attaches-originating-diagnostic` |

## Duplicate production authority is a failing contract

The drift check rejects a ledger in which two generations both claim
`canonical_candidate` for one exact family. The negative fixture proving that
rejection lives in the validator's own tests
(`rejects_two_canonical_candidates_for_one_family`), so the rule is proven
against a synthetic ledger rather than by mutating the tracked one.

## Maintaining this ledger

Run `cargo xtask check-code-action-generation-ledger`. It fails when:

- a disposition, family, or retirement blocker is outside its registry;
- a `(generation, family)` row is duplicated;
- two generations claim `canonical_candidate` for one family;
- a route names a generation that does not exist;
- a generation names a module that does not exist;
- a generation's `production_anchor` no longer occurs in the orchestrator;
- a `.rs` file under an owned module root is claimed by no generation;
- a `unique_behavior` row cites no retirement blocker;
- a non-`retire_candidate` row cites neither a parity fixture nor a `proof_gap`;
- a row claims both a `proof_gap` and parity fixtures;
- a fixture id is named by the ledger but absent from the corpus, or present in
  the corpus but claimed by no row;
- this page and the TOML disagree on any row.
