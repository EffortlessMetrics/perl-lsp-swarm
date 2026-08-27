#!/usr/bin/env bash
# Shared executable policy for the public-api check and update recipes.
set -euo pipefail

mode=${1:?usage: public-api-ratchet.sh {check|update}}
case "$mode" in
    check|update) ;;
    *) echo "usage: public-api-ratchet.sh {check|update}" >&2; exit 2 ;;
esac

baseline_dir=${PUBLIC_API_BASELINES_DIR:-.ci/public-api-baselines}
generator=${PUBLIC_API_GENERATOR:-}
crates=(perl-lsp-rs perl-parser perl-uri perl-dap perllsp)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

generate() {
    local crate="$1"
    if [[ -n "$generator" ]]; then
        "$generator" "$crate"
    else
        ./scripts/cargo-safe public-api -p "$crate" --simplified
    fi
}

surface_for() {
    local crate="$1" raw="$2" current="$3" error="$4"
    if ! generate "$crate" >"$raw" 2>"$error"; then
        echo "INSTRUMENT-FAIL $crate: cargo public-api failed; stderr:" >&2
        cat "$error" >&2
        return 1
    fi
    grep "^pub " "$raw" >"$current" || true
    if [[ ! -s "$current" ]]; then
        echo "INSTRUMENT-FAIL $crate: generated API surface is empty; an empty surface is never a diff" >&2
        return 1
    fi
}

failed=0
for crate in "${crates[@]}"; do
    baseline="${baseline_dir}/${crate}.txt"
    raw="${work_dir}/${crate}-raw.txt"
    current="${work_dir}/${crate}-current.txt"

    if [[ "$mode" == check && ! -f "$baseline" ]]; then
        echo "FAIL Missing baseline: $baseline (run: just public-api-update)" >&2
        failed=1
        continue
    fi

    if ! surface_for "$crate" "$raw" "$current" "${work_dir}/${crate}-err.txt"; then
        failed=1
        continue
    fi

    if [[ "$mode" == check ]]; then
        if ! diff -u "$baseline" "$current" >"${work_dir}/${crate}-diff.txt" 2>&1; then
            echo "FAIL Public API changed in $crate:" >&2
            cat "${work_dir}/${crate}-diff.txt" >&2
            failed=1
        else
            echo "OK $crate: API surface unchanged"
        fi
    else
        mkdir -p "$baseline_dir"
        mv "$current" "$baseline"
        echo "Updated $crate: $(wc -l < "$baseline") lines"
    fi
done

if [[ "$failed" -ne 0 ]]; then
    if [[ "$mode" == check ]]; then
        echo "Run 'just public-api-update' to regenerate baselines if the change is intentional." >&2
    fi
    exit 1
fi
