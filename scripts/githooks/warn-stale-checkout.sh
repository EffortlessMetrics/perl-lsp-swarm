#!/usr/bin/env bash
# warn-stale-checkout.sh — opt-in git hook for stale checkout warnings.
#
# Calls `cargo xtask freshness-check --mode warn` and parses the JSON receipt.
# When `safe_for_code_state_claims == false`, prints a one-line warning to
# stderr with the `behind_by` count and the remediation command.
#
# Always exits 0 — this is a warning hook, not a blocking hook.
#
# Installation (opt-in):
#   bash scripts/install-githooks.sh
#
# Or link manually:
#   ln -sf ../../scripts/githooks/warn-stale-checkout.sh .git/hooks/post-checkout
#
# Reference: docs/devex/freshness-check.md

set -euo pipefail

# This hook is deliberately warning-only and must remain usable before a
# checkout has a working toolchain. Its Cargo invocation is not a build
# entrypoint, so it is an explicit non-blocking exception to #12593.
# cargo-toolchain-guard: exempt — freshness warnings must never block checkout.

# Locate cargo — fall back gracefully if not available.
if ! command -v cargo &>/dev/null; then
    exit 0
fi

# Run the freshness check in warn mode, capturing JSON output.
RECEIPT="$(cargo xtask freshness-check --mode warn --no-fetch 2>/dev/null)" || {
    # If xtask itself fails (not built yet, etc.), silently skip.
    exit 0
}

# Parse once. Invalid receipts or unavailable/broken parsers must not turn this
# advisory hook into a failing checkout. Do not use jq -e: false is valid data.
if command -v jq &>/dev/null; then
    FIELDS="$(printf '%s' "$RECEIPT" | jq -r '
        if type == "object" and
           (.safe_for_code_state_claims | type) == "boolean" and
           (.behind_by | type) == "number" and
           (.base_ref | type) == "string"
        then [.safe_for_code_state_claims, .behind_by, .base_ref] | @tsv
        else error("invalid freshness receipt") end
    ' 2>/dev/null)" || exit 0
else
    if command -v python3 &>/dev/null; then
        PYTHON=python3
    elif command -v python &>/dev/null; then
        PYTHON=python
    else
        exit 0
    fi
    FIELDS="$(printf '%s' "$RECEIPT" | "$PYTHON" -c '
import sys, json
d = json.load(sys.stdin)
if (not isinstance(d, dict) or
        type(d.get("safe_for_code_state_claims")) is not bool or
        type(d.get("behind_by")) is not int or
        not isinstance(d.get("base_ref"), (str, type(u"")))):
    sys.exit(1)
sys.stdout.write("%s\t%s\t%s\n" % (
    str(d["safe_for_code_state_claims"]).lower(), d["behind_by"], d["base_ref"]))
' 2>/dev/null)" || exit 0
fi
IFS=$'\t' read -r SAFE BEHIND BASE_REF <<< "$FIELDS" || exit 0

if [ "$SAFE" = "false" ]; then
    echo "⚠  stale checkout: HEAD is ${BEHIND} commit(s) behind ${BASE_REF}" >&2
    echo "   run: git pull --rebase" >&2
fi

exit 0
