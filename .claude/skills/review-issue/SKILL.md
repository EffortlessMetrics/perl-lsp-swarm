---
name: review-issue
description: Challenge a researched issue for vision, authority, slice boundary, duplication, and coherent claim boundaries before planning.
user-invocable: false
---

# Review issue

Use the vision, authority, slice-boundary, and claim-boundary lenses. Test whether the concern is real, current, correctly owned, proportionate, and aligned with the applicable current repository vision and roadmap. Check explicitly whether it is already solved, duplicated, or superseded, and whether the proposed concern forms one coherent acceptance-and-rollback claim. That may include real-world Perl-tooling reliability, Perl-specific editor capability, embeddable Rust libraries, corpus-driven development, feature governance, or the development operating system. A clean review is valid.

The review may propose narrowing, reframing, or closure. Only an admitted issue integrator applies those durable GitHub updates. A read-only reviewer returns the proposed issue delta and evidence through `READ_ONLY_RESULT`.

## Routes

- `ISSUE_VALID` → `issue-to-plan`
- `NARROW_OR_REFRAME` → issue integrator applies the correction, then `issue-to-plan`
- `READ_ONLY_RESULT` → return the proposed issue correction and evidence to the issue integrator
- `MORE_RESEARCH_NEEDED` → `research-issue`
- `ALREADY_SATISFIED` / `SUPERSEDED` / `CLOSE_NO_ACTION` → `deliver-pr`
- `MATERIAL_PRODUCT_DECISION` → preserve real options for the accountable owner
