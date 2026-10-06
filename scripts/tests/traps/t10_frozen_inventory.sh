#!/usr/bin/env bash
# T10 -- Hand-editing the frozen inventory to "fix" the policy gate.
#
# Mishandling: edit docs/policy/NON_RUST_INVENTORY.md counts/rows by hand so
# the policy check turns green. Guard under test: the required-tier
# non_rust_inventory_check gate, whose frozen-pointer comparison bails when
# the working-tree pointer differs from the base blob and instructs a
# pointer restore instead of a hand edit.
#
# The trap is fully hermetic: the frozen comparison is demonstrated on a
# fixture repo in scratch (the mechanism trips on any edit regardless of
# content), and the real guard is pinned statically (xtask source + gate
# wiring + regression test), since the executable is cargo-built and this
# suite performs no cargo build. No git command ever touches $TRAP_ROOT, so
# the trap passes under any shell/git flavor (notably WSL git against a
# Windows-created worktree pointer, which cannot resolve the gitdir).
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
POINTER='docs/policy/NON_RUST_INVENTORY.md'

# The real pointer must exist where the gate expects it.
POINTER_EXISTS=0
[[ -f "$TRAP_ROOT/$POINTER" ]] && POINTER_EXISTS=1

# Frozen-comparison mechanism, demonstrated on a fixture repo: commit a
# pointer-shaped file (seeded from the real pointer's first lines so the
# shape is honest), hand-edit the worktree copy, and confirm the base-blob
# comparison trips exactly as the gate's verify_frozen_inventory_publication
# does.
git init -q -b trap/t10-frozen "$SCRATCH/wt"
trap_git_identity "$SCRATCH/wt"
head -20 "$TRAP_ROOT/$POINTER" > "$SCRATCH/wt/pointer.md" 2>/dev/null || printf '# pointer\n' > "$SCRATCH/wt/pointer.md"
git -C "$SCRATCH/wt" add pointer.md
git -C "$SCRATCH/wt" commit -qm 'freeze pointer'
git -C "$SCRATCH/wt" show HEAD:pointer.md > "$SCRATCH/base.md"
cp "$SCRATCH/base.md" "$SCRATCH/edited.md"
printf '\n| hand-added-row | fake | to-force-green |\n' >> "$SCRATCH/edited.md"
if cmp -s "$SCRATCH/base.md" "$SCRATCH/edited.md"; then
    printf 'HARNESS-ERROR t10: simulated hand edit unexpectedly identical\n' >&2
    exit 2
fi
trap_say "hand-edited pointer differs from fixture base blob: the frozen comparison trips"

# Static pins on the executable guard itself (xtask source, no cargo build):
# the bail message, the restore instruction, the required merge_gate entry,
# and the regression test that rejects a regenerated pointer.
BAIL_PIN="$(grep -c 'differs from the frozen pointer' "$TRAP_ROOT/xtask/src/tasks/file_policy.rs" || true)"
RESTORE_PIN="$(grep -c 'git checkout .* -- {NON_RUST_INVENTORY_POINTER_PATH}' "$TRAP_ROOT/xtask/src/tasks/file_policy.rs" || true)"
GATE_BLOCK="$(sed -n '/name: non_rust_inventory_check/,/command:/p' "$TRAP_ROOT/.ci/gate-policy.yaml")"
GATE_REQUIRED="$(printf '%s' "$GATE_BLOCK" | grep -c 'required: true' || true)"
GATE_TIER="$(printf '%s' "$GATE_BLOCK" | grep -c 'tier: merge_gate' || true)"
REGTEST_PIN="$(grep -c 'non_rust_inventory_check_rejects_regenerated_pointer_and_retains_evidence' "$TRAP_ROOT/xtask/src/tasks/file_policy.rs" || true)"
trap_say "pointer-exists=$POINTER_EXISTS; frozen-bail pins=$BAIL_PIN restore-hint pins=$RESTORE_PIN gate(required=$GATE_REQUIRED,tier=$GATE_TIER) regression-test pins=$REGTEST_PIN"

if [[ "$POINTER_EXISTS" -eq 1 && "$BAIL_PIN" -ge 1 && "$RESTORE_PIN" -ge 1 && "$GATE_REQUIRED" -ge 1 && "$GATE_TIER" -ge 1 && "$REGTEST_PIN" -ge 1 ]]; then
    verdict T10 PASS 'required merge_gate bails on hand-edited pointer and orders a base-blob restore (regeneration-tolerant, hand-edit-intolerant)'
else
    printf 'HARNESS-ERROR t10: EXISTS=%s BAIL=%s RESTORE=%s REQ=%s TIER=%s REG=%s\n' "$POINTER_EXISTS" "$BAIL_PIN" "$RESTORE_PIN" "$GATE_REQUIRED" "$GATE_TIER" "$REGTEST_PIN" >&2
    exit 2
fi
