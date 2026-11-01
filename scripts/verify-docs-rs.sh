#!/usr/bin/env bash
# verify-docs-rs.sh
# Run `cargo doc --no-deps` for every publishable crate and report failures.
# Simulates docs.rs build conditions locally.
#
# Usage: bash scripts/verify-docs-rs.sh [--fast] [--list]
#   --fast: skip large crates (perl-parser, perl-lsp-rs) to reduce runtime
#   --list: print publishable workspace crate names (from cargo metadata) and exit
#
# Exit codes:
#   0: all crates documented successfully (or --list printed)
#   1: one or more crates failed

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "${SCRIPT_DIR}/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

FAST_MODE=0
LIST_MODE=0
for arg in "$@"; do
    case "$arg" in
        --fast) FAST_MODE=1 ;;
        --list) LIST_MODE=1 ;;
        *)
            echo "verify-docs-rs: unknown argument: $arg" >&2
            echo "Usage: bash scripts/verify-docs-rs.sh [--fast] [--list]" >&2
            exit 1
            ;;
    esac
done

# Large crates to skip in fast mode
SLOW_CRATES=(perl-parser perl-lsp-rs perl-corpus perllsp perl-dap perl-workspace-index)

derive_publishable_crates() {
    local metadata_err meta_out
    metadata_err="$(mktemp)"
    meta_out="$(mktemp)"
    if ! cargo metadata --no-deps --format-version 1 \
        --manifest-path "${REPO_ROOT}/Cargo.toml" \
        >"${meta_out}" 2>"${metadata_err}"; then
        echo "verify-docs-rs: REFUSED: cargo metadata is unavailable; cannot derive publishable crates." >&2
        if [[ -s "${metadata_err}" ]]; then
            echo "verify-docs-rs: cargo metadata error:" >&2
            cat "${metadata_err}" >&2
        fi
        rm -f "${metadata_err}" "${meta_out}"
        exit 1
    fi
    rm -f "${metadata_err}"

    PUBLISH_TOPO="${SCRIPT_DIR}/publish-topo.py" python3 - "${meta_out}" <<'PY'
import importlib.util
import json
import os
import sys

scripts_dir = os.path.dirname(os.environ["PUBLISH_TOPO"])
spec = importlib.util.spec_from_file_location(
    "publish_topo", os.path.join(scripts_dir, "publish-topo.py")
)
if spec is None or spec.loader is None:
    print("verify-docs-rs: REFUSED: publish-topo.py is unavailable.", file=sys.stderr)
    sys.exit(1)
publish_topo = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish_topo)

with open(sys.argv[1], encoding="utf-8") as handle:
    meta = json.load(handle)
for name in publish_topo.derive_publish_list(meta):
    print(name)
PY
    local py_status=$?
    rm -f "${meta_out}"
    return "${py_status}"
}

crate_list="$(derive_publishable_crates)"
mapfile -t ALL_CRATES <<< "${crate_list}"

if [[ ${#ALL_CRATES[@]} -eq 0 ]]; then
    echo "verify-docs-rs: REFUSED: derived publishable crate list is empty." >&2
    exit 1
fi

if [[ "${LIST_MODE}" == "1" ]]; then
    printf '%s\n' "${ALL_CRATES[@]}"
    exit 0
fi

is_slow() {
    local crate="$1"
    for slow in "${SLOW_CRATES[@]}"; do
        if [[ "$crate" == "$slow" ]]; then
            return 0
        fi
    done
    return 1
}

echo "=== docs.rs verification (fast=$FAST_MODE) ==="
echo ""

PASSED=()
FAILED=()
SKIPPED=()

for crate in "${ALL_CRATES[@]}"; do
    if [[ "$FAST_MODE" == "1" ]] && is_slow "$crate"; then
        SKIPPED+=("$crate")
        echo "  SKIP  $crate (fast mode)"
        continue
    fi

    printf "  %-45s " "$crate"
    if cargo doc --no-deps -p "$crate" --quiet 2>&1; then
        PASSED+=("$crate")
        echo "OK"
    else
        FAILED+=("$crate")
        echo "FAIL"
    fi
done

echo ""
echo "=== Results ==="
echo "  Passed:  ${#PASSED[@]}"
echo "  Failed:  ${#FAILED[@]}"
echo "  Skipped: ${#SKIPPED[@]}"

if [[ ${#FAILED[@]} -gt 0 ]]; then
    echo ""
    echo "=== FAILURES ==="
    for crate in "${FAILED[@]}"; do
        echo "  $crate"
    done
    exit 1
fi

echo ""
echo "All crates documented successfully."
