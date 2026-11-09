# Cargo state retirement handoff

Managed worktrees and Cargo output do not necessarily have the same lifetime.
Removing a Git worktree does not make an external `target`, `build`, or temporary
path safe to delete. The worktree manager therefore classifies the selected row
first and writes an exact evidence-only retirement manifest only for storage with
matching admitted allocation provenance.

## Storage classes

Cleanup reports one of four classes:

- `MANAGED_ADMITTED`: the authoritative `cargo_admitted` planner resolves a
  worktree-private member with expected directory/file shapes. This is the only
  class allowed to produce a retirement manifest. A matching immutable
  `cargo_private_allocation.v1` record strengthens the evidence for later
  reclaim; older or non-Linux state may still be handed off with allocation
  provenance and reclaim eligibility explicitly `NOT_PROVEN`.
- `LEGACY_SHARED`: direct `cargo-safe` roles are present at its legacy DEVPLANE
  root. Cleanup preserves the worktree and storage for the separately owned
  migration/reclaim path.
- `NOT_PROVEN`: a private path has an unexpected type/link shape, the planner
  cannot establish the exact worktree member, or another required identity is
  ambiguous. Cleanup fails closed and preserves everything.
- `UNMANAGED`: no known external admitted or legacy Cargo state is present.
  Cleanup may remove the Git worktree after an immediate read-only revalidation;
  no retirement manifest is fabricated.

The legacy observation is retained separately even when a matching admitted
allocation exists. A legacy target can never masquerade as the worktree-private
admitted member.

## Managed-admitted handoff

For a `MANAGED_ADMITTED` cleanup candidate,
`scripts/worktree-manager.py cleanup`:

1. Reuses `scripts/cargo_admitted.py::resource_plan` through a worktree-bound,
   read-only Git observer. It does not copy the DEVPLANE hash/path formula.
2. Revalidates the source worktree, HEAD, branch/upstream, dirty and untracked
   state, worktree lock, admitted paths, allocation provenance, lease state, and
   native path identities.
3. Refuses automatic removal when the source is dirty, untracked, detached,
   locked, unpushed or not proven pushed; when an admitted lease is present; or
   when any managed path has an unexpected type or identity.
4. Writes one canonical `cargo_asset_retirement.v1` JSON manifest under the
   worktree-manager state directory using a same-directory temporary file,
   file `fsync`, atomic publication, and bounded readback. POSIX publication
   uses `os.replace` plus parent-directory `fsync`; Windows uses
   `MoveFileExW(MOVEFILE_WRITE_THROUGH)` without destination replacement.
5. Revalidates the same plan again immediately before `git worktree remove`.
6. Verifies afterward that every external Cargo asset retained the same
   existence/type/device/inode identity.

A failed worktree removal retains the manifest receipt but preserves the slot's
original lifecycle status, so a later cleanup pass can retry instead of stranding
the row in a status that is no longer selectable.

The manifest is evidence, not deletion permission. It always records
`reclaim_eligibility = NOT_PROVEN`; `scripts/worktree-manager.py` never deletes,
renames, truncates, adopts, or recreates target/build/tmp state. The separately
owned #11671 reclaim planner may consume and revalidate these records later.
The current admitted temporary root is repository-domain shared rather than
worktree-private; the manifest records that role explicitly.

## Dry run

```bash
python3 scripts/worktree-manager.py cleanup --dry-run
```

Dry-run performs the bounded observations and prints the storage class and exact
manifest path, when one would be required, but writes no manifest and removes no
worktree, branch, lease, process, or Cargo asset. A blocked candidate is reported
as `retirement=REVIEW` with stable blocker tokens.

## Manifest location

By default:

```text
.ops-perl-lsp/worktree-manager/cargo-retirements/
  <sha256-canonical-worktree>/
    <source-head>-<cleanup-plan-fingerprint>.json
```

The file binds the canonical repository/common Git identity, source worktree,
source HEAD and branch state, selected cleanup record, exact DEVPLANE member and
output paths, native identities, allocation record, lease evidence, legacy
storage observation, and the `cargo_admitted.py` source digest that produced the
plan.

Already-missing worktrees are retained in manager state as
`missing_no_retirement_manifest`. Absence is not converted into a successful
handoff or inferred reclaim authority.

## Proof

Run the focused unit and real-Git controls with:

```bash
python3 scripts/tests/test_cargo_retirement.py
bash scripts/tests/test-worktree-manager.sh
```

The focused suite covers deterministic zero-mutation planning, all four storage
classes, sibling-target substitution, active lease refusal, linked-path refusal,
unpushed source refusal, atomic replace failure, post-removal identity
verification, and replacement identity detection. The worktree-manager suite
continues to exercise the existing allocation, ownership, locking, and
atomic-state controls through the bundled CLI entrypoint.
