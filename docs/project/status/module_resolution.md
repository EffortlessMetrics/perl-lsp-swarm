# @INC / Module Resolution Conformance

This page is current support authority for the **Selected static @INC consumer rail**.
It tracks live LSP module-resolution behavior for that rail's four provider
consumers. It is distinct from HIR compiler-substrate module-request facts,
which are tracked in [compiler_facts.md](compiler_facts.md) and
[#8242](https://github.com/EffortlessMetrics/perl-lsp/issues/8242).

This page does **not** claim complete effective-root authority; it is
not complete effective-root authority. Historical selected-rail closeouts from
2026-05-11 remain receipts for the Scenario 14 denominator; they are not current
proof of the broader module programme.

#8479 / #7460 generated claim identities are not on `main` (those issues remain
open). The table below is therefore denominator-bound to Scenario 14 plus exact
issue owners. It is not a generated row count and must not be hand-counted as
if those identities already existed.

## Current claim boundary

| Level | Current state | Denominator / evidence | Promotion owner |
|---|---|---|---|
| Selected static consumer rail | proven | Scenario 14 four consumers (PL701 diagnostic, completion, goto-definition, hover) × the named resolution modes; harness `ux_scenario_14_inc_conformance` | this page; [SUPPORT_TIERS.md](SUPPORT_TIERS.md) module-resolution row |
| Contextual resolver authority | not_proven | validated request; source mutation facts/effects; accepted environment roots; document ownership; effective root composition; candidate report; selected-source open-revision overlay | M04 [#10568](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10568)–[#10572](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10572); M07 [#10573](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10573)/[#10575](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10575)/[#10578](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10578)/[#8170](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8170) |
| Provider/product support | not_proven | definition, completion, hover, diagnostics, symbols, refactors, installed VS Code, other clients/platforms — independent owners; not implied by the selected rail | [#1744](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1744) / [#4243](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4243) and exact consumer owners |
| Exact-process | not_proven | `#11624` profiles under #9270 (`module_exact_process_resolution_core`, `module_exact_process_semantic_edit`, `module_exact_process_full_closeout`) | [#9270](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9270) / [#11624](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11624) |
| Dynamic / unsupported | bounded | hooks, arbitrary runtime `@INC` mutation, and project code remain non-executing and may correctly produce bounded/refused outcomes | M04E [#10572](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10572) |

A lower row cannot make a higher row pass. Issue closure, spec existence, or
helper/unit tests do not promote public claims. Stronger public claims remain
#9270 plus provider/installed owners.

When this page says "all consumers", it means those four Scenario 14 consumers,
not every module-resolution consumer in the repository.

**Test**: `cargo test -p perl-lsp-ux-tests --test ux_scenario_14_inc_conformance -- --nocapture`

## Selected static @INC consumer rail

### Consumer consistency matrix (Scenario 14 receipt)

Each cell indicates whether the named Scenario 14 consumer agrees on module
resolution for the given mode. A `+` means the consumer produced the expected
answer (resolved or not-resolved consistently). A `-` means the consumer
diverges or the feature is not yet fully enforced.

**Fixture semantics**: completion uses prefix fixtures (`use Gre<cursor>`);
PL701, goto-definition, and hover use exact-module fixtures (`use GreetModule;`).

| Resolution Mode | PL701 diagnostic | completion | goto-definition | hover | Notes |
|---|---|---|---|---|---|
| Workspace `includePaths` | + | + | + | + | Config-driven: `includePaths: ["lib"]` |
| Absolute `includePaths` | + | + | + | + | Config-driven: absolute path entry |
| Lexical `use lib` | + | + | + | + | In-source pragma extraction |
| `no lib` cancellation | + | + | + | + | Position-aware negative; the four Scenario 14 consumers enforce #8516 |
| FindBin-relative | + | + | + | + | `$FindBin::Bin/lib` pattern |
| PERL5LIB env | + | + | + | + | `usePerl5lib=true` gates PERL5LIB |
| interpreter startup `@INC` | + | + | + | + | `useSystemInc=true` gates interpreter startup paths |

**Key**: Consumer cells are `+` (consistent) or `-` (divergent / unimplemented).
Conformance means the four Scenario 14 consumers (PL701 diagnostic, completion,
goto-definition, hover) agree on the named mode — not that every `@INC` mode,
consumer, root family, or provider surface resolves.

Current harness functions for this rail live in
`crates/perl-lsp-ux-tests/tests/ux_scenario_14_inc_conformance.rs`. Absolute
`includePaths` and interpreter-startup resolution remain historical selected-rail
receipts in the matrix above; the current file also proves PERL5LIB/`useSystemInc`
gating independence and unauthorized `externalIncludePaths` zero-visibility
([#4998](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4998)). That
gating proof is not complete system-`@INC` support.

## Historical selected-rail closeout — no-lib workspace-index strictness (2026-05-11)

The eight selected-rail closeouts landed on 2026-05-11. They are dated historical
receipts for this Scenario 14 denominator, not current complete effective-root
authority. Workspace-symbol candidates are filtered through `EffectiveIncContext`
at the lookup boundary, so no-`use lib` consumers can no longer leak through the
workspace-index path:

| # | Closeout | Receipt PR |
|---|---|---|
| 1 | `PERL5LIB` gated by `usePerl5lib`; startup-`@INC` probe strips `PERL5LIB` when `usePerl5lib=false` | [#8493](https://github.com/EffortlessMetrics/perl-lsp/pull/8493) |
| 2 | Nested multi-root workspaces resolve against the deepest matching folder | [#8496](https://github.com/EffortlessMetrics/perl-lsp/pull/8496) |
| 3 | Interpreter startup `@INC` gated by `useSystemInc`; bounded probe + cache | [#8497](https://github.com/EffortlessMetrics/perl-lsp/pull/8497) |
| 4 | Module completion uses prefix-directed scan for namespaced prefixes | [#8498](https://github.com/EffortlessMetrics/perl-lsp/pull/8498) |
| 5 | `EffectiveIncContext` shared across completion, PL701, goto-definition, hover | [#8504](https://github.com/EffortlessMetrics/perl-lsp/pull/8504), [#8505](https://github.com/EffortlessMetrics/perl-lsp/pull/8505), [#8506](https://github.com/EffortlessMetrics/perl-lsp/pull/8506) |
| 6 | Startup-`@INC` probe failures emit targeted warnings; fail-closed cache preserved | [#8518](https://github.com/EffortlessMetrics/perl-lsp/pull/8518) |
| 7 | Position-aware `no lib` cancellation enforced across PL701, pull diagnostics, completion, goto-definition, hover; workspace-index-backed consumers filtered | [#8540](https://github.com/EffortlessMetrics/perl-lsp/pull/8540) (impl of #8516) |
| 8 | Workspace-symbol lookups filtered through `EffectiveIncContext` at the lookup boundary — final no-lib strictness gap closed | [#8544](https://github.com/EffortlessMetrics/perl-lsp/pull/8544) (impl of #8537) |

The consumer consistency matrix above is the historical selected-rail receipt:
every named Scenario 14 consumer cell is `+` after these closeouts landed. The
include-root classification table added in [#8553](https://github.com/EffortlessMetrics/perl-lsp/pull/8553)
records why `.` remains a wildcard-like root distinct from configured and
lexical roots.

### `.`-wildcard caveat

Per [#8552](https://github.com/EffortlessMetrics/perl-lsp/pull/8552), `.`-wildcard
entries in include roots remain a known edge with documented semantics, not a
regression: the prefix-vs-exact fixture rule distinguishes prefix completion
(`use Gre<cursor>`) from exact-module fixtures (`use GreetModule;`), and
wildcard roots resolve under the exact-module path. This is intentional and is
covered by Scenario 14 — it is **not** an open `@INC` rail item.

## Historical selected-rail status (2026-05-11)

The selected static `@INC` consumer rail landed across `#8493 → #8506`. This is
dated historical selected-rail closure, not current complete effective-root
authority.

- `PERL5LIB` is gated by `usePerl5lib`; the startup-`@INC` probe also strips `PERL5LIB` from its subprocess environment when `usePerl5lib=false` so the two flags stay independent. (#8493)
- Interpreter startup `@INC` is gated by `useSystemInc`; the probe is bounded by `SYSTEM_INC_PROBE_TIMEOUT = 1000 ms` and cached. (#8497)
- Completion, PL701, goto-definition, and hover share `EffectiveIncContext` for include-root assembly. (#8504, #8505, #8506)
- PL701 displays labeled search roots via `ModuleSearchPathDisplay`. (#8502)
- Nested multi-root workspaces resolve folder, config, include paths, and completion-cache write-back against the most-specific (deepest) matching folder. (#8496)
- Module completion uses prefix-directed scan for namespaced prefixes. (#8498)
- Startup-`@INC` probe failures and timeouts emit targeted warnings while preserving the cached-empty fail-closed behavior. (#8518)
- Docs and JSON schema document `usePerl5lib`, `perl5libPrecedence`, and the three sources of search paths. (#8494)
- Scenario 14 conformance harness has a completion column and prefix-vs-exact fixture semantics. (#8495)

Known follow-ups from that 2026-05-11 selected-rail closeout (each its own
issue; none of these promote this page to complete effective-root authority):

- Pull-diagnostics path (`features/diagnostics/pull.rs`) and workspace-index-backed consumers now honor per-use-statement `no lib` cancellation — resolved in the follow-up commit to #8516.
- Runtime-owned short TTL cache for prefix module scans — split out of #8491 after PR 7a (scan-only) landed in #8498.

## Resolution Mode Details

### Workspace `includePaths`

Configured via `workspace/didChangeConfiguration`:

```json
{ "settings": { "perl": { "workspace": { "includePaths": ["lib"] } } } }
```

Module lives at `lib/Module.pm` relative to the workspace root.

### Absolute `includePaths`

Configured via `workspace/didChangeConfiguration` with an absolute folder path:

```json
{ "settings": { "perl": { "workspace": { "includePaths": ["/abs/path/to/lib"] } } } }
```

Module lives at `/abs/path/to/lib/AbsoluteModule.pm`.

### Lexical `use lib`

Source-level pragma: `use lib 'lib';` before `use Module;`. The LSP extracts
`use lib` and `no lib` operations in lexical order via
`resolve_use_lib_paths_from_source()` in
`crates/perl-module/src/resolution/use_lib.rs`.

### `no lib` Cancellation

Position-aware negative test: `use lib 'lib'; no lib 'lib'; use GoneModule;`.
The module file exists on disk but must NOT resolve because `no lib` cancelled
the earlier `use lib` before the `use GoneModule` line.

Selected-rail proof is that cancellation for a named path at the use-statement
offset across the four Scenario 14 consumers. It is not complete `lib`
expansion-family membership or full source-order `use lib`/`no lib` semantics;
those remain not_proven under M04 [#10569](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10569)
and [#10571](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10571).

### FindBin-Relative

Pattern: `use FindBin; use lib "$FindBin::Bin/lib";`. `$FindBin::Bin` resolves
to the directory containing the script being analyzed. The module must be at
`<script_dir>/lib/Module.pm`.

Selected-rail proof is that `$FindBin::Bin/lib` pattern on the analyzed file.
It does not prove authoritative invoked-script identity and `Bin`/`RealBin`
distinction; those remain not_proven under M04C [#10570](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10570).

### PERL5LIB env (`usePerl5lib`)

Module lives outside the workspace in a directory injected via the `PERL5LIB`
environment variable. Controlled by `usePerl5lib: true` in workspace config.
This flag is independent of `useSystemInc`.

### Interpreter startup `@INC` (`useSystemInc`)

Modules reachable via the interpreter's startup `@INC` (the result of
`perl -e 'print join("\n", @INC)'`). Controlled by `useSystemInc: true` in
workspace config. The startup-@INC probe strips `PERL5LIB` from the spawned
environment when `usePerl5lib=false` to prevent cross-flag leakage.

The two flags are independent: `usePerl5lib` controls PERL5LIB; `useSystemInc`
controls interpreter startup roots. Setting one does not imply the other.
Gating independence is part of the selected rail. Complete interpreter-startup
`@INC` as effective-root authority remains not_proven.

## Include-Root Classification

When filtering workspace-symbol candidates through `EffectiveIncContext`,
include roots are NOT equivalent paths — they have semantically distinct
sources that affect cancellation, reachability, and filter behavior.

| Kind | Source | Subject to `no lib` cancellation? | Notes |
|---|---|---|---|
| `WorkspaceRelative` | Relative configured include paths such as `lib`, `t/lib`, or `.` | Yes, when the matching configured path is cancelled at the request position | The `.` entry resolves to the workspace root and is **wildcard-like** for reachability filtering; it is not a separate kind |
| `FileLocalLexical` | A `use lib '...'` path from the source under analysis after lexical resolution | Yes (position-scoped) | Workspace-contained absolute lexical paths are normalized to workspace-relative paths and use this kind; absolute lexical paths outside the workspace are rejected before effective roots are assembled |
| `ExternalAbsolute` | An absolute configured include path already admitted by the upstream configuration boundary | Yes when it is a cancelled configured path | Lexical paths are normalized or rejected before effective-root classification; this kind is not the production representation for workspace-contained absolute lexical paths |
| `Perl5LibEnv` | A `PERL5LIB` entry when `usePerl5lib` is enabled | No | Environment-supplied roots are labeled separately; subprocess environment handling is governed by #8551 |
| `InterpreterStartup` | An entry returned by the selected interpreter's startup `@INC` probe when `useSystemInc` is enabled | No | Output of the subprocess seam — see [perl-subprocess-seams.md](../../architecture/perl-subprocess-seams.md) (#8555) |
| `RuntimeDerived` | A future trusted runtime-derived include root | No | Reserved by the enum; the current effective-root builder does not produce it |

**Why this matters**: if `.` (the workspace default) is treated like any
other configured include root, reachability filters incorrectly conclude
that nearly every workspace file is reachable, which defeats checks like
"after `no lib 'lib'`, `lib/GoneModule.pm` should NOT resolve." Filters
must branch on the kind, not the path. Nested multi-root closeout #8496 is
selected-rail folder matching; explicit unowned document context versus
first-folder fallback remains not_proven under M07
[#10575](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10575)
and is not complete multi-root effective-root authority.

See `crates/perl-lsp-rs/src/runtime/lifecycle/inc_context/mod.rs` for the
runtime implementation and `crates/perl-lsp-rs/CLAUDE.md` for the per-crate
rule.

## Implementation Notes

- Position-aware resolution is implemented in `crates/perl-module/src/resolution/use_lib.rs`.
- Effective include-root assembly is shared through
  `build_effective_inc_roots()` in `crates/perl-module/src/resolution/uri.rs`;
  it preserves source labels for configured paths, lexical `use lib`, PERL5LIB,
  and interpreter startup paths.
- The four Scenario 14 consumers (PL701 diagnostic, completion, goto-definition,
  hover) all call either `resolve_module_to_path_with_doc()` or
  `resolve_module_path_with_uri()` from
  `crates/perl-lsp-rs/src/runtime/lifecycle/module_resolution.rs`.
- Consumer call sites:
  - PL701 diagnostic: runtime diagnostics and pull-diagnostics paths
  - completion: module completion path (uses `perl5lib_paths_for_completion()`)
  - goto-definition: runtime navigation path
  - hover: runtime hover path

## Compiler-Substrate Boundary

HIR `CompileEnvironment` already records module requests and include-root facts.
Static module requests now produce HIR candidate facts through
[#8242](https://github.com/EffortlessMetrics/perl-lsp/issues/8242). That HIR
lane does not read ambient environment or spawn Perl from parser lowering;
callers provide configured, lexical, PERL5LIB-labeled, and system-labeled roots
explicitly. Runtime consumers still own filesystem-backed resolution and LSP
provider behavior.

## Follow-up Scope

Tracked follow-ups from the 2026-05-11 selected-rail closeout (each its own
issue; not a claim that the `@INC` rail is complete effective-root authority):

- **Position-aware `no lib` cancellation** — landed in [#8516](https://github.com/EffortlessMetrics/perl-lsp/issues/8516). PL701, pull diagnostics, completion, goto-definition, and hover now reject modules whose path was cancelled by `no lib`; workspace-index-backed consumers are filtered so they cannot bypass active `@INC` state.
- **Runtime-owned TTL cache for module-completion scans** — see [#8514](https://github.com/EffortlessMetrics/perl-lsp/issues/8514). Builds on the prefix-directed scan in #8498.

Backlog (pre-existing, not part of the 2026-05-11 selected-rail closeout):

- `inc_nested_use_lib` — `use lib` inside `BEGIN` block
- `inc_qw_use_lib` — `use lib qw(lib t/lib)` multi-path form
- Cross-scorecard: add `expected.json` diagnostic sidecars to all `inc_*` fixtures

## Literal `require` / `import`

Spec lane tracked under [#4280](https://github.com/EffortlessMetrics/perl-lsp/issues/4280)
(umbrella) and [#8616](https://github.com/EffortlessMetrics/perl-lsp/issues/8616)
(this spec). All literal-form resolution **must** flow through
`EffectiveIncContext` (the same filter introduced by [#8544](https://github.com/EffortlessMetrics/perl-lsp/pull/8544)
for workspace-symbol lookups). No consumer may bypass active `@INC` state to
resolve a literal `require` or `import` form.

### Consumer × form matrix

| Form | PL701 diagnostic | completion | goto-definition | hover |
|---|---|---|---|---|
| `require Foo;` (bareword) | resolve via `EffectiveIncContext` | suggest from prefix scan, filtered by context | jump to module file | show module summary |
| `require "Foo.pm";` (literal, single-segment) | resolve same as bareword `Foo` | suggest matching `.pm` paths under context roots | jump to module file | show module summary |
| `require "Foo/Bar.pm";` (literal, multi-segment) | resolve same as bareword `Foo::Bar` | suggest matching nested `.pm` paths | jump to module file | show module summary |
| `import Foo;` (static bareword) | treat as `use Foo;` for resolution purposes | suggest from prefix scan | jump to module file | show module summary |
| `Foo->import;` (static method call on bareword) | resolve `Foo`, do not interpret import list | suggest the bareword target | jump to `Foo` | show `Foo` summary |

### Boundary table

| In scope | Out of scope |
|---|---|
| `require Foo;` (bareword) | `eval STRING` where STRING contains `use`/`require` |
| `require "Foo.pm";` (string literal, single segment) | `require $module;` (variable holds the path) |
| `require "Foo/Bar.pm";` (string literal, multi segment) | `require "${prefix}::Foo";` (string interpolation) |
| `import Foo;` (static, no list) | Runtime `import` with computed module names |
| `Foo->import;` (static method call) | Plugin frameworks that synthesize module names at runtime |

Out-of-scope forms remain explicitly unresolved — diagnostics may flag them
as "dynamic; cannot statically resolve" but must not guess.

### Acceptance test path (when impl lands)

- New fixtures under `crates/perl-lsp-ux-tests/tests/fixtures/literal_require/`.
- Consumer harness reused from Scenario 14 (`@INC` conformance).
- Boundary table out-of-scope rows MUST produce a documented "unresolved (dynamic)" outcome — not a panic, not a stale hit, not a false positive.

Receipts:

- [#4280](https://github.com/EffortlessMetrics/perl-lsp/issues/4280) — umbrella ux-journey
- [#8616](https://github.com/EffortlessMetrics/perl-lsp/issues/8616) — this spec
- [#8544](https://github.com/EffortlessMetrics/perl-lsp/pull/8544) — `EffectiveIncContext` filter that literal forms must reuse
