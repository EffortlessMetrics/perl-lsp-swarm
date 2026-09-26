#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

# Keep Cargo's output visible while retaining a bounded classification input.
# A doctor result is only possible after its explicit startup marker appears.
log_file="$(mktemp)"
trap 'rm -f "$log_file"' EXIT
set +e
"$SCRIPT_DIR/cargo-safe" xtask devex-doctor 2>&1 | tee "$log_file"
statuses=("${PIPESTATUS[@]}")
result=${statuses[0]}
set -e

if [ "${statuses[1]}" -ne 0 ]; then
    echo "devex: bootstrap_instrument_failed (output capture exit ${statuses[1]}; Cargo exit $result; doctor status NOT_PROVEN)" >&2
    exit 1
fi

if [ "$result" -eq 0 ]; then
    if ! grep -Fq 'devex-doctor: started' "$log_file"; then
        echo 'devex: bootstrap_instrument_failed (exit 0 without doctor startup marker)' >&2
        exit 1
    fi
    exit 0
fi

if grep -Fq 'devex-doctor: started' "$log_file"; then
    if grep -Eq 'doctor_required_tool_missing|doctor_required_component_missing|doctor_check_failed' "$log_file"; then
        echo "devex: doctor_exited (exit $result; see typed doctor finding above)" >&2
    else
        echo "devex: doctor_instrument_failed (exit $result; inspect the error above)" >&2
    fi
elif grep -Eq 'cargo-toolchain-guard: REFUSED|cargo-toolchain-guard:.*predates' "$log_file"; then
    echo "devex: bootstrap_toolchain_rejected (exit $result; use the toolchain guard action above)" >&2
elif grep -Eiq 'cargo: command not found|cargo: not found|cargo: No such file or directory' "$log_file"; then
    echo "devex: bootstrap_tool_missing (exit $result; install Rust from https://rustup.rs)" >&2
elif grep -Eiq '(being used by another process|process cannot access the file|os error 32|os error 5|Access is denied)' "$log_file" && grep -Eiq 'xtask(\.exe)?' "$log_file"; then
    echo "devex: bootstrap_artifact_locked_or_in_use (exit $result; inspect the process holding xtask before retrying; owner NOT_PROVEN)" >&2
else
    echo "devex: bootstrap_build_failed (exit $result; inspect the Cargo error above)" >&2
fi
exit "$result"
