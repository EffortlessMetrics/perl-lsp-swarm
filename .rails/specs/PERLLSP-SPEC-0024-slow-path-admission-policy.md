# PERLLSP-SPEC-0024 — Slow-Path Admission Policy

## Goal
Prevent unreviewed latency regressions in critical runtime paths.

## Required PR questionnaire for touched surfaces
- Does this add synchronous work?
- Why must it be synchronous?
- Can it be latest-only?
- Can it be deferred?
- Can it be cancelled?
- Can it be measured?
- Which latency receipt proves no regression?

## Surfaces
`didOpen`, `didChange`, diagnostics, scheduler, semantic tokens, workspace indexing, runtime launch/config, file watchers, external tools.
