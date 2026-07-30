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
- Does it advance the applicable current repository vision and roadmap, including real-world Perl-tooling reliability, Perl-specific editor capability, embeddable Rust libraries, corpus-driven development, feature governance, or the development operating system where relevant?

A clean review is valid. Do not manufacture work to prove review effort.

## Durable write boundary

The review may propose narrowing, reframing, or closure. Only an admitted issue integrator applies those durable GitHub updates. A read-only reviewer returns the proposed issue delta and evidence through `READ_ONLY_RESULT`.

## Routes

- `ISSUE_VALID` → `$issue-to-plan`
- `NARROW_OR_REFRAME` → issue integrator applies the correction, then `$issue-to-plan`
- `READ_ONLY_RESULT` → return the proposed issue correction and evidence to the issue integrator
- `MORE_RESEARCH_NEEDED` → `$research-issue`
- `ALREADY_SATISFIED` / `SUPERSEDED` / `CLOSE_NO_ACTION` → return to `$deliver-pr`
- `MATERIAL_PRODUCT_DECISION` → preserve real options for the accountable owner
