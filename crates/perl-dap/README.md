# perl-dap

Use this crate when you need the native Debug Adapter Protocol server for Perl.

`perl-dap` is the runtime layer of the debugger stack. It speaks DAP over stdio
to editors, dispatches requests, validates breakpoints, and renders observed
runtime state for DAP-capable editors and tools. Authenticated debugger-peer TCP
is a backend transport, not an editor transport.

## Boundaries

- The native parser and source-fact stack validates breakpoints and source identities.
- Platform and shell helpers resolve the Perl executable, normalize paths, and build launch environments.
- Stack, variable, value, and evaluation modules project current debugger state into DAP types.
- The backend-neutral model supports the native runtime and explicit optional debugger peers.

## Key pieces

- `DapServer`, `DapConfig`, and `DapMode` wire the native server runtime.
- `DebugAdapter` handles request routing and protocol state.
- `TcpAttachConfig` and `BreakpointStore` support DAP attach configuration and breakpoint tracking.

## Run modes

### Native launch

```bash
perl-dap --stdio
```

Editor TCP (`--socket` / editor `--port`) is retired. Those flags still parse
and fail before bind with a `perl-dap --stdio` migration, including when
combined with `--external-peer`. Do not start `perl-dap` as an ambient DAP
listener for an editor.

## External dependencies

Native launch and DAP attach use the built-in Rust adapter plus a local Perl
installation. The Rust parser-backed runtime and workspace support crates are
compiled into the shipped `perl-dap` binary; users do not install internal
crates separately.

`Perl::LanguageServer` is not required: it is not a runtime backend, package
feature, or user prerequisite. External tools may be used in repository-only
conformance lanes, but the published crate contains no PLS process launcher or
DAP proxy.

Optional debugger peers such as ptkdb are explicit, unbundled integrations where
`perl-dap` remains the DAP server. They are not selected automatically and are
bounded by their own capability and proof status.

## Benchmarks

```bash
# Full benchmark suite (config/platform + live session groups)
cargo bench -p perl-dap --bench dap_benchmarks

# Filter to live-session groups (stable names for diffing)
cargo bench -p perl-dap --bench dap_benchmarks -- dap_live_launch
cargo bench -p perl-dap --bench dap_benchmarks -- dap_live_attach
cargo bench -p perl-dap --bench dap_benchmarks -- dap_live_session
```

Live-session benchmark function names:

- `launch_cold`
- `launch_warm`
- `attach_loopback`
- `set_breakpoints_100`
- `step_continue_p95`
- `stack_trace_live`
- `variables_root`
- `variables_child_page`
- `evaluate_safe_blocked`
- `evaluate_live_simple`
