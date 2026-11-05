# perl-lsp Status and Evidence

> Human-maintained navigation for `perl-lsp-swarm`. Each linked dashboard owns
> its observations and limitations; live GitHub controllers own current decisions.

## Start with a Question

| What do you need to decide? | Start here |
| --- | --- |
| What needs work before a release, and who owns the decision? | [Release and next work](#release-and-next-work) |
| What can a user rely on in the editor or debugger? | [Product and editor behavior](#product-and-editor-behavior) |
| Which parser and compiler capabilities have evidence? | [Parser and compiler](#parser-and-compiler) |
| What was measured, tested, or left unproven? | [Reliability and proof](#reliability-and-proof) |

Before using a number or status, check its [evidence boundary](#read-the-evidence)
and the linked receipt's date, source revision, environment, and tested scope.

## Release and Next Work

Use these live controllers for current scope, next actions, and release decisions.
Their current records govern when a dated roadmap or status narrative differs.

| Question | Decision owner |
| --- | --- |
| What is the release-candidate scope and authorization? | [RC programme — #13768](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13768) |
| Which carriers and prerequisites need work next? | [Executable RC frontier — #16464](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/16464) |
| What is the feature-intake and product-freeze decision? | [Final scope and freeze — #5888](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5888) |
| Which landed changes belong in the release candidate? | [Landed-main release ledger — #16399](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/16399) |
| Has the complete live PR frontier been observed? | [Final PR observation — #16465](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/16465) |
| What execution evidence supports the selected main subject? | [Exact-subject honest-main receipt — #16466](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/16466) |

Development happens in `perl-lsp-swarm`; publication lineage and public releases
belong to [EffortlessMetrics/perl-lsp](https://github.com/EffortlessMetrics/perl-lsp).
Read [release narrative and dated receipts](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/release.md) and the
[cross-channel release history](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/RELEASE_HISTORY.md)
for recorded observations, with release notes in the
[changelog](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/CHANGELOG.md).
Check the relevant channel receipt when judging what was actually published or installed.

Source-owned facts and planning context:

- [Workspace manifest](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/Cargo.toml)
  owns workspace versions and the publication allowlist.
- [Capability catalog](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/features.toml)
  owns declared LSP capabilities; [LSP status](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/lsp.md) explains their evidence.
- [Roadmap](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/ROADMAP.md) supplies planning context. Read its dated sections
  against the live controllers before selecting work.

## Product and Editor Behavior

| Question | Dashboard | Evidence to inspect |
| --- | --- | --- |
| Which LSP capabilities are declared and evidenced? | [LSP status](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/lsp.md) | Catalog, evidence state, and compliance scope. |
| Which real editor and workspace claims are supported? | [Real Perl Editor Trust](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/real_perl_editor_trust_v1.md), [support tiers](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/SUPPORT_TIERS.md) | Named workflows, support limits, and installed or fixture receipts. |
| Which semantic provider path serves the editor? | [Semantic UX](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/ux_capability_dashboard.md), [provider cutover](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/provider_cutover.md), [provider confidence](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/provider_confidence_matrix.md) | Live, shadow, legacy, fallback, and insufficient-data boundaries for each provider. |
| What has the editor workflow harness measured? | [Editor UX scorecard](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/editor_ux.md), [planning scaffold](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/editor_ux.json) | Measurement date, measured scenarios, and declared scenarios still needing proof. |
| What is known about workspace indexing? | [Workspace scorecard](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/workspace.md) | Lifecycle, indexing scope, and the receipts behind each row. |
| Which debugger flows have evidence? | [DAP scorecard](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/dap.md) | Adapter mode, scenario, platform, and execution receipt. |
| What is covered by the native formatter and critic? | [Native tooling](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/native_tooling.md) | Declared coverage and the corresponding runtime or conformance proof. |
| Which command-line forms are covered? | [One-liner capability matrix](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/oneliner_capability_matrix.md) | Corpus scope and per-capability evidence. |

## Parser and Compiler

| Question | Dashboard | Evidence to inspect |
| --- | --- | --- |
| Which Perl inputs parse successfully? | [Parser corpus and coverage](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/parser.md) | Keep Ubuntu system Perl, CPAN, and repository-owned corpus lanes distinct; use each lane's receipt date and denominator. |
| Which syntax lowers into the compiler representation? | [HIR lowering coverage](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/hir_lowering.md) | Supported constructs, coverage denominator, and lowering evidence. |
| Which shared compiler facts are available? | [Compiler fact substrate](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/compiler_facts.md) | Fact ownership, supported boundaries, and consumer wiring. |
| What do the semantic fixtures establish? | [Semantic capability summary](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/semantic_capability_dashboard.md), [semantic scorecard](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/semantic_scorecard.md) | The named release or fixture scope and its receipts; check provider cutover for user-facing behavior. |
| Which providers consume shared facts? | [Provider fact-read inventory](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/provider_fact_reads.md) | Consumer ownership and remaining local interpretation. |
| Which module-resolution cases conform? | [Module resolution](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/module_resolution.md) | Case-specific behavior, conformance proof, and remaining work. |

Use the [compiler-backed LSP roadmap](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/COMPILER_BACKED_LSP_ROADMAP.md) for the
implementation direction and its linked work owners.

## Reliability and Proof

| Question | Dashboard | Evidence to inspect |
| --- | --- | --- |
| How many tests are discovered, and which debt is tracked? | [Test inventory and debt](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/tests.md) | Discovery scope and ignore categories. A test count alone supplies no execution verdict. |
| Which performance and engineering-health measurements exist? | [Quality metrics](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/quality.md) | Individual measurement receipts, test inventory, mutation data, and collection gaps. |
| What was measured in the lean Neovim profile? | [Neovim latency](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/neovim_latency.md) | Profile, workload, environment, and receipt date. |
| What supports memory guardrails and trends? | [Memory plateau](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/memory_plateau.md), [memory trends](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/memory_plateau_trends.md) | Workload, budget, baseline identity, and measured runs. |
| Which coverage and regression obligations are enforced? | [Coverage and RIPR](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/coverage_and_ripr_enforcement.md) | Required proof lanes, scope, and documented exceptions. |
| How is CI hardening implemented? | [CI hardening](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/ci_hardening.md) | Implemented controls and their evidence; inspect live required checks for a candidate's integration state. |

The [verification protocol](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/protocols/verification.md)
describes verification commands and receipts. Current PR reviews, required checks,
and repository policy determine integration readiness.

## Read the Evidence

| Surface | What it establishes | What to check before relying on it |
| --- | --- | --- |
| Live controller or PR | Current decisions, ownership, blockers, and candidate disposition. | Latest applicable decision, subject identity, reviews, and linked proof. |
| Generated observation or inventory | What its producer collected from the selected inputs. | Producer, input scope, collection outcome, and source revision. Refreshing inventory does not execute the listed tests. |
| Dated measurement or execution receipt | A result for its recorded subject and environment. | Date, commit or artifact identity, corpus or scenario denominator, result, and limitations. |
| Human-maintained summary or plan | Interpretation and navigation to the underlying owners. | Linked evidence and live decisions, especially where the prose names an older release or milestone. |

Keep missing, stale, or insufficient evidence visibly unproven, and preserve
explicit failure results. A capability count, checked-in baseline, or successful
status refresh cannot establish a passing test run, installed support, or release
readiness for another subject.

## Refresh and Maintain

Run commands from the repository root. `just status-update` accepts one optional
positional subsystem: `lsp`, `tests`, `parser`, `quality`, `dap`, `workspace`, or
`provider-facts`.

```bash
just status-update parser          # refresh the parser subsystem
just status-update provider-facts  # refresh provider fact-read inventory
just status-update                 # refresh all update-status outputs
just status-check                  # check generated status, HIR coverage, and doc drift
```

The [status producer](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/xtask/src/tasks/update_status/mod.rs)
owns its generated outputs; other scorecards name their own producers. Refresh a
measurement with its documented producer when new execution evidence is needed.
Use the [recipes](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/justfile)
for the exact command behavior. Status refresh and drift checks do not replace
the focused tests or current integration evidence required for a change.

Edit this index for navigation and evidence guidance. Update a subsystem's source
or receipt before its summary, and preserve its generated sections. After changing
canonical documentation, regenerate the book copies with
`bash scripts/populate-book.sh` and review the intended diff.

Earlier narratives remain available in
[this page's history](https://github.com/EffortlessMetrics/perl-lsp-swarm/commits/main/docs/project/status/index.md)
and the [reference archive](https://github.com/EffortlessMetrics/perl-lsp-swarm/tree/main/docs/reference/archive/).
