# PERLLSP-SPEC-0017 — Provider freshness and fallback gates

## Problem
Exact completion must not consume stale or ambiguous semantic facts.

## Proposal
Formalize `FactFreshness` and provider exactness prerequisites.

## Exactness requirements
- Fresh fact generation,
- high confidence,
- source-backed provenance,
- no dynamic boundary,
- exact fallback state,
- package exists,
- receiver-class receipt coverage exists.

## Non-exact outcomes
Dynamic, stale, unknown, generated/no-source, or medium-confidence classes stay fallback/blocked.
