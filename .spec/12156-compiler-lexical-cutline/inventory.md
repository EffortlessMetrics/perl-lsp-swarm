# Inventory: #12156 — compiler lexical cut-line manifest

## Files

| Path | Role |
|---|---|
| `.spec/12156-compiler-lexical-cutline/manifest.json` | Canonical `compiler_lexical_cutline_cases.v1` proof authority |
| `schemas/compiler_lexical_cutline_cases.v1.schema.json` | JSON Schema for the manifest wire format |
| `xtask/src/compiler_lexical_cutline.rs` | Validator library |
| `xtask/src/tasks/compiler_lexical_cutline.rs` | CLI task (`list` / `validate` / `explain`) |
| `xtask/src/lib.rs`, `xtask/src/main.rs`, `xtask/src/tasks/mod.rs` | Module/command wiring |
| `xtask/tests/compiler_lexical_cutline.rs` | `compiler_lexical_cutline_manifest` proof target (33 tests) |
| `xtask/tests/fixtures/compiler_lexical_cutline/invalid-*.json` | Static invalid fixtures |

## Fixtures (16)

| ID | Purpose |
|---|---|
| `for-loop-decl-read` | `for my $i (1 .. 3) { print $i }` — required for-loop anchors |
| `read-write-modify` | declaration + read/write/modify roles |
| `declaration-only` | exact empty from complete facts |
| `nested-shadowing` | inner/outer same-name bindings |
| `same-spelling-two-bodies` | same spelling in two bodies |
| `all-sigils` | scalar/array/hash/code sigil slots |
| `closure-capture` | admitted closure capture (`$c++` in anon sub) |
| `unicode-astral-geometry` | astral literal; byte columns ≠ UTF-16 columns |
| `crlf-geometry` | CRLF line endings |
| `package-globals` | excluded `our` / fully qualified |
| `typeglob-dynamic` | excluded typeglob/symbolic/dynamic |
| `destructuring` | excluded destructuring declarations |
| `named-sub` | excluded named subs / cross-file |
| `alias-localize-tied` | excluded alias/localize/tied/magical |
| `edited-still-valid` | generation N+1 with the target still valid |
| `deleted-target` | generation N+1 with the target deleted |

## Cases (43)

- **Admitted references (16):** LX-POS-001..016 — for-loop decl/read,
  ordinary nonempty, read/write/modify roles, declaration-only exact empty,
  nested shadowing (outer/inner), same spelling in two bodies, four sigils,
  closure capture, Unicode/astral geometry, CRLF geometry,
  edit/requery + source-identical regeneration.
- **Admitted rename (2):** LX-RN-001 (no prior prepare), LX-RN-002 (matching
  prepare; returned range/placeholder never authorizes).
- **Excluded (15):** LX-EXC-001..015 — includeDeclaration=true, destructuring,
  package globals, subs/methods/cross-file, typeglob/dynamic, partial facts,
  alias/tied/magical, stale held request, wrong root/configuration, not-ready,
  instrument failure, name collision, old-text mismatch, malformed/foreign
  observation (degrades to no-prior-preparation), projection refusal.
- **Lifecycle (10):** LX-LC-001..010 — full no-prepare loop with
  didChange/reopen/requery, matching-prepare loop, stale-prepare/fresh-N+1
  success, stale-prepare/current refusal, close/reopen, cache miss/eviction,
  supersession/cancellation/deadline, client rejects edit, client applies +
  postcondition verified, rollback to pre-promotion behavior.

## Work invariants (18)

WI-01 anchors (#12191), WI-02/03 contribution inputs (#12110), WI-04
contribution sharing (#12111), WI-05 duplicate semantic builds (#12151),
WI-06 provider work (#8669), WI-07 request-time reconstruction (#12329),
WI-08..10 result union/semantic/text fallback (#8692), WI-11/12 plan
membership + ID identity (#8718), WI-13 protocol continuation fields (#12358),
WI-14..16 prepare-not-required/stale-plan reuse/runtime membership (#11083),
WI-17 projection identity (#8614), WI-18 final old-work zero (**pending**
before #4306).

## Mutations (37)

LX-MUT-01..LX-MUT-37 mirror the issue's numbered list one-for-one, each with
wrong behavior, expected detection, and bidirectionally-owned failing rows.

## Test targets (5)

| Target | Status | Owner / proof |
|---|---|---|
| `compiler_lexical_cutline_manifest` | active | `xtask/tests/compiler_lexical_cutline.rs` (this PR) |
| `compiler_references_stdio` | named-pending | #12157, registration #12125–#12129 |
| `compiler_rename_stdio` | named-pending | #12157, registration #12125–#12129 |
| `rename_preparation_correlation` | named-pending | #11083, registration #12125–#12129 |
| `compiler_lexical_cutline_mutations` | named-pending | #12157, registration #12125–#12129 |
