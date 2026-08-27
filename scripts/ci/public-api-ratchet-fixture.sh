#!/usr/bin/env bash
# Execute the public-api recipes against controlled generator outcomes.
#
# This fixture intentionally runs the repository's real justfile recipes from a
# temporary working directory. Only the generator command is replaced; the
# fail-closed recipe body and its nested _public-api-install helper remain the
# repository implementation under test.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURE_ROOT="$(mktemp -d)"
trap 'rm -rf "$FIXTURE_ROOT"' EXIT

ln -s "$REPO_ROOT/justfile" "$FIXTURE_ROOT/justfile"
mkdir -p "$FIXTURE_ROOT/.ci/public-api-baselines" "$FIXTURE_ROOT/scripts" "$FIXTURE_ROOT/bin"

cat > "$FIXTURE_ROOT/scripts/cargo-safe" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${FAKE_PUBLIC_API_MODE:?}" in
    fail)
        echo "controlled generator failure" >&2
        exit 42
        ;;
    empty)
        exit 0
        ;;
    surface)
        crate=""
        while (($#)); do
            if [[ "$1" == "-p" ]]; then
                crate="$2"
                shift 2
            else
                shift
            fi
        done
        printf 'pub fixture_%s\n' "$crate"
        ;;
    *)
        echo "unknown FAKE_PUBLIC_API_MODE" >&2
        exit 2
        ;;
esac
EOF
chmod +x "$FIXTURE_ROOT/scripts/cargo-safe"

# _public-api-install only needs to see the pinned tool name already present;
# the controlled runner above is the command whose result is under test.
cat > "$FIXTURE_ROOT/bin/cargo-public-api" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$FIXTURE_ROOT/bin/cargo-public-api"

for crate in perl-lsp-rs perl-parser perl-uri perl-dap perllsp; do
    printf 'pub fixture_%s\n' "$crate" > "$FIXTURE_ROOT/.ci/public-api-baselines/${crate}.txt"
done

run_recipe() {
    local mode="$1"
    local recipe="$2"
    local output="$FIXTURE_ROOT/${mode}-${recipe}.log"
    local status=0
    set +e
    (
        cd "$FIXTURE_ROOT"
        PATH="$FIXTURE_ROOT/bin:$PATH" \
            PUBLIC_API_RUNNER="$FIXTURE_ROOT/scripts/cargo-safe" \
            FAKE_PUBLIC_API_MODE="$mode" \
            just "$recipe"
    ) >"$output" 2>&1
    status=$?
    set -e
    printf '%s\n' "$output"
    return "$status"
}

assert_instrument_fail() {
    local mode="$1"
    local recipe="$2"
    local output
    if output="$(run_recipe "$mode" "$recipe")"; then
        echo "expected $recipe/$mode to fail closed" >&2
        return 1
    fi
    grep -q "INSTRUMENT-FAIL" "$output"
}

# A failed or empty generator must reach the named instrument-failure branch.
assert_instrument_fail fail public-api-check
assert_instrument_fail empty public-api-check

# Update must preserve an existing baseline when generation fails or is empty.
for mode in fail empty; do
    before="$(sha256sum "$FIXTURE_ROOT/.ci/public-api-baselines/perl-parser.txt")"
    if run_recipe "$mode" public-api-update >/dev/null; then
        echo "expected public-api-update/$mode to fail closed" >&2
        exit 1
    fi
    after="$(sha256sum "$FIXTURE_ROOT/.ci/public-api-baselines/perl-parser.txt")"
    [[ "$before" == "$after" ]]
done

# A controlled non-empty surface must exercise the ordinary update and compare
# paths rather than only proving refusal.
run_recipe surface public-api-update >/dev/null
run_recipe surface public-api-check >/dev/null

echo "public API ratchet fixture passed: fail, empty, update-preservation, and surface paths"
