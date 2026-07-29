---
name: review-issue
description: Challenge a researched issue for vision, authority, duplication, and coherent claim boundaries before planning.
user-invocable: false
---

# Review issue

Use the vision, authority, and slice-boundary lenses. Test whether the concern is real, current, correctly owned, proportionate, and aligned with the product. A clean review is valid.

## Routes

- `ISSUE_VALID` → `issue-to-plan`
- `NARROW_OR_REFRAME` → update, then `issue-to-plan`
- `MORE_RESEARCH_NEEDED` → `research-issue`
- `ALREADY_SATISFIED` / `SUPERSEDED` / `CLOSE_NO_ACTION` → `deliver-pr`
- `MATERIAL_PRODUCT_DECISION` → preserve real options for the accountable owner
