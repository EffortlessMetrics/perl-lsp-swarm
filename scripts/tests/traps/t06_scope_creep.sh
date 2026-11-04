#!/usr/bin/env bash
# T6 — Scope creep ("while I'm here": fix the off-by-one AND the neighbor).
#
# Mishandling: one task-scoped edit plus one unrelated "obvious cleanup" in
# a single diff. Guard under test: the required-tier change_scope_check gate
# (#17429 Option 2 — no clean write seam exists for Option 1, since writer
# admission is advisory-first with no enforcement consumer), which diffs the
# change against the `.agents/change-scope` declaration the change itself
# carries and fails naming every unadmitted file.
#
# The trap is fully hermetic: the diff-vs-declaration comparison is
# demonstrated on a fixture repo in scratch (it trips on exactly the
# out-of-scope file), and the real guard is pinned statically (check source +
# CLI + gate wiring + regression test), since the executable is cargo-built
# and this suite performs no cargo build. No git command ever touches
# $TRAP_ROOT.
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

# The fix (in scope) plus the tempting unrelated cleanup (out of scope), with
# the change carrying its own scope declaration: interpolation_scan.rs only.
printf 'fn scan() { fixed(); }\n' > "$SCRATCH/wt/interpolation_scan.rs"
printf '// refreshed comment\nfn neighbor() {}\n' > "$SCRATCH/wt/neighbor.rs"
mkdir -p "$SCRATCH/wt/.agents"
printf '# fix scope for the off-by-one\ninterpolation_scan.rs\n' > "$SCRATCH/wt/.agents/change-scope"
git -C "$SCRATCH/wt" add .
NAMES="$(git -C "$SCRATCH/wt" diff --name-only HEAD | tr '\n' ' ')"
trap_say "one change touches: $NAMES (declared scope was interpolation_scan.rs only)"

# Mechanism demo: the guard's core comparison — every changed path except the
# declaration itself must match a declared pattern — trips on exactly the
# creeping file, while the declared fix matches (non-vacuity in both
# directions, same as find_unadmitted over exact patterns).
TRIPS=""
MATCHES=""
while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    [[ "$path" == ".agents/change-scope" ]] && continue
    if grep -q -Fx "$path" "$SCRATCH/wt/.agents/change-scope"; then
        MATCHES="$MATCHES $path"
    else
        TRIPS="$TRIPS $path"
    fi
done < <(git -C "$SCRATCH/wt" diff --name-only HEAD)
trap_say "comparison matches:$MATCHES trips:$TRIPS"
if [[ "$TRIPS" != " neighbor.rs" || "$MATCHES" != " interpolation_scan.rs" ]]; then
    printf 'HARNESS-ERROR t06: mechanism demo wrong (trips=%s matches=%s)\n' "$TRIPS" "$MATCHES" >&2
    exit 2
fi

# Static pins on the executable guard itself (no cargo build): the declaration
# path, the naming failure message, the CLI registration + dispatch, the
# required merge_gate entry, the regression test, and the hosted shard route.
GUARD='crates/perl-ci-hygiene/src/commands/change_scope.rs'
SCOPE_PIN="$(grep -c 'SCOPE_DECLARATION_PATH: &str = ".agents/change-scope"' "$TRAP_ROOT/$GUARD" || true)"
REFUSE_PIN="$(grep -c 'outside the declared scope' "$TRAP_ROOT/$GUARD" || true)"
REGTEST_PIN="$(grep -c 'fn scope_creep_neighbor_is_named_as_unadmitted' "$TRAP_ROOT/$GUARD" || true)"
CLI_PIN="$(grep -c 'CheckChangeScope' "$TRAP_ROOT/crates/perl-ci-hygiene/src/cli.rs" || true)"
DISPATCH_PIN="$(grep -c 'commands::change_scope::check' "$TRAP_ROOT/crates/perl-ci-hygiene/src/main.rs" || true)"
GATE_BLOCK="$(sed -n '/name: change_scope_check/,/command:/p' "$TRAP_ROOT/.ci/gate-policy.yaml")"
GATE_REQUIRED="$(printf '%s' "$GATE_BLOCK" | grep -c 'required: true' || true)"
GATE_TIER="$(printf '%s' "$GATE_BLOCK" | grep -c 'tier: merge_gate' || true)"
GATE_CMD="$(printf '%s' "$GATE_BLOCK" | grep -c 'command: cargo xtask ci-hygiene check-change-scope' || true)"
SHARD_PIN="$(grep -c 'must_context_check change_scope_check' "$TRAP_ROOT/.github/workflows/ci.yml" || true)"
trap_say "scope-path pins=$SCOPE_PIN refuse-msg pins=$REFUSE_PIN regression-test pins=$REGTEST_PIN cli pins=$CLI_PIN dispatch pins=$DISPATCH_PIN gate(required=$GATE_REQUIRED,tier=$GATE_TIER,cmd=$GATE_CMD) shard pins=$SHARD_PIN"

if [[ "$SCOPE_PIN" -ge 1 && "$REFUSE_PIN" -ge 1 && "$REGTEST_PIN" -ge 1 && "$CLI_PIN" -ge 1 && "$DISPATCH_PIN" -ge 1 && "$GATE_REQUIRED" -ge 1 && "$GATE_TIER" -ge 1 && "$GATE_CMD" -ge 1 && "$SHARD_PIN" -ge 1 ]]; then
    verdict T6 PASS 'required merge_gate change_scope_check fails the change naming files outside the declared .agents/change-scope (caveats: no declaration = unevaluated; a change may widen its own declaration, which makes creep explicit for review rather than impossible)'
else
    printf 'HARNESS-ERROR t06: SCOPE=%s REFUSE=%s REG=%s CLI=%s DISPATCH=%s REQ=%s TIER=%s CMD=%s SHARD=%s\n' "$SCOPE_PIN" "$REFUSE_PIN" "$REGTEST_PIN" "$CLI_PIN" "$DISPATCH_PIN" "$GATE_REQUIRED" "$GATE_TIER" "$GATE_CMD" "$SHARD_PIN" >&2
    exit 2
fi
