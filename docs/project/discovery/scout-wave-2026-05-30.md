# Issue Discovery — Scout Wave 1 (2026-05-30)

First run of the [Issue Discovery / Bug Scout Desk](../../reference/ISSUE_DISCOVERY_DOCTRINE.md).
6 read-only scouts swept recently-hot surfaces. **0 GitHub mutations by scouts**; the
orchestrator triaged centrally and filed 6 high-confidence candidates issue-by-issue.

## Scouts dispatched

| Scout | Surface | Verdict |
|-------|---------|---------|
| `scout-dap` | DAP stack/scopes/variables/evaluate/setVariable/lifecycle/transport | Mostly corroborated existing issues (#902/#895/#898/#803); 1 downgraded lead |
| `scout-lsp` | document state, URI isolation, completion/hover/code-action/semantic-token | 2 NEW leads (needs-repro / needs-plan-review) |
| `scout-parser` | AST shape, recovery, fixtures, NodeKind | **Parser healthy** (99.3% clean, 0 active buckets); 3 low-conf fixture leads only |
| `general-purpose` (ci/ops) | workflow routing, branch triggers, path filters | 2 NEW HIGH + 1 MED |
| `general-purpose` (robustness) | panic/DoS/byte-boundary/incorrect-result | 2 NEW HIGH + 1 MED (conditional reachability) |
| `general-purpose` (docs/receipt) | status `.md` ↔ `.json` receipts, basis conflicts | 4 NEW (3 HIGH + 1 MED) |

## Filed (6 — verified by orchestrator AND dedup-clean)

| Issue | Finding | Class | Verified |
|-------|---------|-------|----------|
| [#956](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/956) | `workspace_rename.rs` byte-checks UTF-8 continuation bytes → rename corrupts identifiers adjacent to Unicode names | bug / incorrect-result | code (`refactor/workspace_rename.rs:489-492,812`) |
| [#957](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/957) | `find_pragma_insert_pos` `+1` assumes LF → CRLF pragma insert splits `\r\n` | bug / incorrect-result | code (`code_actions/modernize.rs:543-563`) |
| [#958](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/958) | Workflows trigger on nonexistent `master` (default=`main`): `post-merge-status` + `workflow-trigger-lint` dead | ci-ops | mechanically (triggers + `git remote`) |
| [#959](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/959) | `em-ci-routed-rust` GitHub fallback skips parser+LSP smoke → fork/bot PRs pass gate on `cargo check` | ci / merge-gate trust | YAML (`em-ci-routed-rust.yml:235-237/319-321/371-372/82`) |
| [#960](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/960) | `quality.md` hardcodes `<50ms / 931ns`; receipts say 53ms / 37–73µs; no receipt has 931ns | docs-drift / basis-conflict | grep (`update_status/quality.rs:136`) |
| [#962](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/962) | Feature count: 125 actual `[[feature]]` vs 119 `[meta]`/lsp.md vs 116 index.md | docs-drift / basis-conflict | `grep -c` (125 blocks / 124 advertised) |

All filed with `swarm-discovered` + `needs-plan-review` + `size/*` (never `builder-ready`).

## Held — report-only / handoff (not filed)

| Lead | Conf | Next workflow | Why held |
|------|------|---------------|----------|
| D2 — `editor_ux.md` "9 of 22 (41%)" vs 64 fixture workflows | HIGH (scout) | file next pass or needs-research | Strong but the `64` count not orchestrator-verified |
| D4 — `.ci/blockers.yaml` non-zero affected_files vs 0-dirty baseline | MED | needs-plan-review | Low-risk cleanup; corroborates clean parser |
| R3 — `IncrementalParserV2` non-char-boundary slice panic (helpers dead-code) | MED | needs-research | Conditional reachability — no live caller on default LSP path |
| LSP1 — didClose→reopen stale AST cache | MED | needs-repro | Test gap; bug not yet reproduced |
| LSP2 — codeAction/resolve lacks stale-version guard | LOW-MED | needs-plan-review | Verify resolve handler version contract first |
| DAP P1-4 — frameId scoping / no-session success / threadId / scopes validation | — | evidence-only | Corroborate existing #902/#895/#898/#803; no new issue |
| Parser P1-3 — postfix chains, tie+attrs, given/when fixtures | LOW | report-only | Speculative fixture gaps; given/when strongest (recent #822 lexer fix) |
| LSP semantic-token drift; completion bleed | LOW | report-only / discard | →#262 (unverified); completion bleed covered by merged #757 |

## Cross-scout corroboration (the wave's synthesis)

- **#958 is the root cause of #962 (and D2):** status docs are stale *because* their
  regenerator (`post-merge-status.yml`) never fires (wrong branch trigger). Fixing #958
  + regenerating addresses the generated half of #962.
- **Parser-scout "clean" + D4 agree:** the parser corpus is clean (99.3%) while
  `.ci/blockers.yaml` still lists resolved buckets — independent confirmation of staleness.
- **Systemic theme:** `master`-vs-`main` branch triggers affect ≥5 workflows
  (`post-merge-status`, `workflow-trigger-lint`, `ripr`, `pr-plan`, `tokmd`).

## Metrics

- Candidate packets produced: ~22 across 6 scouts
- Filed (high-confidence, dedup-clean): **6**
- Held as report-only / handoff: **8** (medium/low or conditional)
- Discarded as noise / already-covered: ~4 (incl. completion-bleed → #757, DAP dups)
- Duplicate-of-existing surfaced during triage: closed #781 (setVariable), merged #757 (URI isolation)
- Scout GitHub mutations: **0** (read-only contract held)

## Operational notes for next pass

- **Tooling hygiene:** general-purpose scouts run in the main working dir and left two
  0-byte stray files (`@*`, `method-`) from shell glob/redirect mishaps. Prefer worktree
  isolation or stricter quoting for grep-heavy scouts; cleaned this run.
- **Next-pass candidates:** verify + file D2; confirm R3 reachability (repro-lab); write
  a repro for LSP1. Consider creating the lane's functional labels (`docs-drift`, `ci-ops`,
  `robustness`, `candidate-issue`) so triage is filterable.
- **Wave-two surfaces (held this run):** workspace-facts (#892/#893/#894/#900/#811-813)
  and editor UX (#917/#916/#932).
