# Context: #11633 — typed writer-preflight subject and decision core

WPF-01 of the writer-preflight train (#11617). This bundle lands one pure,
Rust-owned domain — `WriterPreflightSubject`,
`WriterPreflightObservationSet`, `WriterPreflightDecision`,
`WriterPreflightReason`, outcomes `PASS`/`BLOCKED`/`ADVISORY`/`NOT_PROVEN`
— at `xtask/src/writer_preflight/`. It performs no Git, filesystem,
process, shell, or network call anywhere under the module.

## Problem

Current preflight behavior is distributed across `just agent-preflight`
(storage-doctor only, prints success), `scripts/agent-preflight.sh`
(rejects inherited `CARGO_TARGET_DIR`), `scripts/agent-preflight.ps1`
(requires per-issue target dir, hard-codes a machine layout), and
`scripts/test-agent-preflight.sh` (older target-export expectations). The
paths disagree about policy and can print success without proving the
exact writer transition is safe. Before any caller migrates (#11634), the
repository needs one provider-neutral domain that says what subject is
being checked, which facts were observed, which are unavailable, and why
the transition is admitted, blocked, advisory, or not proven.

## Why this shape

- **Location**: lib-level domain directory (`xtask/src/writer_preflight/`),
  mirroring `xtask/src/publication_drift/`; both the xtask bin (#11634's
  live command) and future consumers reach it as `xtask::writer_preflight`.
  The bin-side tasks tree stays untouched.
- **Continuity with #3957** (landed): `writer_admission.rs` already owns
  the read-only snapshot/checks/guidance layer; its abbreviated-SHA match
  rule is restated verbatim here (lib/bin split prevents import) and its
  instrument-failure-never-PASS invariant becomes the `ObservationState`
  model. #3982 consumption remains #11635's job.
- **Purity**: no I/O imports exist in the module; decisions are total over
  `(subject, observations)` so #11634 adapters cannot leak platform
  behavior into policy and #11636 parity can key cells by digests.

## Schemas (exact)

- Subject: repository identity (common dir + optional canonical remote),
  operation (`read_only|create|resume|mutate`), claim identity (issue,
  branch, worktree path), expected base SHA, expected candidate head SHA,
  expected writer owner, selected capacity requirement
  (`focused_build|heavy_build` or none), executor-policy identity.
- Observations: fifteen facts, each wrapped in an availability state
  (`current|absent|unsupported|provider_unavailable|stale`) with a typed
  payload; values carry polarity themselves. No free-form text exists in
  the domain.
- Decision: schema version, outcome, sorted deduplicated closed-vocabulary
  reasons, canonical subject digest; plus a whole-decision digest.

The full field-level schemas live in code (`subject.rs`, `observation.rs`,
`decision.rs`) with `deny_unknown_fields` everywhere; unknown facts,
states, reasons, and outcome tokens fail deserialization instead of being
ignored.

## Decision table by requested operation

Required-current facts (unavailable/stale → `provider_unavailable_or_stale`,
outcome NOT_PROVEN):

| Fact | read_only | create | resume | mutate |
|---|---|---|---|---|
| repository identity matches subject | yes (block on mismatch) | yes | yes | yes |
| checkout relation current/binding | – | yes | yes | yes |
| head state binding (branch/protected) | current only | – | yes | yes |
| head SHA vs candidate expectation | – | current | yes | yes |
| base SHA vs expected base | – | yes | yes | yes |
| remote branch presence polarity | – | must be absent | must be present | current |
| worktree registration map | – | yes | yes | yes |
| same-candidate writer state | – | yes | yes | yes |
| index conflict state | – | clean required | clean required | clean required |
| working-tree disposition | – | yes | yes | yes |
| reserved local refs | – | yes | yes | yes |
| ambient Cargo overrides | – | must be empty | must be empty | must be empty |
| executor Cargo config | – | must match policy | must match policy | must match policy |
| capacity (only if requirement selected) | – | must meet | must meet | must meet |

Polarity rules produce BLOCKED reasons: canonical-checkout targeting →
`canonical_checkout_mutation`; protected/detached in-place HEAD →
`protected_or_detached_mutation`; wrong repo → `wrong_or_unknown_repository`;
empty mutation branch / missing-or-moved candidate / existing remote branch
on create → `wrong_or_unknown_candidate`; cross-subject path or branch
binding → `branch_worktree_mismatch`; reserved-ref shadow →
`reserved_local_ref_collision`; foreign active same-candidate writer →
`same_candidate_collision`; unmerged index → `unresolved_index_or_merge`;
unique-work risk → `unique_state_at_risk`; ambient overrides →
`ambient_execution_override`; executor mismatch →
`executor_configuration_mismatch`; failed selected capacity →
`critical_capacity_block`. Confirmed-absent base → `base_or_remote_not_proven`
(NOT_PROVEN class). Outcome precedence: Blocked > NotProven > Advisory >
Pass; a verified read-only subject gains `safe_read_only_subject`.

Advisories (never deny, never weaken a required fact):
`advisory_behind_only` (behind without divergence), 
`advisory_shared_stash_present`, `advisory_unrelated_host_load`.

## Provider ownership for every fact

This domain owns NO providers. Ownership maps forward to #11634: native
POSIX/Windows adapters populate exactly these fields reusing #3957/#9551
typed fact providers where they exist, gathering platform-specific evidence
only where no provider owns the fact; WSL/Git Bash is a separate host
profile. Path spelling normalization is adapter work — the core compares
identity tokens opaquely so no machine layout becomes policy.

## Ambient-versus-executor environment model

Ambient persistent Cargo overrides (`ambient_cargo_overrides`, provenance
class `persistent_config_file|inherited_environment|unknown_provenance`)
are distinct from executor-owned process-local configuration
(`executor_cargo_config` presence + policy id, #9548). Presence alone is
never provenance. A declared executor policy must be present and equal; an
undeclared one must be absent; matching process-local selection is never
rejected as ambient contamination (falsifiers 6–7).

## False-pass and false-block fixtures

False-pass guards (must refuse): stale/unavailable/absent required facts;
read-only PASS reused for mutation; checked-A/mutate-B path or branch
binding; moved candidate head; collision-as-advisory; unique-state-safe;
behind-only-as-block; unrelated-load-as-denial; exit-zero-over-typed-
unknown; ordering-sensitive identity; unknown-variant tolerance; human/JSON
divergence. False-block guards (must admit): exact matching executor-owned
process-local configuration; behind-only context; unrelated host load;
plain dirtiness without uniqueness risk; single legitimate worktree
registration on resume/mutate; abbreviated-SHA base prefixes ≥4 hex chars.

## Deterministic serialization/order rules

Closed serde types with fixed declaration order; no maps, no free-form
text. Vec-valued observations are scanned by membership/count only, never
order, so input order cannot change decision identity. Reasons are a
sorted deduplicated set in declaration (severity) order. Digests are
SHA-256 over canonical serde JSON (`digest_subject`, `Decision::digest`).

## Review map, rollback, and successor handoffs

- Review lenses: mint-a-false-PASS from missing/stale/cross-subject/
  merely-conventional evidence; mint-a-false-BLOCK against paved-road
  create/resume; purity (no I/O) audit; schema strictness audit.
- Rollback: revert the single commit; nothing consumes the module yet, no
  runtime/CI/GitHub state depends on it. Issue bodies remain authoritative.
- Successors: #11634 resolves subjects from CLI args, gathers one
  observation set per subject, calls `decide` once, renders human/JSON/
  explain from that object, and migrates front doors; #11635/#3982 consume
  decision+digest for compare-before-mutate dispositions; #11636 keys
  parity/race packets on subject+decision digests; #9569/#9576 retire
  contradictory surfaces after parity.
