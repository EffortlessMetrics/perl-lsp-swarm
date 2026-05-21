# PERLLSP-ADR-0005 — Dynamic receivers never authorize exact behavior

## Status
Accepted

## Decision
Dynamic receivers may preserve fallback, but never authorize exact completion, navigation, edits, rename, or safe-delete actions.

## Applies to
- Dynamic hash keys,
- dynamic method names,
- runtime class in `bless`,
- runtime import/source-filter boundaries,
- stale or unsupported generated/no-source equivalents.

## Consequence
Provider policy must block exact claims whenever a dynamic boundary is present.
