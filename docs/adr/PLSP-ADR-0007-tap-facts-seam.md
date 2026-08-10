# PLSP-ADR-0007: TAP result facts seam

Status: accepted  
Date: 2026-07-15  
Owner: perl-lsp maintainers  
Implementation: deferred to PR 7B
Relates to:

- [PLSP-ADR-0006](PLSP-ADR-0006-perl-workspace-core-facts-substrate.md)
- `crates/perl-workspace-core/src/test.rs`
- `crates/perl-tdd-support/src/tdd/test_runner.rs`
- `crates/perl-core-test-runner/src/main.rs`
- `crates/perl-lsp-rs-core/src/providers/testing/tap.rs`

## Context

The native tooling lane needs test intelligence in two different time
domains:

1. Source-time facts: which framework a test file uses, where its assertions
   are, and whether it declares a plan.
2. Result-time facts: which TAP assertions passed, failed, were skipped, or
   were marked TODO, including plans, bailouts, YAML diagnostics, and
   structural protocol errors.

These are related but not interchangeable. `perl-workspace-core` already
produces the first kind through its `TESTS` fact class. It must remain a pure
source-analysis substrate and must not own test execution or subprocess
state.

The current result-time implementation is split differently: the TAP parser
inside `perl-tdd-support::TestRunner` is private and coupled to execution and
LSP-shaped test results, while `perl-core-test-runner` owns execution and
emits only aggregate assertion counts in its runner record. Neither is a
stable, dependency-light result model for CLI, LSP, Kwalitee, and future test
adapters.

The existing editor-facing TAP reader in `perl-lsp-rs-core` is an additional
consumer contract that the extraction must preserve. Its relevant semantics
are:

- `TapTest.number` is the optional 1-based protocol test number, `depth` is
  derived from TAP indentation (four spaces or one tab per level), and
  `TapTest.line` is the optional 1-based source line from an `# at FILE line N.`
  diagnostic. A source line is not the same thing as the TAP stream line on
  which the record appeared.
- A buffered subtest owns its contiguous indented test records until the
  following depth-zero summary record. The reader retains both the nested
  records and summary, and `focus_subtest` attributes the immediately
  preceding nested failures to that summary without double-counting them in
  the focus result.
- Unknown or future non-comment protocol lines are non-fatal raw evidence;
  malformed records that violate a recognized grammar are structural
  diagnostics. The current reader ignores both categories, while retaining
  recognized diagnostics verbatim, so the extracted facts crate must preserve
  the distinction instead of silently losing evidence. Duplicate plans
  currently replace the stored plan, while the first parsed `file`, `line`,
  `got`, or `expected` diagnostic value wins and all diagnostic text remains
  available to callers.
- `passed()` means no hard assertion failures and no bail-out. A plan mismatch
  is reported independently by `plan_mismatch()` and does not by itself make
  `passed()` false; a skip-all plan is not a mismatch. Adapters must preserve
  these independent meanings.

## Options considered

### A. Extract a pure `perl-test-facts` crate

Move the result model and TAP parser into a dependency-free leaf crate. Test
runners produce `TapReport` values, while CLI, LSP, Kwalitee, and receipts
consume them. Existing execution crates can retain adapters and migrate their
private projections incrementally.

### B. Keep the parser in `perl-tdd-support` and expose a facts API

This minimizes crate churn, but the current crate already combines parser
helpers, execution, generated tests, and optional LSP compatibility. Exposing
the parser there would make a shared result model inherit those boundaries or
require a second dependency split later.

### C. Reject a standalone result crate

Keep each runner's parser and expose only aggregate records. This avoids a
new crate but leaves result semantics duplicated and prevents LSP/Kwalitee
from consuming assertion-level evidence consistently.

## Decision

Choose **A**. Create `perl-test-facts` as a pure TAP result-facts leaf.

The crate will:

- parse TAP text only;
- expose plans, assertion records, directives, YAML diagnostic blocks,
  bailouts, and structural diagnostics;
- preserve source line numbers and raw evidence needed by editor and receipt
  projections;
- avoid subprocesses, filesystem access, parser AST dependencies, LSP types,
  and runtime execution;
- use stable, non-exhaustive public result types suitable for CLI, LSP,
  Kwalitee, and test-runner adapters.

Execution remains in `perl-tdd-support`, `perl-core-test-runner`, and future
runtime adapters. Those crates may attach command, file, duration, and
environment evidence around a `TapReport`, but they do not redefine TAP
assertion semantics.

`perl-workspace-core::TestFact` remains the source-time model. It may later
carry a relation or receipt reference to result-time evidence, but it will not
depend on `perl-test-facts` merely to analyze source files.

## Consequences

- The historical `perl-test-facts` implementation is recreated only after
  this boundary decision, and its API can be tested independently of any
  runner.
- `perl-tdd-support` can keep its current LSP-facing `TestResult` projection
  during migration; a later adapter will map `TapReport` into that shape.
- `perl-core-test-runner` can retain its compact runner record while adding a
  structured report field or sidecar receipt without making the facts crate
  execute processes.
- Kwalitee and editor features can consume the same assertion-level result
  evidence instead of reverse-engineering stdout or aggregate counts.

## Follow-up: PR 7B

Implement the smallest pure `perl-test-facts` crate, port the existing TAP
fixtures, and add adapters only where a current consumer has a demonstrated
need. The first implementation must not broaden into test discovery,
framework-specific execution, or LSP projection.
