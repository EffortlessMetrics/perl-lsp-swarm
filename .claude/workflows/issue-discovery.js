// issue-discovery.js — Deterministic Issue Discovery / Bug Scout Desk fan-out
// for perl-lsp-swarm.
//
// Ports the manual `/issue-discovery` command (.claude/commands/issue-discovery.md)
// into a saved, reproducible Workflow. The command tells the orchestrator to
// hand-issue six `Agent(subagent_type: "scout-find-*")` calls "in one message";
// this workflow makes that fan-out deterministic and entry-independent — same
// six read-only scouts, same triage contract, no re-derivation by hand each run.
//
// Capability-read-only: every phase-1 worker is bound (via agentType) to an
// existing `scout-find-*` agent definition, each of which is read-only on
// product code except filing/updating its own candidate issue one at a time
// (see .claude/agents/AGENT_CATALOG.md "Discovery Scouts" and each
// scout-find-*.md frontmatter). The phase-2 synthesizer only reads the six
// summaries and produces a triage table — it does not touch GitHub itself.
//
// Usage:
//   Invoke via the Claude Code Workflow tool with name="issue-discovery" and
//   optional args:
//     { wave: "first" }
//   The workflow currently always runs the fixed first-wave fan-out (the same
//   six scouts /issue-discovery Step 1 names). `args.wave` is accepted for
//   forward compatibility with the command's `[wave]` argument-hint and is
//   surfaced to each scout for context only — it does not yet subset which
//   scouts run (subsetting is out of scope for this increment; see issue #3778).
//
// Cross-references:
//   .claude/commands/issue-discovery.md — the manual fan-out this workflow ports
//   docs/reference/ISSUE_DISCOVERY_DOCTRINE.md — packet format, confidence tiers, triage rules
//   .claude/agents/AGENT_CATALOG.md — "Discovery Scouts (6)" roster this fans out over
//   .claude/workflows/spec-builder.js — the parallel-fan-out + synthesizer pattern this mirrors
//   Issue #3778 — meta-loop front-end modernization plan (this is the first increment)

export const meta = {
  name: "issue-discovery",
  description:
    "Deterministic port of the manual /issue-discovery fan-out for perl-lsp-swarm. Runs the " +
    "six read-only scout-find-* discovery scouts (DAP, LSP, parser, ci-ops, robustness, " +
    "docs-receipt-drift) in parallel over their assigned surfaces, then a triage synthesizer " +
    "merges the results into one triage table and a handoff list for the Issue Research / " +
    "Plan Review Desk. Every worker is capability-read-only except filing its own candidate " +
    "issue packets.",
  whenToUse:
    "When starting a discovery sweep — the first wave of the Issue Discovery / Bug Scout Desk " +
    "(docs/reference/ISSUE_DISCOVERY_DOCTRINE.md) — instead of the orchestrator hand-issuing " +
    "six Agent() calls from .claude/commands/issue-discovery.md by hand each time. Skip this " +
    "workflow for a single-surface deep dive (invoke the one relevant scout-find-* agent " +
    "directly) or for anything past discovery — this workflow never plans, builds, or marks " +
    "an issue builder-ready.",
  phases: [
    {
      id: "phase-1-fan-out",
      name: "Discovery Scout Fan-Out",
      description:
        "Six independent read-only discovery scouts, each sweeping a distinct surface for " +
        "evidence-backed candidate defects. True fan-out — scouts do not depend on each " +
        "other and run in parallel, worktree-isolated per their own agent definitions. " +
        "Each scout follows its own todo list: read source/tests/receipts, form candidate " +
        "findings, dedupe by failure mode (gh issue/pr search), write up to 5 packets, file " +
        "at most 2 high-confidence packets via the Candidate Issue template " +
        "(.github/ISSUE_TEMPLATE/candidate_issue.yml, labels candidate-issue + " +
        "swarm-discovered), and return a summary — never a builder-ready spec.",
      model: "haiku",
      parallel: true,
      agents: [
        {
          label: "A-dap-gaps",
          phase: "phase-1-fan-out",
          agentType: "scout-find-dap-gaps",
          prompt:
            "Discovery sweep: ${args.wave}.\n\n" +
            "Sweep DAP stack/scopes/variables/lifecycle/transport surfaces for " +
            "evidence-backed candidate defects. Follow your own todo list. Stay read-only " +
            "except for filing/updating a candidate issue one at a time (max 2 filed, " +
            "high-confidence only). Do not mark anything builder-ready, close issues, or " +
            "touch code.\n\n" +
            "Return: your packet list (finding · evidence · impact · minimal DAP sequence · " +
            "suspected root area · dedupe notes · confidence) and the issue URL(s) you filed, if any."
        },
        {
          label: "B-lsp-gaps",
          phase: "phase-1-fan-out",
          agentType: "scout-find-lsp-gaps",
          prompt:
            "Discovery sweep: ${args.wave}.\n\n" +
            "Sweep LSP document-state/URI isolation/completion/hover/code-action/semantic-token " +
            "surfaces for evidence-backed candidate defects. Follow your own todo list. Stay " +
            "read-only except for filing/updating a candidate issue one at a time (max 2 filed, " +
            "high-confidence only). Do not mark anything builder-ready, close issues, or touch code.\n\n" +
            "Return: your packet list (finding · evidence · impact · minimal repro · suspected " +
            "root area · dedupe notes · confidence) and the issue URL(s) you filed, if any."
        },
        {
          label: "C-parser-gaps",
          phase: "phase-1-fan-out",
          agentType: "scout-find-parser-gaps",
          prompt:
            "Discovery sweep: ${args.wave}.\n\n" +
            "Sweep parser/AST/NodeKind/recovery/fixture surfaces for evidence-backed candidate " +
            "defects. Follow your own todo list. Stay read-only except for filing/updating a " +
            "candidate issue one at a time (max 2 filed, high-confidence only). Do not mark " +
            "anything builder-ready, close issues, or touch code.\n\n" +
            "Return: your packet list (finding · evidence · impact · minimal Perl snippet · " +
            "suspected root area · dedupe notes · confidence) and the issue URL(s) you filed, if any."
        },
        {
          label: "D-ci-ops-gaps",
          phase: "phase-1-fan-out",
          agentType: "scout-find-ci-ops-gaps",
          prompt:
            "Discovery sweep: ${args.wave}.\n\n" +
            "Sweep workflow routing/gate classification/path filters/stale labels/cleanup/" +
            "runner-capacity surfaces for evidence-backed candidate defects. Follow your own " +
            "todo list. Stay read-only except for filing/updating a candidate issue one at a " +
            "time (max 2 filed, high-confidence only). Do not mark anything builder-ready, " +
            "close issues, or touch code/workflows.\n\n" +
            "Return: your packet list (finding · evidence · impact · suspected root area · " +
            "dedupe notes · confidence) and the issue URL(s) you filed, if any."
        },
        {
          label: "E-robustness-gaps",
          phase: "phase-1-fan-out",
          agentType: "scout-find-robustness-gaps",
          prompt:
            "Discovery sweep: ${args.wave}.\n\n" +
            "Sweep parser/lexer/LSP/DAP/transport surfaces for panic/DoS/unsafe-indexing/" +
            "byte-boundary-slicing/unbounded-growth candidates. Follow your own todo list. Stay " +
            "read-only except for filing/updating a candidate issue one at a time (max 2 filed, " +
            "high-confidence only). Do not mark anything builder-ready, close issues, or touch code.\n\n" +
            "Return: your packet list (finding · evidence · impact · minimal adversarial input · " +
            "suspected root area · dedupe notes · confidence) and the issue URL(s) you filed, if any."
        },
        {
          label: "F-docs-receipt-drift",
          phase: "phase-1-fan-out",
          agentType: "scout-find-docs-receipt-drift",
          prompt:
            "Discovery sweep: ${args.wave}.\n\n" +
            "Compare status docs against receipts for drift and basis conflicts. Follow your " +
            "own todo list. Stay read-only except for filing/updating a candidate issue one at " +
            "a time (max 2 filed, high-confidence only). Do not mark anything builder-ready, " +
            "close issues, or edit the docs/receipts you are auditing.\n\n" +
            "Return: your packet list (finding · evidence · impact · suspected root area · " +
            "dedupe notes · confidence) and the issue URL(s) you filed, if any."
        }
      ]
    },
    {
      id: "phase-2-triage",
      name: "Triage Synthesis",
      description:
        "Triage synthesizer merges the six scout summaries from phase 1 into the one triage " +
        "table and handoff list /issue-discovery already specifies. Read-only: it consumes " +
        "the six text summaries, it does not file, close, relabel, or otherwise touch GitHub " +
        "or code itself — the scouts already filed their own candidate packets in phase 1.",
      model: "haiku",
      parallel: false,
      agents: [
        {
          label: "triage-synthesizer",
          phase: "phase-2-triage",
          prompt:
            "You are the triage synthesizer for this Issue Discovery / Bug Scout Desk wave " +
            "(${args.wave}). You have received six read-only discovery-scout summaries from " +
            "phase 1: A-dap-gaps, B-lsp-gaps, C-parser-gaps, D-ci-ops-gaps, E-robustness-gaps, " +
            "F-docs-receipt-drift.\n\n" +
            "Your job: produce the one triage table and handoff list /issue-discovery Steps 3-4 " +
            "specify. Do not build from any finding, do not file/close/relabel any issue " +
            "yourself — every mutation already happened (or did not happen) inside each scout's " +
            "own run.\n\n" +
            "1. Collect every candidate named by each scout's summary, including any filed " +
            "   issue number.\n" +
            "2. Dedupe by failure mode — never by file/theme/base-commit overlap. Two findings " +
            "   that touch the same area but represent distinct failure modes stay separate.\n" +
            "3. For each surviving candidate, assign exactly one next lane: keep · merge into " +
            "   existing issue · send to plan-review · send to architecture review · send to " +
            "   repro-lab · discard as noise.\n" +
            "4. Note confidence (high/medium/low, per the scout's own report) and whether the " +
            "   candidate is a duplicate of another candidate in this same wave.\n\n" +
            "Output format:\n\n" +
            "--- TRIAGE TABLE ---\n" +
            "| Candidate | Confidence | Duplicate? | Next lane |\n" +
            "|---|---|---|---|\n" +
            "<one row per surviving candidate>\n\n" +
            "--- HANDOFF LIST ---\n" +
            "Filed candidate issue numbers grouped by next lane, ready for the Issue Research / " +
            "Plan Review Desk. The headline metric is not volume — it is the share of filed " +
            "findings that survive plan review."
        }
      ]
    }
  ]
};
