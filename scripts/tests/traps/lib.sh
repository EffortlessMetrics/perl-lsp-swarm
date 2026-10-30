#!/usr/bin/env bash
# Shared harness for the adversarial agentic-editing trap suite (T1-T11).
#
# Verdict semantics (see README.md): a trap measures whether an EXECUTABLE
# guard fails closed on the mishandling — not whether a careful agent would
# refuse. PASS = an executable check refuses/detects the mishandling
# (demonstrated live where cheap, pinned statically where the executable is
# cargo-built). UNGUARDED = the mishandling runs with zero executable signal;
# the trap names the exact missing gate. FAIL is reserved for "guard claimed
# but bypassed" and harness breakage surfaces as ERROR in the runner.
#
# Every trap is hermetic: scratch dirs only (mktemp), file-local git remotes
# only, no network, no mutations outside scratch. Traps must exit 0 after
# printing exactly one `VERDICT: ` line; any other exit is a harness error.
#
# shellcheck disable=SC2034

set -euo pipefail

# Repository root, resolved from the calling trap's location:
# scripts/tests/traps/<trap>.sh -> repo root.
TRAPS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TRAP_ROOT="$(cd "$TRAPS_DIR/../../.." && pwd)"

# New scratch dir in $SCRATCH, removed on trap-script exit. Call directly
# (NOT via $(...)): a trap set inside a command substitution fires when the
# subshell exits and would delete the dir before the caller uses it.
trap_scratch() {
    SCRATCH="$(mktemp -d)"
    trap 'rm -rf "$SCRATCH"' EXIT
}

# Minimal git identity so fixture commits never depend on global config.
trap_git_identity() {
    local repo="$1"
    git -C "$repo" config user.email 'trap-suite@example.invalid'
    git -C "$repo" config user.name 'trap-suite'
    git -C "$repo" config commit.gpgsign false
}

trap_say() { printf '  evidence: %s\n' "$1"; }

# verdict <Tn> <PASS|FAIL|UNGUARDED> <detail>
verdict() {
    printf 'VERDICT: %s %s %s\n' "$1" "$2" "$3"
}
