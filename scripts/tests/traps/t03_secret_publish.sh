#!/usr/bin/env bash
# T3 — Secret in diff + publish pressure.
#
# Mishandling: commit a file containing a token-shaped line and push. The
# token is assembled at runtime from two inert fragments so this trap file
# itself carries no token-shaped string for scanners to flag. Guard under
# test: any blocking secret gate (local pre-commit scan or required-tier CI).
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
git init -q --bare "$SCRATCH/origin.git"
git init -q -b trap/t3-secret "$SCRATCH/wt"
trap_git_identity "$SCRATCH/wt"
git -C "$SCRATCH/wt" remote add origin "$SCRATCH/origin.git"

# Synthetic, inert, token-SHAPED line only; never a real credential.
FRAG_A='ghp_'
FRAG_B='FAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE0000'
printf 'api_token = "%s%s"\n' "$FRAG_A" "$FRAG_B" > "$SCRATCH/wt/fixture.rs"
git -C "$SCRATCH/wt" add fixture.rs
git -C "$SCRATCH/wt" commit -qm 'add fixture with embedded token'
git -C "$SCRATCH/wt" push -q origin trap/t3-secret
trap_say "token-shaped line committed and pushed to fixture origin with zero local signal"

# Static pins: no local scanner config, no secret scan in the hook
# installer, no secret gate in the required-checks inventory.
GITLEAKS=0
[[ -f "$TRAP_ROOT/.gitleaks.toml" ]] && GITLEAKS=1
[[ -f "$TRAP_ROOT/gitleaks.toml" ]] && GITLEAKS=1
HOOK_SECRET_REFS="$(grep -ciE 'secret|gitleaks|token' "$TRAP_ROOT/crates/perl-ci-hygiene/src/git_hooks.rs" || true)"
REQUIRED_SECRET_REFS="$(grep -ciE 'secret|trivy|gitleaks|ci-security' "$TRAP_ROOT/.ci/policies/required-checks.toml" || true)"
trap_say "gitleaks config present=$GITLEAKS; secret refs in git_hooks.rs=$HOOK_SECRET_REFS; secret refs in required-checks.toml=$REQUIRED_SECRET_REFS"

if [[ "$GITLEAKS" -eq 0 && "$HOOK_SECRET_REFS" -eq 0 && "$REQUIRED_SECRET_REFS" -eq 0 ]]; then
    verdict T3 UNGUARDED 'token-shaped commit met no local or required-tier scan; missing gate: blocking secret scan (local pre-commit/push hook + required-tier gate covering commits and PR bodies)'
else
    printf 'HARNESS-ERROR t03: GITLEAKS=%s HOOK=%s REQUIRED=%s\n' "$GITLEAKS" "$HOOK_SECRET_REFS" "$REQUIRED_SECRET_REFS" >&2
    exit 2
fi
