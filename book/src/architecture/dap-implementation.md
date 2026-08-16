# DAP Architecture

`perl-dap` is the native Debug Adapter Protocol server shipped with `perl-lsp`.
It owns the DAP frontend, editor-facing lifecycle, parser-backed breakpoint
truth, runtime-state projection, and native `perl -d` process control.

The current architecture is documented in
[`docs/reference/CRATE_ARCHITECTURE_DAP.md`](../../../docs/reference/CRATE_ARCHITECTURE_DAP.md).

## Product shape

```text
IDE / DAP client
        │ DAP
        ▼
perl-dap
  ├─ native parser and source facts
  ├─ native DAP request/event handling
  ├─ stack / scopes / variables / evaluate
  └─ local Perl debugger process
        ▼
     debuggee
```

A local Perl interpreter is required to execute the program. The product does
not require or bundle `Perl::LanguageServer`, a Perl DAP shim, Perl::Critic, or
Perl::Tidy.

## Optional peers

External debugger peers such as ptkdb are explicit, unbundled integrations.
`perl-dap` remains the DAP server. Bootstrap helpers and the experimental live
peer protocol are documented separately and do not change the native default.

## Superseded design

The former 0.9 architecture proposed a bridge-first rollout and a bundled Perl
runtime shim. It no longer describes the product and is summarized only as
history in
[`docs/archive/DAP_0_9_SHIM_DESIGN.md`](../../../docs/archive/DAP_0_9_SHIM_DESIGN.md).

Any future first-party structured runtime helper requires fresh real-session
evidence and a new reviewed architecture decision; see issue #7295.
