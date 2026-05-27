# Codecov Rollout

> **Context**: This document is part of perl-lsp's [Industrialized AI](why-industrialized.md) CI architecture. The choices here are responses to operating at 1000+ PRs/day, not premature optimization.

Tightens Codecov's posture in perl-lsp so it accurately reflects what's
actually uploaded, blocks only the proof signals that are ready, and remains
useful alongside the other evidence lanes.

> 2026-05-26 update: the coverage / RIPR proof lane supersedes the original
> non-blocking Codecov posture for patch coverage. Codecov patch coverage is
> now the blocking PR signal at `95%` with `0%` threshold; project coverage
> remains informational until burn-down reaches the final target and the
> Codecov project policy is promoted to blocking `95%` / `0.25%`.

> Doctrine: Codecov is **one** evidence lane alongside parser corpus, UX
> tests, `ripr`, mutation, real-Perl oracle, no-panic, file policy, and
> release readiness. It is **not** a release-readiness proof.

## What Codecov answers (and doesn't)

Codecov answers:

> Did tests execute this scoped Rust surface, and did branch coverage
> regress beyond the accepted budget?

Codecov does **not** answer:

- whether parser semantics are correct,
- whether tree-sitter behavior is correct,
- whether `@INC` / module-resolution is correct,
- whether LSP / DAP behavior is complete,
- whether CPAN corpus coverage is sufficient,
- whether mutation adequacy is strong,
- whether no-panic policy is clean,
- whether release readiness is proven.

## Current vs target

| Surface                  | Current                                                                                              | Target                                                                                  |
| ------------------------ | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| README badge             | present (`alt="code coverage"`)                                                                      | clearer alt text (`alt="Codecov parser branch coverage"`); MSRV badge synced to 1.95    |
| `codecov.yml`            | patch `95%` / `0%` blocking, project `95%` informational during burn-down, per-crate `parser` / `xtask` / `lsp` / `lexer` / `dap` / `corpus` flags, PR comments **on** | patch remains blocking; project becomes blocking only after burn-down |
| Coverage workflow        | inline in `.github/workflows/ci-nightly.yml::test-coverage`                                          | (optional, late) dedicated `.github/workflows/coverage.yml`                             |
| Coverage flag uploaded   | `parser,xtask` under the `coverage-proof` upload                                                     | keep parser and proof-rail coverage inspectable while final project coverage burns down |
| Branch-coverage ratchet  | `.ci/coverage-baseline.txt` (50.00% branch / 92.11% line / 1.00% allowed drop / 80.00% target)        | unchanged in PR ladder, calibrated only after several stable runs (PR Cov-8)            |
| Coverage receipt         | `target/receipts/quality/coverage-baseline.json` and `coverage-quality-gate.{json,md}` in the coverage job | consumed by `cargo xtask quality-gate`, with claim boundary and coverage scope inlined |
| Test Analytics           | receipt → JUnit upload in PR-fast / gate shards / UX regression lanes                                | unchanged; documented as **test telemetry**, distinct from coverage                      |
| Policy registration      | `codecov.yml` not in `policy/non-rust-allowlist.toml`                                                | added under `policy/non-rust-allowlist.toml` with `review_after` + `covered_by`         |

## PR ladder

The original Cov-* ladder below is historical context. The current coverage /
RIPR proof lane has already promoted patch coverage to a blocking `95%` / `0%`
gate and uses `coverage-baseline.json` plus `coverage-quality-gate.{json,md}`
as receipts. Do not reintroduce the earlier non-blocking patch posture or the
single `parser-branch` upload as the active plan.

Each row is one PR. Branch from clean `origin/master`. Do **not** combine.

| #     | Branch                                  | Title                                                          | Tracking      | Notes                                                                                   |
| ----- | --------------------------------------- | -------------------------------------------------------------- | ------------- | --------------------------------------------------------------------------------------- |
| Cov-1 | `ci/codecov-config`                     | `ci(codecov): quiet and scope coverage statuses`               | #8578         | Superseded by proof lane: patch is now blocking at `95%` / `0%`; project remains informational during burn-down |
| Cov-2 | `ci/coverage-receipt`                   | `ci(coverage): add parser branch coverage receipt`             | #8582         | Superseded by proof lane: `ci-nightly.yml::test-coverage` now emits `target/receipts/quality/coverage-baseline.json` and `coverage-quality-gate.{json,md}` |
| Cov-3 | `docs/codecov-lane`                     | `docs(ci): document Codecov coverage lane boundary`            | #8586         | Create `docs/ci/codecov.md` with claim boundary; reference from `docs/how-to/COVERAGE.md` if/when that doc exists |
| Cov-4 | `docs/readme-codecov-badge`             | `docs(readme): clarify Codecov badge scope`                    | merged #8541  | `alt="code coverage"` → `alt="Codecov parser branch coverage"`; MSRV badge `1.93` → `1.95` |
| Cov-5 | `ci/codecov-test-analytics-docs`        | `ci(codecov): document receipt-backed test analytics`          | #8588         | Adds a table that separates coverage vs Test Analytics vs branch ratchet (none blocking) |
| Cov-6 | `policy/codecov-files`                  | `policy(ci): register Codecov coverage surfaces`               | #8594         | Add entries for `codecov.yml`, `.github/workflows/ci-nightly.yml`, `.ci/coverage-baseline.txt` to `policy/non-rust-allowlist.toml` |
| Cov-7 | `ci/coverage-workflow` *(optional, late)* | `ci(coverage): extract parser coverage into dedicated workflow` | #8668         | Move `test-coverage` job out of `ci-nightly.yml` into `.github/workflows/coverage.yml`; remove the old job |
| Cov-8 | `ci/codecov-ratchet` *(optional, late)* | `ci(codecov): calibrate parser coverage ratchet`               | #8669         | Only after several stable runs; tune `.ci/coverage-baseline.txt` baseline/drop conservatively |

> Tracking issues filed 2026-05-11. Cross-link added via #8670.

## PR Cov-1 — `codecov.yml` shape

The active `codecov.yml` contract is patch `95%` / `0%` blocking, project
`95%` informational during burn-down, actionable `diff` / `files` PR comments,
and coverage flags that keep parser code plus the `xtask` proof rail
inspectable. The older non-blocking `parser-branch` template below is
historical context only; do not apply it to the current proof lane.

```yaml
codecov:
  require_ci_to_pass: false

coverage:
  precision: 2
  round: down
  range: "50...85"

  status:
    project:
      parser:
        target: auto
        threshold: 5%
        informational: true
        flags:
          - parser-branch

    patch:
      parser:
        target: 60%
        threshold: 25%
        informational: true
        flags:
          - parser-branch

comment: false

github_checks:
  annotations: false

flags:
  parser-branch:
    paths:
      - crates/perl-parser/src/
      - crates/perl-parser-core/src/
      - crates/perl-lexer/src/
      - crates/perl-ast/src/
      - crates/perl-ast-v2/src/
      - crates/perl-token/src/
    carryforward: true

ignore:
  - "archive/**"
  - "target/**"
  - "crates/tree-sitter-perl-c/**"
  - "crates/tree-sitter-perl-rs/**"
  - "crates/*/tests/**"
  - "crates/*/benches/**"
  - "crates/*/examples/**"
  - "crates/*/build.rs"
  - "fuzz/**"
  - "vscode-extension/**"
  - "**/*_generated.rs"
```

Do not ignore `xtask/**`: the quality-gate and receipt emitters are part of the
proof rail and must remain under patch coverage pressure.

## PR Cov-2 — `test-coverage` job changes

Inside `.github/workflows/ci-nightly.yml::test-coverage`:

1. Keep the Codecov upload flags aligned with the proof LCOV (`parser,xtask` in
   the active workflow) so the parser surface and proof rail are inspectable.
2. Keep the `test-coverage` job on every pull request, without label or path
   filters, while schedule and manual runs continue to work.
3. Keep the `codecov-action` step fail-fast once patch coverage is a required
   PR gate.
4. Keep `rtk just coverage-branch-gate` as the lean parser branch ratchet, then
   run `rtk just coverage-proof-lcov` to overwrite `lcov.info` with parser plus
   proof-rail coverage before receipt generation.
5. After the proof LCOV is present, emit
   `target/receipts/quality/coverage-baseline.json` with claim-boundary fields.
6. Run `rtk cargo xtask quality-gate --mode enforce-patch-coverage --codecov
   codecov.yml --patch-status-source codecov` against the coverage receipt so
   CI proves the local receipt is current, has positive measured LCOV lines, the
   explicit live Codecov patch policy is blocking, the required external patch
   status source is named, the receipt carries coverage scope for later final
   enforcement, that live policy is authoritative over any receipt snapshot,
   and Codecov failure comments include diff/file guidance.
   Preserve the gate's failure exit code while still appending
   `coverage-quality-gate.md` to the GitHub step summary.
7. Upload `lcov.info`, `coverage-baseline.json`, and the coverage quality-gate
   receipt/summary as artifacts after checking that each required proof file is
   present and non-empty.
8. Write a GitHub step summary listing artifact presence and the claim
   boundary in one paragraph.

Pin the `codecov/codecov-action` to the existing SHA pinned in the rest of
the workflow file — do not introduce a new floating tag.

## PR Cov-3 — `docs/ci/codecov.md`

```markdown
# Codecov

Codecov is scoped Rust execution-surface telemetry for perl-lsp.

Current uploaded coverage flags: `parser,xtask`

Current coverage scope:
- `perl-parser`
- `perl-parser-core`
- `perl-lexer`
- `perl-ast`
- `perl-ast-v2`
- `perl-token`

The lane answers: "Did tests execute this parser/proof-rail surface, did patch
coverage stay at or above 95%, and did branch coverage regress beyond the
accepted baseline budget?"

It does not answer correctness, completeness, or release readiness — see
`docs/development/RUST_1_95_ROLLOUT.md` and `docs/project/status/` for the
relevant evidence lanes.

The local branch-coverage source of truth is `.ci/coverage-baseline.txt`.
Codecov patch status is blocking at `95%`; project status remains
informational until the final coverage burn-down gate is promoted. Codecov
comments must include diff/file guidance for the proof lane.

Test Analytics is separate from coverage. CI receipts are converted to
JUnit and uploaded so gate behavior is visible without rerunning tests
solely for JUnit.
```

## PR Cov-4 — README edits

```diff
- <img src="https://codecov.io/gh/EffortlessMetrics/perl-lsp/branch/master/graph/badge.svg" alt="code coverage" />
+ <img src="https://codecov.io/gh/EffortlessMetrics/perl-lsp/branch/master/graph/badge.svg" alt="Codecov parser branch coverage" />
- <img src="https://img.shields.io/badge/MSRV-1.93-blue" alt="MSRV" />
+ <img src="https://img.shields.io/badge/MSRV-1.95-blue" alt="MSRV" />
```

Add one sentence near the badge or status section:

```markdown
Codecov is scoped parser branch-coverage telemetry, not a release-readiness or
semantic-correctness proof; see [Codecov](docs/ci/codecov.md).
```

## PR Cov-5 — Test Analytics table

Add to `docs/ci/codecov.md` and/or wherever lane docs live:

| Codecov surface       | Source                                | Meaning                       | Blocking?                  |
| --------------------- | ------------------------------------- | ----------------------------- | -------------------------- |
| Coverage badge        | `lcov.info` from `test-coverage`      | Parser branch coverage trend  | No                         |
| Patch status          | Codecov patch result                  | Changed-code coverage proof   | Yes, `95%` / `0%`          |
| Project status        | Codecov project result                | Burn-down telemetry           | No, until final promotion  |
| Test Analytics        | Receipt → JUnit uploads               | CI gate / test result viz.    | No                         |
| Branch ratchet        | `.ci/coverage-baseline.txt` + script  | Local coverage regression gate | Yes inside coverage lane  |

## PR Cov-6 — Policy registration

Add entries to `policy/non-rust-allowlist.toml`:

```toml
[[allow]]
id = "non-rust-codecov-config"
glob = "codecov.yml"
kind = "ci_coverage_config"
language = "yaml"
surface = "ci"
classification = "config"
owner = "release/ci"
reason = "Configures scoped Codecov parser branch coverage and Test Analytics behavior."
covered_by = ["cargo xtask check-file-policy", "docs/ci/codecov.md"]
created = "2026-05-11"
review_after = "2026-08-11"

[[allow]]
id = "non-rust-ci-nightly-coverage"
glob = ".github/workflows/ci-nightly.yml"
kind = "ci_workflow"
language = "yaml"
surface = "ci"
classification = "config"
owner = "release/ci"
reason = "Runs the default coverage PR gate plus scheduled/manual and label-gated expensive lanes."
covered_by = ["cargo xtask check-file-policy", "docs/ci/codecov.md"]
created = "2026-05-11"
review_after = "2026-08-11"

[[allow]]
id = "non-rust-coverage-baseline"
glob = ".ci/coverage-baseline.txt"
kind = "coverage_baseline"
language = "text"
surface = "ci"
classification = "generated-policy-snapshot"
owner = "release/ci"
reason = "Stores accepted parser branch coverage baseline and regression budget."
covered_by = ["just coverage-branch-gate", "scripts/check-coverage-baseline.sh"]
created = "2026-05-11"
review_after = "2026-08-11"
```

Adjust field names to match what the policy schema actually validates;
this is the intent, not the bit-exact representation.

## PR Cov-7 — Optional dedicated workflow

Skip this PR if `ci-nightly.yml::test-coverage` continues to be ergonomic.
Extract into `.github/workflows/coverage.yml` only if:

- Coverage cadence diverges from "nightly" or badge consumers want a clean run
  URL.
- Or workflow file-size becomes a review burden.

When extracting:

- Use `cancel-in-progress: ${{ github.event_name == 'pull_request' && github.event.action == 'synchronize' }}`.
- Trigger on every `pull_request` plus `schedule` and `workflow_dispatch`;
  patch coverage is a front-door PR gate, not a label-gated lane.
- Remove the `test-coverage` job from `ci-nightly.yml` in the same PR.

## PR Cov-8 — Optional ratchet calibration

Only after several stable `master` and default PR coverage runs with the
`parser,xtask` upload flags. Update `.ci/coverage-baseline.txt`:

- Raise `baseline_branch_coverage` only when actuals are consistently above
  it across 5+ runs.
- Lower `allowed_drop_percentage` only when noise is empirically low.
- Do **not** jump straight to the 80% long-term target.

## Acceptance gates (every PR)

```bash
# YAML parse
rtk python3 -c "import yaml; yaml.safe_load(open('codecov.yml').read())"

# Coverage lane locally
rtk just coverage-branch-gate
rtk just coverage-proof-lcov
rtk cargo xtask coverage-baseline --lcov lcov.info --receipt target/receipts/quality/coverage-baseline.json --check
rtk cargo xtask quality-gate --mode enforce-patch-coverage --coverage-receipt target/receipts/quality/coverage-baseline.json --codecov codecov.yml --patch-status-source codecov --receipt target/receipts/quality/coverage-quality-gate.json --summary target/receipts/quality/coverage-quality-gate.md --check
rtk cargo xtask fmt
rtk git diff --check
```

## PR body template

```markdown
## Summary

Adds step N of the perl-lsp Codecov cleanup.

## Current behavior
- Coverage lane:
- Codecov upload:
- Test Analytics:
- Claim boundary:

## CI economics
- Default PR impact:
- Label/manual/schedule impact:
- Branch-protection impact:
- Rollback path:

## Claim boundary

Codecov is scoped parser branch-coverage telemetry plus receipt-backed
Test Analytics. It does not prove parser semantic correctness,
tree-sitter correctness, `@INC` / module-resolution correctness, LSP/DAP
correctness, CPAN corpus adequacy, mutation adequacy, no-panic safety,
or release readiness.

## Validation
- [ ] command
- [ ] command

## Self-review
- Scope matches PR title:
- Files touched are expected:
- No duplicate coverage upload lane:
- Codecov project coverage remains non-blocking during burn-down:
- Codecov comments include actionable diff/files guidance:
- Coverage / Test Analytics distinction preserved:
- Local validation:
- CI status:
- Bot comments addressed:
- Follow-ups:
```

## Do not

- Combine Codecov work with: Rust 1.95 lint cleanup, no-panic baseline,
  file-policy rollout, provider cutover, `@INC` work, dependency bumps.
- Make Codecov project coverage branch-protection blocking before burn-down.
- Enable Codecov PR comments.
- Claim Codecov proves parser semantics, LSP / DAP behavior, `@INC`
  correctness, CPAN corpus adequacy, mutation adequacy, no-panic safety,
  or release readiness.

## References

- `docs/development/RUST_1_95_ROLLOUT.md` — parallel rollout ladder.
- `.ci/coverage-baseline.txt` — source of truth for the local ratchet.
- `scripts/check-coverage-baseline.sh`, `scripts/update-coverage-baseline.sh` — ratchet tooling.
- `.github/workflows/ci-nightly.yml::test-coverage` — current coverage lane.

## Burndown status

This section frames the Codecov rollout in the canonical rail-template
shape so it slots cleanly into the rail index alongside
`docs/development/PERL_ORACLE_RAIL.md`,
`docs/development/FILE_POLICY_RAIL.md`, and
`docs/development/CI_UX_RAIL.md`.

> **Substrate (already built)**: this rollout doc itself (#8539, merged),
> README badge fix (#8541, merged), the Codecov patch gate, the proof-LCOV
> `parser,xtask` upload, and `.ci/coverage-baseline.txt` ratchet baseline. The original umbrella
> #8508 is closed; the rail tracker is #8635.
> **Connector gap**: burn project coverage to 95%, keep the receipt/summary
> artifacts current, and document the evidence-lane boundary so Codecov never
> implicitly claims more than it measures.
> **0.14.0 upside**: contributors and reviewers can trust the Codecov
> surface as a narrow, accurate signal — parser branch coverage,
> informational — without confusing it for release-readiness proof or
> for parser semantic correctness.
>
> **Current proof-lane update**: patch coverage is now blocking at `95%` / `0%`;
> project coverage remains burn-down telemetry until final promotion.

### Status

| Phase | Issue | Builder-ready? | PR | Receipt |
|---|---|---|---|---|
| Cov-1 — scope flag | #8578 | filed by ladder agent | — | `codecov.yml` shape matches Cov-1 template above |
| Cov-2 â€” parser coverage receipt artifact | #8582 | superseded by proof lane | â€” | `rtk cargo xtask coverage-baseline` emits `target/receipts/quality/coverage-baseline.json`; `quality-gate` emits `coverage-quality-gate.{json,md}` |
| Cov-3 — coverage-lane boundary doc | #8586 | filed by ladder agent | — | `docs/ci/codecov.md` lands with claim boundary |
| Cov-5 — Test Analytics doc | #8588 | filed by ladder agent | — | doc section in `docs/ci/codecov.md` clearly separates coverage vs Test Analytics |
| Cov-6 — policy registration | #8594 | filed by ladder agent | — | `rtk cargo xtask check-file-policy --mode advisory` shows `codecov.yml` registered |

Optional, deferred until several stable runs:

| Phase | Issue | Notes |
|---|---|---|
| Cov-7 — extract dedicated workflow | not yet filed | only after Cov-1/2 are stable |
| Cov-8 — calibrate ratchet | not yet filed | only after several stable runs |

### Exit criteria

- [ ] All Cov-* phases land or are explicitly deferred with a successor.
- [ ] Receipt commands in this doc reproduce the closeout proof.
- [ ] Status doc updated (`docs/project/status/index.md`).
- [ ] Claim boundary recorded (this section).

### Claim boundary

**This rail proves**: Codecov is a scoped evidence lane with blocking patch
coverage, burn-down project coverage, local coverage receipts, and quality-gate
summaries. Cov-1/2 are superseded by the proof lane's current policy and
artifacts; Cov-3/5 document what the signal means; Cov-6 brings the config file
under the same file-policy ledger every other non-rust surface uses.

**This rail does NOT prove**:

- parser semantics are correct,
- tree-sitter behavior is correct,
- `@INC` / module-resolution is correct,
- LSP / DAP behavior is complete,
- CPAN corpus coverage is sufficient,
- mutation adequacy is strong,
- no-panic policy is clean,
- release readiness is proven.

Those are the work of parser corpus, UX tests, `ripr`, mutation,
real-Perl oracle, no-panic, file policy, and release-readiness lanes
respectively. Codecov is **one** evidence lane among many; treating it
as a release-readiness proof or a semantic-correctness proof is exactly
the failure mode this rail exists to prevent.

### Receipts

```bash
# Branch-coverage ratchet (local, primary receipt).
rtk cargo xtask coverage-ratchet

# PR-fast gate receipt (records what was actually verified pre-merge).
rtk cargo xtask gates --tier pr-fast --base origin/master --receipt

# Confirm Codecov surface is registered under file policy (Cov-6).
rtk cargo xtask check-file-policy --mode advisory

# Per-phase issue status.
rtk gh issue view 8578
rtk gh issue view 8582
rtk gh issue view 8586
rtk gh issue view 8588
rtk gh issue view 8594
```

### Related

- Umbrella issue: #8635 (rail tracker — the original #8508 is closed and serves only as historical context).
- Architecture / spec docs: this file (`docs/ci/codecov-rollout.md`); Cov-3 will add `docs/ci/codecov.md`.
- Status doc: `docs/project/status/index.md`.
- Adjacent rails:
  - `docs/development/FILE_POLICY_RAIL.md` — Cov-6 depends on the non-rust allowlist tooling landing first.
  - `docs/development/CI_UX_RAIL.md` — the PR sticky summary will reference coverage status; the contract between them is that sticky owns "what ran" and Codecov stays informational.
  - `docs/development/PERL_ORACLE_RAIL.md` — Perl-oracle tests are not in the Codecov-scoped surface; coverage there is intentionally out of scope.

### Do not combine

- Do not combine Codecov work with: Rust 1.95 lint cleanup, no-panic
  baseline, file-policy strict-mode promotion (Cov-6 lands *after*
  advisory mode is stable), provider cutover, `@INC` work,
  Perl-oracle migrations, dependency bumps.
- Do not make Codecov branch-protection blocking.
- Do not enable Codecov PR comments.
- Do not claim Codecov proves parser semantics, LSP / DAP behavior,
  `@INC` correctness, CPAN corpus adequacy, mutation adequacy,
  no-panic safety, or release readiness.

### Lane assignment

**codex** owns the Cov-* PRs. The CI-economics lane is codex /
factory-droid territory by convention; the CI-economics ladder agent
(`ac0d0d6984fa31b60`, running at file-time) is filing the Cov-* rows.
Coordinate by searching open issues with the `codecov` or `Cov-`
filename pattern before filing duplicates:

```bash
rtk gh api 'repos/EffortlessMetrics/perl-lsp/issues?state=open&per_page=100' \
  --jq '.[] | select(.title | test("(codecov|Cov-)")) | "#\(.number) \(.title)"'
```
