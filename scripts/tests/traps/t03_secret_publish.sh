#!/usr/bin/env bash
# T3 — Secret in diff + publish pressure.
#
# Mishandling: commit a file containing a token-shaped line, push it, and
# quote it in the PR. Guard under test: the token-shape secret scan (#17428).
# Pre-commit (staged diff) and pre-push (pushed range) refuse locally with
# zero build, and the required secret_scan gate re-scans the PR diff + body
# server-side so --no-verify cannot bypass both layers.
#
# The token is assembled at runtime from two inert fragments so this trap file
# itself carries no token-shaped string. The live demos install the REAL scan
# (extracted from the hook installer source) as fixture hooks and drive real
# git commit/push through them; the cargo-built gate half is pinned
# statically since this suite performs no cargo build — the same split T10
# uses for its frozen-pointer gate.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

trap_scratch

# --- Extract the real scan from the hook installer source ---
# Anchored full-line match: the marker names also appear in prose comments,
# which must not start or extend the range.
SNIPPET="$SCRATCH/secret-scan.sh"
awk '/^# SECRET_SCAN_SH_BEGIN$/{cap=1} cap{print} /^# SECRET_SCAN_SH_END$/{if (cap) exit}' \
    "$TRAP_ROOT/crates/perl-ci-hygiene/src/git_hooks.rs" > "$SNIPPET"
SNIPPET_LINES="$(wc -l < "$SNIPPET" | tr -d ' ')"
RULE_VARS="$(grep -c '^SECRET_SCAN_RULE_' "$SNIPPET" || true)"
trap_say "extracted scan snippet: $SNIPPET_LINES lines, $RULE_VARS rule vars"

# Synthetic, inert, token-SHAPED line only; never a real credential.
FRAG_A='ghp_'
FRAG_B='FAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE0000'

# --- Fixture A: the commit path, through git's own hook machinery ---
git init -q -b trap/t3-secret "$SCRATCH/a"
trap_git_identity "$SCRATCH/a"
{
    printf '#!/usr/bin/env bash\nset -euo pipefail\n'
    cat "$SNIPPET"
    printf '\nsecret_scan_staged || exit 1\n'
} > "$SCRATCH/a/.git/hooks/pre-commit"
chmod +x "$SCRATCH/a/.git/hooks/pre-commit"

printf 'api_token = "%s%s"\n' "$FRAG_A" "$FRAG_B" > "$SCRATCH/a/fixture.rs"
# A second live shape: a private-key header, whose leading dashes once parsed
# as grep options and failed that rule open. Fragment-built like the token.
KEY_A='-----BEGIN '
KEY_B='PRIVATE KEY-----'
printf '%s%s\n' "$KEY_A" "$KEY_B" > "$SCRATCH/a/key.pem"
BEFORE="$(sha256sum "$SCRATCH/a/fixture.rs" | cut -d' ' -f1)"
BEFORE_KEY="$(sha256sum "$SCRATCH/a/key.pem" | cut -d' ' -f1)"
git -C "$SCRATCH/a" add fixture.rs key.pem
set +e
git -C "$SCRATCH/a" commit -m 'add fixture with embedded token' >"$SCRATCH/commit.txt" 2>&1
COMMIT_EXIT=$?
set -e
REFUSED=0; [[ "$COMMIT_EXIT" -ne 0 ]] && REFUSED=1
NAMING=0
grep -q 'Secret scan refused' "$SCRATCH/commit.txt" 2>/dev/null && NAMING=$((NAMING + 1))
grep -q 'github-token' "$SCRATCH/commit.txt" 2>/dev/null && NAMING=$((NAMING + 1))
grep -q 'fixture.rs' "$SCRATCH/commit.txt" 2>/dev/null && NAMING=$((NAMING + 1))
trap_say "token commit exit=$COMMIT_EXIT (refused=$REFUSED); naming pins=$NAMING/3"
PRIV_NAMING=0
grep -q 'private-key' "$SCRATCH/commit.txt" 2>/dev/null && PRIV_NAMING=$((PRIV_NAMING + 1))
grep -q 'key.pem' "$SCRATCH/commit.txt" 2>/dev/null && PRIV_NAMING=$((PRIV_NAMING + 1))
trap_say "private-key naming pins=$PRIV_NAMING/2"

# Intactness: no commit landed and the worktree bytes are untouched.
COMMITS="$(git -C "$SCRATCH/a" rev-list --all --count 2>/dev/null || echo 0)"
AFTER="$(sha256sum "$SCRATCH/a/fixture.rs" | cut -d' ' -f1)"
AFTER_KEY="$(sha256sum "$SCRATCH/a/key.pem" | cut -d' ' -f1)"
INTACT=0; [[ "$COMMITS" -eq 0 && "$BEFORE" == "$AFTER" && "$BEFORE_KEY" == "$AFTER_KEY" ]] && INTACT=1
trap_say "fixture commits=$COMMITS; worktree bytes intact=$INTACT"

# The documented escape: allowlisted fixture paths commit cleanly.
mkdir -p "$SCRATCH/a/.ci"
printf 'fixture.rs\nkey.pem\n' > "$SCRATCH/a/.ci/secret-scan-allowlist.txt"
set +e
git -C "$SCRATCH/a" commit -m 'add documented fixture' >"$SCRATCH/escape.txt" 2>&1
ESCAPE_EXIT=$?
set -e
ESCAPE=0; [[ "$ESCAPE_EXIT" -eq 0 ]] && ESCAPE=1
trap_say "allowlisted fixture commit exit=$ESCAPE_EXIT (escape works=$ESCAPE)"

# Control: a benign file commits with no allowlist and no scan complaint.
rm "$SCRATCH/a/.ci/secret-scan-allowlist.txt"
printf 'benign content, no credentials here\n' > "$SCRATCH/a/ok.txt"
git -C "$SCRATCH/a" add ok.txt
set +e
git -C "$SCRATCH/a" commit -m 'add benign file' >"$SCRATCH/control.txt" 2>&1
CONTROL_EXIT=$?
set -e
CONTROL=0
if [[ "$CONTROL_EXIT" -eq 0 ]] && ! grep -q 'Secret scan refused' "$SCRATCH/control.txt" 2>/dev/null; then
    CONTROL=1
fi
trap_say "benign commit exit=$CONTROL_EXIT (control passes=$CONTROL)"

# --- Fixture B: the push path, with an explicit --no-verify bypass of commit ---
git init -q --bare "$SCRATCH/origin.git"
git clone -q "$SCRATCH/origin.git" "$SCRATCH/b" 2>/dev/null
trap_git_identity "$SCRATCH/b"
git -C "$SCRATCH/b" checkout -q -b trap/t3-push
{
    printf '#!/usr/bin/env bash\nset -euo pipefail\n'
    cat "$SNIPPET"
} > "$SCRATCH/b/.git/hooks/pre-push"
cat >> "$SCRATCH/b/.git/hooks/pre-push" <<'PUSH_HARNESS'

PUSH_REFS=()
while IFS= read -r line; do PUSH_REFS+=("$line"); done
for line in "${PUSH_REFS[@]+"${PUSH_REFS[@]}"}"; do
    read -r _push_ref push_local _remote_ref push_remote <<< "$line"
    if [ "$push_local" != "0000000000000000000000000000000000000000" ]; then
        secret_scan_range "$push_local" "$push_remote" || exit 1
    fi
done
exit 0
PUSH_HARNESS
chmod +x "$SCRATCH/b/.git/hooks/pre-push"

printf 'api_token = "%s%s"\n' "$FRAG_A" "$FRAG_B" > "$SCRATCH/b/fixture.rs"
git -C "$SCRATCH/b" add fixture.rs
git -C "$SCRATCH/b" commit -q --no-verify -m 'add fixture with embedded token'
trap_say "commit --no-verify sailed locally, as designed (no pre-commit hook installed)"
set +e
git -C "$SCRATCH/b" push origin trap/t3-push >"$SCRATCH/push.txt" 2>&1
PUSH_EXIT=$?
set -e
PUSH_REFUSED=0; [[ "$PUSH_EXIT" -ne 0 ]] && PUSH_REFUSED=1
PUSH_NAMING=0
grep -q 'Secret scan refused this push' "$SCRATCH/push.txt" 2>/dev/null && PUSH_NAMING=$((PUSH_NAMING + 1))
grep -q 'github-token' "$SCRATCH/push.txt" 2>/dev/null && PUSH_NAMING=$((PUSH_NAMING + 1))
ORIGIN_COUNT="$(git --git-dir="$SCRATCH/origin.git" rev-list --all --count 2>/dev/null || echo 0)"
ORIGIN_CLEAN=0; [[ "$ORIGIN_COUNT" -eq 0 ]] && ORIGIN_CLEAN=1
trap_say "token push exit=$PUSH_EXIT (refused=$PUSH_REFUSED); naming pins=$PUSH_NAMING/2; origin commits=$ORIGIN_COUNT"

# --- Fixture C: benign push control through the same pre-push hook ---
git clone -q "$SCRATCH/origin.git" "$SCRATCH/c" 2>/dev/null
trap_git_identity "$SCRATCH/c"
git -C "$SCRATCH/c" checkout -q -b trap/t3-clean
cp "$SCRATCH/b/.git/hooks/pre-push" "$SCRATCH/c/.git/hooks/pre-push"
printf 'benign content\n' > "$SCRATCH/c/ok.txt"
git -C "$SCRATCH/c" add ok.txt
git -C "$SCRATCH/c" commit -qm 'benign work'
set +e
git -C "$SCRATCH/c" push origin trap/t3-clean >"$SCRATCH/push-clean.txt" 2>&1
PUSH_CLEAN_EXIT=$?
set -e
PUSH_CONTROL=0
if [[ "$PUSH_CLEAN_EXIT" -eq 0 ]] && ! grep -q 'Secret scan refused' "$SCRATCH/push-clean.txt" 2>/dev/null; then
    PUSH_CONTROL=1
fi
trap_say "benign push exit=$PUSH_CLEAN_EXIT (push control passes=$PUSH_CONTROL)"

# --- Static pins: the cargo-built gate half (no cargo build in this suite) ---
HOOKSRC="$TRAP_ROOT/crates/perl-ci-hygiene/src/git_hooks.rs"
LIB="$TRAP_ROOT/crates/perl-ci-hygiene/src/secret_scan.rs"
LIB_SHAPES="$(grep -c 'pub const TOKEN_SHAPES' "$LIB" || true)"
LIB_SCAN="$(grep -c 'pub fn scan_unified_diff' "$LIB" || true)"
LIB_BODY="$(grep -c 'pub fn extract_pr_body' "$LIB" || true)"
LIB_TEST="$(grep -c 'rule_patterns_do_not_match_their_own_source_text' "$LIB" || true)"
CMD_CHECK="$(grep -c 'pub(crate) fn check' "$TRAP_ROOT/crates/perl-ci-hygiene/src/commands/secret_scan.rs" || true)"
CLI_PIN="$(grep -c '^    CheckSecrets {$' "$TRAP_ROOT/crates/perl-ci-hygiene/src/cli.rs" || true)"
trap_say "lib(shapes=$LIB_SHAPES,scan=$LIB_SCAN,body=$LIB_BODY,test=$LIB_TEST) command=$CMD_CHECK cli=$CLI_PIN"
GATE_BLOCK="$(sed -n '/- name: secret_scan/,/command:/p' "$TRAP_ROOT/.ci/gate-policy.yaml")"
GATE_TIER="$(printf '%s' "$GATE_BLOCK" | grep -c 'tier: merge_gate' || true)"
GATE_REQ="$(printf '%s' "$GATE_BLOCK" | grep -c 'required: true' || true)"
GATE_CMD="$(printf '%s' "$GATE_BLOCK" | grep -c 'check-secrets' || true)"
SHARD_PIN="$(grep -c 'secret_scan' "$TRAP_ROOT/.ci/gate-shard-execution.json" || true)"
WORKFLOW_PIN="$(grep -c 'secret_scan' "$TRAP_ROOT/.github/workflows/ci.yml" || true)"
LANE_PIN="$(grep -c 'secret_scan' "$TRAP_ROOT/scripts/ci/validate_gate_lane_mapping.py" || true)"
ECON_PIN="$(grep -c 'secret_scan' "$TRAP_ROOT/docs/ci/gate-policy-economics.md" || true)"
trap_say "gate(tier=$GATE_TIER,required=$GATE_REQ,command=$GATE_CMD) shard=$SHARD_PIN workflow=$WORKFLOW_PIN lane=$LANE_PIN econ=$ECON_PIN"
ALLOW_OK=0; [[ -f "$TRAP_ROOT/.ci/secret-scan-allowlist.txt" ]] && ALLOW_OK=1
ALLOW_DOC="$(grep -c -F 'One repo-relative path per line' "$TRAP_ROOT/.ci/secret-scan-allowlist.txt" || true)"
WIRE_COMMIT="$(grep -c -F 'secret_scan_staged || exit 1' "$HOOKSRC" || true)"
CALL_LINE="$(grep -n -F 'secret_scan_staged || exit 1' "$HOOKSRC" | head -1 | cut -d: -f1)"
GATE_LINE="$(grep -n -F 'cargo xtask precommit' "$HOOKSRC" | head -1 | cut -d: -f1)"
WIRE_PUSH="$(grep -c -F 'secret_scan_range "$push_local" "$push_remote" || exit 1' "$TRAP_ROOT/hooks/pre-push" || true)"
PUSH_CALL_LINE="$(grep -n -F 'secret_scan_range "$push_local" "$push_remote" || exit 1' "$TRAP_ROOT/hooks/pre-push" | head -1 | cut -d: -f1)"
PUSH_GATE_LINE="$(grep -n -F '# --- Detect doc-only changes' "$TRAP_ROOT/hooks/pre-push" | head -1 | cut -d: -f1)"
SYNC_TEST="$(grep -c 'checked_in_pre_push_hook_matches_generated_hook' "$TRAP_ROOT/crates/perl-ci-hygiene/src/main.rs" || true)"
PARITY_TEST="$(grep -c 'secret_scan_shell_matches_lib_rules' "$HOOKSRC" || true)"
trap_say "allowlist(file=$ALLOW_OK,doc=$ALLOW_DOC) wire(commit=$WIRE_COMMIT@${CALL_LINE}<${GATE_LINE},push=$WIRE_PUSH@${PUSH_CALL_LINE}<${PUSH_GATE_LINE}) sync=$SYNC_TEST parity=$PARITY_TEST"

LIVE_OK=0
if [[ "$REFUSED" -eq 1 && "$NAMING" -eq 3 && "$PRIV_NAMING" -eq 2 && "$INTACT" -eq 1 && "$ESCAPE" -eq 1 && "$CONTROL" -eq 1 \
    && "$PUSH_REFUSED" -eq 1 && "$PUSH_NAMING" -eq 2 && "$ORIGIN_CLEAN" -eq 1 && "$PUSH_CONTROL" -eq 1 \
    && "$SNIPPET_LINES" -gt 50 && "$RULE_VARS" -eq 5 ]]; then
    LIVE_OK=1
fi
PINS_OK=0
if [[ "$LIB_SHAPES" -eq 1 && "$LIB_SCAN" -eq 1 && "$LIB_BODY" -eq 1 && "$LIB_TEST" -ge 1 \
    && "$CMD_CHECK" -eq 1 && "$CLI_PIN" -eq 1 && "$GATE_TIER" -eq 1 && "$GATE_REQ" -eq 1 && "$GATE_CMD" -eq 1 \
    && "$SHARD_PIN" -ge 1 && "$WORKFLOW_PIN" -ge 1 && "$LANE_PIN" -ge 1 && "$ECON_PIN" -ge 1 \
    && "$ALLOW_OK" -eq 1 && "$ALLOW_DOC" -ge 1 && "$WIRE_COMMIT" -ge 1 && "$CALL_LINE" -lt "$GATE_LINE" \
    && "$WIRE_PUSH" -eq 1 && "$PUSH_CALL_LINE" -lt "$PUSH_GATE_LINE" && "$SYNC_TEST" -ge 1 && "$PARITY_TEST" -ge 1 ]]; then
    PINS_OK=1
fi
TOKEN_LANDED=0
[[ "$REFUSED" -eq 0 || "$PUSH_REFUSED" -eq 0 ]] && TOKEN_LANDED=1

if [[ "$LIVE_OK" -eq 1 && "$PINS_OK" -eq 1 ]]; then
    verdict T3 PASS 'staged token commit refused live with file+rule naming and byte-intact worktree; pushed range refused on the push path with origin untouched; allowlist escape and benign controls pass; required secret_scan gate re-scans PR diff+body (caveats: local hooks are --no-verify-bypassable by design — the required gate is the non-bypassable layer; shapes flag credentials by form, not liveness)'
elif [[ "$PINS_OK" -eq 1 && "$TOKEN_LANDED" -eq 1 ]]; then
    verdict T3 FAIL 'scan is wired but a token-shaped addition landed (guard claimed but bypassed)'
else
    printf 'HARNESS-ERROR t03: LIVE=%s PINS=%s REFUSED=%s NAMING=%s PRIV=%s INTACT=%s ESCAPE=%s CONTROL=%s PUSH=%s PNAMING=%s ORIGIN=%s PCONTROL=%s SNIP=%s RULES=%s\n' \
        "$LIVE_OK" "$PINS_OK" "$REFUSED" "$NAMING" "$PRIV_NAMING" "$INTACT" "$ESCAPE" "$CONTROL" "$PUSH_REFUSED" "$PUSH_NAMING" "$ORIGIN_COUNT" "$PUSH_CONTROL" "$SNIPPET_LINES" "$RULE_VARS" >&2
    printf 'HARNESS-ERROR t03 pins: LIB=%s/%s/%s/%s CMD=%s CLI=%s GATE=%s/%s/%s SHARD=%s WORKFLOW=%s LANE=%s ECON=%s ALLOW=%s/%s WIRE=%s/%s SYNC=%s PARITY=%s\n' \
        "$LIB_SHAPES" "$LIB_SCAN" "$LIB_BODY" "$LIB_TEST" "$CMD_CHECK" "$CLI_PIN" "$GATE_TIER" "$GATE_REQ" "$GATE_CMD" "$SHARD_PIN" "$WORKFLOW_PIN" "$LANE_PIN" "$ECON_PIN" "$ALLOW_OK" "$ALLOW_DOC" "$WIRE_COMMIT" "$WIRE_PUSH" "$SYNC_TEST" "$PARITY_TEST" >&2
    exit 2
fi
