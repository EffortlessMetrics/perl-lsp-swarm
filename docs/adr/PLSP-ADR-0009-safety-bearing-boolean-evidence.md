# PLSP-ADR-0009: Safety-bearing booleans require independent evidence

Status: accepted
Date: 2026-11-02
Owner: perl-lsp maintainers
Tracking issue: [#14304](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14304)
Related issues: [#14294](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14294), [#13303](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13303)

## Context

Several `xtask` admission and closeout surfaces serialize safety decisions into
JSON or typed structs. When a load-bearing field is a bare `bool` (or digest)
supplied by the same caller that wants the pass outcome, validation can check
shape and pairwise consistency only. That pattern is fail-open: the document
can claim safety without an independent witness that could falsify the claim.

Wave-1 and wave-2 reviews on the authority-transfer closeout engine and writer
preflight domain surfaced the same root cause across multiple files. A
repo-wide principle prevents each new surface from re-deriving the anti-pattern.

## Decision

Every self-declared boolean (or equivalent pass/fail flag) in a **safety-bearing
document** must satisfy one of:

1. **Independent evidence path** — the field's truth value is derivable from a
   source other than the field itself. Acceptable sources include witness
   structures parsed from artifact bodies or read-only host queries (`git`,
   filesystem facts). Peer fields in the same caller-supplied request are not
   independent, even when bound by deterministic computation; a pair of
   caller-supplied fields is a partial bind. The source must be able to
   contradict a lie in the declared field.
2. **Documented exemption** — a comment adjacent to the field (or on the type)
   explains why pairwise or syntactic validation is sufficient for that
   specific claim, with an issue link when the exemption is temporary.

Safety-bearing documents include closeout requests, writer preflight observation
packets, generated status contracts, and any other serialized gate whose pass
outcome authorizes mutation, merge, or authority transfer.

### Review gate

Substantive review of changes that introduce or extend safety-bearing documents
**must refuse** new load-bearing booleans that lack (1) or (2). This ADR is the
review authority until a dedicated Clippy lint lands (#14304 follow-up).

### Default impl rule

Types whose fields gate safety must not derive `Default` when the default value
is pass-permitting. Adapters must construct every load-bearing field explicitly.

## Known instances (inventory)

| ID | Location | Acceptance criterion | Status on `main` |
|----|----------|----------------------|------------------|
| WP-1 | `xtask/src/writer_preflight/decision.rs:619` — resume/mutate with empty worktree map | `evaluate_registration_conflicts` refuses when `branch_matches == 0` for non-Create operations | Fixed; regression: `resume_or_mutate_with_unregistered_candidate_refuses` |
| WP-2 | `xtask/src/writer_preflight/observation.rs:142–148` — `WorkingTreeDisposition` | Default-impl rule: no `Default` derive/impl; fields set explicitly (see type comment) | Type comment on `main`; contract test lands with this ADR |
| WP-3 | `xtask/src/tasks/writer_admission.rs:854–859` — `compute_guidance` tri-state | Returns `Some` only when exactly one worktree matches | Fixed; bin-side witness retained |
| CO-1 | `xtask/src/authority_transfer_closeout.rs:296`, `authority_transfer_closeout.rs:1046` — `subject_matches_candidate` | Cross-checked against `executed_against_sha` vs `request.candidate.head_sha` | Partial bind; regression: `controls_executed_elsewhere_cannot_claim_subject_alignment` |
| CO-2 | Same file `:292–294` — `red_before_evidence`, `passes_only_intended_implementation` | Required true for load-bearing controls; future: bind to executed proof receipts | Shape gate only; tracked in #14294 |
| CO-3 | `xtask/src/authority_transfer_closeout.rs:630` — artifact digest binds | Recomputed SHA-256 of supplied bodies vs bound digests | Fixed; `bind_artifacts`, `evaluated_profile_must_be_the_digest_bound_artifact` |
| CO-4 | Same file — wave-1 sites (digest case, packet subject, forbidden surface prefix) | Typed results with independent binds/tests | Fixed; see module tests |
| CO-5 | Same file `:319–320`, `:578` — `LiveCandidateState` vs `observation_digest` | Pairwise consistency enforced in `validate_request` | Read in validation; tracked in #14294 for full evidence path |

Remaining closeout fields without full independent paths are owned by #14294.
This ADR does not claim that issue is closed.

## Consequences

- New safety-bearing booleans need an evidence plan in the PR body or adjacent
  documentation.
- `xtask/AGENTS.md` lists pinned regression instances and points agents at their
  regression tests (full inventory lives in this ADR).
- Machine contract tests in `xtask/tests/safety_bearing_boolean_contract.rs`
  check ADR headings, inventory IDs, location anchors, and key regression test
  names.

## Follow-up

- Optional Clippy lint or `xtask` policy checker for new `bool` fields in
  designated modules (#14304 acceptance item 3, mechanical enforcement).
- Complete independent-evidence binds for remaining closeout fields (#14294).
