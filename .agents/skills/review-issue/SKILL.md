---
name: review-issue
description: Explicit atomic skill for challenging whether a researched concern should exist, is vision-aligned, has the correct semantic owner, and forms a coherent claim before planning.
---

# Review issue

Challenge the researched concern using the vision, authority, and slice-boundary lenses.

Ask:

- Is the problem real and still current?
- Is it already solved, duplicated, or superseded?
- Is the named owner and consumer correct?
- Is the proposed work at the right semantic layer?
- Is there a simpler existing path?
- Is the claim coherent and proportionate?
- Does it advance compiler-backed Perl tooling and editor trust?

A clean review is valid. Do not manufacture work to prove review effort.

## Routes

- `ISSUE_VALID` → `$issue-to-plan`
- `NARROW_OR_REFRAME` → update the issue, then `$issue-to-plan`
- `MORE_RESEARCH_NEEDED` → `$research-issue`
- `ALREADY_SATISFIED` / `SUPERSEDED` / `CLOSE_NO_ACTION` → return to `$deliver-pr`
- `MATERIAL_PRODUCT_DECISION` → preserve real options for the accountable owner
