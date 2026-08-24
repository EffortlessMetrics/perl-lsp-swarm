# Coroutine Scope Decision

Historical origin: issue `#3539` (now an unrelated PR). `#3539` is **not** the
current coroutine owner and must not be cited as one.

## Current ownership

The coroutine/thread programme is owned by the live graph, not by this
historical issue reference:

- programme umbrella: `#8290`
- claims controller (evidence-backed coro/thread docs): `#8355`
- immediate truth repair slice: `#9076`
- deterministic support-cell generation (post-implementation): `#9080`
- implementation train: `#9026`
- DAP capability truth: `#6688`; LSP claim policy: `#6731`
- current DAP catalog: `crates/perl-dap/features_sot.toml` (projection of the
  root `features.toml` authority)

## Current truth at its exact strength

```text
DAP threads request: at most one synthetic execution context for the active session (main thread, attached process, or TCP attach; empty before any session)
CPAN Coro static LSP intelligence: not proven beyond generic Perl behavior
Coro runtime discovery/lifecycle: not proven
Coro stack/variables/evaluate: not proven
process-global execution: existing DAP behavior, separate from Coro
selected-context (single-Coro) control: not proven or unsupported
Perl interpreter-thread discovery: not proven
hypothetical core coroutine syntax: deferred pending an authoritative upstream contract
```

A mock test, capability bit, issue closure, or neighboring passing cell cannot
promote another row. Coro (CPAN userland coroutines) and Perl interpreter
threads (`threads`/`ithreads`) remain separate subjects; neither implies the
other.

## Summary of the original decision

`#3539` should not be implemented as core-language parser support in its current form.

1. **Defer core syntax work** (`coro sub`, `yield`) until upstream Perl publishes a stable, documented syntax contract.
2. **Split delivery into distinct scopes** instead of one mixed issue:
   - upstream-tracking (core status + syntax contract)
   - CPAN `Coro` static intelligence (LSP surface)
   - CPAN `Coro` runtime debugging (DAP surface)
   - Perl interpreter threads (a separate runtime subject)

## Why core syntax is deferred

The original issue draft conflated four different scopes:

- hypothetical core syntax (`coro sub`, `yield`)
- version/status claims for core Perl experimental features
- CPAN library APIs (`Coro`/`async` style workflows)
- Perl interpreter threads

These require different implementation paths and must not ship under one mixed
issue. Static CPAN intelligence (what the LSP can know about `Coro` code) is a
different claim from runtime debugging (what the debugger can discover and
control in a live process); neither is interpreter-thread support.

## Decision Checklist for Reopening Core Syntax Work

Before any core coroutine parser implementation starts, capture all of the following in the issue:

- authoritative upstream syntax documentation link(s)
- explicit version/feature gate and warning behavior
- syntax examples that define parse shape and error recovery expectations
- compatibility notes for existing identifiers named `yield` or `coro`

Without this checklist, core syntax implementation remains deferred.
