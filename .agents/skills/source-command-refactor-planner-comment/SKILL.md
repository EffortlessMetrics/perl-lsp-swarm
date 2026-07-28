---
name: "source-command-refactor-planner-comment"
description: "Refactor planner step 3 — post the refactoring plan as a PR comment"
---

# source-command-refactor-planner-comment

Use this skill when the user asks to run the migrated source command `refactor-planner-comment`.

## Command Template

# Refactor Planner: Comment

Post your analysis and set the sign-off label.

## Steps

1. Post the refactoring plan as a PR comment (from analyze step output).

2. Set sign-off label (verified apply — see `/label-apply-verified`):
   ```
   /label-apply-verified pr <number> "refactor-planner-reviewed"
   ```

3. If no refactoring opportunities found, say so explicitly:
   "No material simplification opportunities. Builder's implementation is
   already clean and idiomatic. Recommend skipping green-refactor for this PR."
