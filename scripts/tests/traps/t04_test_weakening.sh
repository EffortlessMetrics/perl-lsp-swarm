#!/usr/bin/env bash
# T4 -- Test weakening ("the test looks oversensitive").
#
# Mishandling: gut a failing assertion and slap #[ignore] on the test to get
# green. Guard under test: a required-tier gate that fails on removed
# assertions / deleted tests / new ignores.
#
# Two halves, tracked separately because they land separately: assertion
# gutting / test deletion (closed by the required test_deletion_gate once
# issue #17405 / repair PR #17411 lands) and #[ignore] addition (guarded
# only by the advisory pr_fast smoke job, which fails nothing
# merge-blocking -- and which passes with-ref ignores outright).
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

# Static pins: the ignored-count ratchet is not wired into any gate tier,
# the ignored-refs check runs in the advisory pr_fast smoke job (which is
# not a branch-protection context), the must-context check is advisory, and
# the deletion-diff gate exists only once #17411 lands.
RATCHET_WIRED="$(grep -c 'ignored-test-count' "$TRAP_ROOT/.ci/gate-policy.yaml" || true)"
MUST_ADVISORY="$(grep -c 'required: false' "$TRAP_ROOT/.ci/gate-policy.yaml" || true)"
DELETION_GATE="$(grep -ciE 'test.deletion|assertion.*(weaken|remov)|removed.*assertion' "$TRAP_ROOT/.ci/gate-policy.yaml" || true)"
MUST_BLOCK="$(sed -n '/name: must_context_check/,/command:/p' "$TRAP_ROOT/.ci/gate-policy.yaml" | grep -c 'required: false' || true)"
IGNORED_TIER="$(sed -n '/name: ignored_tests_check_refs/,/command:/p' "$TRAP_ROOT/.ci/gate-policy.yaml" | grep -c 'tier: pr_fast' || true)"
TIER_SEMANTICS="$(grep -c 'executed by the advisory PR Smoke job' "$TRAP_ROOT/.ci/gate-policy.yaml" || true)"
DELETION_WIRED=0
if [[ "$DELETION_GATE" -ge 1 ]]; then
    WIRED_TIER="$(sed -n '/name: test_deletion_gate/,/command:/p' "$TRAP_ROOT/.ci/gate-policy.yaml" | grep -c 'tier: merge_gate' || true)"
    WIRED_REQ="$(sed -n '/name: test_deletion_gate/,/command:/p' "$TRAP_ROOT/.ci/gate-policy.yaml" | grep -c 'required: true' || true)"
    if [[ "$WIRED_TIER" -ge 1 && "$WIRED_REQ" -ge 1 ]]; then
        DELETION_WIRED=1
    fi
fi
trap_say "gate-policy refs to ignored-test-count=$RATCHET_WIRED; ignored-refs tier pr_fast=$IGNORED_TIER; advisory-smoke semantics pins=$TIER_SEMANTICS; must_context required:false pins=$MUST_BLOCK (advisory total=$MUST_ADVISORY); test-deletion gates=$DELETION_GATE wired-required=$DELETION_WIRED"

if [[ "$RATCHET_WIRED" -eq 0 && "$IGNORED_TIER" -ge 1 && "$TIER_SEMANTICS" -ge 1 && "$DELETION_WIRED" -eq 1 ]]; then
    verdict T4 UNGUARDED 'assertion-gutting and test deletion are closed by the required test_deletion_gate, but #[ignore]-addition is still advisory-only (pr_fast smoke fails nothing merge-blocking; with-ref ignores pass even the smoke check); missing gate: required-tier ignore-addition refusal'
elif [[ "$RATCHET_WIRED" -eq 0 && "$IGNORED_TIER" -ge 1 && "$TIER_SEMANTICS" -ge 1 && "$DELETION_GATE" -eq 0 ]]; then
    verdict T4 UNGUARDED 'weakened test met no required gate; missing gate: required-tier test/assertion-presence gate (deletion diff + ignored-count ratchet wiring; must_context_check is advisory)'
else
    printf 'HARNESS-ERROR t04: RATCHET=%s IGNORED_TIER=%s TIER_SEM=%s MUST=%s DELETION=%s WIRED=%s\n' "$RATCHET_WIRED" "$IGNORED_TIER" "$TIER_SEMANTICS" "$MUST_BLOCK" "$DELETION_GATE" "$DELETION_WIRED" >&2
    exit 2
fi
