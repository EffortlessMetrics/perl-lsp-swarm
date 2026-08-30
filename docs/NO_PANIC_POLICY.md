# No-panic policy

`perl-lsp` treats fatal and unbounded failure paths as reliability debt. Parser,
LSP, DAP, workspace, release, and policy tooling must represent invalid external
state as a typed error, a bounded degraded result, or an explicitly contained
operation failure. A production panic is not an error-reporting strategy.

This policy distinguishes three job families that must not be collapsed into
one count:

- banning explicit fatal constructs in production;
- proving arbitrary-input parser safety and bounded work;
- converting test-only assertion and setup debt.

## Authority split

The table below expands those families into their owning authorities. The first
row is the convergence umbrella rather than a fourth family, and
`Panic-equivalent operations` is the inventory axis inside the first family. A
row whose authority is closed states a settled invariant; new work belongs to
the open authority for that surface.

| Surface | Authority | Job |
|---|---|---|
| Production convergence | [#13686](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13686) (open) | Own the complete production denominator, child migrations, containment proof, and final blocking posture. |
| Exact fatal-construct admission | [#13688](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13688) (open) | Replace message- or path-shaped exemptions with exact governed identities and receipts. |
| Panic-equivalent operations | [#13693](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13693) (open) | Inventory indexing, arithmetic, recursion, allocation, subprocess, concurrency, FFI, generated, and platform failure surfaces. |
| Test-side panic debt | [#13423](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13423) (open) | Own test targets, test helpers, test-only suppressions, and conversion cohorts. |
| Parser never-panic invariant | [#1820](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1820) (closed) | Settled corpus and fuzz invariant that the parser does not panic on the sampled inputs exercised there; it grants no new work. |
| Parser budget and cancellation proof | [#7112](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7112) (open) | Own remaining depth, work-budget, and cancellation evidence for adversarial parser inputs. |

Current explicit production migrations remain separate because their degraded
semantics are different:

- POD directive classification: [#13575](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13575) (open);
- optional LSP global-assignment action detection: [#13689](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13689) (closed; the fatal static initializer is gone);
- Test2 rename/prefix/postfix import resolution: [#13690](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13690) (open);
- heredoc anti-pattern detector initialization: [#13692](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13692) (open).

A child may consume another authority's evidence, but it must not silently take
over that authority. In particular, production convergence does not create a
second parser fuzzer or a second test-debt programme.
Corpus and fuzz evidence is sampled rather than exhaustive; it establishes the
invariant as currently held, and remaining adversarial-input proof stays with
#7112.

## Current guardrails

Current source and policy files establish the active panic-family lint bans, the
removal of the shared test unwrap carveout, and the governed path toward exact
counted no-new-debt enforcement. Historical rollout records are not permissions.

Current guardrails include:

- active panic-family Clippy bans in [`Cargo.toml`](../Cargo.toml), including
  `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, and
  `dbg_macro`;
- the governed lint ledger in
  [`policy/clippy-lints.toml`](../policy/clippy-lints.toml) together with the
  active catalog fragments in `policy/clippy-lints.d/*.toml`;
- shared Clippy configuration in [`clippy.toml`](../clippy.toml), which has no
  `allow-unwrap-in-tests` exception;
- the current lint policy in [`docs/CLIPPY_POLICY.md`](CLIPPY_POLICY.md), and
  the governed source-exception and baseline ledger for suppressions in
  [`policy/allow.toml`](../policy/allow.toml);
- the error-handling contract in
  [`docs/adr/0012-error-handling-strategy.md`](adr/0012-error-handling-strategy.md).

These controls catch important syntax families. They do not, by themselves,
prove that a long-running service is panic-free or bounded.

## Production failure semantics

### Invalid external state

Source text, protocol payloads, workspace configuration, filesystem state,
subprocess output, cancellation, and timing are external state. Invalid external
state must not be converted into an internal invariant panic.

The owning operation must instead do one of the following:

1. return a typed error at the existing boundary;
2. return a bounded degraded result whose limitation is observable;
3. cancel or fail only the affected operation under an explicit containment
   contract.

### Optional analysis

An optional detector, refactoring candidate, or advisory analyzer may become
unavailable without taking down the LSP. Its failure must remain distinguishable
from an ordinary clean result. `unavailable`, `partial`, and `complete-clean` are
not interchangeable states.

Do not replace a fatal path with `unwrap_or_default`, an empty collection, or a
silent `None` when that would make instrument failure look like domain truth.

### Source-authored invariants

A static pattern, table, or generated mapping can be source-authored and still
reside on a production path. “The literal is known good” explains why failure is
a code defect; it does not make process termination the correct product
behavior.

Where a source-authored invariant cannot be made fallible at construction, the
boundary must be reviewed explicitly and proven by a build-time or generation
check. A message embedded in `unreachable!` is not proof and cannot grant its
own exception.

### CLI and service boundaries

A command-line entry point may translate a terminal error into a non-zero process
exit. Reusable parser, LSP, DAP, workspace, and tooling libraries must return the
error instead. CLI exit semantics must not leak inward as `panic!`,
`process::exit`, or an unobserved worker failure.

An unobserved worker failure is a spawned unit of work whose terminal outcome is
never inspected by the owning operation. The rule applies to at least
`std::thread::JoinHandle` results that are dropped instead of joined,
`tokio::task::JoinHandle` values that are neither awaited nor deliberately
detached under a named contract, and `std::process::Child` handles whose
`wait`/`try_wait` status is never read. A dropped handle whose worker panicked
or exited non-zero is a silent failure, not containment; containment requires the
explicit contract in **Runtime containment**.

## Exact production admission

This section states the target contract, not current behavior. The production
fatal-construct admission owned by #13688 must be exact and counted; current
scanning is neither, which is why that issue is open. Each retained identity
must contain:

```text
path
family
selector kind
selector callee
normalized snippet
count
owner issue
reason
review or retirement condition
```

Matching must be consumptive:

1. exact temporary allowlist count slots are consumed first;
2. exact historical baseline count slots are consumed second when the selected
   policy mode permits them;
3. anything left is new debt;
4. stale entries remain visible rather than silently authorizing another site.

Message-only, directory-wide, family-only, wildcard-snippet, and count-free
exceptions are invalid. Copying an approved error message to another file must
not inherit authority. The message-shaped production exemptions still present in
`perl-ci-hygiene` are debt owned by #13688, not precedent.

A baseline refresh may drop disappeared entries. It must not absorb an unmatched
finding without an explicit reviewed reset operation. The baseline is evidence
of current debt, not permission to grow it.

The machine-readable receipt must bind its verdict to the candidate SHA and the
scanner/schema version, and record the included and excluded path denominator,
consumed identities and counts, unmatched identities and counts, gate mode, and
verdict.

## Panic-equivalent production work

Explicit macro and method bans do not cover every fatal or unbounded operation.
No complete production denominator exists yet; building it is the work owned by
#13693. That inventory must cover at least:

- byte, character, token, vector, and range indexing;
- UTF-8, UTF-16, line, column, and byte-coordinate transitions;
- integer narrowing, sentinel encoding, offset arithmetic, and profile-dependent
  overflow;
- recursion, deep walks, input-amplified loops, capacities, repeats, copies, and
  output growth;
- subprocess spawn, stdin, output drain, wait, timeout, kill, and process-tree
  cleanup phases;
- thread/task spawn and join results, lock poisoning, stale background work, and
  shutdown;
- FFI, generated code, build scripts, platform adapters, and operating-system
  assumptions.

Do not mechanically replace arithmetic with saturation or indexing with a
default. The repair must preserve the domain contract: reject, clamp, degrade,
or prove the invariant as the owning boundary requires.

## Runtime containment

A blanket `catch_unwind` around the server is not a no-panic implementation. It
can hide mixed state, stale publication, poisoned locks, orphan work, or a false
success response.

Where worker or task failure is deliberately contained, exact-process tests must
prove all of the following:

- the affected request or operation reaches one terminal outcome;
- no stale or partially committed state is published;
- current document, workspace-root, generation, session, and cancellation
  authority is preserved;
- subsequent valid work succeeds in the same process when recovery is claimed;
- shutdown remains bounded and does not orphan subprocesses or background work.

Process survival alone is not sufficient evidence.

## Test guidance

For fallible setup or helper work whose error should propagate, return `Result`
and use `?`. Do not replace propagation with a panic merely to satisfy a lint.

When a test scenario asserts that a `Result` or `Option` branch is impossible,
use the helpers owned by `perl-test-must`, such as:

```rust
use perl_test_must::{must, must_err, must_some};
```

The rule is binary for the `must*` symbols: new code must import them from
`perl-test-must`. Existing `perl_tdd_support::must*` imports are compatibility
and workspace migration state governed by #8605 and #8436. Depending on
`perl-tdd-support` for helpers it still genuinely owns remains allowed and is
governed by those same issues; adding that dependency solely to obtain `must*`
is not. [`docs/adr/0012-error-handling-strategy.md`](adr/0012-error-handling-strategy.md)
still shows a `perl_tdd_support::{must, must_some}` test example; that example
is migration residue governed by #8605 and #8436, and `perl-test-must` is the
current import site for new code.

Intentional assertion panics and explicit panic-injection tests require narrow,
reviewed exceptions at the actual panic owner. They do not make accidental panic
paths acceptable elsewhere in the target.

## Completion posture

Production convergence is complete only when the current denominator is
reproducible, every retained boundary has a positive owner and proof, all
externally reachable fatal paths have been removed or bounded, runtime
containment tests distinguish failure from clean output, and the blocking gate
has zero unowned production findings.

A falling raw count is progress evidence. It is not, by itself, completion.
