# PERLLSP-SPEC-0001 — Parser Target Registry

## Requirement

`perl-parser-comparison` must support a generic parser target registry rather than hard-coded `v1/v2/v3` columns.

## Baseline and future targets

- Historical/default baseline: vendored tree-sitter C, pest legacy, v3 native.
- Future current-upstream fairness targets:
  - `ts-upstream-crate`
  - `ts-upstream-c`

## Claim boundary

This specification defines comparison rails only; parser behavior is unchanged.
