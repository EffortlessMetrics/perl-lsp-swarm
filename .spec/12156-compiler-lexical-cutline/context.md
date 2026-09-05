# Context: #12156 — compiler lexical cut-line manifest and controlled mutations

CP-LX01. This packet defines the exact proof population for the first
compiler-backed lexical references/rename cohort. It is a proof-definition PR:
no compiler, provider, plan, handler, projection, fallback, client, or
accepted-state behavior changes. The only Rust surface is the `xtask`
validator/CLI that checks the manifest; no product crate is touched.

Parent cut line/controllers: #12075 / #5218 / #8230. Claim-row authority:
#7430.

## What exists

- `manifest.json` — the canonical `compiler_lexical_cutline_cases.v1` manifest:
  16 fixtures, 43 case rows, 37 controlled mutations, 18 work invariants,
  5 named test targets. It is the proof authority; nothing in it is generated
  from current semantic/compiler/provider/rename output.
- `../../schemas/compiler_lexical_cutline_cases.v1.schema.json` — the JSON
  Schema for the wire format.
- `../../xtask/src/compiler_lexical_cutline.rs` — the validator library.
- `../../xtask/src/tasks/compiler_lexical_cutline.rs` + `main.rs` wiring —
  `cargo xtask compiler-lexical-cutline list|validate|explain <case-id>`.
- `../../xtask/tests/compiler_lexical_cutline.rs` +
  `../../xtask/tests/fixtures/compiler_lexical_cutline/` — the
  `compiler_lexical_cutline_manifest` proof target (33 tests, nonzero
  execution).

## Substrate

The manifest builds on the landed `FilePirLexicalContributionV1` envelope
(#12109, merged as `05312ba9`) and the canonical compiler-owned lexical
anchors (#12191). The envelope's `LexicalSigil`/`OccurrenceRole`/
`LexicalBindingIdentity` vocabulary is the documented INTERIM vocabulary keyed
to open #2660; this packet binds rows to that landed vocabulary as-is and does
not implement or absorb #2660. Sigils map `scalar|array|hash|code`, roles map
`declaration|read|write|modify` (`Modify` never folds into `Write`).

## Protocol lifecycle ruling (follows #12358, LSP 3.18)

The standard wire contract carries no preparation/plan token from
`textDocument/prepareRename` into `textDocument/rename`:

```text
prepareRename result = Range | {range, placeholder} | {defaultBehavior} | null
rename params        = textDocument + position + newName
```

The manifest therefore keeps four things distinct: the prior internal
preparation observation, the current rename request subject, the current
authorization and family plan, and the correlation between them. A stale prior
observation proves only that the old subject/plan cannot be reused; it never
forces refusal when fresh current re-resolution independently earns a valid
rename. The correlation outcome vocabulary is the nine-way #12358 distinction
(`no_prior_preparation` … `instrument_failure`). #12358 itself is an unlanded
contract issue; this packet follows its ruling as restated in the #12156 body
and binds rows to the named outcomes, not to any unlanded Rust type.

## Independence of expectations

Every expected anchor, reference location, occurrence/edit ID set, plan,
projection, applied set, and post-apply source is hand-authored from fixture
source geometry and operation semantics. Fixture bytes are pinned by SHA-256;
anchor byte ranges are machine-checked to select exactly the binding's
sigil+name text; UTF-16 line/character positions are machine-recomputed from
the byte offsets (the `unicode-astral-geometry` fixture makes byte columns and
UTF-16 columns diverge); rename postconditions are machine-reproduced by
applying the declared edits. Legacy/provider/text/AST output has zero positive
anchor authority here — it is a negative/comparison oracle only.

## Denominators

Admitted positive denominator (exactly): initialized same-file lexical
binding, `textDocument/references`, `includeDeclaration=false`, complete
current lexical occurrence denominator, exact #12191 compiler-owned anchors,
current #12111 contribution through #8669, current #10650 authorization, one
#12327/#8718 exact plan. 18 admitted rows cover the required bullets
(for-loop decl/read, declaration-only exact empty, read/write/modify roles,
shadowing, same-spelling-other-body, four sigils, closure capture,
Unicode/astral + CRLF geometry, edit/requery, no-prepare and matching-prepare
renames).

Exclusion/refusal/lifecycle denominator: 15 excluded rows plus 10 lifecycle
rows cover includeDeclaration=true, destructuring, package globals,
subs/methods/cross-file, typeglob/dynamic, partial facts, alias/tied/magical,
stale held requests, wrong root/configuration, not-ready, instrument failure,
unprojectable edits, old-text mismatch, name collision, malformed
observations, and the nine lifecycle/application sequences. The four
preparation postures — no-prepare, matching-prepare,
stale-prepare/fresh-success, stale-prepare/current-refusal — are distinct
rows, and `old_plan_reuse` is `forbidden` on every row.

## Work assertions

18 work invariants (WI-01..WI-18) name the stage, authority issue, subject,
assertion (`zero`/`false`/`identity`), and instrument. Unknown or
uninstrumented work is never numeric zero: zero/false/identity assertions must
name their instrument, and the final #4306 old-work zero (WI-18) is honestly
`pending` — pending is not zero, and #12157 cannot pass without it.

## Controlled mutations

37 mutations (LX-MUT-01..LX-MUT-37) mirror the issue's numbered list. Each
names its wrong behavior, its expected detection, and at least one stable row
that fails for that reason; the validator enforces bidirectional
mutation/row ownership so a mutation cannot drift away from the rows that
discriminate it.

## Test topology

Five named targets: `compiler_lexical_cutline_manifest` (active in this PR,
proof `xtask/tests/compiler_lexical_cutline.rs`), and
`compiler_references_stdio`, `compiler_rename_stdio`,
`rename_preparation_correlation`, `compiler_lexical_cutline_mutations`
(named-pending, owners #12157/#11083, registration through the unlanded
#12125–#12129 topology train). A target that only compiles, executes zero
tests, is skipped, or is missing remains NOT_PROVEN.
