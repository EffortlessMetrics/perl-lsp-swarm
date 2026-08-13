# IntelliJ / LSP4IJ `perl-dap` Setup and Evidence Boundary

LSP4IJ carries a generic Debug Adapter Protocol integration and Perl DAP material. That is **distribution/configuration evidence only**. It does not prove that `perl-dap` launches, binds breakpoints, exposes variables, steps correctly, or shuts down cleanly through an IntelliJ-platform IDE.

> **Current support boundary:** LSP and DAP are separate subjects. `perllsp` working through LSP4IJ does not prove `perl-dap`. Actual debugger support is promoted only from the exact-host receipt tracked by [#7877](https://github.com/EffortlessMetrics/perl-lsp/issues/7877) and the support registry in [#7122](https://github.com/EffortlessMetrics/perl-lsp/issues/7122).

## Subject identity

Record these independently for every debugger run:

```text
IDE product + exact version/build
LSP4IJ exact version/plugin identity
platform + architecture
DAP template stage
perl-dap exact version/build/source SHA/hash
adapter installation route
Perl interpreter/runtime used by the debuggee
workspace/fixture identity
adapter PID
intended debuggee PID
```

Another debug adapter, another Perl debugger plugin, or the language-server process cannot satisfy this subject.

## Template and installation stages

Keep these stages separate:

1. **Local imported corrected DAP template + local/external exact `perl-dap`** — useful for pre-upstream verification.
2. **Released LSP4IJ built-in DAP template + exact external/local `perl-dap`** — proves the released template independently of managed installation.
3. **LSP4IJ-managed public artifact** — proves the LSP4IJ installer actually selected and installed the expected release archive rather than reusing PATH.
4. **Future corrected released built-in template** — requires a later upstream LSP4IJ release containing the reviewed correction and a fresh real-host receipt.

Selecting a built-in template is not proof that the managed downloader ran.

Platform, architecture, Linux libc disposition, release-asset identity, and checksum-manifest limitations come from the LSP4IJ installer contract in [#7876](https://github.com/EffortlessMetrics/perl-lsp/issues/7876).

## Adapter identity

Before debugging semantics, prove the adapter you intended to test is the process that launched.

For a local/external binary, record at least:

```text
path
version/build identity
source SHA or binary hash where available
```

For an LSP4IJ-managed public artifact, additionally record:

```text
release/tag
archive name/hash
installer/template digest
resolved executable path
fallback_used = false
external_path_candidate_present = false for the managed-install receipt
```

A PATH-resolved adapter cannot satisfy the managed-install cell.

## Launch is the first required debugger journey

The first supportable debugger cohort is **launch**. A passing launch receipt must exercise the actual IntelliJ/LSP4IJ debugger path through this sequence:

```text
initialize
launch
configuration sequencing
plain source breakpoint
breakpoint verification/disposition
continue/run
stopped at the intended source
stackTrace
scopes
variables
evaluate when advertised and supported
one next/step operation
continue or terminate
adapter cleanup
debuggee cleanup
```

A debugger window opening or a breakpoint icon appearing is not enough.

## Launch configuration

Use only fields the actual LSP4IJ DAP template/client under test consumes. Do not copy VS Code `launch.json` fields by resemblance.

The first proven launch configuration should document the exact semantics of the fields that materially affect execution, such as:

```text
program/script
cwd
Perl interpreter selection where the template exposes it
arguments
environment only where the template safely exposes it
source/path assumptions
```

Until [#7877](https://github.com/EffortlessMetrics/perl-lsp/issues/7877) produces a passing exact-host launch receipt, treat launch-field examples as candidate configuration rather than a public support guarantee.

## Breakpoint cells

Track breakpoint types independently:

| Cell | Promotion rule |
| --- | --- |
| Plain source breakpoint | LSP4IJ action exists + `perl-dap` supports it + actual session verifies and stops at the intended source |
| Conditional breakpoint | independent actual-host cell |
| Exception breakpoint | independent actual-host cell |
| Function breakpoint | independent actual-host cell |

Do not infer advanced breakpoint types from a plain breakpoint, UI presence, or protocol handler existence.

## Source and path identity

Breakpoint and frame evidence must make wrong-source satisfaction difficult.

The real-host fixture should include a same-named Perl file in another directory/root so these assertions are load-bearing:

- the breakpoint binds to the intended file;
- the stopped frame resolves to that same logical source;
- a same-named file elsewhere cannot satisfy the receipt;
- source canonicalization remains coherent on the tested platform.

Only document Unicode, spaces, Windows path behavior, or multi-root mapping for platforms where the actual host exercised them.

## Stack, scopes, variables, and evaluate

A `stopped` event is not the end of the proof.

For the first launch receipt, confirm through the actual IntelliJ debugger model:

1. `stackTrace` returns the expected source/frame;
2. `scopes` exposes the expected scope set;
3. `variables` exposes expected program state;
4. `evaluate` is exercised when the selected `perl-dap` backend/capability advertises it;
5. one `next`/step operation advances source/state as expected.

If evaluate or another action is not supported by the selected backend, record that exact limitation rather than turning it into a whole-session failure.

## Launch versus attach

Launch and attach are separate support cells.

### Launch

Required for the first debugger support claim.

### Attach

Attach is optional and independent. It may remain `not_proven` or unsupported while launch is valid.

A future attach receipt must prove:

- the exact intended target/process or transport was selected;
- stack/source identity matches the target;
- detach/disconnect behavior is correct;
- the attached target is not killed unexpectedly unless the contract explicitly requires that behavior.

Never reuse launch evidence as attach proof.

## Capability boundary

For each debugger feature, the supported cell is the intersection of:

```text
LSP4IJ exposes the action
- perl-dap advertises/supports it
- selected backend/runtime supports it
- the actual [#7877](https://github.com/EffortlessMetrics/perl-lsp/issues/7877) session passes
```

A visible IntelliJ action does not override `perl-dap` capability truth from #6688.

## Cleanup

A working debugger receipt includes process cleanup.

After normal completion, terminate/disconnect, and IDE shutdown where relevant, confirm:

```text
perl-dap adapter is gone
intended debuggee is gone or preserved according to the requested operation
no stale adapter is reused by the next run
```

An orphaned adapter or debuggee is a failed/limited cleanup cell even if earlier debugger actions passed.

## Troubleshooting

### Adapter never starts

1. Record IDE/LSP4IJ versions.
2. Record template stage and installation route.
3. Prove the selected `perl-dap` path/version/hash.
4. Check the LSP4IJ DAP/client log separately from adapter/debuggee output.

### Wrong `perl-dap` binary starts

Treat this as subject-identity failure. Do not continue semantic verification against the wrong adapter and relabel the result later.

### Launch starts but initialization/configuration stalls

Separate:

```text
adapter process started
DAP initialize completed
launch request completed
configuration sequencing completed
```

Identify the first missing transition.

### Breakpoint is not verified

Check source/path identity, requested line, template mapping, and adapter capability before debugging variables or stepping.

### Breakpoint is verified but never hit

Confirm the intended debuggee/script is running and that the expected code path executes. A verified breakpoint that never produces the intended stop does not satisfy the source-breakpoint cell.

### Stopped at the wrong source

Compare the frame source with the intended fixture and the same-named-file discriminator. Do not accept basename-only agreement.

### Variables or evaluate are unavailable

Distinguish client UI exposure, `perl-dap` capability/backend support, and session failure. Record the narrowest failing cell.

### Step does not advance as expected

Verify the selected frame/source and actual program state before treating the problem as a generic stepping defect.

### Adapter or debuggee remains orphaned

Record which process survived which shutdown path. Cleanup is part of the support receipt, not a separate optional polish item.

## Coexistence with the language server and other Perl plugins

`perllsp` and `perl-dap` are separate processes and protocols even when LSP4IJ distributes both templates.

Keep these ownership questions independent:

```text
which integration owns Perl language features?
which debugger integration owns breakpoints/run configuration?
are duplicate debugger integrations competing?
```

A debugger failure does not automatically invalidate a passing LSP row, and a passing LSP row does not promote DAP.

## Support-state vocabulary

Use exact cell states rather than “JetBrains debugger supported”:

```text
proven
limited
client_not_exposed
not_proven
unsupported
```

The registry must bind the state to exact host/plugin/adapter/template/install/platform subjects and invalidate it when those subjects change materially.

## Upstream documentation boundary

The desired LSP4IJ DAP material maintained under #7772 should eventually carry only the behavior and examples proven here:

```text
adapter command/binary identity
installer behavior
file mappings
launch example
attach example only if proven/retained
cwd/program/interpreter assumptions
supported and limited debugger cells
```

Repository automation may prepare that upstream delta, but external submission remains a manual maintainer action.

## Related documentation

- [IntelliJ IDEA / LSP4IJ Setup](INTELLIJ_IDEA_SETUP.md)
- [Installation](../how-to/INSTALLATION.md)
- [Troubleshooting](../how-to/TROUBLESHOOTING.md)
- DAP capability truth: #6688
- actual LSP4IJ debugger receipt: [#7877](https://github.com/EffortlessMetrics/perl-lsp/issues/7877)
- installer/topology contract: [#7876](https://github.com/EffortlessMetrics/perl-lsp/issues/7876)
- support registry: [#7122](https://github.com/EffortlessMetrics/perl-lsp/issues/7122)
