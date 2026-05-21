# PERLLSP-PROP-0002: Parser edge-gap closure

Status: proposed
Owner: parser
Created: 2026-05-21
Target milestone: parser-trust-v1
Linked specs:
- PERLLSP-SPEC-0005-parser-edge-gap-ledger
- PERLLSP-SPEC-0006-parser-boundedness-budgets
- PERLLSP-SPEC-0007-parser-impossible-case-boundaries
- PERLLSP-SPEC-0008-parser-fixture-bank
- PERLLSP-SPEC-0009-parser-gap-closeout
Linked ADRs:
- PERLLSP-ADR-0002-parser-gaps-are-fixture-ledger-backed
- PERLLSP-ADR-0003-impossible-perl-is-bounded-not-silently-claimed
Linked lanes:
- parser-edge-gap-closure

## Problem

perl-lsp has strong parser architecture and a useful differential suite, but known
parser edge gaps are spread across docs, tests, historical comparison prose, and
ad hoc PRs.

The corpus gap index already identifies missing GA coverage, NodeKind status, and
timeout/hang risks. Those gaps need a durable closure rail so parser claims are
backed by fixtures, structural assertions, boundedness tests, and explicit
impossible-case boundaries.

## Users and surfaces

Users:
- Perl developers using LSP completion, hover, diagnostics, semantic tokens, and navigation.
- Maintainers comparing v3 parser behavior to Tree-sitter and other parser targets.
- Contributors adding parser features or edge-case fixes.

Surfaces:
- `perl-parser-core`
- `perl-lexer`
- `perl-token`
- `perl-parser`
- `perl-parser-comparison`
- corpus fixtures
- parser docs
- support / claim boundaries
- parser metrics and ratchets

## Success criteria

- Every current corpus gap has a row in `policy/parser-edge-gap-ledger.toml`.
- Every P0/P1 parser gap has a fixture or an accepted-impossible boundary.
- Every timeout/hang risk has a boundedness budget.
- Every closed gap has a closeout note or ledger status.
- Parser docs distinguish correctness, bounded degradation, and impossible/runtime-only Perl.
- No parser support claim depends only on prose.
- V3 regressions are caught by fixture or boundedness tests.

## Proposed shape

Create a repo-owned parser edge-gap closure rail under `.perl-lsp-spec/`.

Add:
- a gap ledger;
- a boundedness budget file;
- fixture-bank conventions;
- structural assertion rules;
- impossible-case boundaries;
- closeout rules;
- PR-sized implementation plan.

## Alternatives considered

### Keep current docs only

Rejected. Current docs are useful but do not enforce fixture, budget, or closure
state.

### Put this in `.codex`

Rejected. `.codex` is agent execution state, not durable repo truth.

### Put this in `.spec`

Rejected. `.spec` is reserved for Spec Kit / speckit workflows.

### Merge with Track A

Rejected. Track A is parser-target fairness. Track B is production parser edge
closure. They interact but have different proof obligations.

## Evidence plan

- `cargo test -p perl-parser-comparison`
- targeted parser-core / lexer tests per fixture family
- boundedness tests with timeout assertions
- `cargo xtask check-parser-edge-gaps`
- `cargo xtask metrics parser-accuracy --check`
- `cargo xtask metrics ratchet-check parser_accuracy`
- `git diff --check`

## Risks

- Turning accepted impossible cases into false correctness claims.
- Adding fixtures without structural assertions.
- Adding boundedness tests that are flaky under CI.
- Closing gaps without updating docs/ledger status.
- Duplicating Track A parser-target comparison logic.

## Non-goals

- Does not compare current upstream Tree-sitter targets. That is Track A.
- Does not implement type inference or receiver facts.
- Does not change LSP provider behavior.
- Does not claim perfect Perl parsing.
- Does not execute source filters, `BEGIN` effects, runtime prototypes, or regex code blocks.

## Exit criteria

This proposal is complete when all linked specs are accepted, the parser edge
ledger/checker exist, fixture-bank conventions are in place, and every known
gap from the current corpus gap index is assigned a status.
