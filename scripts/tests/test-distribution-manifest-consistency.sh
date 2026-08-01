#!/usr/bin/env bash
# Self-test for public package-manifest consistency (issue #5448).
#
# The strings in `distribution/**` and `Formula/perllsp.rb` are what users read
# in `winget show`, `scoop info`, the Chocolatey listing, and `brew info`. They
# had drifted from the repository's own truth in three ways that no gate looked
# at:
#
#   - posture: manifests advertised "public-alpha" while the project shipped a
#     public beta;
#   - license: manifests advertised "MIT" while the project is dual-licensed
#     MIT OR Apache-2.0 (`Cargo.toml` [workspace.package], LICENSE-MIT and
#     LICENSE-APACHE, `Formula/perllsp.rb` `any_of`);
#   - binary set: channels disagreed about whether `perl-dap` ships.
#
# Authorities used here, in order:
#   - license      -> Cargo.toml [workspace.package] license
#   - posture      -> README.md release-posture sentence
#   - asset prefix -> .github/workflows/release.yml NAME=
#
# This gate reads those authorities rather than hardcoding their values, so
# repointing the project (beta -> stable, relicensing) does not require editing
# the assertions — only the authority.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

SCOOP="$ROOT/distribution/scoop/perl-lsp.json"
WINGET="$ROOT/distribution/winget/perl-lsp.yaml"
NUSPEC="$ROOT/distribution/chocolatey/perl-lsp.nuspec"
LINUX_META="$ROOT/distribution/linux/package-metadata.toml"
FORMULA="$ROOT/Formula/perllsp.rb"
INSTALL_PS1="$ROOT/install.ps1"
CANONICAL_INSTALLER="$ROOT/scripts/install.sh"

PASS=0
FAIL=0

pass() {
    printf 'PASS  %s\n' "$1"
    PASS=$((PASS + 1))
}

fail() {
    printf 'FAIL  %s\n' "$1"
    printf '      %s\n' "$2"
    FAIL=$((FAIL + 1))
}

require_file() {
    if [[ ! -f "$1" ]]; then
        fail "manifest exists: ${1#"$ROOT"/}" "missing file"
        return 1
    fi
    return 0
}

# ── Authorities ───────────────────────────────────────────────────────────────

workspace_license() {
    python3 - "$ROOT/Cargo.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as fh:
    data = tomllib.load(fh)
print(data["workspace"]["package"]["license"])
PY
}

release_asset_prefix() {
    # .github/workflows/release.yml: NAME="perllsp"
    sed -n 's/^[[:space:]]*NAME="\([^"]*\)".*/\1/p' \
        "$ROOT/.github/workflows/release.yml" | head -1
}

readme_posture() {
    # "The verified GitHub `v0.17.0` release assets are public beta."
    local readme="$ROOT/README.md"
    local has_beta has_alpha
    has_beta="$(grep -ci 'public beta' "$readme" || true)"
    has_alpha="$(grep -ci 'public alpha\|public-alpha' "$readme" || true)"

    if [[ "$has_beta" -gt 0 && "$has_alpha" -eq 0 ]]; then
        printf 'beta\n'
    elif [[ "$has_alpha" -gt 0 && "$has_beta" -eq 0 ]]; then
        printf 'alpha\n'
    else
        printf 'ambiguous\n'
    fi
}

LICENSE_EXPR="$(workspace_license)"
ASSET_PREFIX="$(release_asset_prefix)"
POSTURE="$(readme_posture)"

echo "Authorities:"
echo "  license expression (Cargo.toml [workspace.package]): $LICENSE_EXPR"
echo "  release asset prefix (release.yml NAME=):            $ASSET_PREFIX"
echo "  release posture (README.md):                         $POSTURE"
echo

if [[ "$POSTURE" == "ambiguous" ]]; then
    fail "README.md states one release posture" \
        "README.md mentions both 'public alpha' and 'public beta' (or neither); cannot derive the posture manifests must match"
fi

if [[ -z "$ASSET_PREFIX" ]]; then
    fail "release.yml declares an asset NAME" "could not parse NAME= from .github/workflows/release.yml"
fi

# The license authority must name both licenses; if it ever becomes single, the
# per-manifest assertions below need revisiting rather than silently passing.
if [[ "$LICENSE_EXPR" != *MIT* || "$LICENSE_EXPR" != *Apache-2.0* ]]; then
    fail "workspace license is the expected dual expression" \
        "Cargo.toml [workspace.package] license is '$LICENSE_EXPR'; this gate asserts manifests carry both MIT and Apache-2.0"
fi

# ── Posture consistency ──────────────────────────────────────────────────────

test_no_stale_posture() {
    local label="no distribution surface advertises the retired posture"
    local wrong="alpha"
    if [[ "$POSTURE" == "alpha" ]]; then
        wrong="beta"
    fi

    local hits
    hits="$(grep -rni "public-${wrong}\|public ${wrong}" \
        "$ROOT/distribution" "$ROOT/Formula" "$ROOT/install.ps1" "$ROOT/install.sh" \
        "$ROOT/scripts/render-linux-packages.py" 2>/dev/null || true)"

    if [[ -n "$hits" ]]; then
        fail "$label" "release posture is '$POSTURE' but these advertise '$wrong':
$hits"
        return
    fi

    pass "$label"
}

test_posture_is_stated() {
    local label="user-visible manifests state the current posture"
    local missing=""
    local file
    for file in "$SCOOP" "$WINGET" "$NUSPEC" "$LINUX_META"; do
        if ! grep -qi "public-${POSTURE}\|public ${POSTURE}" "$file"; then
            missing+="  ${file#"$ROOT"/}
"
        fi
    done

    if [[ -n "$missing" ]]; then
        fail "$label" "these do not mention the '$POSTURE' posture at all:
$missing"
        return
    fi

    pass "$label"
}

# ── License consistency ──────────────────────────────────────────────────────

test_scoop_license() {
    local label="scoop manifest license names both licenses"
    local value
    value="$(jq -r '.license' "$SCOOP")"

    # Scoop's manifest schema takes a free-form SPDX identifier string and uses
    # `|` for OR (ScoopInstaller/Scoop schema.json -> definitions/license).
    if [[ "$value" != *MIT* || "$value" != *Apache-2.0* ]]; then
        fail "$label" "distribution/scoop/perl-lsp.json .license is '$value', expected both MIT and Apache-2.0 (authority: $LICENSE_EXPR)"
        return
    fi

    pass "$label"
}

test_winget_license() {
    local label="winget manifest license names both licenses and links a license page"
    local value url
    value="$(sed -n 's/^License:[[:space:]]*//p' "$WINGET" | head -1)"
    url="$(sed -n 's/^LicenseUrl:[[:space:]]*//p' "$WINGET" | head -1)"

    if [[ "$value" != *MIT* || "$value" != *Apache-2.0* ]]; then
        fail "$label" "distribution/winget/perl-lsp.yaml License is '$value', expected both MIT and Apache-2.0 (authority: $LICENSE_EXPR)"
        return
    fi

    if [[ -z "$url" ]]; then
        fail "$label" "distribution/winget/perl-lsp.yaml has no LicenseUrl; a dual license needs a page a user can read"
        return
    fi

    pass "$label"
}

test_nuspec_license_url() {
    local label="chocolatey licenseUrl does not point at one half of the dual license"
    local url
    url="$(sed -n 's@.*<licenseUrl>\(.*\)</licenseUrl>.*@\1@p' "$NUSPEC" | head -1)"

    if [[ -z "$url" ]]; then
        fail "$label" "distribution/chocolatey/perl-lsp.nuspec has no licenseUrl"
        return
    fi

    case "$url" in
        */LICENSE-MIT|*/LICENSE-APACHE)
            fail "$label" "licenseUrl is '$url' — that is one half of '$LICENSE_EXPR'; point at a page naming both"
            return
            ;;
    esac

    pass "$label"
}

test_linux_metadata_license() {
    local label="linux package metadata license names both licenses"
    local value
    value="$(python3 - "$LINUX_META" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as fh:
    print(tomllib.load(fh)["license"])
PY
)"

    if [[ "$value" != *MIT* || "$value" != *Apache-2.0* ]]; then
        fail "$label" "distribution/linux/package-metadata.toml license is '$value', expected both MIT and Apache-2.0 (authority: $LICENSE_EXPR)"
        return
    fi

    pass "$label"
}

test_formula_license() {
    local label="homebrew formula license names both licenses"

    if ! grep -q 'license any_of: \["MIT", "Apache-2.0"\]' "$FORMULA"; then
        fail "$label" "Formula/perllsp.rb does not declare license any_of: [\"MIT\", \"Apache-2.0\"] (authority: $LICENSE_EXPR)"
        return
    fi

    pass "$label"
}

# ── Binary-set consistency ───────────────────────────────────────────────────

test_every_channel_ships_dap() {
    local label="every install channel installs perl-dap alongside perllsp"
    local missing=""

    if ! jq -e '.bin | index("perl-dap.exe")' "$SCOOP" >/dev/null; then
        missing+="  distribution/scoop/perl-lsp.json .bin
"
    fi
    if ! grep -q '^      - perl-dap$' "$WINGET"; then
        missing+="  distribution/winget/perl-lsp.yaml Commands
"
    fi
    if ! grep -q 'bin.install .*perl-dap' "$FORMULA"; then
        missing+="  Formula/perllsp.rb install
"
    fi
    if ! grep -q 'DAP_BIN_NAME' "$CANONICAL_INSTALLER"; then
        missing+="  scripts/install.sh
"
    fi
    # install.ps1 must both locate and copy the DAP binary, not merely mention it.
    if ! grep -q 'DapSourcePath' "$INSTALL_PS1" || ! grep -q 'Copy-Item -Path \$DapSourcePath' "$INSTALL_PS1"; then
        missing+="  install.ps1 (Windows PowerShell installer)
"
    fi

    if [[ -n "$missing" ]]; then
        fail "$label" "these channels do not install perl-dap:
$missing"
        return
    fi

    pass "$label"
}

# ── Release-asset naming ─────────────────────────────────────────────────────

test_asset_names_match_release_workflow() {
    local label="manifests reference release assets by their published name"
    local bad=""

    # Every archive URL / asset template in the distribution surface must use the
    # prefix the release workflow actually publishes.
    local hits
    hits="$(grep -rn 'x86_64-pc-windows-msvc\.zip\|unknown-linux-gnu\.tar\.gz\|apple-darwin\.tar\.gz' \
        "$ROOT/distribution" "$FORMULA" 2>/dev/null | grep -v '\.md:' || true)"

    local line
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        local asset
        # The char class keeps `$ { }` so templated names such as
        # `perllsp-${packageVersion}-x86_64-...` and `perllsp-$version-...`
        # are captured whole, prefix included.
        asset="$(printf '%s\n' "$line" \
            | grep -o '[A-Za-z0-9_.${}#-]*-\(x86_64\|aarch64\)-[a-z0-9-]*\.\(zip\|tar\.gz\)' | head -1)"
        [[ -z "$asset" ]] && continue
        case "$asset" in
            "$ASSET_PREFIX"-*) ;;
            *) bad+="  $line
" ;;
        esac
    done <<< "$hits"

    if [[ -n "$bad" ]]; then
        fail "$label" "release.yml publishes '${ASSET_PREFIX}-<version>-<target>' assets; these reference something else (they would 404):
$bad"
        return
    fi

    pass "$label"
}

# ── Single Homebrew formula ──────────────────────────────────────────────────

test_single_homebrew_formula() {
    local label="exactly one Homebrew formula exists"

    local found
    found="$(find "$ROOT/distribution" "$ROOT/Formula" -name '*.rb' -type f 2>/dev/null | sort)"
    local expected="$FORMULA"

    if [[ "$found" != "$expected" ]]; then
        fail "$label" "Formula/perllsp.rb is the live artifact (xtask/src/tasks/update_homebrew.rs, .github/workflows/brew-bump.yml). Found instead:
$found"
        return
    fi

    pass "$label"
}

# ── Placeholder guards ───────────────────────────────────────────────────────

test_release_placeholders_intact() {
    local label="version placeholders are still tokens, not a frozen version"
    local missing=""
    local file
    for file in "$SCOOP" "$WINGET" "$NUSPEC" "$LINUX_META" "$FORMULA"; do
        if ! grep -q '__RELEASE_VERSION__' "$file"; then
            missing+="  ${file#"$ROOT"/}
"
        fi
    done

    if [[ -n "$missing" ]]; then
        fail "$label" "release automation replaces __RELEASE_VERSION__; these no longer carry it:
$missing"
        return
    fi

    pass "$label"
}

# ── Structural validity ──────────────────────────────────────────────────────

test_manifests_parse() {
    local label="every manifest parses in its own format"
    local errors=""

    if ! jq empty "$SCOOP" 2>/dev/null; then
        errors+="  distribution/scoop/perl-lsp.json is not valid JSON
"
    fi

    if ! python3 - "$NUSPEC" <<'PY' 2>/dev/null
import sys, xml.etree.ElementTree as ET
ET.parse(sys.argv[1])
PY
    then
        errors+="  distribution/chocolatey/perl-lsp.nuspec is not well-formed XML (choco pack would fail)
"
    fi

    if ! python3 - "$LINUX_META" <<'PY' 2>/dev/null
import sys, tomllib
with open(sys.argv[1], "rb") as fh:
    tomllib.load(fh)
PY
    then
        errors+="  distribution/linux/package-metadata.toml is not valid TOML
"
    fi

    if [[ -n "$errors" ]]; then
        fail "$label" "$errors"
        return
    fi

    pass "$label"
}

for f in "$SCOOP" "$WINGET" "$NUSPEC" "$LINUX_META" "$FORMULA" "$INSTALL_PS1" "$CANONICAL_INSTALLER"; do
    require_file "$f" || true
done

if [[ "$FAIL" -eq 0 ]]; then
    test_manifests_parse
    test_no_stale_posture
    test_posture_is_stated
    test_scoop_license
    test_winget_license
    test_nuspec_license_url
    test_linux_metadata_license
    test_formula_license
    test_every_channel_ships_dap
    test_asset_names_match_release_workflow
    test_single_homebrew_formula
    test_release_placeholders_intact
fi

echo
echo "-- Summary --"
echo "Passed: $PASS"
echo "Failed: $FAIL"

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
