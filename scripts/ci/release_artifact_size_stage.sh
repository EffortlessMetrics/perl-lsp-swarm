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

rm -rf "$PKG_DIR" "$ARCHIVE"
mkdir -p "$PKG_DIR"

for binary in "${BINARIES[@]}"; do
  if [ ! -f "${BUILD_DIR}/${binary}" ]; then
    printf 'missing %s: the %s build did not produce it\n' "${BUILD_DIR}/${binary}" "$VARIANT" >&2
    exit 1
  fi
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

tar czf "$ARCHIVE" -C "$STAGE_DIR" "$PKG_NAME"

# Prove the archive carries exactly the staged bytes before any measurement
# consumes it.
EXTRACT_DIR="$(mktemp -d)"
trap 'rm -rf "$EXTRACT_DIR"' EXIT
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

printf 'staged %s archive %s\n' "$VARIANT" "$ARCHIVE"
