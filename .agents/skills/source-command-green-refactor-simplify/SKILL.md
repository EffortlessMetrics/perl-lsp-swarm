---
name: "source-command-green-refactor-simplify"
description: "Green refactor step 2 — refactor while keeping tests green"
---

# source-command-green-refactor-simplify

Use this skill when the user asks to run the migrated source command `green-refactor-simplify`.

## Command Template

# Green Refactor: Simplify

Apply refactoring changes. After EVERY change, verify tests still pass.

## Rules

1. **One logical change per commit** — rename in one commit, extract helper in another
2. **Test after every change** — `cargo test -p <crate>` must pass before moving on
3. **If a test fails, revert** — you changed behavior, not just structure
4. **Stay in the diff** — only refactor code the builder changed, not surrounding code
