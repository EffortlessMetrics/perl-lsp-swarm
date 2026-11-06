#!/usr/bin/env bash
# Exercise the actual smoke section with a controlled published wrapper.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
awk '/^section "Root install.sh wrapper \(identity-bound bootstrap\)"/{on=1} /^# Section 2: Library consumption/{on=0} on {print}' "$root/scripts/post-publish-smoke.sh" > "$tmp/section"
cat > "$tmp/harness" <<'HARNESS'
set -uo pipefail
VERSION=1.2.3 INSTALLER_HOST=Linux SKIP_INSTALL=0
CLEANUP_DIRS=() failures=0
section() { :; }
pass() { printf 'PASS %s\n' "$*"; }
fail() { failures=$((failures + 1)); printf 'FAIL %s\n' "$*"; }
git() { printf '%040d\trefs/tags/v1.2.3^{}\n' 1; }
curl() {
    case "$*" in
        *scripts/install.sh*) printf 'canonical installer';;
        *install.sh*)
            cat > "$4" <<'WRAPPER'
#!/usr/bin/env bash
if [[ "$PERL_LSP_INSTALLER_REF" == v* && "${ACCEPT_TAG:-0}" != 1 ]]; then
    echo 'PERL_LSP_INSTALLER_REF must be a full lowercase commit SHA' >&2; exit 1
fi
if [[ "$PERL_LSP_INSTALLER_SHA256" == 0000000000000000000000000000000000000000000000000000000000000000 && "${ACCEPT_DIGEST:-0}" != 1 ]]; then
    echo 'SHA-256 mismatch' >&2; exit 1
fi
if [[ "$PERL_LSP_INSTALLER_REF" == v* || "$PERL_LSP_INSTALLER_SHA256" == 0000000000000000000000000000000000000000000000000000000000000000 ]]; then
    destination="${INSTALL_DIR:-$HOME/.local/bin}"
    mkdir -p "$destination"
    printf 'attempted write\n' > "$destination/tag-write"
    printf '%s\n%s\n' "$destination" "${VERSION:-latest}" > "$WRITE_RECORD"
    exit 0
fi
printf '#!/bin/sh\necho perllsp 1.2.3\n' > "$INSTALL_DIR/perllsp"
chmod +x "$INSTALL_DIR/perllsp"
WRAPPER
            ;;
        *) return 1;;
    esac
}
source "$SECTION"
for dir in "${CLEANUP_DIRS[@]}"; do rm -rf "$dir"; done
(( failures == 0 ))
HARNESS
if ! SECTION="$tmp/section" bash "$tmp/harness" > "$tmp/good"; then cat "$tmp/good"; exit 1; fi
mkdir -p "$tmp/home"
check_accepting_wrapper() {
    local mode="$1" expected="$2" destination version
    if env SECTION="$tmp/section" "$mode" TMPDIR="$tmp" HOME="$tmp/home" WRITE_RECORD="$tmp/write-record" bash "$tmp/harness" > "$tmp/bad"; then
        echo "FAIL: smoke accepted $mode"; exit 1
    fi
    grep -q "$expected" "$tmp/bad" || { cat "$tmp/bad"; exit 1; }
    [[ -f "$tmp/write-record" ]] || { echo 'FAIL: accepting wrapper did not attempt write'; exit 1; }
    { IFS= read -r destination; IFS= read -r version; } < "$tmp/write-record"
    [[ "$destination" == "$tmp"/* && "$destination" != "$tmp/home"/* && "$version" == v1.2.3 ]] || {
        printf 'FAIL: unsafe destination or version: %s / %s\n' "$destination" "$version"; exit 1;
    }
    [[ ! -e "$destination" ]] || { echo 'FAIL: negative install destination was not cleaned'; exit 1; }
    [[ ! -e "$tmp/home/.local/bin/tag-write" ]] || { echo 'FAIL: wrote to default destination'; exit 1; }
    rm -f "$tmp/write-record"
}
check_accepting_wrapper ACCEPT_TAG=1 'accepted a non-dereferenced tag ref'
check_accepting_wrapper ACCEPT_DIGEST=1 'accepted wrong PERL_LSP_INSTALLER_SHA256'
echo 'PASS: smoke rejects tag- and digest-accepting wrappers and confines their writes'
