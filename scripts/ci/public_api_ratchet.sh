#!/usr/bin/env bash
set -euo pipefail

mode="${1:-}"
if [[ "$mode" != "check" && "$mode" != "update" ]]; then
    echo "usage: $0 check|update" >&2
    exit 2
fi

root="${PUBLIC_API_ROOT:-$(pwd)}"
cd "$root"
work_dir="${PUBLIC_API_WORK_DIR:-/tmp}"
baseline_dir="${PUBLIC_API_BASELINES_DIR:-.ci/public-api-baselines}"
generator="${PUBLIC_API_GENERATOR:-./scripts/cargo-safe}"
crates="${PUBLIC_API_CRATES:-perl-lsp-rs perl-parser perl-uri perl-dap perllsp}"

if [[ "$mode" == "update" ]]; then
    mkdir -p "$baseline_dir"
fi

failed=0
for crate in $crates; do
    baseline="$baseline_dir/${crate}.txt"
    raw="$work_dir/public-api-${crate}-raw.txt"
    err="$work_dir/public-api-${crate}-err.txt"
    current="$work_dir/public-api-${crate}-current.txt"
    diff_file="$work_dir/public-api-${crate}-diff.txt"

    if [[ "$mode" == "check" && ! -f "$baseline" ]]; then
        echo "FAIL Missing baseline: $baseline (run: just public-api-update)"
        failed=1
        continue
    fi

    if ! "$generator" public-api -p "$crate" --simplified >"$raw" 2>"$err"; then
        if [[ "$mode" == "check" ]]; then
            echo "INSTRUMENT-FAIL $crate: cargo public-api failed; stderr:"
            cat "$err"
        else
            echo "INSTRUMENT-FAIL $crate: cargo public-api failed; refusing to overwrite the baseline:" >&2
            cat "$err" >&2
            exit 1
        fi
        failed=1
        continue
    fi

    grep '^pub ' "$raw" >"$current" || true
    if [[ ! -s "$current" ]]; then
        if [[ "$mode" == "check" ]]; then
            echo "INSTRUMENT-FAIL $crate: generated API surface is empty (nightly toolchain missing?) — an empty surface is never a diff"
            failed=1
            continue
        fi
        echo "INSTRUMENT-FAIL $crate: generated API surface is empty; refusing to overwrite the baseline" >&2
        exit 1
    fi

    if [[ "$mode" == "check" ]]; then
        if ! diff -u "$baseline" "$current" >"$diff_file" 2>&1; then
            echo "FAIL Public API changed in $crate:"
            cat "$diff_file"
            failed=1
        else
            echo "OK $crate: API surface unchanged"
        fi
    else
        mv "$current" "$baseline"
        echo "Updated $crate: $(wc -l <"$baseline") lines"
    fi
done

if [[ "$mode" == "check" ]]; then
    [[ "$failed" -eq 0 ]] || {
        echo "Run 'just public-api-update' to regenerate baselines if the change is intentional."
        exit 1
    }
    exit 0
fi

echo "Commit $baseline_dir/ with your PR."
