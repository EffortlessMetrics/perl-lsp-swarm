#!/usr/bin/env bash
# Run the packaged-binary LSP and DAP smokes for one issue #5432 variant and
# retain each receipt beside that variant's artifacts.
#
# Usage: release_artifact_size_smoke.sh <variant> <target> <version>
#
# `xtask lsp-ux-smoke` writes its receipt to one fixed path
# (`target/receipts/ux/lsp-ux-smoke.json`). Running the baseline and the
# candidate in the same job therefore has a real failure mode: if the second run
# does not write, the first run's receipt is still sitting there and would be
# compared against the second variant's binary. This script removes the fixed
# path before the run and fails if it is not recreated, so a stale receipt can
# never be retained as the candidate's evidence.
#
# `release_artifact_size` independently requires each receipt's `binary` field
# to name the measured binary, so a stale receipt is rejected twice.

set -euo pipefail

if [ "$#" -ne 3 ]; then
  printf 'usage: %s <variant> <target> <version>\n' "$0" >&2
  exit 2
fi

VARIANT="$1"
TARGET="$2"
VERSION="$3"

case "$VARIANT" in
  baseline | candidate) ;;
  *)
    printf 'unknown variant %s: expected baseline or candidate\n' "$VARIANT" >&2
    exit 2
    ;;
esac

ROOT="${RELEASE_ARTIFACT_SIZE_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

STAGE_DIR="${ROOT}/target/shadow/${VARIANT}"
PKG_DIR="${STAGE_DIR}/perllsp-${VERSION}-${TARGET}"
LSP_BINARY="${PKG_DIR}/perllsp"
DAP_BINARY="${PKG_DIR}/perl-dap"
LSP_FIXED_RECEIPT="${ROOT}/target/receipts/ux/lsp-ux-smoke.json"
LSP_RECEIPT="${STAGE_DIR}/lsp-smoke.json"
DAP_RECEIPT="${STAGE_DIR}/dap-smoke.json"
DAP_SMOKE_TEST="${DAP_SMOKE_TEST:-stdio_transport_framing_initialize_threads_disconnect}"
# Overridable so the retention contract below can be proven without two release
# builds and a macOS runner. Production dispatches leave this as `cargo`.
CARGO="${CARGO:-cargo}"

for binary in "$LSP_BINARY" "$DAP_BINARY"; do
  if [ ! -x "$binary" ]; then
    printf 'missing packaged binary %s; stage the %s variant first\n' "$binary" "$VARIANT" >&2
    exit 1
  fi
done

rm -f "$LSP_FIXED_RECEIPT" "$LSP_RECEIPT" "$DAP_RECEIPT"

# Smoke the exact packaged binary, not a freshly built one.
"$CARGO" run --locked -p xtask -- lsp-ux-smoke --binary "$LSP_BINARY" --receipt

if [ ! -f "$LSP_FIXED_RECEIPT" ]; then
  printf 'the %s LSP smoke wrote no receipt at %s\n' "$VARIANT" "$LSP_FIXED_RECEIPT" >&2
  exit 1
fi
mv "$LSP_FIXED_RECEIPT" "$LSP_RECEIPT"

PERL_DAP_TEST_BINARY="$DAP_BINARY" \
PERL_DAP_SMOKE_RECEIPT="$DAP_RECEIPT" \
  "$CARGO" test --locked -p perl-dap --test dap_stdio_transport_e2e -- --exact "$DAP_SMOKE_TEST"

if [ ! -f "$DAP_RECEIPT" ]; then
  printf 'the %s DAP smoke wrote no receipt at %s\n' "$VARIANT" "$DAP_RECEIPT" >&2
  exit 1
fi

printf 'retained %s smoke receipts: %s %s\n' "$VARIANT" "$LSP_RECEIPT" "$DAP_RECEIPT"
