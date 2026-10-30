#!/usr/bin/env bash
# T9 — `--no-verify` bypass ("let CI sort it out").
#
# Mishandling: commit and push with --no-verify in a worktree whose hooks
# were never installed (the shipped default). Guards under test: mandatory
# hook provisioning at worktree setup (#17406/#17414/#17426) plus the
# required-tier re-cover gate (#17430), which re-runs hook policy over the
# pushed range of every PR.
#
# The local bypass always succeeds by construction — --no-verify is a
# documented git skip hatch — so the trap demonstrates it live and then
# checks the two non-bypassable halves: (1) the pushed range carries the
# catchable signal (a placeholder-identity commit is visible to a range
# query), with the required-tier executable pinned statically since it is
# cargo-built; (2) per-worktree hook provisioning from #17426.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

# pin_count <file> <literal> — grep -c that yields 0 when the file is absent,
# so a not-yet-landed guard reads as missing rather than breaking the trap.
pin_count() {
    if [[ -f "$1" ]]; then
        grep -c -F -- "$2" "$1" || true
    else
        printf '0'
    fi
}

trap_scratch
git init -q --bare "$SCRATCH/origin.git"
git clone -q "$SCRATCH/origin.git" "$SCRATCH/wt" 2>/dev/null
trap_git_identity "$SCRATCH/wt"
git -C "$SCRATCH/wt" checkout -q -b trap/t9-bypass

ACTIVE_HOOKS="$(find "$SCRATCH/wt/.git/hooks" -type f ! -name '*.sample' | wc -l)"
trap_say "active (non-sample) hooks in fresh fixture worktree: $ACTIVE_HOOKS"

printf 'change\n' > "$SCRATCH/wt/file.txt"
git -C "$SCRATCH/wt" add file.txt
git -C "$SCRATCH/wt" commit -q --no-verify -m 'bypass commit gate'
git -C "$SCRATCH/wt" push -q --no-verify origin trap/t9-bypass
trap_say "commit --no-verify and push --no-verify both succeeded with no hooks installed"

# The re-cover input signal, demonstrated live: a placeholder-identity commit
# (what --no-verify admits past the skipped pre-commit hook) is visible to a
# pushed-range query, so a required-tier range scan can catch it.
git -C "$SCRATCH/wt" checkout -q -b trap/t9-signal
printf 'base\n' > "$SCRATCH/wt/signal.txt"
git -C "$SCRATCH/wt" add signal.txt
git -C "$SCRATCH/wt" commit -qm 'clean base'
BASE="$(git -C "$SCRATCH/wt" rev-parse HEAD)"
printf 'smuggled\n' > "$SCRATCH/wt/signal.txt"
git -C "$SCRATCH/wt" add signal.txt
git -C "$SCRATCH/wt" -c user.name='xtask hook tests' -c user.email='xtask@example.invalid' commit -qm 'bypassed commit gate'
RANGE_HITS="$(git -C "$SCRATCH/wt" log --format='%H %an %ae %cn %ce %s' "$BASE..HEAD" | grep -c -F 'xtask hook tests' || true)"
trap_say "placeholder-identity commits visible to a pushed-range query: $RANGE_HITS"

# Static pins on the required-tier re-cover executable (cargo-built, so the
# suite pins source + wiring + regression test rather than running it):
# the required merge_gate entry, the scan, the naming message, the
# hook-parity test, the end-to-end regression test, and the shard/lane wiring.
GATE_BLOCK="$(sed -n '/- name: no_verify_recover_check/,/quarantine:/p' "$TRAP_ROOT/.ci/gate-policy.yaml")"
GATE_REQUIRED="$(printf '%s' "$GATE_BLOCK" | grep -c 'required: true' || true)"
GATE_TIER="$(printf '%s' "$GATE_BLOCK" | grep -c 'tier: merge_gate' || true)"
GATE_CMD="$(printf '%s' "$GATE_BLOCK" | grep -c 'cargo xtask ci-hygiene check-no-verify-recover' || true)"
IMPL="$TRAP_ROOT/crates/perl-ci-hygiene/src/commands/no_verify_recover.rs"
SCAN_PIN="$(pin_count "$IMPL" 'fn scan_commit_attribution')"
NAME_PIN="$(pin_count "$IMPL" 'no-verify re-cover')"
PARITY_PIN="$(pin_count "$IMPL" 'deny_list_matches_pre_commit_hook_policy')"
REGTEST="$(pin_count "$TRAP_ROOT/crates/perl-ci-hygiene/tests/check_no_verify_recover_test.rs" 'placeholder_identity_commit_in_range_fails_naming_the_commit')"
MATRIX_PIN="$(pin_count "$TRAP_ROOT/.github/workflows/ci.yml" 'no_verify_recover_check')"
LANE_PIN="$(pin_count "$TRAP_ROOT/scripts/ci/validate_gate_lane_mapping.py" '"no_verify_recover_check"')"
trap_say "re-cover pins: gate(required=$GATE_REQUIRED,tier=$GATE_TIER,cmd=$GATE_CMD) scan=$SCAN_PIN naming=$NAME_PIN parity=$PARITY_PIN regtest=$REGTEST matrix=$MATRIX_PIN lane=$LANE_PIN"

# Provisioning-half pins (per-worktree isolation, #17414 rule C / #17426):
# the installer-owned relative hooks path plus the isolation fixture.
PROV_HOOKSPATH="$(pin_count "$TRAP_ROOT/crates/perl-ci-hygiene/src/git_hooks.rs" 'core.hooksPath')"
PROV_TEST=0
[[ -f "$TRAP_ROOT/scripts/tests/test-hookspath-isolation.sh" ]] && PROV_TEST=1
trap_say "provisioning pins: hookspath=$PROV_HOOKSPATH isolation-fixture=$PROV_TEST"

# The local skip hatch stays documented by design; the re-cover is what makes
# it non-bypassing. Evidence only, never a verdict condition.
BYPASS_DOCS="$(pin_count "$TRAP_ROOT/crates/perl-ci-hygiene/src/git_hooks.rs" '--no-verify')"
trap_say "--no-verify skip-hatch mentions in git_hooks.rs=$BYPASS_DOCS"

RECOVER_OK=0
if [[ "$RANGE_HITS" -ge 1 && "$GATE_REQUIRED" -ge 1 && "$GATE_TIER" -ge 1 && "$GATE_CMD" -ge 1 && "$SCAN_PIN" -ge 1 && "$NAME_PIN" -ge 1 && "$PARITY_PIN" -ge 1 && "$REGTEST" -ge 1 && "$MATRIX_PIN" -ge 1 && "$LANE_PIN" -ge 1 ]]; then
    RECOVER_OK=1
fi
PROV_OK=0
if [[ "$PROV_HOOKSPATH" -ge 1 && "$PROV_TEST" -eq 1 ]]; then
    PROV_OK=1
fi

if [[ "$RECOVER_OK" -eq 1 && "$PROV_OK" -eq 1 ]]; then
    verdict T9 PASS 'local --no-verify still bypasses by design, but the pushed range is re-covered: required merge_gate re-runs hook policy with a naming message and hooks are provisioned per worktree at setup'
elif [[ "$RECOVER_OK" -eq 0 && "$PROV_OK" -eq 0 ]]; then
    verdict T9 UNGUARDED 'no-verify commit+push ran with no hooks and no fallback; missing gates: required-tier pushed-range re-cover (#17430) + mandatory per-worktree hook provisioning (post-#17426)'
elif [[ "$RECOVER_OK" -eq 0 ]]; then
    verdict T9 UNGUARDED 'no-verify commit+push ran with no hooks; hooks are provisioned per worktree but the pushed range has no required-tier re-cover; missing gate: pushed-range re-cover (#17430)'
else
    verdict T9 UNGUARDED 'no-verify commit+push ran with no hooks; required-tier pushed-range re-cover is wired but hooks are not provisioned per worktree; missing half: mandatory hook provisioning at worktree setup (post-#17426)'
fi
