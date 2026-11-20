# perl-tidy-perltidy

The explicit external Perl::Tidy adapter. It owns external configuration,
ordered style/profile arguments, and one source-in / text-out executor used by
the LSP adapter and the deprecated `perl-lsp-perltidy` facade.

The package depends on `serde`, `thiserror`, and `perl-subprocess-runtime`.
It has no dependency on the native formatter, parser, LSP, editor positions,
or filesystem APIs. Native formatting belongs to `perl-tidy`.

## Calling boundary

Construct `LegacyPerlTidyExecutor` with a borrowed, already selected
`SubprocessRuntime`. Call `format(program, args, source)` with an explicitly
admitted executable and ordered arguments. The executor sends exact UTF-8 source
bytes on stdin, returns successful UTF-8 stdout verbatim, and withholds stdout
on any process failure or invalid output encoding. It never selects a fallback,
discovers a profile, constructs an OS runtime, or changes native configuration.

The two migrated callers keep their existing command contracts:

| Caller | Ordered command behavior |
| --- | --- |
| Deprecated text facade | `ExternalPerlTidyConfig::to_args()`, then `-st` |
| LSP external whole-document path | `-st -se`, configuration arguments, then applicable editor fallbacks |

A configured profile suppresses scalar defaults. Explicit indentation, explicit
tabs/spaces and extra arguments remain appended after the profile. The LSP
adapter adds editor indentation or tabs/spaces only when there is no profile and
the corresponding configuration field is unset. These are application policy
choices; the executor neither invents nor overwrites them.

`ExternalPerlTidyError` distinguishes injected runtime failures, missing-command
wrapper signals, tool rejection, and invalid UTF-8. Status `9009` and the existing
Windows “is not recognized as an internal or external command” message remain
compatibility heuristics. Ordinary OS-error text from an executing tool remains
a tool rejection. The LSP package owns its local `ErrorClass` implementation and
installation guidance.

## Contained legacy process seam

This extraction moves existing consumers; it does not qualify a new process
lifecycle. `LegacyPerlTidyExecutor` delegates to the existing injected runtime.
That interface cannot express execution authorization, an admitted working
directory/environment, bounded output, cancellation, process-tree cleanup, or
terminal-cause precedence. `timeout_secs` is consumed by the caller that
constructs the runtime, not enforced by this executor.

The shared `ProcessSupervisor` OS implementation, lifecycle and conformance work
remain prerequisites for replacing this route (#11078, #11082, #11085; external
consumer migration #9691/#9692). No supervisor completion or hostile-profile
safety is claimed here. Callers must admit profile paths and arbitrary arguments
under their existing trust policy.

The old facade alone retains file-in-place formatting, inclusive line-range
formatting and its source-text cache. Those compatibility APIs are not exposed
by this package and are not canonical range, provenance or freshness contracts.
