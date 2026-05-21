# PERLLSP-SPEC-0014 — Method-return fact promotion ladder

## Current state
Method-return facts are substrate and generally medium-confidence; completion preserves fallback.

## Promotion ladder
Promote narrowly from direct static constructor returns first, then lexical assignment variants with proof obligations.

## Guardrails
- Same-package conditional branches may promote after receipts.
- Mixed-package conditionals stay union fallback.
- Dynamic constructor class and unscoped bare assignment remain blocked/fallback.

## Requirement
Each promotion step requires exact/fallback/dynamic receipts before cutover.
