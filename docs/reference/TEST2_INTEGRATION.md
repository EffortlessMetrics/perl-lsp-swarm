# Test2 Integration

`perl-lsp` reads [Test2](https://metacpan.org/pod/Test2::V0) source and runner
output. It does **not** implement Test2, evaluate assertions, execute anonymous
`subtest` blocks in isolation, or replace `yath`, `prove`, `perl`, or a
project-specific test command.

> **`perl-lsp` reads Test2 source, structure, and output; Test2 itself runs the
> tests.**

This reference distinguishes the behavior on current `main` from the canonical
architecture being built under the unified testing program. A migration or
shadow implementation is not described as the final authority merely because a
product surface already consumes it.

## Status by surface

| Surface | Current implementation | Canonical owner and target |
| --- | --- | --- |
| Test2 imports and pragma effects | `perl-lsp-rs-core::providers::testing::test2` reads Test2 source, distinguishes reviewed first-party bundles and tools from unknown or dynamic modules, and supplies the current critic and completion bridge. | #4907 with #6946 and #6948: registered FrameworkAdapters emit versioned source facts once. |
| Completion and hover | Completion reconciles generic test candidates against parser-scoped Test2 import facts, including exclusions, aliases, V1 defaults, and reviewed standalone defaults. Hover still routes Test2 contexts through Test::More presentation data. | #6951 / #2384: complete and document only local names authorized by canonical import facts. |
| Subtest discovery | The Rust parser-backed walker supplies document symbols and Run Subtest lenses. Other Rust and VS Code discovery paths still exist. | #4774 / #6953: one generation-aware, nested `TestItem` tree with stable typed identity. |
| TAP result facts | The execute-command path currently uses a provider-local Rust TAP reader. `perl-test-facts` is the accepted pure result-facts authority, but consumer migration is not complete. | #6943 / #4776: `perl-test-facts` becomes the only server-side TAP parser; #4778 removes the TypeScript parser. |
| Test execution | The execute-command provider can run `yath`, `prove`, or `perl`; a separate legacy server path and a direct VS Code `prove` path also remain. | #4776 / #4898 / #4972: one runner plan, one supervised process path, and one typed result contract. |
| Debugging | File-level test debug returns a native `perl-dap` launch configuration. There is no canonical independently implemented Debug Subtest operation. | #4750 / #4776 / #4754: debug derives from the same `TestItem` and runner-plan identity; nearest-subtest focus remains honest about whole-file execution. |

## Current source understanding

### Test2 imports

#### Pinned upstream authority

The first-party import facts in the current migration oracle are checked against
the immutable [`Test-More/test-more` revision
9545e6eebffc1662d50414bf1ed6c09fb229206d](https://github.com/Test-More/test-more/tree/9545e6eebffc1662d50414bf1ed6c09fb229206d),
which reports version `1.302225` for the modeled `Test2::V0`, `Test2::V1`, and
`Test2::Tools::Refcount` modules. The relevant source files are:

- [`lib/Test2/V0.pm`](https://github.com/Test-More/test-more/blob/9545e6eebffc1662d50414bf1ed6c09fb229206d/lib/Test2/V0.pm)
- [`lib/Test2/V1.pm`](https://github.com/Test-More/test-more/blob/9545e6eebffc1662d50414bf1ed6c09fb229206d/lib/Test2/V1.pm)
- [`lib/Test2/Tools/Refcount.pm`](https://github.com/Test-More/test-more/blob/9545e6eebffc1662d50414bf1ed6c09fb229206d/lib/Test2/Tools/Refcount.pm)

This revision is the reproducible authority for this fact-table slice; a later
upstream revision requires an explicit source review and pin update. It does not
claim that every Test2 release or dynamic plugin is statically knowable.

The current provider-local reader recognizes substantial Test2 import behavior,
including examples such as:

```perl
use Test2::V0;
use Test2::V0 ();
use Test2::V0 '!ok';
use Test2::V0 ok => {-as => 'my_ok'};
use Test2::V0 ok => {-prefix => 't2_'};
use Test2::V0 -no_strict;
use Test2::V0 -no_warnings;
use Test2::V1;
use Test2::V1 -import;
use Test2::Bundle::More;
use Test2::Tools::ClassicCompare;
use Test2::Tools::Spec;
```

It preserves the important V0/V1 distinction: plain `Test2::V1` exposes the
`T2()` handle rather than the V0 bare-function set. It also keeps bundle-selected
subsets separate from standalone module defaults. For example, V0 selects only
`cmp_ok` from `Test2::Tools::ClassicCompare`, while a standalone import exposes
the module's full classic-comparison default set. `Test2::Tools::Refcount`
similarly keeps `refcount` optional for its standalone defaults while exposing
it through `:ALL`.

Reviewed first-party contracts include `Test2::Bundle::Extended`,
`Test2::Bundle::More`, `Test2::Bundle::Simple`, and statically enumerable tool
modules. `Test2::Suite` is recognized as the distribution namespace but imports
no symbols and supplies no caller pragmas. Modules with dynamic import behavior,
such as `Test2::Tools::Target`, remain unknown rather than receiving invented
static defaults.

These facts are consumed by native critic behavior and by the parser-scoped
completion bridge. For a supported normal `use Test2::V0;` import, the critic
can treat the bundle as providing `strict` and `warnings`; the documented
opt-outs remain significant. Completion projects only the local Test2 names
resolved for the cursor's active package, including exclusions, aliases, V1's
`T2` default, and reviewed standalone tool defaults.

Hover is not yet import-authoritative. It still presents Test::More-backed cards
for Test2 contexts, so richer Test2-specific signatures and documentation remain
part of #6951 and the canonical adapter migration.

Current migration oracle:
[`providers::testing::test2`](../../crates/perl-lsp-rs-core/src/providers/testing/test2.rs).
Completion bridge:
[`completion::request::test_frameworks`](../../crates/perl-lsp-rs-core/src/providers/completion/completion/request/test_frameworks.rs).
Canonical source-fact program: #4907, #6946, and #6948.

### Test structure

The Rust subtest reader recognizes reviewed Test2/Test::More call forms and
builds nested source structure for statically identifiable names:

```perl
subtest 'user lookup' => sub {
    ok(my $user = find_user('a@example.com'), 'found user');

    subtest 'email' => sub {
        is($user->{email}, 'a@example.com', 'email matches');
    };
};
```

The document outline can represent:

```text
t/user.t
└── user lookup
    └── email
```

Dynamic names are not supposed to be guessed. The canonical `TestItem` work
will additionally provide stable IDs, exact ranges, generation ownership,
duplicate-name handling, deltas, capabilities, and one identity shared by code
lenses, Test Explorer, run-at-cursor, and debug targeting.

Current reader:
[`providers::testing::subtest`](../../crates/perl-lsp-rs-core/src/providers/testing/subtest.rs).
Canonical model: #4774 and #6953.

## Current runner-output understanding

A real runner may emit TAP such as:

```text
not ok 3 - email matches
# at t/user.t line 12.
# got: 'wrong@example.com'
# expected: 'a@example.com'
```

The current execute-command result projects failures into structured fields
such as `file`, `line`, `got`, and `expected`. TODO and SKIP records are not
hard failures, and raw stdout/stderr remains available.

The accepted result-time authority is now
[`perl-test-facts`](../../crates/perl-test-facts/). It is a pure TAP facts crate,
not a Test2 source reader or runtime. It preserves assertion outcomes, plans,
nesting depth, bailouts, source locations reported by the runner, YAML and
ordinary diagnostics, malformed/unknown raw records, and structural errors.

Until #6943 is complete, the server still has an older provider-local Rust TAP
parser. Until #4778 is complete, the VS Code Test Explorer also has its own
TypeScript TAP parser. Documentation and product claims must not treat those
parallel paths as converged.

TAP stream line numbers and runner-reported source line numbers are different
facts and must remain separate. Structured parsing is additive: partial or
malformed TAP never justifies discarding the bounded raw process output.

## Current commands and code lenses

| Surface | Command | Current behavior |
| --- | --- | --- |
| **Run All Tests** at the top of a `.t` file | `perl.runTestFile` | Runs the whole file through a current server test path. The final unified runner-plan and process authority is still pending. |
| **Run Subtest** above a static subtest | `perl.runSubtest` | Runs the **whole file** and focuses the parsed output on the requested label. It reports `subtestMode: "whole-file-focused"`; it does not execute the anonymous block separately. |
| **Debug Test File** at the top of a `.t` file | `perl.debugTestFile` | Returns a native `perl-dap` launch configuration for the real `.t` file. |

A product surface must not advertise success for work it did not perform. A
selective or debug action is implemented only when it executes or delegates a
real operation; otherwise it must be absent or return an explicit unsupported
result. Command convergence is tracked by #4972.

## Snippets

The VS Code extension includes Test2-oriented snippets such as `usetest2`,
`dies`, and `lives`, plus shared assertion and structure snippets. Snippets are
presentation conveniences; they are not evidence that the corresponding import
is active in the current scope.

For Test::More, skip-all is syntax such as:

```perl
plan skip_all => 'reason';
```

It is not a callable Test::More `skip_all()` export. Any completion for this
form belongs with syntax snippets, not the callable export table.

## Canonical architecture

```text
accepted source generation
    -> registered Test::More/Test2 FrameworkAdapters
    -> canonical source facts
    -> canonical TestItem tree
    -> completion / hover / diagnostics / code lens / Test Explorer

canonical TestItem target
    -> versioned runner plan
    -> ProcessSupervisor
    -> real yath / prove / perl / reviewed project command
    -> bounded raw stdout/stderr
    -> perl-test-facts TapReport
    -> LSP / Test Explorer / diagnostics / receipts / debug context
```

Responsibility map:

| Concern | Canonical issue |
| --- | --- |
| Test::More/Test2 source semantics | #4907, #6944, #6946, #6948 |
| Completion and hover presentation | #6951 and conformance checklist #2384 |
| Stable test/subtest identity and discovery | #4774 and #6953 |
| TAP authority and server migration | #6943 and #4776 |
| Runner-plan and result service | #4776 |
| Process supervision | #4898 |
| Command-path convergence | #4972 |
| VS Code Test Explorer migration | #4778 |
| Debug target integration | #4750 and #4754 |

There is no planned `perl-test2` runtime crate. Source semantics belong to
canonical framework facts; result-time TAP semantics belong to
`perl-test-facts`; execution remains with the real Perl testing ecosystem.

## Deliberate non-goals

- No Test2 or Test::More runtime implemented in Rust.
- No assertion evaluation by the LSP.
- No arbitrary Test2 plugin or hub execution.
- No anonymous subtest-block extraction and direct invocation.
- No claim that every dynamic plan or generated test is statically knowable.
- No replacement of project runner configuration with an editor-specific test
  framework.
