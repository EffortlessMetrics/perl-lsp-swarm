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
| `extraction_failure` | extraction fails before a complete executable is available |
| `launch_failure` | the attempted managed candidate fails to launch successfully |

Each scenario must resolve back onto the managed route (fresh download or
last known-good managed binary) without ever falling back to a PATH,
worktree, or explicitly overridden binary.

All nine scenarios remain in the denominator, including extraction and launch
failure. If a real extension failure cannot be injected safely, the evidence
issue must report `not_proven`; it cannot shrink the contract or substitute a
mock route to obtain `pass`.

Each recovery observation is an object with `result = "pass"`,
`known_good_before_sha256`, `known_good_after_sha256`, and
`restored_subject_sha256` matching the selected installed binary. It also
records `failed_candidate_identity` (the attempted request or candidate, even
when no downloadable bytes exist), `failed_candidate_selected = false`,
`fallback_server_id = null`, `rejection_reason`, `restored_result = "pass"`,
and a nonempty `evidence` reference. Selection means adoption as the working
server; attempting to launch a candidate in the launch-failure case does not
count as successful selection. Bare `"pass"` strings cannot establish these
facts. The before/after hashes refer to retained known-good bytes, and the
restored hash refers to the binary used after recovery.

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
  `not_proven`. All nine recovery slots remain present with `result = "not_run"`
  and null evidence fields; none may be omitted or claim an observation.
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

Passing receipts additionally require explicit upstream evidence:

```bash
cargo run -p xtask --bin validate-zed-managed-route -- \
  --receipt /path/to/managed-route.json \
  --asset-receipt /path/to/managed-assets.json \
  --host-receipt /path/to/exact-source-host.json \
  --asset-contract .ci/fixtures/zed-perl-upstream/managed-downloads.v1.json \
  --python python3
```

The CLI requires Python 3.11 or newer for a passing candidate (`python` on
Windows, `python3` elsewhere by default). Missing interpreters or upstream
validation failures reject the candidate. The `not_run` template requires no
Python process or upstream files.

The CLI reads each input once, hashes those bytes, and validates the captured
documents. It invokes only `validate-receipt` in the existing
`scripts/zed_public_asset_receipts.py` authority against temporary copies of
the captured asset receipt and contract. This does not download assets or
launch an editor/server. The host receipt must independently pass
`zed_host_compat::validate_pass` for the exact-source development extension
using `managed_download`, with absent prior managed cache.

`upstream.asset_receipt_sha256` and `upstream.host_receipt_sha256` bind the
exact upstream documents. The subject binds Zed version/build, extension
version/candidate/WASM, workspace fixture identity/digest, release version,
target, archive digest, installed path, and installed binary digest to those
authorities. The selected archive row must match the checked public asset
contract's release, target/platform, asset identity, and member. The host
command must select that managed install path. `asset_sha256` hashes the
downloaded archive; `binary_sha256`, selected/restart digests, and recovery
digests hash the extracted executable. These are different subjects.

The journey tests invoke the just-built CLI with synthetic positive receipts
and independent mutations, including missing evidence, mismatched bytes or
subjects, rejected upstream receipts, path fallback, and recovery gaps. They
prove validator behavior only. Neither matching hashes nor synthetic tests
establish that the recorded real-host observations occurred; the evidence
issue remains responsible for collecting and reviewing those observations.

## Claim boundary

This lane lands the contract, validator, and runbook only. Until the
successor evidence issue performs the real Zed runs, the honest receipt is
the checked-in `not_run` template and `real_zed_managed_route` stays
`not_proven`.
