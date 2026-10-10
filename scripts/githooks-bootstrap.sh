#!/usr/bin/env bash
set -euo pipefail

# Run the revision-owned hygiene implementation in this worktree's admitted
# private output paths. Cargo selects its executable; never use a PATH binary
# or guess target/debug. The admission wrapper owns toolchain/storage refusal.
REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
case "${1:-}" in
  install-githooks|check-githooks) ;;
  *) echo 'githooks-bootstrap: expected install-githooks or check-githooks' >&2; exit 2 ;;
esac
cd -- "$REPO_ROOT"
exec "$REPO_ROOT/scripts/cargo-admitted" run --locked -p perl-ci-hygiene -- "$@"
