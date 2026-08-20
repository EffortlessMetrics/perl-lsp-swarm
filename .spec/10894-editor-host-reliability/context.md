# Context: #11766 — compile the shared editor-host reliability contract

## Problem

Issue #10894 is the accepted generic authority for reliable editor-host runs, but
its durable boundary is still distributed across issue prose and consumer-specific
plans. That makes two errors likely: a consumer may copy generic process and
cleanup rules, or the shared implementation may absorb editor/client semantics in
order to satisfy one proving consumer. This bundle projects the stable decisions
from #10894 into one checked, platform-neutral repository contract. It changes no
host runner, editor adapter, receipt implementation, CI route, support state, or
product behavior.

## Authority and ownership

### Shared authority: #10894

The shared substrate owns the invariants that must mean the same thing for every
consumer:

- `HostRunSubject` and exact host/candidate/run identity;
- `FreshReceiptTarget` and current-run/subject freshness;
- parent-owned deadline and graceful-to-forced settlement;
- the run-owned process domain and complete process ledger;
- independent OS-level cleanup observation;
- bounded capture/finalization and redaction metadata;
- the product, instrument, reporting, and cleanup terminal planes;
- platform capability to `pass | fail | not_proven` semantics; and
- shared negative-control helpers and the consumer-facing API.

The shared contract defines requirements, evidence, and refusal semantics. It does
not prescribe Unix process groups, Windows Job Objects, or another platform
mechanism without evidence that the mechanism establishes the contract.

### Consumer authority

Emacs/Eglot and lsp-mode, LSP4IJ, Coc, Lite XL, Vim, and DAP leaves own:

- exact editor/client/toolchain subject acquisition;
- client-native actions and host-visible observations;
- fixture and expectation membership;
- client/provider semantics and user-facing interpretation; and
- programme receipt-cell mappings, install claims, and support claims.

A consumer may adapt its editor or platform mechanics, but it may not redefine
freshness, deadline, process ownership, cleanup, artifact integrity, or outcome
plane semantics. The representative consumers named here are references to the
same contract, not copies of generic receipt policy.

## Durable laws

### Identity and freshness

Every run carries an exact repository, host, candidate, driver, schema, and run
identity. The accepted #10894 identity tuple is deliberately more specific than
names or paths selected by a caller. It includes, at minimum:

- the host and candidate executable path, content hash, and version;
- the run ID, start time, current stage, run-bound nonce, and subject digest;
- the exact schema identity, candidate identity, and driver identity; and
- a write-after-start marker proving that the receipt was produced by this run,
  rather than inherited from a previous attempt.

`FreshReceiptTarget` binds those fields to the current subject, run, and
generation. A receipt is valid only when every required field matches the
current target and its write-after-start condition. Pre-existing output, a
mutable branch name, a matching filename, a stale run ID, or a client event
cannot satisfy a current-run requirement. A receipt from the wrong executable
path, hash, or version is mismatched evidence even when its schema and run
stage look plausible. Identity mismatch before or after execution is stale
evidence, not success; the stale-receipt and wrong-executable cases are explicit
negative controls in `acceptance.md`.

### Parent-owned deadline

The parent owns one bounded deadline and records the last completed barrier before
settlement. Graceful shutdown is attempted first; forced settlement is a distinct
terminal disposition. Timeout must preserve already-observed product, instrument,
and cleanup evidence and must not discard the last completed barrier or failure
artifacts. A forced settlement is never reported as a clean normal shutdown.

### Run-owned process domain and ledger

The run owns a process domain, not merely a direct editor PID. The ledger retains
direct-host, candidate, ambient, replacement, descendant, and surviving identities
separately, including one or multiple candidate descendants. The denominator is
the exact run-owned host/candidate/descendant set required by the contract.

The platform-neutral cleanup law is:

```text
cleanup = pass
iff every exact run-owned host/candidate/descendant identity in the required
denominator is independently observed terminal or absent after bounded settlement
```

Status 0 and a client `shutdown_completed` event are product observations only;
neither proves OS cleanup. If the platform cannot establish ownership and absence
observation, the result is `not_proven`, never an inferred pass or incompatibility.

### Bounded artifact integrity

Each bounded source records reviewed equivalents of original byte count, retained
byte count, truncation flag, full-source digest where obtainable without retaining
raw content, retained-artifact digest/class, and redaction/finalization
disposition. A digest of a retained prefix is not presented as the full source
identity. Reporting failure cannot erase an already observed product, instrument,
or cleanup disposition.

### Four terminal planes

Product behavior, instrument capability, reporting/finalization, and cleanup are
independent terminal planes. A product pass with an instrument gap is not a fully
proven run; a reporting failure does not rewrite a product or cleanup observation;
and cleanup failure does not become product failure by boolean flattening. Each
plane retains its own result and evidence. Missing capability or missing
instrumentation is `not_proven` unless an independent contract explicitly proves
another result.

## Alternatives rejected

- **Client-event or exit-status cleanup:** rejected because application-level
  completion does not observe OS-level descendants or replacement processes.
- **Direct-PID-only cleanup:** rejected because known descendants may survive the
  direct editor process and remain outside the observed denominator.
- **Consumer-owned generic policy:** rejected because copies drift and allow each
  editor to redefine freshness, deadlines, artifacts, or cleanup.
- **One operating-system mechanism as the abstraction:** rejected because a
  platform capability must be proven per platform; unsupported observation is
  `not_proven`.
- **A single success boolean:** rejected because product, instrument, reporting,
  and cleanup have different evidence and remediation paths.
- **A new generic receipt authority:** rejected because #7777 and #10527 remain
  the generic durable receipt authority; this bundle only defines the #10894
  shared host-run contract.

## Consumer reachability and adoption

Representative references are deliberately bounded: Emacs/Eglot or lsp-mode,
LSP4IJ, Coc, Lite XL, and Vim/DAP host leaves can cite this bundle for shared
semantics while retaining their own client-specific contracts. The #10894
implementation must prove the abstraction with the smallest representative
consumer set; it must not rewrite every host harness. Consumer issues such as
#8734, #8644/#8658, #10685/#10704, and #10673 own conformance and semantic
falsifiers for their respective clients.

Adoption disposition is:

```text
new driver              -> shared substrate required
modified active driver  -> migrate or explicit reviewed exception
untouched legacy driver -> inventoried debt until touched
```

An exception names its owner, reason, affected invariant, evidence gap, and
review/transfer condition. Legacy inventory is not a support claim.

## Rollback, transfer, and stop conditions

Rollback removes the consumer projection or disables adoption while preserving
the shared authority and any already-observed evidence. It must not silently
restore client-local generic policy. Transfer moves ownership only with a current
subject, evidence inventory, and explicit receiving owner; otherwise the result is
`not_proven` and the work stops at the boundary.

Stop before implementation when exact identity, currentness, process ownership,
independent cleanup observation, bounded artifacts, or plane separation cannot be
established. Stop before adoption when the consumer would redefine shared rules,
copy #7777/#10527 receipt semantics, or claim support from missing capability.
Stop before promotion when any required plane is `not_proven`, stale, or lacks its
required artifact. None of these stops authorizes a host, editor, CI, or support
change in this spec-only PR.

## Deterministic checking and proof boundary

This bundle is declarative. The repository currently has no executable `.spec`
graph generator or validator, so it does not claim one. The deterministic proof
surface is structural: each required heading, ownership term, law, falsifier,
consumer, adoption disposition, rollback/transfer/stop rule, and non-goal is
checked by the commands in `checklist.md`; `git diff --check` checks whitespace.
Running that read-only checker twice against an unchanged tree must produce the
same ordered output and a byte-clean second run. A missing checker/tool is
`NOT_PROVEN`, not a green result.

## Prior art and links

- Issue: [#11766](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11766)
- Parent controller: [#9800](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9800)
- Shared architecture authority: [#10894](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10894)
- Recurrence policy: [#10899](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10899)
- Generic receipt authority: [#7777](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7777), [#10527](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10527)
- Reference runner: [#8024](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8024)
- Shared spec method: [#3983](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3983) and [`docs/reference/SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md)
- Representative consumers: #8734, #8644/#8658, #10685/#10704, #10673, and Vim/DAP host leaves

## Scope boundary

In scope: this directory's `context.md`, `acceptance.md`, and `checklist.md`.

Out of scope: host runner code, editor/client adapters, generic receipts,
workflows, CI routing, support/public claims, generated status, external process
execution, dependency changes, and any second spec or receipt framework.
