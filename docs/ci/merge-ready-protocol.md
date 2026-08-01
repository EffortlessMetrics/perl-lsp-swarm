# Merge-ready receipt protocol

`merge-ready` is bound to a receipt for an exact PR head, exact base lineage, and exact gate graph version.

## Receipt

Receipt JSON uses `.ci/receipts/schemas/merge-readiness.schema.json` and includes:

- `check`: `merge-readiness`
- `schema_version`
- `event`
- `pr`
- `head_sha`
- `base_sha`
- `gate_graph_version`
- `required_checks`
- `review_evidence`
- `blocker_labels_absent`
- `verdict`
- `expires_when`

## Required checks source

This repository's `main` branch is gated by **two separate GitHub mechanisms**,
and a merge is blocked by the union of both. Conventional required checks are
read from `.ci/policies/required-checks.toml` first.

Only entries explicitly marked `required = true` are treated as required. The
current proof-floor contexts, by source mechanism, are:

Classic branch protection (`GET /repos/{owner}/{repo}/branches/main/protection`):

- `Perl LSP Rust Small Result`
- `ripr+ New Gap Gate`

Ruleset `main` (id `16664791`, `GET /repos/{owner}/{repo}/rules/branches/main`):

- `Compile All Targets (bit-rot guard)`
- `Conflict marker check`
- `validate-title`

This list must match the live branch protection and ruleset state exactly. It
is not self-verifying: nothing compares it against GitHub, so a context added
to either surface without a corresponding `required = true` entry here
silently understates the gate set in every emitted receipt's `required_checks`
inventory and in `gate_graph_version`, which is hashed over this file. When a
required context is added or removed on either surface, update this file in
the same change. See issue #5418 for this gap's discovery. Reading the live
surfaces instead of trusting this checked-in list remains unbuilt, and is the
recurrence risk this leaves open.

`Codecov / Patch 95` is the repo-owned advisory coverage job. `codecov/patch`
is the external Codecov status context posted after Codecov processes an
explicit coverage upload. Both are advisory and must not block normal PR or
merge-queue flow.

## Gate graph versioning

`gate_graph_version` is a deterministic hash over:

- `.ci/policies/required-checks.toml`
- `.ci/policies/**`
- `.ci/gates.d/**` (when present)
- required-style workflow files under `.github/workflows/**`

Inputs are normalized for line endings and sorted to exclude nondeterministic ordering.

## xtask commands

```bash
cargo xtask merge-ready emit --pr <N> --receipt target/receipts/merge-readiness.json
cargo xtask merge-ready verify --pr <N>
cargo xtask merge-ready verify --fixture xtask/tests/fixtures/merge-ready/valid.json
cargo xtask merge-ready reconcile --dry-run
cargo xtask merge-ready reconcile --apply
```

Verification statuses:

- `valid`
- `stale_head`
- `stale_base`
- `stale_gate_graph`
- `blocked`
- `missing`

## Rollout mode

Reconciliation defaults to advisory dry-run. Apply mode can be enabled explicitly.

See also: [Merge-train protocol](./merge-train-protocol.md).
