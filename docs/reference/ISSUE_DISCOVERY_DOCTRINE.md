# Issue Discovery / Bug Scout Desk

> **Lane position:** upstream of the **Issue Research / Plan Review Desk** (Gate 1 — *Identify*).
> See [PIPELINE_GATES.md](PIPELINE_GATES.md), [OCTOPUS_CLUSTER.md](OCTOPUS_CLUSTER.md), and
> [ORCHESTRATION_DOCTRINE.md](ORCHESTRATION_DOCTRINE.md) for the surrounding model.

The plan-review lane answers *"Is this issue real, current, scoped, deduped, and builder-ready?"*
This lane answers a different question:

```
Where are the next real issues hiding?
```

Its job is **not** to fix, not to plan deeply, and not to flood GitHub. It finds high-signal candidate
defects, gaps, and risks from the repo's actual surfaces, then files — or hands off — only the ones with
evidence.

## Position in the swarm

```
Issue Discovery Desk → Issue Research / Plan Review Desk → Builder lane → Review / Merge lane
   find suspicious seams → verify they are not noise → file candidate issue → plan-review → build → land
```

This lane runs continuously, like radar. It produces **candidate issue packets**, not builder-ready specs.
A good packet contains enough evidence that the plan-review desk can verify it without starting from zero.

## Core doctrine — find issues by evidence, not vibes

A scout may *think* something looks suspicious, but it only files when it can say:

- here is the **source surface** (`file:line`),
- here is the **example** (minimal repro / request sequence),
- here is **why** current behavior is wrong or risky,
- here is **how to verify** it,
- here is **why it is not already covered** by an existing issue.

Optimize for **few, strong findings**. The single most important metric is not volume — it is the
**percentage of filed findings that survive plan review**. If that number is low, scouts are filing too
eagerly.

## Operating rule — discovery can batch, filing cannot

Read-only discovery may run wide and in parallel (grep, source/test inspection, CI/receipt review,
changed-file comparison, docs/source drift review). **Mutations are issue-by-issue:** file, edit, label,
close, dedupe, promote. Filing is centralized through the orchestrator so the mutation budget and dedup are
controlled by one actor that sees every scout's output at once.

> **Why centralized filing matters (first-run evidence, 2026-05-30):** two scouts' *high-confidence* DAP
> findings (stale `variablesReference`; evaluate ignores `frameId`) were already tracked as **#901** and
> **#902**. Had scouts filed directly, that's two duplicates. Centralized triage caught them before filing.

## Candidate issue packet

```md
## Finding              — one sentence
## Evidence             — Source <file:line>; Test/fixture; Receipt/CI/docs; Related issues/PRs
## Impact               — who sees it (user/maintainer/CI/tooling) and how
## Minimal reproduction — perl snippet / LSP sequence / DAP sequence / command
## Suspected root area  — file / function / boundary
## Why this is not already covered — searched terms + what you found
## Confidence           — high / medium / low
## Suggested next workflow — needs-repro / needs-plan-review / needs-architecture-review / direct-small-builder / discard
```

Do **not** overbuild the body — the plan-review desk handles full builder-ready planning.

## Scout waves (surfaces)

| Scout | Looks for |
|-------|-----------|
| **parser / AST** | overly-generic nodes, recovery nodes that mask valid syntax, modern Perl not represented, unreachable `NodeKind` variants, fixture gaps, valid-rejected / invalid-silently-accepted |
| **LSP** | stale document state, URI isolation, completion noise, hover false precision, code actions that appear but can't apply, semantic-token drift, multi-root confusion |
| **DAP** | stopped-state races, stack-frame line drift, stale frame/`variablesReference`, unsupported requests returning malformed responses, lifecycle ordering, transport framing |
| **workspace facts** | `use`/`require` resolution gaps, static-vs-dynamic misclassification, `parent`/`base`/`@ISA`, exporter/importer, goto blind spots. *Unknown is acceptable; pretending dynamic Perl is statically known is the bug.* |
| **editor UX** | activation misses, silent startup failure, settings that fail silently, diagnostics with no next step, quick-fixes that can't apply, docs that promise behavior the code doesn't deliver |
| **CI / ops** | bare self-hosted routing, path-filter holes, stale check names, cleanup blind spots, agents relying on a missing `gh` |
| **robustness** | panic surfaces, unchecked indexing, byte-boundary slicing, recursion/regex DoS, unbounded buffers, `unwrap`/`expect` in server paths |
| **docs / receipts** | status docs vs generated receipts, "complete" claims the code contradicts, stale issue/PR/branch refs, basis conflicts |

## Confidence → routing

- **High** (source evidence + concrete repro + clear impact + not already covered + specific area): file
  directly, label `needs-plan-review` + `swarm-discovered`.
- **Medium** (strong smell, partial evidence, clear next verification step): file as `needs-plan-review`
  with the verification spelled out, or hand to plan-review / source-grounding.
- **Low**: do **not** file. Record in the scout report with "what would raise confidence."

## Dedup discipline

- Search via the GitHub **MCP** tools (`mcp__github__search_issues` / `search_pull_requests` /
  `list_issues`) — **always pass `owner` + `repo`**. A bare query returns `total_count: 0`, which is *not*
  "no results." Sanity-check against a term you know exists. (`gh` is unavailable in MCP/web sessions — see
  `/scout-dedup`.)
- Dedup by **failure mode / source surface / user-visible behavior / intended fix** — **never** by shared
  file, theme, helper, or base commit alone. Two issues touching the same file are not duplicates if they
  have different failure modes or fixes.
- `.ci/blockers.yaml` is manually maintained and often stale (system-corpus only). Verify a match against
  `.ci/parser-corpus-baseline.json` and `.ci/cpan-corpus-baseline.json` before trusting it.
- Prefer **adding evidence to an existing issue** over filing a near-duplicate.

## Guardrails

- Scouts are **read-only** unless explicitly promoted. They must not build, push, open/edit PRs, close
  issues, retitle, remove labels, mark `builder-ready`, or merge/rebase.
- **No flooding:** ≤ 5 candidate packets per scout; ≤ 2 filed per scout unless clearly high-confidence.
- **No destructive action**, **no "curator says so"** (verify from source/primary artifact, not another
  agent's summary), **no high-frequency GitHub polling** (point-in-time snapshots only).
- Do **not** apply `builder-ready` — that belongs to the plan-review lane.

## Worked examples (first run, 2026-05-30)

| Outcome | Examples |
|---------|----------|
| Filed (high-confidence, novel) | #945 (feature-coverage doc drift), #952 (transport memory-DoS), #970 (multi-root completion leak), #971 (`use Foo` wrong-file goto), #983 (block-package `use` invisible) |
| Caught before filing | dup → #901/#902/#932; covered-by → #750; false-positive (`blockers.yaml`); refuted leads (didChange race, `peek_char`) |
| Surfaced for maintainer | #961 (CLAUDE.md vs `cargo metadata` provenance) |
| Tooling debt fixed | #972 (scout dedup used unavailable `gh`) |

The lane's value that run was as much in what it *didn't* file (≈7 dupes/false-positives/refutations) as in
the high-signal candidates it did.
