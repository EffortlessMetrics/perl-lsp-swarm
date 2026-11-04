#!/usr/bin/env bash
# Discriminating proof for the typed local security scan surface (Issue #15366).
#
# Exercises scripts/security_scan.sh against fake scanner binaries so each
# outcome class (clean/findings/instrument_failed/unavailable) and the
# coverage/freshness/digest obligations are observable without network or
# real toolchains.
#
# Run: bash scripts/security_scan_test.sh   (from anywhere; repo root derived)
#
# cargo-toolchain-guard: exempt — hermetic proof harness: every cargo
# invocation resolves to in-test stub binaries under an isolated PATH, never
# to the host toolchain, so there is no real cargo to guard.
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(git -C "$HERE" rev-parse --show-toplevel 2>/dev/null || echo "$HERE/..")"
ENGINE="$ROOT/scripts/security_scan.sh"
TMP="$(mktemp -d)"
FAKEBIN="$TMP/bin"
mkdir -p "$FAKEBIN"
FAILURES=0

cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

# Isolated base PATH: hides real cargo-audit/cargo-deny/trivy (this host has a
# real cargo-audit) while keeping git, coreutils, and the JSON validator.
GITDIR="$(dirname "$(command -v git)")"
PYDIR="$(dirname "$(command -v python)")"
ISOLATED_PATH="/usr/bin:$GITDIR:$PYDIR"

fail() { printf 'FAIL: %s\n' "$*"; FAILURES=$((FAILURES + 1)); }
pass() { printf 'ok: %s\n' "$1"; }

write_fakes() { # writes fakes honoring FAKE_AUDIT / FAKE_DENY / FAKE_TRIVY modes
    mkdir -p "$FAKEBIN"
    cat >"$FAKEBIN/cargo-audit" <<'EOF'
#!/usr/bin/env bash
if [ "${1:-}" = "--version" ]; then echo "cargo-audit 0.20.1 (fake)"; exit 0; fi
case "${FAKE_AUDIT:-clean}" in
  clean)    echo '{"version": 2, "vulnerabilities": {"list": []}}';;
  findings) echo '{"version": 2, "vulnerabilities": {"list": [{"id": "RUSTSEC-2099-0001"}]}}'; exit 1;;
  garbage)  echo "error: advisory db unreadable"; exit 101;;
  silent)   exit 0;;
esac
EOF
    cat >"$FAKEBIN/cargo-deny" <<'EOF'
#!/usr/bin/env bash
if [ "${1:-}" = "--version" ]; then echo "cargo-deny 0.16.1 (fake)"; exit 0; fi
case "${FAKE_DENY:-clean}" in
  clean)    echo '{"type": "begin"}';;
  findings) echo '{"type": "error", "fields": {"code": "banned"}}'; exit 1;;
  garbage)  echo "error: malformed deny.toml"; exit 3;;
esac
EOF
    cat >"$FAKEBIN/trivy" <<'EOF'
#!/usr/bin/env bash
if [ "${1:-}" = "--version" ]; then echo "Version: 0.58.0 (fake)"; exit 0; fi
out="trivy.json"
prev=""
for a in "$@"; do
  [ "$prev" = "--output" ] && out="$a"
  prev="$a"
done
case "${FAKE_TRIVY:-clean}" in
  clean)    printf '{"SchemaVersion": 2, "Results": []}' > "$out";;
  findings) printf '{"SchemaVersion": 2, "Results": [{"Vulnerabilities": [{"VulnerabilityID": "CVE-2099-0001"}]}]}' > "$out"; exit 1;;
  garbage)  printf 'trivy: db download failed' > "$out"; exit 1;;
  silent)   exit 2;;
esac
EOF
    # Windows: `cargo audit` / `cargo deny` resolve the real toolchain via the
    # cargo binary itself, bypassing extensionless shims. Shim `cargo` too.
    cat >"$FAKEBIN/cargo" <<EOF
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then echo "cargo 99.0.0 (fake)"; exit 0; fi
case "\${1:-}" in
  audit) shift; exec "$FAKEBIN/cargo-audit" "\$@" ;;
  deny)  shift; exec "$FAKEBIN/cargo-deny" "\$@" ;;
  *) echo "fake cargo: unsupported subcommand: \$*" >&2; exit 101 ;;
esac
EOF
    chmod +x "$FAKEBIN"/cargo-audit "$FAKEBIN"/cargo-deny "$FAKEBIN"/trivy "$FAKEBIN"/cargo
}

# run_engine <mode> <path-filter...> — path-filter omits fakes by name
run_engine() {
    local mode="$1"; shift
    local bin="$FAKEBIN"
    local omit
    for omit in "$@"; do
        bin="$TMP/omit-$omit"
        mkdir -p "$bin"
        for f in "$FAKEBIN"/*; do
            [ "$(basename "$f")" = "$omit" ] && continue
            cp "$f" "$bin/"
        done
    done
    RECEIPT_OUT="$(PATH="$bin:$ISOLATED_PATH" bash "$ENGINE" "$mode" 2>&1)"
    RC=$?
    RECEIPT="$(printf '%s\n' "$RECEIPT_OUT" | sed -n 's/.*receipt: //p' | tail -n 1)"
}

receipt_has() {
    [ -n "$RECEIPT" ] && grep -q "$1" "$RECEIPT" || fail "receipt missing: $1"
}

# --- T1: all clean -> exit 0, complete, clean aggregate, full-SHA subject -----
write_fakes
run_engine scan
[ "$RC" -eq 0 ] || fail "T1 exit $RC != 0"
receipt_has '"aggregate": "clean"'
receipt_has '"complete": true'
receipt_has '"name": "cargo-audit".*"result": "clean"'
receipt_has '"name": "cargo-deny".*"result": "clean"'
receipt_has '"name": "trivy".*"result": "clean"'
receipt_has '"commit_known": true'
printf '%s\n' "$(grep -o '"commit": "[0-9a-f]*"' "$RECEIPT")" | grep -Eq '[0-9a-f]{40}' \
    && pass "T1 full-SHA subject" || fail "T1 subject commit is not a full SHA"
[ "$FAILURES" -eq 0 ] && pass "T1 all-clean run" || fail "T1 (see above)"

# --- T2: cargo-audit findings must not suppress cargo-deny/trivy --------------
write_fakes
FAKE_AUDIT=findings run_engine scan
[ "$RC" -eq 1 ] || fail "T2 exit $RC != 1"
receipt_has '"name": "cargo-audit".*"result": "findings"'
receipt_has '"name": "cargo-deny".*"result": "clean"'
receipt_has '"name": "trivy".*"result": "clean"'
receipt_has '"aggregate": "findings"'
pass "T2 findings keep coverage; aggregate findings"

# --- T3: missing trivy cannot make an all-tool scan pass ----------------------
write_fakes
run_engine scan trivy
[ "$RC" -eq 3 ] || fail "T3 exit $RC != 3"
receipt_has '"name": "trivy".*"result": "unavailable"'
receipt_has '"aggregate": "incomplete"'
receipt_has '"complete": false'
printf '%s\n' "$RECEIPT_OUT" | grep -qi 'passed' \
    && fail "T3 output claims PASSED" || pass "T3 missing tool never passes"

# --- T4: security-quick without cargo-audit is not success --------------------
write_fakes
run_engine quick cargo-audit
[ "$RC" -eq 3 ] || fail "T4 exit $RC != 3"
receipt_has '"mode": "quick"'
receipt_has '"name": "cargo-audit".*"result": "unavailable"'
receipt_has '"aggregate": "incomplete"'
pass "T4 quick fails closed on unavailable scanner"

# --- T5: process failure with no valid artifact is instrument_failed ----------
write_fakes
FAKE_AUDIT=garbage run_engine scan
[ "$RC" -eq 2 ] || fail "T5 exit $RC != 2"
receipt_has '"name": "cargo-audit".*"result": "instrument_failed"'
receipt_has '"name": "cargo-deny".*"result": "clean"'
receipt_has '"aggregate": "instrument_failed"'
pass "T5 bad artifact is instrument failure, not findings/clean"

# --- T6: exit-0 with no artifact is not clean ---------------------------------
write_fakes
FAKE_AUDIT=silent run_engine scan
[ "$RC" -eq 2 ] || fail "T6 exit $RC != 2"
receipt_has '"name": "cargo-audit".*"result": "instrument_failed"'
pass "T6 silent success is instrument failure"

# --- T7: vulnerability exit with valid output stays findings ------------------
write_fakes
FAKE_TRIVY=findings run_engine scan
receipt_has '"name": "trivy".*"result": "findings"'
[ "$RC" -eq 1 ] || fail "T7 exit $RC != 1"
pass "T7 findings stay findings"

# --- T8: report headline/exit agree; old runs cannot enter a new report -------
write_fakes
FAKE_AUDIT=findings run_engine report
[ "$RC" -eq 1 ] || fail "T8 exit $RC != 1"
REPORT_1="$(printf '%s\n' "$RECEIPT_OUT" | sed -n 's/^Report: //p' | tail -n 1)"
RUN_1="$(dirname "$REPORT_1")"
grep -q "Security Scan Report — FINDINGS" "$REPORT_1" \
    || fail "T8 headline does not say FINDINGS"
FAKE_AUDIT=findings run_engine report
REPORT_2="$(printf '%s\n' "$RECEIPT_OUT" | sed -n 's/^Report: //p' | tail -n 1)"
RUN_2="$(dirname "$REPORT_2")"
[ "$RUN_1" != "$RUN_2" ] || fail "T8 reused run directory"
grep -q "$(basename "$RUN_1")" "$REPORT_2" \
    && fail "T8 new report references old run" || pass "T8 fresh report directory"
grep -q "NOT COMPLETE" "$REPORT_2" && fail "T8 findings rendered NOT COMPLETE" \
    || pass "T8 findings headline is FINDINGS (not incomplete)"
grep -Eq "\]\(cargo-audit.json\)|\]\(trivy.json\)" "$REPORT_2" \
    || fail "T8 artifact links not run-local"

# --- T9: digest mismatch makes the aggregate incomplete -----------------------
write_fakes
run_engine scan
AUDIT_ART="$(grep -o '"name": "cargo-audit"[^}]*' "$RECEIPT" | grep -o '"path": "[^"]*"' | head -n1 | sed 's/"path": "//;s/"//')"
printf 'tampered' >>"$AUDIT_ART"
PATH="$FAKEBIN:$ISOLATED_PATH" bash "$ENGINE" verify "$RECEIPT" >/dev/null 2>&1 \
    && fail "T9 verify accepted tampered artifact" || pass "T9 verify rejects tampered artifact"

# --- T10: verify accepts an intact receipt ------------------------------------
write_fakes
run_engine scan
PATH="$FAKEBIN:$ISOLATED_PATH" bash "$ENGINE" verify "$RECEIPT" >/dev/null 2>&1 \
    || fail "T10 verify rejected intact receipt"
pass "T10 verify accepts intact receipt"

printf '\n%s\n' "----------------------------------------"
if [ "$FAILURES" -eq 0 ]; then
    printf 'ALL TESTS PASSED (%s)\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    exit 0
fi
printf '%s FAILURES\n' "$FAILURES"
exit 1
