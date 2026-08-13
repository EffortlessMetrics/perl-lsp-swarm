#!/usr/bin/env bash
# Apply the reviewed perllsp candidate to the exact upstream zed-perl base.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly PACKET_ROOT="$REPO_ROOT/.ci/fixtures/zed-perl-upstream"
readonly CANDIDATE_ROOT="$PACKET_ROOT/zed-perl"
readonly MANIFEST="$PACKET_ROOT/manifest.toml"

usage() {
    printf 'usage: %s /path/to/zed-perl\n' "$(basename "$0")" >&2
}

if [[ $# -ne 1 ]]; then
    usage
    exit 2
fi

readonly TARGET="$1"

if [[ ! -d "$TARGET/.git" && ! -f "$TARGET/.git" ]]; then
    printf 'error: target is not a Git checkout: %s\n' "$TARGET" >&2
    exit 2
fi

if [[ ! -f "$MANIFEST" ]]; then
    printf 'error: submission manifest is missing: %s\n' "$MANIFEST" >&2
    exit 2
fi

readarray -t packet < <(
    python3 - "$MANIFEST" <<'PY'
import sys
import tomllib
from pathlib import Path

manifest_path = Path(sys.argv[1])
with manifest_path.open("rb") as handle:
    manifest = tomllib.load(handle)

print(manifest["upstream_base"])
for path in manifest["copied_files"]:
    print(path)
PY
)

readonly EXPECTED_BASE="${packet[0]}"
readonly CURRENT_HEAD="$(git -C "$TARGET" rev-parse HEAD)"

if [[ "$CURRENT_HEAD" != "$EXPECTED_BASE" ]]; then
    printf 'error: target HEAD is %s; expected exact prepared base %s\n' \
        "$CURRENT_HEAD" "$EXPECTED_BASE" >&2
    printf 'rebase the packet deliberately instead of bypassing the identity check.\n' >&2
    exit 1
fi

if [[ -n "$(git -C "$TARGET" status --porcelain=v1 --untracked-files=all)" ]]; then
    printf 'error: target checkout is not clean: %s\n' "$TARGET" >&2
    git -C "$TARGET" status --short >&2
    exit 1
fi

for relative_path in "${packet[@]:1}"; do
    readonly_source="$CANDIDATE_ROOT/$relative_path"
    target_path="$TARGET/$relative_path"
    if [[ ! -f "$readonly_source" ]]; then
        printf 'error: staged source is missing: %s\n' "$readonly_source" >&2
        exit 1
    fi
    mkdir -p "$(dirname "$target_path")"
    cp "$readonly_source" "$target_path"
done

git -C "$TARGET" diff --check

printf 'Applied Zed Perl candidate to %s at base %s.\n' "$TARGET" "$EXPECTED_BASE"
printf '\nReview the cumulative diff:\n'
printf '  git -C %q diff --stat\n' "$TARGET"
printf '  git -C %q diff\n' "$TARGET"
printf '\nRun upstream verification:\n'
printf '  cargo fmt --manifest-path %q/Cargo.toml -- --check\n' "$TARGET"
printf '  cargo clippy --manifest-path %q/Cargo.toml --all-targets -- -D warnings\n' "$TARGET"
printf '  cargo test --manifest-path %q/Cargo.toml\n' "$TARGET"
printf '  cargo build --manifest-path %q/Cargo.toml --target wasm32-wasip2 --release\n' "$TARGET"
