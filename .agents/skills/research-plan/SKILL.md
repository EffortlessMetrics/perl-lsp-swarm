---
name: research-plan
description: Explicit atomic skill for verifying a proposed plan against current APIs, ownership, consumers, overlap, proof seams, compatibility, and external constraints.
---

# Research plan

Verify the proposed implementation direction against current source.

Check:

- named APIs, files, types, and command surfaces actually exist;
- the intended owner is the smallest correct owner;
- consumers and production wiring are complete;
- overlapping PRs or migrations do not supersede the plan;
- compatibility, data, schema, packaging, and support boundaries;
- the proposed proof seam can discriminate the claim;
- rollback and deletion gates are credible.

Correct the plan where source authority differs from the issue theory.

## Routes

- `PLAN_RESEARCHED` → `$review-plan`
- `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `MORE_RESEARCH_NEEDED` → continue with the named uncertainty
- `NOT_PROVEN` → preserve the missing authority or instrument
