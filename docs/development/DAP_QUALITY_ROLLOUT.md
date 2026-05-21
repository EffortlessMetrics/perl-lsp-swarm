# DAP Quality and Support Burndown

> **Substrate (already built)**: `perl-dap` binary crate, native and bridge DAP
> modes, stdio/socket transport, launch/attach tests, breakpoint matrix tests,
> golden transcript tests, DAP e2e workflow tests, syntax-check tests,
> scorecard harness, security tests, variable/stack/eval tests, packaging tests,
> VS Code debugger contribution, and a narrow module-resolution DAP smoke rail.
>
> **Connector gap**: DAP has broad implementation and test scaffolding, but not
> one maintained product-quality contract. We need stable support tiers, receipt
> schemas, editor integration checks, packaging proof, latency budgets, fixture
> taxonomy, and clear boundaries for launch, attach, breakpoints, stepping,
> variables, eval, exceptions, watchpoints, and path mapping.
>
> **User-visible upside**: Perl debugging becomes a supported product surface,
> not a pile of passing tests. Users can launch or attach, set breakpoints,
> step, inspect variables, debug workspace modules, and rely on clear docs,
> receipts, and support claims.

## Current substrate

The DAP crate is already real and ships both a binary and a library surface. It
already has broad test categories across bridge/native paths, launch/attach,
breakpoint matrixes, protocol transcripts, security, performance, packaging,
variables, stack traces, eval behavior, and scorecard harnesses.

The `perl-dap` entrypoint currently supports stdio by default, socket mode with
default port `13603`, and a `--bridge` mode that proxies through
Perl::LanguageServer.

This rail does **not** rewrite the DAP implementation strategy. It converts the
existing implementation footprint into a maintained quality/support system.

## R0 — DAP support taxonomy

| Surface | Tier | Notes |
|---|---|---|
| `initialize` / `configurationDone` / `disconnect` | Stable | Protocol session basics |
| `launch` | Stabilizing | Needs editor receipts |
| `attach` | Stabilizing | TCP attach has tests, needs product receipts |
| Plain breakpoints | Stabilizing | Needs path/module receipts |
| Script → module breakpoints | Stabilizing | Covered by narrow module rail |
| Step over / in / out / continue / pause | Stabilizing | Existing tests, needs receipts |
| Stack trace / scopes | Stabilizing | Needs UX fixture receipts |
| Variables | Stabilizing | Needs truncation and deep-structure receipts |
| Evaluate | Experimental | Security-sensitive |
| Conditional breakpoints | Experimental/advisory | Keep separate from plain breakpoints |
| Logpoints | Advisory | Separate rail if implemented |
| Watchpoints / data breakpoints | Experimental | Existing tests, support boundary needed |
| Exception breakpoints | Experimental | Exception tests already exist |
| DAP packaging | Stabilizing | Must align release assets and VS Code/LSP4IJ |
| Editor integration | Stabilizing | VS Code first, LSP4IJ follow-up |

## R1 — DAP policy ledger

Add `policy/dap-quality.toml` with one `[[surface]]` block per claim, including:

- `id`, `tier`, `owner`
- a one-line `claim`
- proof command list (`proof = []`)
- receipt path (`receipt = "target/receipts/dap-quality/<surface>.json"`)
- `review_after`

## R2 — DAP receipt schema

Add `.ci/receipts/schemas/dap-quality.schema.json` with fields for:

- metadata (`schema_version`, `surface`, `perl_dap_version`, `perl_version`)
- execution shape (`dap_mode`, `transport`, `os`, `fixture`)
- evidence (`commands`, `events`, `latency`, `verdict`)
- boundary (`claim_boundary`)

## R3 — Fixture taxonomy

Add fixture taxonomy docs:

- `crates/perl-dap/tests/fixtures/FIXTURE_INDEX.md`
- `docs/status/DAP_FIXTURE_TAXONOMY.md`

Families include simple scripts, module imports, edge breakpoints, syntax-error
launches, warn/exception flows, deep/large variable structures, path forms,
attach targets, unsafe eval attempts, timeout behavior, and OS path variants.

## R4–R15 — quality phases

Each phase maps support claim → fixture(s) → test command(s) → receipt:

- **R4 launch quality**: e2e/golden/syntax-check tests and launch receipts.
- **R5 attach quality**: attach e2e + TCP attach receipts.
- **R6 breakpoint quality**: matrix + edge + module-smoke receipts.
- **R7 stepping quality**: step-through + e2e stepping receipts.
- **R8 stack/scopes/variables quality**: stack + variables + truncation receipts.
- **R9 eval/security quality**: safe eval + timeout + traversal/security receipts.
- **R10 exception/warn/watchpoint quality**: capability and boundary receipts.
- **R11 protocol/transcript quality**: protocol message + non-regression + golden receipts.
- **R12 performance budgets**: advisory `policy/dap-performance-budgets.toml` + perf status doc + harness receipts.
- **R13 packaging/editor integration**: packaging/dependency receipts and explicit editor-scope boundaries.
- **R14 user-facing docs**: DAP reference/tutorial/status/book updates.
- **R15 advisory CI lane**: bounded DAP quality subset with burn-in before promotion.

## Status

| Phase | Issue | Builder-ready? | PR | Receipt |
|---|---|---:|---|---|
| 1. Rail doc + index row | file after doc PR | yes | — | `git diff --check` |
| 2. DAP policy ledger | file after phase 1 | yes | — | `policy/dap-quality.toml parses` |
| 3. DAP receipt schema | file after phase 1 | yes | — | schema validation |
| 4. Fixture taxonomy | file after phase 1 | yes | — | fixture index |
| 5. Launch receipt | file after phase 1 | yes | — | `dap_e2e_workflow_tests` |
| 6. Attach receipt | file after phase 1 | yes | — | `dap_attach_e2e` |
| 7. Breakpoint receipt | file after phase 1 | yes | — | breakpoint matrix + module smoke |
| 8. Stepping receipt | file after phase 1 | yes | — | step-through tests |
| 9. Stack/variables receipt | file after phase 1 | yes | — | variables/stack tests |
| 10. Eval/security receipt | file after phase 1 | yes | — | security/eval tests |
| 11. Exception/watchpoint receipt | file after phase 1 | yes | — | warn/watchpoint tests |
| 12. Protocol transcript receipt | file after phase 1 | yes | — | golden transcript + non-regression |
| 13. Performance budget | file after phase 1 | yes | — | scorecard/perf harness |
| 14. Packaging/editor receipt | file after phase 1 | yes | — | packaging tests |
| 15. User docs | file after phase 1 | yes | — | docs receipt |
| 16. Advisory CI lane | after receipts | yes | — | CI artifact |
| 17. Narrow blocker promotion | after burn-in | no | — | policy update |

## PR sequence

1. docs-only rail and index row.
2. policy ledger.
3. receipt schema.
4. fixture taxonomy.
5. launch receipt.
6. attach receipt.
7. breakpoint receipt.
8. stepping receipt.
9. stack/variables receipt.
10. eval/security receipt.
11. protocol transcript receipt.
12. performance budgets.
13. packaging/editor receipt.
14. user docs.
15. advisory CI lane.
16. close/link narrow module rail.
17. narrow blocker promotion after burn-in.

## Exit criteria

Rail closes when all of the following are true:

- rail doc exists and is indexed
- DAP quality policy exists and validates
- DAP receipt schema exists
- fixture taxonomy exists
- launch/attach/breakpoint/stepping/variables/eval-security/protocol receipts exist
- script-to-module smoke remains linked/closed via narrow rail
- performance budgets, packaging/editor receipts, and user docs land
- advisory CI lane exists
- support claims map to proof commands
- blocker promotion happens only after burn-in

## Claim boundary

This rail proves a maintained DAP quality/support contract with explicit receipt
ownership for launch, attach, breakpoints, stepping, variables, eval/security,
protocol behavior, and packaging/editor integration boundaries.

This rail does not prove complete editor coverage, all Perl debugger edge cases,
all transport/runtime permutations, full conditional/logpoint/watchpoint
stabilization, or stable cross-platform SLOs.

## Do not combine

Do not combine this rail with LSP4IJ template submission/support rails, broad VS
Code extension quality rails outside debugger receipts, Zed compatibility rails,
Neovim latency rails, parser architecture work, or unrelated infra efforts.

## Lane assignment

- **Lane**: codex
- **Open phases**: 17
- **Next action**: Phase 1 rail doc + index row
