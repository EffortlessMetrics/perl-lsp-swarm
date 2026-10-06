#!/usr/bin/env bash
# T6 — Scope creep ("while I'm here": fix the off-by-one AND the neighbor).
#
# Mishandling: one task-scoped edit plus one unrelated "obvious cleanup" in
# a single diff. Guard under test: a mechanical writer-admission verdict that
# blocks out-of-scope mutation. The admission CLI is advisory-first
# by design: it always exits 0.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
git init -q -b trap/t6-scope "$SCRATCH/wt"
trap_git_identity "$SCRATCH/wt"
printf 'fn scan() { off_by_one(); }\n' > "$SCRATCH/wt/interpolation_scan.rs"
printf '// stale comment\nfn neighbor() {}\n' > "$SCRATCH/wt/neighbor.rs"
git -C "$SCRATCH/wt" add .
git -C "$SCRATCH/wt" commit -qm base

# The fix (in scope) plus the tempting unrelated cleanup (out of scope).
printf 'fn scan() { fixed(); }\n' > "$SCRATCH/wt/interpolation_scan.rs"
printf '// refreshed comment\nfn neighbor() {}\n' > "$SCRATCH/wt/neighbor.rs"
NAMES="$(git -C "$SCRATCH/wt" diff --name-only | tr '\n' ' ')"
trap_say "one commit touches: $NAMES (claim scope was interpolation_scan.rs only)"

ADVISORY_PIN="$(grep -c 'always returns `Ok(())`' "$TRAP_ROOT/xtask/src/tasks/writer_admission.rs" || true)"
trap_say "writer_admission.rs advisory-first pins: $ADVISORY_PIN (verdict is informational; nothing is blocked)"

if [[ "$NAMES" == *"neighbor.rs"* && "$ADVISORY_PIN" -ge 1 ]]; then
    verdict T6 UNGUARDED 'out-of-scope hunk sailed with the fix; missing gate: mechanical writer-admission enforcement (verdict must refuse the edit; today run always returns Ok)'
else
    printf 'HARNESS-ERROR t06: NAMES=%s ADVISORY=%s\n' "$NAMES" "$ADVISORY_PIN" >&2
    exit 2
fi
