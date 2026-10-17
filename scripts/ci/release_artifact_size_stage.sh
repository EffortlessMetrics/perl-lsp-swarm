#!/usr/bin/env bash
# Stage one release-shaped variant for the issue #5432 safe-ICF measurement.
#
# Usage: release_artifact_size_stage.sh <variant> <target> <version>
#
# Copies the freshly linked `perllsp` and `perl-dap` out of the target
# directory, strips them, packages them exactly as `.github/workflows/release.yml`
# packages a macOS release, and proves the archive carries the same bytes as the
# staged directory.
#
# `release_artifact_size` compares the staged directory against the archive and
# refuses a measurement whose archive does not contain the measured binaries, so
# this script fails closed rather than leaving that mismatch for the instrument
# to discover after a second full release build.
#
# Everything is written under the gitignored `target/` tree: the instrument
# records `subject_complete` only for a clean working tree, and a staging
# directory committed into the checkout would make every measurement
# `not_proven`.

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

BUILD_DIR="target/${TARGET}/release"
STAGE_DIR="target/shadow/${VARIANT}"
PKG_NAME="perllsp-${VERSION}-${TARGET}"
PKG_DIR="${STAGE_DIR}/${PKG_NAME}"
ARCHIVE="${STAGE_DIR}/${PKG_NAME}.tar.gz"
BINARIES=(perllsp perl-dap)

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# Preflight the whole input set before touching any existing witness.
for binary in "${BINARIES[@]}"; do
  if [ ! -f "${BUILD_DIR}/${binary}" ]; then
    printf 'missing %s: the %s build did not produce it\n' "${BUILD_DIR}/${binary}" "$VARIANT" >&2
    exit 1
  fi
done

# Prepare and validate privately on the destination filesystem. Never reuse or
# sweep a scratch name: another run's interrupted work may still be recoverable.
mkdir -p "$STAGE_DIR"
FINAL_PKG_DIR="$PKG_DIR"
FINAL_ARCHIVE="$ARCHIVE"
LOCK_DIR="${STAGE_DIR}/.${PKG_NAME}.stage-lock"
LOCK_HELD=0
WORK_DIR=""
PUBLISHING=0
COMMITTED=0

restore_previous() {
  # A missing prepared entry means this invocation installed the new entry.
  if [ ! -e "$1" ] && [ ! -L "$1" ]; then
    rm -rf "$2" || return 1
  fi
  if [ -e "$3" ] || [ -L "$3" ]; then
    # Do not nest a backup inside a surviving/uncertain destination directory.
    if [ -e "$2" ] || [ -L "$2" ]; then return 1; fi
    mv "$3" "$2" || return 1
  fi
}

cleanup() {
  status=$?
  trap - EXIT
  trap '' INT TERM
  recovery_failed=0
  if [ "$PUBLISHING" -eq 1 ] && [ "$COMMITTED" -eq 0 ]; then
    restore_previous "$PKG_DIR" "$FINAL_PKG_DIR" "${WORK_DIR}/previous-package" || recovery_failed=1
    restore_previous "$ARCHIVE" "$FINAL_ARCHIVE" "${WORK_DIR}/previous-archive" || recovery_failed=1
  fi
  if [ "$recovery_failed" -ne 0 ]; then
    printf 'staging rollback incomplete; retained recovery data at %s; lock retained at %s\n' "$WORK_DIR" "$LOCK_DIR" >&2
    exit 1
  fi
  if [ -n "$WORK_DIR" ]; then
    rm -rf "$WORK_DIR" || exit 1
  fi
  if [ "$LOCK_HELD" -eq 1 ]; then
    rm -f "${LOCK_DIR}/transaction" || exit 1
    rmdir "$LOCK_DIR" || exit 1
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Refuse overlapping writers, including one with incomplete retained recovery.
# Never reclaim another invocation's lock or scratch directory automatically.
if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  printf 'staging lock unavailable at %s; refusing overlapping publication\n' "$LOCK_DIR" >&2
  exit 1
fi
LOCK_HELD=1
WORK_DIR="$(mktemp -d "${STAGE_DIR}/.stage.XXXXXX")"
printf '%s\n' "$WORK_DIR" > "${LOCK_DIR}/transaction"
PKG_DIR="${WORK_DIR}/${PKG_NAME}"
ARCHIVE="${WORK_DIR}/${PKG_NAME}.tar.gz"
mkdir -p "$PKG_DIR"

for binary in "${BINARIES[@]}"; do
  cp "${BUILD_DIR}/${binary}" "${PKG_DIR}/${binary}"
  # Unlike release.yml this does not swallow a strip failure. The whole claim is
  # a post-strip size comparison, so an unstripped binary is a measurement
  # error, not a packaging inconvenience.
  if ! strip "${PKG_DIR}/${binary}"; then
    printf 'refusing to package unstripped %s: strip failed\n' "$binary" >&2
    exit 1
  fi
done

# Mirror the release package layout so the measured archive is release-shaped.
for extra in README.md LICENSE-APACHE LICENSE-MIT; do
  if [ -f "$extra" ]; then
    cp "$extra" "${PKG_DIR}/"
  fi
done

(
  cd "$PKG_DIR"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${BINARIES[@]}" > SHA256SUMS.txt
  else
    shasum -a 256 "${BINARIES[@]}" > SHA256SUMS.txt
  fi
)

tar czf "$ARCHIVE" -C "$WORK_DIR" "$PKG_NAME"

# Prove the archive carries exactly the staged bytes before any measurement
# consumes it.
EXTRACT_DIR="${WORK_DIR}/extracted"
mkdir "$EXTRACT_DIR"
tar xzf "$ARCHIVE" -C "$EXTRACT_DIR"

for binary in "${BINARIES[@]}"; do
  staged="$(sha256_of "${PKG_DIR}/${binary}")"
  embedded="$(sha256_of "${EXTRACT_DIR}/${PKG_NAME}/${binary}")"
  if [ "$staged" != "$embedded" ]; then
    printf '%s archive member %s does not match the staged binary\n' "$VARIANT" "$binary" >&2
    exit 1
  fi
  printf '%s %s %s %s\n' "$VARIANT" "$binary" "$(wc -c < "${PKG_DIR}/${binary}" | tr -d ' ')" "$staged"
done

# Hold writer exclusion through promotion and any rollback. Readers can still
# observe the two separate renames; pair-atomic visibility is not provided.
# Keep the old pair until both promotions succeed. Failed recovery retains the
# backups and lock. Unrelated variant data and scratch allocations survive.
PUBLISHING=1
if [ -e "$FINAL_PKG_DIR" ] || [ -L "$FINAL_PKG_DIR" ]; then
  mv "$FINAL_PKG_DIR" "${WORK_DIR}/previous-package"
fi
if [ -e "$FINAL_ARCHIVE" ] || [ -L "$FINAL_ARCHIVE" ]; then
  mv "$FINAL_ARCHIVE" "${WORK_DIR}/previous-archive"
fi
mv "$PKG_DIR" "$FINAL_PKG_DIR"
mv "$ARCHIVE" "$FINAL_ARCHIVE"
COMMITTED=1

printf 'staged %s archive %s\n' "$VARIANT" "$FINAL_ARCHIVE"
