# Historical DAP 0.9 Shim Design — Superseded

- **Status**: Historical record; not product architecture
- **Original design date**: 2025-10-04
- **Superseded by**: native `perl-dap` architecture and `docs/reference/NATIVE_STACK_POLICY.md`

The original DAP greenfield plan proposed two transitional/runtime components:

1. a Rust `BridgeAdapter` that proxied DAP traffic to `Perl::LanguageServer`;
2. a first-party Perl module named `Devel::TSPerlDAP`, potentially bundled in
   the VS Code extension as a fallback runtime shim.

The design was useful for decomposing early DAP concerns—protocol framing,
process lifecycle, breakpoints, stack frames, variables, evaluate, security,
and packaging—but its product assumptions no longer apply.

## Why it was superseded

The current product ships its own native stack:

```text
IDE / DAP client
→ perl-dap native DAP frontend
→ parser-backed source truth + local perl -d runtime
→ debuggee
```

The project does not ship or require PLS, `Devel::TSPerlDAP`, Perl::Critic, or
Perl::Tidy for native debugging. The PLS runtime proxy is being removed from
published source under #6956. Release checks reject old shim payload names.

The optional ptkdb peer is not a revival of this design: `perl-dap` remains the
DAP server, the peer is explicit and unbundled, and capabilities are negotiated
per session.

## Historical ideas that remain useful

The following concerns survive in the current implementation, under different
ownership:

- parser-backed breakpoint validation;
- platform-safe process and path handling;
- bounded debugger queries and shutdown;
- typed stack, variable, and evaluation projection;
- editor/package artifact identity;
- real-session and installed-artifact proof.

## What must not be revived from this document

Do not treat this history as authority to:

- create or bundle `Devel::TSPerlDAP`;
- add a `--install-shim` command;
- restore a PLS DAP backend or `--bridge` mode;
- ship a Perl module under `resources/perl-shim/`;
- claim a runtime helper is required to make native `perl-dap` work.

A future first-party structured runtime helper is possible only if current
real-session evidence demonstrates that the direct `perl -d` boundary cannot
satisfy load-bearing native behavior. Issue #7295 owns that decision. Any new
helper requires a new ADR, protocol, threat model, package inventory, and
comparative proof. It does not inherit this module name or bundle layout.

The complete original wording remains available in repository history before
#7272.
