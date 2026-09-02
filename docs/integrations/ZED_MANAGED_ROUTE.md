# Zed managed route: contract, validator, and runbook

Authority: #8753 (infrastructure only). Asset subject: #7980. Exact-source
host lane: #7984. This document is the runbook for proving the **managed
public-artifact route** and the known-good cache-recovery cases inside real
Zed. The checked-in receipt template ships as `not_run`; the successor
evidence issue owns the actual Zed runs and the `pass` receipts.

## Route contract

Contract fixture: `.ci/fixtures/zed-perl-upstream/managed-route.v1.json`.

The managed route requires, for the exact subject under proof:

```text
extension route        = managed public artifact
explicit binary override = absent
worktree/PATH candidate  = absent
first-mile prior cache   = prior_managed_cache_absent
selected provider        = perllsp (perllsp --stdio)
other providers          = disabled
provider fallback        = forbidden
```

`failure_invariants` in the contract are fail-closed: every listed row must
stay `true`. A validator run against a contract with any invariant flipped,
or with a `resolution_route` other than `managed_public_artifact`, is an
error — never a degraded pass.

## Recovery authority

The contract owns exactly the known-good cache-recovery scenarios:

| scenario | meaning |
|---|---|
| `missing_asset` | managed cache points at an asset that no longer exists |
| `duplicate_matching_asset` | two equally matching assets are published |
| `wrong_target` | the published asset targets a different platform/channel |
| `checksum_mismatch` | downloaded bytes do not match the published digest |
| `unsafe_archive_member` | archive contains an unsafe path/member |
| `missing_expected_executable` | archive lacks the expected `perllsp` binary |
| `partial_download` | download ends before the full archive is present |

Each scenario must resolve back onto the managed route (fresh download or
last known-good managed binary) without ever falling back to a PATH,
worktree, or explicitly overridden binary.

The upstream acceptance list also names `extraction_failure` and
`launch_failure`. They are intentionally deferred from this infrastructure
contract: the current extension fixture has no safe injection seam for
forcing either failure without substituting a different implementation for
the real Zed route. The evidence issue must add those two scenarios before
claiming complete upstream recovery coverage; this document does not count
the seven listed scenarios as nine.

## Journeys

A `pass` receipt records all four journeys:

1. `first_mile_install` — clean machine state (`prior_managed_cache_absent`),
   exact release/target/asset selection, archive member/path check, installed
   binary digest/version, exactly one `perllsp --stdio`, core Zed journey.
2. `restart_cache_reuse` — restart reuses the managed cache without
   re-download; `restart_subject_sha256` equals `selected_subject_sha256`.
3. `normal_disable` — disabling the extension keeps the managed cache intact;
   `older_versions_preserved_until_launch` stays true.
4. `shutdown_no_orphan` — shutdown leaves no orphan `perllsp` process.

## Receipt lifecycle

Template: `.ci/fixtures/zed-perl-upstream/receipts/managed-route-template.json`.

- `result = "not_run"` — checked-in template; no `observed_at`, boundaries
  `not_proven`.
- `result = "pass"` — requires `observed_at`, the contract `sha256` (verified
  against the file by the validator), the managed `resolution_route`, exact
  subject digests, all four journeys, and the claim boundary
  `real_zed_managed_route = "proven_for_exact_subject"`.
- `mismatch` / `unsupported` / `not_proven` — bounded non-pass outcomes.

Receipts must never claim the official registry; that boundary stays
`not_proven` for this infrastructure lane.

## Validator

```bash
cargo run -p xtask --bin validate-zed-managed-route
cargo run -p xtask --bin validate-zed-managed-route -- \
  --contract .ci/fixtures/zed-perl-upstream/managed-route.v1.json \
  --receipt .ci/fixtures/zed-perl-upstream/receipts/managed-route-template.json
```

The CLI recomputes the contract file digest and rejects any receipt that
records a `contract digest mismatch`. The journey tests in
`xtask/tests/zed_managed_route.rs` exercise the same authority, including
mutations that must fail closed (path fallback, provider fallback allowed,
dropped recovery scenario, `pass` candidate on a worktree route).

## Claim boundary

This lane lands the contract, validator, and runbook only. Until the
successor evidence issue performs the real Zed runs, the honest receipt is
the checked-in `not_run` template and `real_zed_managed_route` stays
`not_proven`.
