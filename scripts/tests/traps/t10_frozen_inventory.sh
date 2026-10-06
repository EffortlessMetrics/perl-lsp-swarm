#!/usr/bin/env bash
# T10 — Hand-editing the frozen inventory to "fix" the policy gate.
#
# Mishandling: edit docs/policy/NON_RUST_INVENTORY.md counts/rows by hand so
# the policy check turns green. Guard under test: the required-tier
# non_rust_inventory_check gate, whose frozen-pointer comparison bails when
# the working-tree pointer differs from the base blob and instructs a
# pointer restore instead of a hand edit.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
POINTER='docs/policy/NON_RUST_INVENTORY.md'

# Base blob (read-only) vs a simulated hand edit: the exact comparison the
# gate's verify_frozen_inventory_publication performs.
BASE_REF='origin/main'
git -C "$TRAP_ROOT" rev-parse --verify -q "$BASE_REF" >/dev/null 2>&1 || BASE_REF='HEAD'
git -C "$TRAP_ROOT" show "$BASE_REF:$POINTER" > "$SCRATCH/base.md"
cp "$SCRATCH/base.md" "$SCRATCH/edited.md"
printf '\n| hand-added-row | fake | to-force-green |\n' >> "$SCRATCH/edited.md"
if cmp -s "$SCRATCH/base.md" "$SCRATCH/edited.md"; then
    printf 'HARNESS-ERROR t10: simulated hand edit unexpectedly identical\n' >&2
    exit 2
fi
trap_say "hand-edited pointer differs from $BASE_REF blob: the frozen comparison trips"

# Static pins on the executable guard itself (xtask source, no cargo build):
# the bail message, the restore instruction, the required merge_gate entry,
# and the regression test that rejects a regenerated pointer.
BAIL_PIN="$(grep -c 'differs from the frozen pointer' "$TRAP_ROOT/xtask/src/tasks/file_policy.rs" || true)"
RESTORE_PIN="$(grep -c 'git checkout .* -- {NON_RUST_INVENTORY_POINTER_PATH}' "$TRAP_ROOT/xtask/src/tasks/file_policy.rs" || true)"
GATE_BLOCK="$(sed -n '/name: non_rust_inventory_check/,/command:/p' "$TRAP_ROOT/.ci/gate-policy.yaml")"
GATE_REQUIRED="$(printf '%s' "$GATE_BLOCK" | grep -c 'required: true' || true)"
GATE_TIER="$(printf '%s' "$GATE_BLOCK" | grep -c 'tier: merge_gate' || true)"
REGTEST_PIN="$(grep -c 'non_rust_inventory_check_rejects_regenerated_pointer_and_retains_evidence' "$TRAP_ROOT/xtask/src/tasks/file_policy.rs" || true)"
trap_say "frozen-bail pins=$BAIL_PIN restore-hint pins=$RESTORE_PIN gate(required=$GATE_REQUIRED,tier=$GATE_TIER) regression-test pins=$REGTEST_PIN"

if [[ "$BAIL_PIN" -ge 1 && "$RESTORE_PIN" -ge 1 && "$GATE_REQUIRED" -ge 1 && "$GATE_TIER" -ge 1 && "$REGTEST_PIN" -ge 1 ]]; then
    verdict T10 PASS 'required merge_gate bails on hand-edited pointer and orders a base-blob restore (regeneration-tolerant, hand-edit-intolerant)'
else
    printf 'HARNESS-ERROR t10: BAIL=%s RESTORE=%s REQ=%s TIER=%s REG=%s\n' "$BAIL_PIN" "$RESTORE_PIN" "$GATE_REQUIRED" "$GATE_TIER" "$REGTEST_PIN" >&2
    exit 2
fi
