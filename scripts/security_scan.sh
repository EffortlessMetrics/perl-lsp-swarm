#!/usr/bin/env bash
# Typed local security scan and report engine (Issue #15366).
#
# Modes:
#   scan              Run every requested scanner independently; emit typed receipt.
#   quick             cargo-audit only; states its narrower denominator and fails
#                     closed when that scanner is unavailable.
#   report            scan, then render report.md from this run's receipt into a
#                     fresh private run directory.
#   verify <receipt>  Re-verify a prior run's receipt: every referenced artifact
#                     must exist, match its recorded size and sha256 digest.
#
# Per-scanner results: clean | findings | instrument_failed | unavailable | not_run
# Aggregate:           clean | findings | instrument_failed | incomplete
# Exit codes:          0 clean | 1 findings | 2 instrument_failed | 3 incomplete
#
# Contract and vocabulary: docs/SECURITY_SCANNING.md
set -u

die() { printf 'security_scan: %s\n' "$*" >&2; exit 3; }

json_validator() {
    # Prefer the native interpreter; the WindowsApps python3 alias is correct
    # but orders of magnitude slower to spawn.
    if command -v python >/dev/null 2>&1; then
        printf 'python'
    elif command -v python3 >/dev/null 2>&1; then
        printf 'python3'
    else
        printf ''
    fi
}

MODE="${1:-}"
case "$MODE" in
    scan|quick|report) ;;
    verify)
        RECEIPT="${2:-}"
        [ -n "$RECEIPT" ] || die "verify requires a receipt path"
        ;;
    *)
        printf 'usage: security_scan.sh <scan|quick|report|verify <receipt>>\n' >&2
        exit 3
        ;;
esac

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT" || die "cannot enter repository root"

if [ "$MODE" = "verify" ]; then
    # Verify-only path: no scanners run. Load the receipt, check each
    # referenced artifact for existence, recorded size, and recorded digest.
    py="$(json_validator)"
    [ -n "$py" ] || die "no JSON validator available; cannot verify receipt"
    "$py" - "$RECEIPT" <<'PYEOF'
import json, hashlib, os, sys
receipt = json.load(open(sys.argv[1], encoding="utf-8"))
bad = []
for s in receipt["scanners"]:
    art = s["artifact"]
    p = art["path"]
    if not os.path.isfile(p):
        bad.append(f'{s["name"]}: missing artifact {p}')
        continue
    size = os.path.getsize(p)
    if size != int(art["bytes"]):
        bad.append(f'{s["name"]}: size {size} != recorded {art["bytes"]}')
    digest = hashlib.sha256(open(p, "rb").read()).hexdigest()
    if digest != art["sha256"]:
        bad.append(f'{s["name"]}: digest mismatch')
if bad:
    print("VERIFY FAILED:")
    for b in bad:
        print("  " + b)
    sys.exit(2)
print(f'VERIFY OK: {receipt["run_id"]} aggregate={receipt["aggregate"]} '
      f'complete={receipt["complete"]}')
sys.exit(0)
PYEOF
    exit $?
fi

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUNS_ROOT="$REPO_ROOT/target/security-reports"
RUN_ID="run-${STAMP}-$$"
RUN_DIR="$RUNS_ROOT/$RUN_ID"
mkdir -p "$RUN_DIR" || die "cannot create run directory $RUN_DIR"

# Toolchain guard (issue #12593): scanners invoke cargo as a command, so a
# stale or missing cargo is refused before any scanner runs (exit 78).
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh"
cargo_toolchain_guard || exit $?

# ---------------------------------------------------------------- helpers ---

# schema_ok <file> <json|jsonl>  -> 0 valid, 1 invalid, 2 no validator available
schema_ok() {
    local f="$1" kind="$2" py
    [ -s "$f" ] || return 1
    py="$(json_validator)"
    [ -n "$py" ] || return 2
    if [ "$kind" = "jsonl" ]; then
        "$py" -c 'import json,sys
for line in sys.stdin:
    if line.strip():
        json.loads(line)' <"$f" >/dev/null 2>&1
    else
        "$py" -c 'import json,sys; json.load(open(sys.argv[1], encoding="utf-8"))' "$f" >/dev/null 2>&1
    fi
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" 2>/dev/null | cut -d" " -f1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" 2>/dev/null | cut -d" " -f1
    else
        printf 'unavailable'
    fi
}

json_escape() {
    # Minimal JSON string escaping; run paths are POSIX-style and printable.
    printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'
}

# ------------------------------------------------------------- run context ---

COMMIT="$(git rev-parse HEAD 2>/dev/null || printf '')"
COMMIT_KNOWN=true
[ -n "$COMMIT" ] || COMMIT_KNOWN=false
DIRTY=false
if [ -n "$(git status --porcelain 2>/dev/null | head -n 1)" ]; then
    DIRTY=true
fi

SCAN_MODE="$MODE"
case "$MODE" in
    report) SCAN_MODE="scan" ;;
esac
case "$SCAN_MODE" in
    quick) REQUESTED="cargo-audit" ;;
    *)     REQUESTED="cargo-audit cargo-deny trivy" ;;
esac

RUN_STARTED_EPOCH="$(date +%s)"
RUN_STARTED_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# ------------------------------------------------------- scanner execution ---

NAMES=(); RESULTS=(); EXITS=(); VERSIONS=(); ART_PATHS=(); ART_BYTES=()
ART_SHA=(); ART_SCHEMA=(); DIGEST_OK=(); STARTS=(); ENDS=(); LIMITS=()

run_cargo_audit() { # stdout -> run_dir/cargo-audit.json
    cargo audit --json >"$RUN_DIR/cargo-audit.json" 2>"$RUN_DIR/cargo-audit.stderr.log"
}

run_cargo_deny() { # stdout (JSON lines) -> run_dir/cargo-deny.json
    cargo deny --locked --format json check advisories licenses bans sources \
        >"$RUN_DIR/cargo-deny.json" 2>"$RUN_DIR/cargo-deny.stderr.log"
}

run_trivy() { # writes run_dir/trivy.json via --output
    trivy fs --format json --output "$RUN_DIR/trivy.json" \
        --severity CRITICAL,HIGH,MEDIUM \
        --scanners vuln,config,secret \
        --skip-dirs target,archive,.runs,.git \
        --ignore-unfixed \
        --exit-code 1 \
        . >"$RUN_DIR/trivy.stdout.log" 2>"$RUN_DIR/trivy.stderr.log"
}

# classify <exit_status> <schema_status: ok|invalid|novault|none>
classify() {
    if [ "$1" -eq 0 ] && [ "$2" = "ok" ]; then
        printf 'clean'
    elif [ "$1" -ne 0 ] && [ "$2" = "ok" ]; then
        printf 'findings'
    else
        printf 'instrument_failed'
    fi
}

schema_kind_for() {
    case "$1" in
        cargo-audit) printf 'json' ;;
        cargo-deny)  printf 'jsonl' ;;
        trivy)       printf 'json' ;;
    esac
}

artifact_name_for() {
    case "$1" in
        cargo-audit) printf 'cargo-audit.json' ;;
        cargo-deny)  printf 'cargo-deny.json' ;;
        trivy)       printf 'trivy.json' ;;
    esac
}

run_one() {
    local name="$1" runner="$2" art schema_status rc vs start end limits result last
    NAMES+=("$name")
    STARTS+=("$(date +%s)")
    limits=""
    art="$(artifact_name_for "$name")"
    ART_PATHS+=("$RUN_DIR/$art")
    ART_BYTES+=(0)
    ART_SHA+=("none")
    ART_SCHEMA+=("none")

    if ! command -v "$name" >/dev/null 2>&1; then
        RESULTS+=("unavailable")
        EXITS+=("")
        VERSIONS+=("")
        DIGEST_OK+=("n/a")
        ENDS+=("$(date +%s)")
        LIMITS+=("scanner binary not found on PATH; not executed")
        printf '  [unavailable] %s: not found on PATH\n' "$name"
        return 0
    fi

    vs="$( "$name" --version 2>/dev/null | head -n 1 || printf '' )"
    VERSIONS+=("$vs")

    start="$(date +%s)"
    "$runner"; rc=$?
    end="$(date +%s)"
    ENDS+=("$end")

    art="$RUN_DIR/$(artifact_name_for "$name")"

    last=$(( ${#NAMES[@]} - 1 ))
    schema_ok "$art" "$(schema_kind_for "$name")"
    schema_status=$?
    case "$schema_status" in
        0) ART_SCHEMA[$last]="ok" ;;
        1) ART_SCHEMA[$last]="invalid" ;;
        2) ART_SCHEMA[$last]="novault"
           limits="no JSON validator available; artifact not schema-verified" ;;
        *) ART_SCHEMA[$last]="invalid" ;;
    esac

    ART_BYTES[$last]="$(wc -c <"$art" 2>/dev/null || printf 0)"
    ART_SHA[$last]="$(sha256_of "$art" 2>/dev/null || printf 'unavailable')"

    result="$(classify "$rc" "${ART_SCHEMA[$last]}")"
    RESULTS+=("$result")
    EXITS+=("$rc")
    DIGEST_OK+=("pending")
    LIMITS+=("$limits")
    printf '  [%s] %s (exit %s)\n' "$result" "$name" "$rc"
    return 0
}

printf 'security scan (%s) — subject %s dirty=%s\n' "$SCAN_MODE" \
    "${COMMIT:-unknown}" "$DIRTY"
printf 'run directory: %s\n' "$RUN_DIR"

i=0
for name in $REQUESTED; do
    case "$name" in
        cargo-audit) run_one cargo-audit run_cargo_audit ;;
        cargo-deny)  run_one cargo-deny  run_cargo_deny  ;;
        trivy)       run_one trivy       run_trivy       ;;
    esac
    i=$((i + 1))
done

# ------------------------------------------------------------ verification ---

# Re-verify every recorded artifact digest inside this run; a mismatch means the
# receipt cannot vouch for its own evidence, which makes the aggregate incomplete
# regardless of the scanner outcomes.
INTEGRITY_INCOMPLETE=false
for idx in "${!NAMES[@]}"; do
    case "${RESULTS[$idx]}" in
        clean|findings)
            actual="$(sha256_of "${ART_PATHS[$idx]}" 2>/dev/null || printf 'unavailable')"
            if [ "$actual" = "${ART_SHA[$idx]}" ] && [ "$actual" != "unavailable" ]; then
                DIGEST_OK[$idx]="ok"
            else
                DIGEST_OK[$idx]="mismatch"
                LIMITS[$idx]="${LIMITS[$idx]}artifact digest changed after execution;"
                RESULTS[$idx]="instrument_failed"
                INTEGRITY_INCOMPLETE=true
            fi
            ;;
        *) DIGEST_OK[$idx]="n/a" ;;
    esac
done

# -------------------------------------------------------------- aggregate ---

COMPLETE=true
AGGREGATE="clean"
HAVE_UNAVAILABLE=false
HAVE_INSTRUMENT=false
HAVE_FINDINGS=false
for idx in "${!NAMES[@]}"; do
    case "${RESULTS[$idx]}" in
        clean|findings) : ;;
        *) COMPLETE=false ;;
    esac
    case "${RESULTS[$idx]}" in
        unavailable|not_run)    HAVE_UNAVAILABLE=true ;;
        instrument_failed)      HAVE_INSTRUMENT=true ;;
        findings)               HAVE_FINDINGS=true ;;
    esac
done
if [ "$INTEGRITY_INCOMPLETE" = "true" ] || [ "$HAVE_UNAVAILABLE" = "true" ]; then
    AGGREGATE="incomplete"
elif [ "$HAVE_INSTRUMENT" = "true" ]; then
    AGGREGATE="instrument_failed"
elif [ "$HAVE_FINDINGS" = "true" ]; then
    AGGREGATE="findings"
fi

case "$AGGREGATE" in
    clean)             EXIT_CODE=0 ;;
    findings)          EXIT_CODE=1 ;;
    instrument_failed) EXIT_CODE=2 ;;
    incomplete)        EXIT_CODE=3 ;;
esac

RUN_ENDED_EPOCH="$(date +%s)"
RUN_ENDED_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# ------------------------------------------------------------- receipt ------ 

RECEIPT="$RUN_DIR/receipt.json"
{
    printf '{\n'
    printf '  "schema": "perl-lsp.security-receipt/v1",\n'
    printf '  "mode": "%s",\n' "$SCAN_MODE"
    printf '  "run_id": "%s",\n' "$RUN_ID"
    printf '  "run_dir": "%s",\n' "$(json_escape "$RUN_DIR")"
    printf '  "started_utc": "%s",\n' "$RUN_STARTED_UTC"
    printf '  "ended_utc": "%s",\n' "$RUN_ENDED_UTC"
    printf '  "duration_seconds": %s,\n' "$((RUN_ENDED_EPOCH - RUN_STARTED_EPOCH))"
    printf '  "subject": {"commit": "%s", "commit_known": %s, "dirty": %s},\n' \
        "${COMMIT:-unknown}" "$COMMIT_KNOWN" "$DIRTY"
    printf '  "expected_scanners": [%s],\n' \
        "$(printf '"%s"' "$(printf '%s' "$REQUESTED" | sed 's/ /", "/g')")"
    printf '  "scanners": [\n'
    last=$(( ${#NAMES[@]} - 1 ))
    for idx in "${!NAMES[@]}"; do
        printf '    {"name": "%s", "version": "%s", "result": "%s", "exit_status": %s, ' \
            "${NAMES[$idx]}" "$(json_escape "${VERSIONS[$idx]}")" "${RESULTS[$idx]}" \
            "${EXITS[$idx]:-null}"
        printf '"artifact": {"path": "%s", "bytes": %s, "sha256": "%s", "schema": "%s", "digest_verified": "%s"}, ' \
            "$(json_escape "${ART_PATHS[$idx]}")" "${ART_BYTES[$idx]}" "${ART_SHA[$idx]}" \
            "${ART_SCHEMA[$idx]}" "${DIGEST_OK[$idx]}"
        printf '"started_epoch": %s, "ended_epoch": %s, "limitations": "%s"}%s\n' \
            "${STARTS[$idx]}" "${ENDS[$idx]}" "$(json_escape "${LIMITS[$idx]}")" \
            "$([ "$idx" -lt "$last" ] && printf ',')"
    done
    printf '  ],\n'
    printf '  "complete": %s,\n' "$COMPLETE"
    printf '  "aggregate": "%s",\n' "$AGGREGATE"
    printf '  "exit_code": %s\n' "$EXIT_CODE"
    printf '}\n'
} >"$RECEIPT"

# --------------------------------------------------------------- report ----- 

render_report() {
    local headline
    case "$AGGREGATE" in
        clean)             headline="CLEAN" ;;
        findings)          headline="FINDINGS" ;;
        instrument_failed) headline="INSTRUMENT FAILED" ;;
        incomplete)        headline="INCOMPLETE" ;;
    esac
    {
        printf '# Security Scan Report — %s\n\n' "$headline"
        printf 'This headline and the process exit code agree with `receipt.json` '
        printf '(aggregate `%s`, exit `%s`).\n\n' "$AGGREGATE" "$EXIT_CODE"
        printf '**Run:** `%s`\n\n' "$RUN_ID"
        printf '**Started (UTC):** %s  \n' "$RUN_STARTED_UTC"
        printf '**Ended (UTC):** %s  \n' "$RUN_ENDED_UTC"
        printf '**Subject commit (full SHA):** `%s`\n\n' "${COMMIT:-unknown}"
        printf '**Commit known:** %s  \n' "$COMMIT_KNOWN"
        printf '**Dirty tree:** %s\n\n' "$DIRTY"
        printf 'A short SHA alone is not the report subject; the full SHA above plus the '
        printf 'dirty flag identify what was scanned.\n\n'
        printf '## Expected coverage\n\n'
        printf 'Requested scanners (%s): %s\n\n' "$SCAN_MODE" "$REQUESTED"
        printf '## Per-tool outcomes\n\n'
        printf '| scanner | version | result | exit | artifact | bytes | sha256 | schema | digest verified | limitations |\n'
        printf '|---|---|---|---|---|---|---|---|---|---|\n'
        for idx in "${!NAMES[@]}"; do
            local rel
            rel="$(basename "${ART_PATHS[$idx]}")"
            printf '| %s | %s | **%s** | %s | [%s](%s) | %s | `%s` | %s | %s | %s |\n' \
                "${NAMES[$idx]}" "${VERSIONS[$idx]:-n/a}" "${RESULTS[$idx]}" \
                "${EXITS[$idx]:-n/a}" "$rel" "$rel" "${ART_BYTES[$idx]}" \
                "${ART_SHA[$idx]}" "${ART_SCHEMA[$idx]}" "${DIGEST_OK[$idx]}" \
                "${LIMITS[$idx]:-—}"
        done
        printf '\n## Coverage truth\n\n'
        if [ "$COMPLETE" = "true" ]; then
            printf 'Every requested scanner executed and produced a schema-valid artifact '
            printf 'in this run. Old artifacts outside `%s` were not consulted.\n' "$RUN_ID"
        else
            printf 'NOT COMPLETE: at least one requested scanner has no explicit acceptable '
            printf 'result (clean/findings) in this run. Missing coverage is recorded above '
            printf 'and cannot be substituted by prior runs.\n'
        fi
        printf '\n## Result vocabulary\n\n'
        printf '`clean` ran and no findings; `findings` ran, valid artifact, non-zero exit; '
        printf '`instrument_failed` ran without a valid artifact (or evidence changed); '
        printf '`unavailable` not installed; `not_run` skipped by explicit policy.\n'
        printf '\nFindings and instrument failure are distinct non-clean outcomes; neither '
        printf 'is rendered as success.\n\n'
        printf 'Vocabulary reconciles with `.github/workflows/ci-security.yml`, which '
        printf 'remains the separate CI enforcement surface (see docs/SECURITY_SCANNING.md).\n'
    } >"$RUN_DIR/report.md"
}

# ---------------------------------------------------------------- finish ---- 

printf '\nAggregate: %s (exit %s) — receipt: %s\n' "$AGGREGATE" "$EXIT_CODE" "$RECEIPT"

case "$MODE" in
    report)
        render_report
        printf 'Report: %s\n' "$RUN_DIR/report.md"
        ;;
esac

exit "$EXIT_CODE"
