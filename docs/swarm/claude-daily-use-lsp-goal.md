# Claude long-running goal: daily-use LSP product quality

This is the durable launch charter for the **daily-use LSP product-quality lane**.
It does not replace [the active goal manifest](../../.perl-lsp/goals/active.toml),
change provider behavior, or assign work in the compiler and freshness lanes.

The goal deliberately names an outcome and a work-selection loop rather than a
first PR. Live `main`, status documents, receipts, and GitHub state decide the
first and subsequent work.

## Launch

Use Claude Code 2.1.203 or newer so `--effort ultracode` is available:

```powershell
claude --effort ultracode
```

Then paste the `/goal` condition below. `/goal` itself requires Claude Code
2.1.139 or newer.

Recommended session setup:

- Keep dynamic workflows enabled.
- Use a small workflow-size guideline by default; Claude may scale up when work
  decomposes into genuinely independent tasks.
- Do not set `CLAUDE_CODE_SUBAGENT_MODEL` for this campaign. That environment
  variable overrides per-invocation and project-agent model routing.
- Start from a clean checkout of current `main`.
- Resume an interrupted active goal with `claude --resume` or
  `claude --continue`.

## Orchestration policy

Claude owns the choice of method. The goal supplies defaults, not a rigid relay:

- Use ultracode workflows for independent discovery, broad audits, migration
  fan-out, and adversarial verification.
- Prefer the existing project agents under `.claude/agents/`.
- Route cheap search, mechanical verification, protocol fact-checking, and
  narrow review to Haiku-class agents.
- Route planning, implementation, synthesis, refactoring, and deep correctness
  review to Sonnet-class agents.
- Override those defaults when the task shape, evidence quality, cost, or model
  capability warrants it.
- Prefer one warm builder plus one independent directional review for
  sequential or same-file work.
- Use worktree-isolated parallel workers only when the tasks can land without
  file conflicts.
- Use agent teams only when workers need to communicate or challenge one
  another; ordinary subagents or workflows are cheaper for focused work.
- Avoid agent theater. More agents are not evidence.

## Copy-ready `/goal`

The condition below is under Claude Code's 4,000-character goal limit.

```text
/goal Drive the daily-use LSP product-quality lane in EffortlessMetrics/perl-lsp-swarm to governed green through serialized, evidence-backed PRs.

Outcome: a current SHA-anchored board and hard-asserted real-workspace scorecard prove that the core editor loop—completion, hover, definition, references, diagnostics, and safe-edit refusal—returns correct, protocol-valid, fresh-or-honestly-fallback answers on representative Perl workspaces. Remaining failures are explicitly owned by this lane or routed to compiler, freshness, or release work.

Authority: before each decision fetch origin/main; read docs/project/status/real_perl_editor_trust_v1.md, ux_capability_dashboard.md, provider_confidence_matrix.md, provider_cutover.md, .perl-lsp/goals/active.toml, live issues/PRs, and any daily-use board/scorecard created by this campaign. Handoffs are hints, not truth.

Lane boundary: own provider fidelity, LSP protocol/UTF-16/range/capability contracts, real-workspace fixtures, scorecard generation, honest fallback/blocker behavior, and installed-client smoke proof. Do not implement parser/HIR/compiler semantics, Perl-core harness/runtime buckets, ParsedSnapshot/text_sync/scheduler/readiness architecture, broad fact-class promotion, DAP, or release publishing. When a red needs one of those, produce a minimal reproducible handoff and route it; do not absorb it.

Operating loop: finish/classify any active same-lane PR first. Otherwise select the highest-impact measured red: receipt/scorecard integrity, protocol-invalid answer, false-exact/unsafe result, silent empty success, cross-file navigation failure, completion/diagnostic quality, then client smoke. Make one scoped general fix, verify focused behavior, update board/scorecard, open one PR, obtain required proof, merge, fetch main, and repeat. Never hard-code a fixture or claim broad support from one case.

Autonomy: choose the orchestration that best fits each step. Use ultracode workflows when independent fan-out or adversarial verification helps; prefer existing project agents. Route cheap discovery/mechanical/external fact checks to Haiku-class agents and implementation, synthesis, planning, or deep correctness review to Sonnet-class agents, but override these defaults when evidence, cost, or task shape warrants. Prefer one warm builder plus one independent directional review for sequential/same-file work; use worktree-isolated parallel agents only for independent tasks. Do not create agent theater.

Completion requires Claude to surface evidence in the transcript that:
1) a daily-use LSP board and generated/derivable SHA-anchored scorecard exist and are current;
2) selected real-workspace core-loop expectations are hard assertions, not soft logs;
3) measured responses have valid UTF-16 positions/ranges and advertised protocol shapes;
4) measured exact answers have zero known false-exact cases, and unsupported/stale/dynamic cases fall back or refuse explicitly;
5) measured rename/safe-delete workflows return zero unsafe edits;
6) at least two materially different real-workspace fixtures cover the selected core loop;
7) required checks pass on each merged PR; remaining reds are owned/routed and documented; git status is clean.

Required merge proof: Perl LSP Rust Small Result and ripr+ New Gap Gate. Treat coverage and unrelated advisory checks as non-blocking unless they expose a scoped defect. Do not stop after one PR or because context compacts. Before refresh, commit/revert scoped work and persist PR, proof, blockers, and next selection inputs. Finish only when all criteria hold or report a precise external blocker after no further safe progress is possible.
```

## Claim boundary

Achieving this goal proves a governed, measured daily-use editor loop for the
selected real-workspace scenarios. It does **not** prove broad Perl compiler or
runtime conformance, solve the asynchronous parse/freshness architecture, grant
new provider fact-class promotions, or publish a release.

The compiler lane produces language facts. The freshness lane makes those facts
current and non-blocking. This lane verifies that the LSP delivers them
correctly, usefully, and honestly to editors.

## References

- [Claude Code goals](https://code.claude.com/docs/en/goal)
- [Claude Code dynamic workflows and ultracode](https://code.claude.com/docs/en/workflows)
- [Claude Code custom subagents](https://code.claude.com/docs/en/sub-agents)
- [Claude Code agent teams](https://code.claude.com/docs/en/agent-teams)
- [Anthropic: effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents)
