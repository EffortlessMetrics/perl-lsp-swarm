# PERLLSP-SPEC-0021 — Generation-Aware Read Cancellation

## Goal
Cancel stale read requests created against older document generations.

## Applies to
hover, completion, definition, declaration, typeDefinition, implementation, references, and semantic tokens where practical.

## Rule
If a request was created against an older open-document generation than the current one, cancel before execution unless provider contract explicitly allows stale fallback.

## Outcome
Latest user intent wins under rapid edits/cursor movement.
