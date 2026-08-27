#!/usr/bin/env bash
# Deterministic executable regression fixture for #12894.
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
adapter="${script_dir}/public-api-ratchet.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
baseline_dir="$tmp/baselines"
mkdir -p "$baseline_dir"
for crate in perl-lsp-rs perl-parser perl-uri perl-dap perllsp; do
    printf 'pub fn stable();\n' >"${baseline_dir}/${crate}.txt"
done

cat >"$tmp/fake-public-api" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
case "${PUBLIC_API_FIXTURE_MODE:?}" in
    valid) printf 'pub fn stable();\n' ;;
    changed) printf 'pub fn changed();\n' ;;
    empty) : ;;
    fail) echo "controlled generator failure" >&2; exit 7 ;;
    *) echo "unknown fixture mode" >&2; exit 2 ;;
esac
FAKE
chmod +x "$tmp/fake-public-api"

run_expect_failure() {
    local mode="$1" expected="$2" action="$3" output status
    set +e
    output=$(PUBLIC_API_FIXTURE_MODE="$mode" PUBLIC_API_GENERATOR="$tmp/fake-public-api" PUBLIC_API_BASELINES_DIR="$baseline_dir" bash "$adapter" "$action" 2>&1)
    status=$?
    set -e
    [[ $status -ne 0 ]] || { echo "expected failure for $mode/$action" >&2; return 1; }
    grep -F "$expected" <<<"$output" >/dev/null
}

run_expect_failure empty INSTRUMENT-FAIL check
before=$(sha256sum "$baseline_dir/perl-lsp-rs.txt")
run_expect_failure fail INSTRUMENT-FAIL update
after=$(sha256sum "$baseline_dir/perl-lsp-rs.txt")
[[ "$before" == "$after" ]] || { echo "failed generation changed baseline" >&2; exit 1; }
run_expect_failure empty INSTRUMENT-FAIL update
PUBLIC_API_FIXTURE_MODE=valid PUBLIC_API_GENERATOR="$tmp/fake-public-api" PUBLIC_API_BASELINES_DIR="$baseline_dir" bash "$adapter" check
PUBLIC_API_FIXTURE_MODE=changed PUBLIC_API_GENERATOR="$tmp/fake-public-api" PUBLIC_API_BASELINES_DIR="$baseline_dir" bash "$adapter" update
grep -F 'pub fn changed();' "$baseline_dir/perl-lsp-rs.txt" >/dev/null
echo "public-api ratchet fixture passed"
