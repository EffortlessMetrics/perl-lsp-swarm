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
if [[ "$PERL_LSP_INSTALLER_SHA256" == 0000000000000000000000000000000000000000000000000000000000000000 ]]; then
    echo 'SHA-256 mismatch' >&2; exit 1
fi
if [[ "$PERL_LSP_INSTALLER_REF" == v* ]]; then exit 0; fi
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
if SECTION="$tmp/section" ACCEPT_TAG=1 bash "$tmp/harness" > "$tmp/bad"; then
    echo 'FAIL: smoke accepted a wrapper that accepts a tag ref'; exit 1
fi
grep -q 'accepted a non-dereferenced tag ref' "$tmp/bad" || { cat "$tmp/bad"; exit 1; }
echo 'PASS: smoke accepts correct wrapper and rejects tag-accepting wrapper'
