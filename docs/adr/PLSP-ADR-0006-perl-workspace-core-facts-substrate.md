# PLSP-ADR-0006: perl-workspace-core facts substrate

Status: accepted
Date: 2026-07-03
Owner: perl-lsp maintainers
Linked policy: [NATIVE_STACK_POLICY.md](../reference/NATIVE_STACK_POLICY.md)
Supersedes: n/a
Relates to:
- [PLSP-ADR-0004](PLSP-ADR-0004-lsp-stack-extraction.md) (lsp-stack extraction boundary)
- [PLSP-ADR-0005](PLSP-ADR-0005-hir-body-pir-eir-boundaries.md) (semantic body boundaries)
- `crates/perl-ripr-facts/README.md` (RIPR dependency contract)

## Context

perl-lsp grew a family of product surfaces — the LSP server, the DAP server,
native `perlcritic`/`perltidy` replacements, the RIPR facts exporter, and
Kwalitee-style distribution scoring — each of which needs the **same** picture
of a Perl project: which files exist and what role they play, which packages and
subs/methods they declare, what each file imports and exports, where the
dynamic boundaries are (string `eval`, runtime `require`, typeglob assignment,
generated methods), and how confident we are in each of those facts.

Today that picture is re-derived independently in several places:

- `perl-workspace` owns a workspace symbol index, but it depends on
  `perl-position-tracking`'s `lsp-compat` feature and (optionally) `lsp-types`,
  so it drags the editor/LSP surface into anything that consumes it. RIPR
  explicitly avoids it for exactly this reason
  (`crates/perl-ripr-facts/Cargo.toml` comment: *"perl-workspace is
  deliberately NOT used here (it transitively pulls lsp-types)"*).
- `perl-ripr-facts` re-implements file/package/sub extraction directly on top
  of `perl-parser-core` + `perl-symbol` because it must stay below the LSP
  stack.
- DAP, native critic, and native tidy each reach for parser/symbol primitives
  ad hoc.

The result is duplicated extraction logic, inconsistent ID/range/provenance
conventions, and no single place to state "this fact came from exact AST" vs
"this fact is a heuristic" vs "this is a dynamic boundary we could not resolve".

## Decision

Introduce **one LSP-free project-facts substrate**, the crate
`perl-workspace-core`, that every product surface consumes. It owns the
deterministic project model — files, packages, symbols, imports/exports,
dynamic boundaries — with **stable IDs, byte-and-line source ranges,
provenance, and confidence** on every fact.

Naming and layering follow the design in
[NATIVE_STACK_POLICY.md](../reference/NATIVE_STACK_POLICY.md):

```
perl-parser-core / perl-lexer / perl-position-tracking
perl-semantic-facts / perl-symbol / perl-module / perl-uri
                    ↓
            perl-workspace-core          ← this ADR
                    ↓
 ┌───────────────┬───────────────┬──────────────────────┐
 │               │               │                      │
perl-ripr-facts  (kwalitee     perl-tree-sitter-compat  (later)
 │                scorer)*      │
perl-workspace  →  perl-lsp-rs-core  →  perl-lsp-rs
perl-dap consumes the substrate but keeps its own runtime surface.
```

\* A substrate-consuming CPAN-`Kwalitee` *distribution* scorer is an intended
consumer (below). The existing `perl-kwalitee` crate is a **separate**
repo-release-readiness evaluator that does not consume this substrate; see the
rollout note on PR 6.

### What `perl-workspace-core` owns

- The typed project model (`ProjectModel`) and its per-fact records.
- Deterministic, host-path-free identity (`FileId`, `PackageId`, `SymbolId`).
- One internal range format (`SourceRange`: byte offsets + UTF-8 line/column).
  LSP UTF-16 positions are produced only at the LSP boundary, never stored in
  core facts.
- `Provenance` + `Confidence` + `EvidenceSource` on every fact.
- Explicit `DynamicBoundary` reporting (Perl demands honesty about `eval`,
  runtime `require`, typeglob assignment, `AUTOLOAD`, generated methods, XS).
- A `FactClasses` selector so a consumer asking for one class does not pay to
  compute unrelated ones.

### Forbidden dependencies (enforced)

`perl-workspace-core` MUST NOT depend, directly or transitively, on:
`perl-lsp-rs`, `perl-lsp-rs-core`, `perllsp`, `perl-dap`, `lsp-types`, `tokio`,
`tower-lsp`, `perl-workspace`, or any editor/transport/runtime crate. It may
depend only on the leaf facts crates (`perl-parser-core`,
`perl-position-tracking`, `perl-semantic-facts`, `perl-symbol`, `perl-uri`) and
small utilities (`serde`, `serde_json` for receipts, `walkdir`). This contract
is asserted by a test (`tests/dependency_contract.rs`) and documented in the
crate README.

### Identity + digest decision

The design proposes SHA-256 IDs. We adopt the repo's existing, dependency-free
convention instead: a deterministic **FNV-1a 64-bit** digest with an explicit
`fnv64:` prefix, matching `perl-ripr-facts`
(`crates/perl-ripr-facts/src/emitter.rs`, which records a `digest-algorithm`
limitation for exactly this reason). This keeps identity deterministic and
host-path-free without adding a crypto dependency. If a future consumer needs
collision-resistance guarantees SHA-256 provides, the `Digest`/`*Id` types are
the single place to change, and the `fnv64:`/`sha256:` prefix already
distinguishes them. **Decision recorded, not deferred.**

### Scope boundary: `source_identity.v1`

*Amended 2026-08-15 (issue #7652). The FNV-1a decision above is unchanged; this
section states what it does and does not govern.*

The decision above governs **workspace-local fact identity** — the `FileId`,
`PackageId`, and `SymbolId` values `perl-workspace-core` mints while extracting
a `ProjectModel`. Those IDs are derived within one extraction run, compared
within one process, and never need to survive as durable cross-repository
references.

It does **not** govern `source_identity.v1`, the durable source/project/content
identity vocabulary specified in #4835 and implemented by
`crates/perl-source-identity`. That vocabulary answers a different question —
*which project, root, logical source, and exact content revision is this,
across machines, checkouts, and time* — and carries requirements the
workspace-local IDs do not:

| | `perl-workspace-core` IDs | `source_identity.v1` |
|---|---|---|
| Lifetime | one extraction run | durable, cross-repository |
| Compared across machines | no | yes |
| Adversarial collision matters | no | yes |
| Digest | FNV-1a 64 (`fnv64:`) | SHA-256 (`sha256:`) |

FNV-1a is a non-cryptographic hash with no collision-resistance claim, so it
cannot satisfy #7652's requirement to "use a reviewed collision-resistant digest
for durable cross-repository identity". The two algorithms coexist without
ambiguity because both wire forms are explicitly prefixed — exactly the
migration seam this ADR anticipated.

**Owner:** `perl-source-identity` owns `source_identity.v1`.
`perl-workspace-core` was considered and rejected as the owner: it depends on
`perl-parser-core`, which places it above the layer #7652 requires the owner to
sit below. `perl-source-identity` is a Tier-1 leaf whose only job is
source/project/content/origin identity; its closure is `serde` + `sha2` and is
mechanically enforced by `crates/perl-source-identity/tests/dependency_contract.rs`
and registered in the workspace publish allowlist.

**Not aliases.** `perl-workspace-core`'s `FileId` and
`source_identity.v1`'s `LogicalSourceId` are distinct types with distinct
construction inputs and lifetimes. No adapter between them exists yet; per
#7652 one may be added only when #7604 has mapped the proposition. Consumer
migration (#7604, #7614, #7619, #7620, #7204, DAP) is tracked separately.

## Consequences

- New consumers (Kwalitee, tree-sitter-compat, future test-facts) build on one
  model instead of re-deriving extraction.
- `perl-ripr-facts` can migrate its string-scan/ad-hoc extraction onto
  `perl-workspace-core` behind a byte-identical-packet guard (a later PR; the
  dependency contract is already compatible since both avoid `lsp-types`).
- `perl-workspace` keeps its LSP-facing index but can be re-expressed as a thin
  consumer of `perl-workspace-core` over time.
- The substrate is additive: existing crates are untouched until each is
  migrated, so master stays green.

## Staged rollout (PR sequence)

This ADR is landed alongside the substrate skeleton. The full rollout is
staged so each PR is independently green and reviewable:

1. **PR 1 — this ADR + `NATIVE_STACK_POLICY.md`.** ✅ (this change)
2. **PR 2 — `perl-workspace-core` skeleton + core model** (IDs, ranges,
   provenance, confidence, file roles, dynamic boundaries, fact classes,
   errors, `ProjectModel`). ✅ (this change)
3. **PR 3 — file/package/sub/method fact extraction** (`build_project_model`
   parses files via `perl-parser-core` + `perl-symbol`, produces typed records
   with real ranges + deterministic IDs + provenance). ✅ (this change)
4. **PR 4 — module/import/compile-effect facts** (`use`/`no`/`require` as
   `ImportFact`s; `parent`/`base` → `PackageRecord.parents`; strict/warnings/
   feature/version effects via reused `perl-pragma` → `CompileEffectFacts`;
   string `eval` / runtime `require` / typeglob / source-filter → explicit
   `DynamicBoundary`s; **Exporter `@EXPORT`/`@EXPORT_OK` symbol lists →
   `ExportFact`**, completing the module interface; **`TestFact`** — test-file
   framework + assertion-count discovery for the TESTS fact class; **`PodFact`**
   — structured POD (reusing the zero-dep `perl-pod` leaf) + ranged `=head`/
   `=item` sections for the POD class; **`RelationFact`** — inherits/uses/tests
   edges synthesized over existing facts for the RELATIONS class). ✅ — **all 11
   fact classes now have producers**; `Makefile.PL`/`Build.PL`/`dist.ini`/
   `META.yml` content parsing and caller→callee edges remain follow-ups.
5. **PR 5 — rewire `perl-ripr-facts`** onto the substrate behind a
   byte-identical-packet guard.
6. **PR 6 — substrate-consuming `Kwalitee` distribution scorer.** ⏸ **not in
   this PR.** A separate, already-merged `perl-kwalitee` crate (#3309) occupies
   that name but is a *different tool* — it scores this **repo's own** release
   readiness from xtask gate results / `evidence/` surfaces and does **not**
   consume the substrate. A CPAN-`Kwalitee` scorer that reads `ProjectModel`
   (`dist.declares_version`/`dist.declares_license`, POD coverage, …) is a
   documented follow-up; it must be named to avoid colliding with the existing
   crate. (An earlier draft of this branch added such a scorer under the
   `perl-kwalitee` name; it was dropped on rebase once #3309 landed the name for
   the other tool.)
7. **PR 7 — distribution metadata facts** (`META.json` name/version/abstract/
   license/prereqs via `serde_json`; `cpanfile` prereqs via a dependency-light
   scan) on the model's `dist` facts. ✅ (this change; these are substrate facts
   in `perl-workspace-core`, independent of any scorer) —
   `Makefile.PL`/`Build.PL`/`dist.ini`/`META.yml` content parsing remains a
   documented follow-up.
8. **PR 8 — wire native critic / tidy / DAP** to read `ProjectModel`.
9. **PR 9 — `perl-tree-sitter-compat`** adapter (node/capture output over the
   native model), only once the model is stable.

Items marked ✅ have landed in this PR (PRs 1, 2, 3, 4, 7, 9), with an
adversarial multi-agent correctness review pass over the new code (8 confirmed
defects fixed, each with a regression test). The substrate is now ready to be
consumed. `perl-tree-sitter-compat` (PR 9) ships in this PR as the first
substrate consumer; the `Kwalitee` scorer (PR 6) is deferred per the note above.

**Remaining follow-ups (5, 6, 8) — deliberately separate PRs.** These migrate
or extend *existing production crates* (ripr-facts, critic, tidy, DAP) or add a
large new adapter surface, and each carries regression risk that warrants its
own reviewable PR rather than bloating the substrate PR:

- **PR 5 (rewire `perl-ripr-facts`)** — must preserve the byte-identical
  `ripr-perl-facts-v1` packet against ripr-facts' existing test suite; do it
  behind that guard as an isolated change. The dependency contract is already
  compatible (both avoid `lsp-types`).
- **PR 6 (substrate-consuming `Kwalitee` scorer)** — a CPAN-`Kwalitee`
  *distribution* scorer over `ProjectModel`, under a name that does not collide
  with the existing repo-readiness `perl-kwalitee` (#3309). Deferred so the
  naming/scope question is settled in its own PR rather than forced through a
  rebase.
- **PR 8 (wire critic / tidy / DAP)** — read-only consumption of `ProjectModel`
  in three shipped runtime crates; each should land and be verified separately
  so a regression is attributable.

**PR 9 (`perl-tree-sitter-compat`)** — ✅ landed in this PR as an additive
adapter crate: named-node `TsNode` tree (kind + byte/point ranges), S-expression
rendering (`to_sexp`), and a node-granular highlight capture map, all over the
native parser via `perl-parser-core` + `perl-workspace-core`. Token-precise
highlighting and locals/injection capture queries remain follow-ups.

This is the *re-create-over-untangle* / reviewable-PR discipline: the substrate
is proven and additive; the consuming migrations are staged so master stays
green and each change is independently attributable.
