# DAP Implementation

The active implementation is the native Rust `perl-dap` server. The historical
bridge-to-native rollout plan in this page was completed and superseded; it is
not a current roadmap or dependency contract.

## Current implementation

`perl-dap` provides:

- DAP framing, request dispatch, responses, and events;
- native launch and supported attach paths;
- parser-backed source breakpoint validation;
- continue, stepping, pause, and termination where proven;
- stack, scope, variable, and evaluate projection from the local Perl debugger;
- bounded process, path, timeout, and expression-admission policy;
- a backend-neutral model for native and explicit optional debugger peers.

The product uses the local Perl interpreter to run the debuggee. Workspace Rust
crates are compiled into the adapter binary. No external language server,
linter, formatter, or bundled Perl DAP shim is required for normal debugging.

## Authoritative documents

- [DAP crate architecture](../../../docs/reference/CRATE_ARCHITECTURE_DAP.md)
- [DAP user guide](../../../docs/tutorials/DAP_USER_GUIDE.md)
- [DAP status](../../../docs/project/status/dap.md)
- [Native stack policy](../../../docs/reference/NATIVE_STACK_POLICY.md)
- [External debugger peer decisions](../../../docs/reference/EXTERNAL_DEBUGGER_PEER_DECISIONS.md)

## Evidence and remaining work

The implementation breadth is not itself the release claim. Current proof and
hardening are owned by:

- #6684 — real `perl -d` core session matrix;
- #6691 — session/reference invalidation and cleanup;
- #2301 — scopes, variables, evaluate, and mutation semantics;
- #6688 — capability truth derived from the selected backend;
- #6694 — exact packaged adapter and installed VSIX proof.

DAP remains preview until those installed and real-session contracts earn a
stronger posture.

## Historical bridge and shim plan

The 0.9 document proposed a PLS bridge and a bundled `Devel::TSPerlDAP` module.
Neither is the current implementation. PLS is a repository-only conformance
oracle, and the shim design is archived as historical context in
[`docs/archive/DAP_0_9_SHIM_DESIGN.md`](../../../docs/archive/DAP_0_9_SHIM_DESIGN.md).
