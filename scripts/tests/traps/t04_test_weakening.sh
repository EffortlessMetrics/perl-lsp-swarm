#!/usr/bin/env bash
# T4 — Test weakening ("the test looks oversensitive").
#
# Mishandling: gut a failing assertion and slap #[ignore] on the test to get
# green. Guard under test: a required-tier gate that fails on removed
# assertions / deleted tests / new ignores.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch
git init -q -b trap/t4-weak "$SCRATCH/wt"
trap_git_identity "$SCRATCH/wt"

# A failing test, then the "just get green" weakening: ignore + gutted body.
printf '#[test]\nfn parses_edge_case() {\n    assert_eq!(lex("%%("), Token::LParen);\n}\n' > "$SCRATCH/wt/lex_tests.rs"
git -C "$SCRATCH/wt" add lex_tests.rs
git -C "$SCRATCH/wt" commit -qm 'add edge-case test'
printf '#[test]\n#[ignore] // oversensitive, silence until later\nfn parses_edge_case() {\n}\n' > "$SCRATCH/wt/lex_tests.rs"
git -C "$SCRATCH/wt" commit -qam 'make suite green'
trap_say "assertion deleted and #[ignore] added; fixture diff accepted without any executable verdict"

# Static pins: the ignored-count ratchet is not wired into any gate tier, the
# must-context check is advisory, and no gate diffs test/assertion removal.
RATCHET_WIRED="$(grep -c 'ignored-test-count' "$TRAP_ROOT/.ci/gate-policy.yaml" || true)"
MUST_ADVISORY="$(grep -c 'required: false' "$TRAP_ROOT/.ci/gate-policy.yaml" || true)"
DELETION_GATE="$(grep -ciE 'test.deletion|assertion.*(weaken|remov)|removed.*assertion' "$TRAP_ROOT/.ci/gate-policy.yaml" || true)"
MUST_BLOCK="$(sed -n '/name: must_context_check/,/command:/p' "$TRAP_ROOT/.ci/gate-policy.yaml" | grep -c 'required: false' || true)"
trap_say "gate-policy refs to ignored-test-count=$RATCHET_WIRED; must_context required:false pins=$MUST_BLOCK; test-deletion/assertion gates=$DELETION_GATE (advisory total=$MUST_ADVISORY)"

if [[ "$RATCHET_WIRED" -eq 0 && "$MUST_BLOCK" -ge 1 && "$DELETION_GATE" -eq 0 ]]; then
    verdict T4 UNGUARDED 'weakened test met no required gate; missing gate: required-tier test/assertion-presence gate (deletion diff + ignored-count ratchet wiring; must_context_check is advisory)'
else
    printf 'HARNESS-ERROR t04: RATCHET=%s MUST=%s DELETION=%s\n' "$RATCHET_WIRED" "$MUST_BLOCK" "$DELETION_GATE" >&2
    exit 2
fi
