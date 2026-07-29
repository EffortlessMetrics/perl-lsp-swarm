---
name: "source-command-green-refactor-verify"
description: "Green refactor step 3 — run all tests, clippy, fmt"
---

# source-command-green-refactor-verify

Use this skill when the user asks to run the migrated source command `green-refactor-verify`.

## Command Template

# Green Refactor: Verify

Confirm the refactoring didn't break anything.

## Steps

1. Run tests:
   ```bash
   cargo test -p <crate>
   ```

2. Run clippy:
   ```bash
   cargo clippy -p <crate> --tests
   ```

3. Run formatter:
   ```bash
   cargo xtask fmt
   ```

4. If any test fails, your refactoring changed behavior. Revert and try a smaller change.
