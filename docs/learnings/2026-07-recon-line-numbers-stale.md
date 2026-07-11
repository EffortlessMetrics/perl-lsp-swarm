---
tags: [stale-context, recon, ground-truth, verification]
repos: [perl-lsp-swarm]
related: []
portable: true
article_asset: false
search_terms: [recon line numbers, stale checkout, 568 commits behind, phase5-recon, grep fresh main, builder verify source, symbol location]
---

# Recon line numbers from 568-commit-stale checkout don't resolve on fresh origin/main

**Date**: 2026-07
**Hazard class**: stale-context / ground-truth verification
**Portable lesson**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)

## What happened

During the phase5-recon pass (deep-review of builder changes), file:line references were extracted from a local checkout that was 568 commits behind origin/main. A builder agent following the recon's line-number citations would grep the exact symbol at the cited line on a fresh `origin/main` checkout and find the symbol had moved or no longer existed. The recon's line numbers were evidence-quality (relative to the stale checkout) but not ground-truth (relative to the code the builder would modify).

## Why

A recon pass extracts source locations and context as evidence for the builder. That evidence is only current against the checkout recon was run on. If recon is run on a stale branch (common in multi-agent sessions where recon runs on a branch that accumulates work), and the builder runs on a fresh `origin/main`, the line numbers diverge.

The fix is not to re-run recon; the fix is to grep symbols fresh on origin/main before relying on line-number citations from recon.

## Fix

Agent discipline:

1. **Before acting on recon's line-number citations**, grep the symbol fresh on origin/main to confirm the line number.
2. If the symbol has moved or no longer exists, either (a) run recon again on fresh main, or (b) grep the symbol manually and verify the builder's change is against the current code.

This is a one-line verification: `git fetch origin main && git show origin/main:path/to/file | grep -n symbol`.

## Spec impact

- [docs/agents/SPEC_UPDATE_CHECKLIST.md](../agents/SPEC_UPDATE_CHECKLIST.md): added acceptance criterion under "Input verification" — recon-cited line numbers must be verified against fresh origin/main before builder acts on them.
- [AGENT_CATALOG.md](.claude/agents/AGENT_CATALOG.md): added guidance to builder agents — "Recon citations are evidence-quality only. Before moving code at a cited line, grep the symbol fresh on origin/main to confirm the line number and context."

## Portable lesson

A recon pass produces citations that are valid only relative to the recon's source checkout. When the builder works from a different checkout (fresh main), the citations require re-verification. Line numbers age quickly; treat recon citations as signposts, not ground-truth coordinates.

- **Pattern**: [docs/concepts/signal-truth-verification.md](../concepts/signal-truth-verification.md)
- **Class**: Stale-context / signal-truth divergence
- **Generalization**: Any artifact that embeds coordinates (line numbers, SHA-based refs, URL fragments) from a source it does not control can age. Before using the coordinates, verify they still point to the right location in the current state of the system.

## Related PRs

- (phase5-recon incident — internal, not filed as PR)
