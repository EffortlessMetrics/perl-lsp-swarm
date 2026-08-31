#!/usr/bin/env bash
# Perl LSP installer — Linux and macOS
#
# Run from a reviewed clone or through the identity-bound root install.sh
# wrapper. The canonical installer is not itself a mutable curl-pipe authority.
#
# Options via environment variables:
#   VERSION=v0.12.0 INSTALL_DIR=/usr/local/bin bash scripts/install.sh
#   PERL_LSP_LINUX_LIBC=gnu bash scripts/install.sh
#   PERL_LSP_LINUX_LIBC=musl bash scripts/install.sh
#   BUILD_FROM_SOURCE=1 bash scripts/install.sh   # force cargo build/install
#   bash scripts/install.sh --print-target
#   bash scripts/install.sh --with-claude        # install perllsp, then reconcile Claude
#
# Supported platforms:
#   Linux x86_64 (musl/gnu), Linux aarch64 (musl/gnu), macOS x86_64, macOS aarch64
set -euo pipefail

REPO="EffortlessMetrics/perl-lsp"
BIN_NAME="perllsp"
DAP_BIN_NAME="perl-dap"
VERSION="${VERSION:-latest}"
PERL_LSP_LINUX_LIBC="${PERL_LSP_LINUX_LIBC:-auto}"
PREFER_GNU="${PREFER_GNU:-0}"
BUILD_FROM_SOURCE="${BUILD_FROM_SOURCE:-0}"
PRINT_TARGET=0
WITH_CLAUDE=0
CLAUDE_SETUP_RESULT="not_requested"

# Determine install directory: user-local by default, system-wide if explicitly set
if [ -z "${INSTALL_DIR:-}" ]; then
    if [ -n "${TERMUX_VERSION:-}" ] || [ -d "/data/data/com.termux/files/usr/bin" ]; then
        INSTALL_DIR="/data/data/com.termux/files/usr/bin"
    elif [ -w /usr/local/bin ] 2>/dev/null; then
        INSTALL_DIR="/usr/local/bin"
    else
        INSTALL_DIR="$HOME/.local/bin"
    fi
fi

# ── Output helpers ─────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

say()     { printf '%b\n' "$1"; }
info()    { say "${GREEN}=>${NC} $1"; }
warn()    { say "${YELLOW}warning:${NC} $1" >&2; }
# First-install selector rollback is invoked from err() and from INT/TERM/HUP
# during the selector-to-commit window so injected faults and signals drop
# newly created PATH names without replacing the caller's EXIT trap (main
# removes TMPDIR that way). Once current already names the incoming candidate,
# those PATH names are live for that unit and must not be removed.
committed_incoming_product_unit() {
    local _store _cur
    [ -n "${_plsp_incoming_id:-}" ] || return 1
    _store="$(product_store_dir)"
    [ -L "${_store}/current" ] || return 1
    _cur="$(readlink "${_store}/current" 2>/dev/null || true)"
    [ "$_cur" = "candidates/${_plsp_incoming_id}" ]
}
rollback_new_path_selectors() {
    if committed_incoming_product_unit; then
        _plsp_rollback_server=""
        _plsp_rollback_dap=""
        return 0
    fi
    if [ -n "${_plsp_rollback_server:-}" ]; then
        rm -f -- "$_plsp_rollback_server"
    fi
    if [ -n "${_plsp_rollback_dap:-}" ]; then
        rm -f -- "$_plsp_rollback_dap"
    fi
    _plsp_rollback_server=""
    _plsp_rollback_dap=""
}
restore_saved_signal_trap() {
    local _saved="$1" _sig="$2"
    if [ -n "$_saved" ]; then
        eval "$_saved"
    else
        trap - "$_sig"
    fi
}
# Do not install an EXIT handler here: that would replace main's TMPDIR
# cleanup. exit after rollback so the caller's EXIT trap still runs.
rollback_new_path_selectors_on_signal() {
    rollback_new_path_selectors
    disarm_new_path_selector_signal_rollback
    exit 1
}
arm_new_path_selector_signal_rollback() {
    _plsp_prev_int="$(trap -p INT 2>/dev/null || true)"
    _plsp_prev_term="$(trap -p TERM 2>/dev/null || true)"
    _plsp_prev_hup="$(trap -p HUP 2>/dev/null || true)"
    trap rollback_new_path_selectors_on_signal INT TERM HUP
}
disarm_new_path_selector_signal_rollback() {
    restore_saved_signal_trap "${_plsp_prev_int:-}" INT
    restore_saved_signal_trap "${_plsp_prev_term:-}" TERM
    restore_saved_signal_trap "${_plsp_prev_hup:-}" HUP
    _plsp_prev_int=""
    _plsp_prev_term=""
    _plsp_prev_hup=""
}
err()     { rollback_new_path_selectors; say "${RED}error:${NC} $1" >&2; exit 1; }

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        err "required command not found: $1"
    fi
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --print-target)
            PRINT_TARGET=1
            shift
            ;;
        --with-claude)
            WITH_CLAUDE=1
            shift
            ;;
        -h|--help)
            cat <<'USAGE'
Usage: scripts/install.sh [--print-target] [--with-claude]

Options:
  --print-target                         Print selected release target and exit.
  --with-claude                          After installing/verifying perllsp, run
                                         `perllsp setup claude` through the exact
                                         installed binary.

Environment:
  VERSION=<latest|vX.Y.Z|X.Y.Z>        Release to install.
  INSTALL_DIR=/path/to/bin             Install destination.
  PERL_LSP_LINUX_LIBC=auto|gnu|glibc|musl
                                      Linux libc target. Default: auto.
  BUILD_FROM_SOURCE=1                  Build with cargo instead of downloading.

Most Linux distributions use gnu/glibc. Use musl mainly for Alpine Linux and
musl-based containers. --print-target prints the selected release target and
exits without downloading.

--with-claude is composition only: the installer still installs one `perllsp`
binary and delegates all Claude marketplace/plugin lifecycle to the Rust-owned
`perllsp setup claude` command. It does not prove fresh-process PATH visibility;
that remains a separate installation receipt.
USAGE
            exit 0
            ;;
        *)
            err "unknown argument: $1"
            ;;
    esac
done

is_musl_system() {
    if command -v ldd >/dev/null 2>&1; then
        local _ldd
        _ldd="$(ldd --version 2>&1 || true)"
        if printf '%s\n' "$_ldd" | grep -qi musl; then
            return 0
        fi
        if printf '%s\n' "$_ldd" | grep -Eqi 'glibc|gnu libc'; then
            return 1
        fi
    fi

    if command -v getconf >/dev/null 2>&1 && getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
        return 1
    fi

    if [ -f /etc/alpine-release ]; then
        return 0
    fi

    if compgen -G "/lib/ld-musl-*.so.1" >/dev/null 2>&1 ||
       compgen -G "/usr/lib/ld-musl-*.so.1" >/dev/null 2>&1 ||
       compgen -G "/lib/libc.musl-*.so.1" >/dev/null 2>&1 ||
       compgen -G "/usr/lib/libc.musl-*.so.1" >/dev/null 2>&1; then
        return 0
    fi

    return 1
}

resolve_linux_libc() {
    local _choice
    _choice="$(printf '%s' "$PERL_LSP_LINUX_LIBC" | tr '[:upper:]' '[:lower:]')"

    # Backwards-compatible escape hatch from older docs/scripts. The new public
    # spelling is PERL_LSP_LINUX_LIBC=gnu.
    if [ "$_choice" = "auto" ] && [ "$PREFER_GNU" = "1" ]; then
        _choice="gnu"
    fi

    case "$_choice" in
        auto)
            if is_musl_system; then
                printf '%s\n' "musl"
            else
                printf '%s\n' "gnu"
            fi
            ;;
        gnu|glibc)
            printf '%s\n' "gnu"
            ;;
        musl)
            printf '%s\n' "musl"
            ;;
        *)
            err "invalid PERL_LSP_LINUX_LIBC=${PERL_LSP_LINUX_LIBC}; expected auto, gnu, glibc, or musl"
            ;;
    esac
}

# ── Platform detection ─────────────────────────────────────────────────────────

detect_platform() {
    local _os _arch _libc _termux

    _os="$(uname -s)"
    _arch="$(uname -m)"
    _termux=0
    SOURCE_TARGET=""
    INSTALL_MODE="release"

    if [ -n "${TERMUX_VERSION:-}" ] || [ -d "/data/data/com.termux/files/usr/bin" ]; then
        _termux=1
    fi

    case "$_os" in
        Linux)
            _os="linux"
            if [ "$_termux" = "1" ]; then
                # Termux uses Android's bionic libc. The release artifacts are
                # Linux glibc/musl binaries, not Android/bionic binaries.
                INSTALL_MODE="source"
                SOURCE_TARGET=""
            else
                _libc="$(resolve_linux_libc)"
            fi
            ;;
        Darwin)
            _os="darwin"
            _libc=""
            ;;
        MINGW*|MSYS*|CYGWIN*)
            # Do not send Windows users to the piped PowerShell installer: the
            # copy published at $REPO/master still builds a perl-lsp-*.zip asset
            # name while releases ship perllsp-*.zip, so it 404s (#5461, fix
            # pending promotion in #4348). Point at the archive that works.
            err "Windows is not supported by this script. Download
  perllsp-<version>-x86_64-pc-windows-msvc.zip
from https://github.com/$REPO/releases, extract it, and put the folder
containing perllsp.exe on your PATH.

The PowerShell installer is not usable yet — the published copy builds a
download URL that 404s. See
https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5461"
            ;;
        *)
            err "unsupported operating system: $_os"
            ;;
    esac

    case "$_arch" in
        x86_64|amd64|x64) _arch="x86_64" ;;
        aarch64|arm64)    _arch="aarch64" ;;
        armv8l|armv7l|armv7*|armhf)
            if [ "$_os" = "linux" ]; then
                _arch="armv7"
                if [ "$_termux" != "1" ]; then
                    SOURCE_TARGET="armv7-unknown-linux-gnueabihf"
                fi
                INSTALL_MODE="source"
            else
                err "unsupported architecture: $_arch"
            fi
            ;;
        armv6l|armv6*|arm)
            if [ "$_os" = "linux" ]; then
                _arch="armv6"
                if [ "$_termux" != "1" ]; then
                    SOURCE_TARGET="arm-unknown-linux-gnueabihf"
                fi
                INSTALL_MODE="source"
            else
                err "unsupported architecture: $_arch"
            fi
            ;;
        *) err "unsupported architecture: $_arch" ;;
    esac

    if [ "$BUILD_FROM_SOURCE" = "1" ]; then
        INSTALL_MODE="source"
    fi

    if [ "$INSTALL_MODE" = "release" ]; then
        if [ "$_os" = "linux" ]; then
            TARGET="${_arch}-unknown-linux-${_libc}"
        else
            TARGET="${_arch}-apple-darwin"
        fi
        if [ "$PRINT_TARGET" != "1" ]; then
            info "platform: $_os $_arch (target: $TARGET)"
        fi
    else
        TARGET="${SOURCE_TARGET:-}"
        if [ "$PRINT_TARGET" != "1" ]; then
            if [ -n "$TARGET" ]; then
                info "platform: $_os $_arch (source build target: $TARGET)"
            else
                info "platform: $_os $_arch (source build mode)"
            fi
            if [ "$_termux" = "1" ]; then
                info "termux environment detected; no Android/bionic release asset is available, using source build mode"
            fi
        fi
    fi
}

# ── Version resolution ─────────────────────────────────────────────────────────

resolve_version() {
    if [ "$VERSION" = "latest" ]; then
        info "fetching latest release..."
        local _api_url="https://api.github.com/repos/${REPO}/releases/latest"
        local _json

        if ! _json="$(curl -fsSL "$_api_url" 2>/dev/null)"; then
            err "failed to query GitHub API: ${_api_url}
Check your internet connection or set VERSION=v<x.y.z> to pin a version."
        fi

        TAG="$(printf '%s' "$_json" | grep '"tag_name"' | sed -E 's/.*"tag_name": ?"([^"]+)".*/\1/')"
        if [ -z "$TAG" ]; then
            err "could not parse tag_name from GitHub API response"
        fi
    else
        # Accept "0.12.0" or "v0.12.0"
        case "$VERSION" in
            v*) TAG="$VERSION" ;;
            *)  TAG="v$VERSION" ;;
        esac
    fi

    # Strip leading 'v' to form the version number used in asset names.
    VERSION_NUM="${TAG#v}"
    info "version: $TAG"
}

# ── Download and verify ────────────────────────────────────────────────────────

select_sha256_tool() {
    if command -v sha256sum >/dev/null 2>&1; then
        printf '%s\n' "sha256sum"
    elif command -v shasum >/dev/null 2>&1; then
        printf '%s\n' "shasum"
    else
        err "sha256sum or shasum is required to verify release artifacts"
    fi
}

calculate_sha256() {
    local _tool="$1" _path="$2" _output
    case "$_tool" in
        sha256sum)
            _output="$(sha256sum "$_path")" || err "sha256sum failed for $_path"
            ;;
        shasum)
            _output="$(shasum -a 256 "$_path")" || err "shasum failed for $_path"
            ;;
        *)
            err "unsupported SHA-256 tool: $_tool"
            ;;
    esac
    printf '%s\n' "${_output%%[[:space:]]*}"
}

checksum_for_asset() {
    local _sums="$1" _asset="$2"
    local _line _hash _rest _name _expected="" _count=0

    while IFS= read -r _line || [ -n "$_line" ]; do
        _line="${_line%$'\r'}"
        [ -z "$_line" ] && continue

        _hash="${_line%%[[:space:]]*}"
        _rest="${_line#"$_hash"}"
        while [ -n "$_rest" ]; do
            case "$_rest" in
                ' '*) _rest="${_rest# }" ;;
                $'\t'*) _rest="${_rest#$'\t'}" ;;
                *) break ;;
            esac
        done
        _name="${_rest#\*}"

        if [ "$_name" != "$_asset" ]; then
            continue
        fi

        _count=$((_count + 1))
        if [ "${#_hash}" -ne 64 ]; then
            err "malformed SHA256SUMS entry for ${_asset}: expected 64 hexadecimal characters"
        fi
        case "$_hash" in
            *[!0-9a-f]*)
                err "malformed SHA256SUMS entry for ${_asset}: hash must be lowercase hexadecimal"
                ;;
        esac
        _expected="$_hash"
    done < "$_sums"

    if [ "$_count" -eq 0 ]; then
        err "SHA256SUMS contains no exact entry for ${_asset}"
    fi
    if [ "$_count" -ne 1 ]; then
        err "SHA256SUMS contains duplicate entries for ${_asset}"
    fi

    printf '%s\n' "$_expected"
}

download_and_verify() {
    local _asset="${BIN_NAME}-${VERSION_NUM}-${TARGET}.tar.gz"
    local _base_url="https://github.com/${REPO}/releases/download/${TAG}"
    local _archive="${TMPDIR}/${_asset}"
    local _sums="${TMPDIR}/SHA256SUMS"
    local _sha_tool _expected _actual

    ASSET_URL="${_base_url}/${_asset}"
    CHECKSUM_URL="${_base_url}/SHA256SUMS"

    # Fail before network access when this host cannot verify the downloaded
    # artifact. Integrity is a required control, not a warning-only feature.
    if ! _sha_tool="$(select_sha256_tool)"; then
        return 1
    fi

    info "downloading SHA256SUMS"
    if ! curl -fsSL "$CHECKSUM_URL" -o "$_sums"; then
        err "failed to download required checksum manifest: $CHECKSUM_URL"
    fi
    if ! _expected="$(checksum_for_asset "$_sums" "$_asset")"; then
        return 1
    fi

    info "downloading ${_asset}"
    if ! curl -fsSL --progress-bar "$ASSET_URL" -o "$_archive"; then
        err "download failed: $ASSET_URL

If this version does not have a pre-built binary for your platform, try:
  cargo install perllsp
  # or
  cargo install perllsp --target $TARGET"
    fi

    if ! _actual="$(calculate_sha256 "$_sha_tool" "$_archive")"; then
        return 1
    fi
    if [ "$_expected" != "$_actual" ]; then
        err "checksum mismatch for ${_asset}
  expected: $_expected
  actual:   $_actual
The download may be corrupted. Delete any cached files and retry."
    fi
    info "checksum verified"

    ARCHIVE_PATH="$_archive"
    EXTRACT_DIR="${TMPDIR}/${BIN_NAME}-${VERSION_NUM}-${TARGET}"
}

# ── Archive safety (#8352) ─────────────────────────────────────────────────────
# Inspect the verified tar.gz before any member is written. Extract only the
# topology-required regular files into a new private staging root. Limits and
# membership match policy/standalone-archive-safety.v1.toml; installers embed
# the constants because they must run without a git checkout.

ARCHIVE_SAFETY_POLICY_ID="standalone-archive-safety.v1"
ARCHIVE_SAFETY_MAX_COMPRESSED_BYTES=268435456
ARCHIVE_SAFETY_MAX_UNCOMPRESSED_BYTES=536870912
ARCHIVE_SAFETY_MAX_ENTRY_BYTES=268435456
ARCHIVE_SAFETY_MAX_ENTRIES=32
ARCHIVE_SAFETY_MAX_PATH_BYTES=255
ARCHIVE_SAFETY_MAX_PATH_DEPTH=3

archive_safety_policy_id() {
    printf '%s\n' "$ARCHIVE_SAFETY_POLICY_ID"
}

archive_safety_limit() {
    local name="$1" default="$2" override=""
    case "$name" in
        compressed) override="${PERL_LSP_ARCHIVE_SAFETY_MAX_COMPRESSED_BYTES:-}" ;;
        uncompressed) override="${PERL_LSP_ARCHIVE_SAFETY_MAX_UNCOMPRESSED_BYTES:-}" ;;
        entry) override="${PERL_LSP_ARCHIVE_SAFETY_MAX_ENTRY_BYTES:-}" ;;
        entries) override="${PERL_LSP_ARCHIVE_SAFETY_MAX_ENTRIES:-}" ;;
        *) override="" ;;
    esac
    if [ -n "$override" ]; then
        printf '%s\n' "$override"
    else
        printf '%s\n' "$default"
    fi
}

archive_file_bytes() {
    wc -c < "$1" | tr -d '[:space:]'
}

# First non-empty stderr line, with a known private path redacted and
# length-capped. Empty when the command wrote nothing diagnosable.
archive_command_diagnostic() {
    local errfile="$1"
    local needle="${2:-}"
    local line
    [ -s "$errfile" ] || return 0
    line=$(awk 'NF { sub(/\r$/, ""); print; exit }' "$errfile") || return 0
    [ -n "$line" ] || return 0
    if [ -n "$needle" ]; then
        line=$(printf '%s\n' "$line" | awk -v n="$needle" '{
            while ((i = index($0, n)) > 0) {
                $0 = substr($0, 1, i - 1) "archive" substr($0, i + length(n))
            }
            print
        }')
    fi
    line=$(printf '%s' "$line" | cut -c1-120)
    printf ': %s' "$line"
}

fail_archive_staging() {
    if [ -n "${STAGING_ROOT:-}" ] && [ -d "${STAGING_ROOT:-}" ]; then
        rm -rf "$STAGING_ROOT"
    fi
    STAGING_ROOT=""
    err "$1"
}

# Print a canonical relative member path or return 1. Shared with the
# PowerShell adapter's semantic rules (drive/UNC/parent/aliases/reserved).
normalize_archive_member_path() {
    local raw="$1"
    local inspect part rest depth folded
    local max_path max_depth

    max_path="$ARCHIVE_SAFETY_MAX_PATH_BYTES"
    max_depth="$ARCHIVE_SAFETY_MAX_PATH_DEPTH"

    case "$raw" in
        *$'\n'*|*$'\r'*|*$'\t'*) return 1 ;;
        *'\\'*) return 1 ;;
        *:*) return 1 ;;
        /*) return 1 ;;
        //*) return 1 ;;
        [A-Za-z]:*) return 1 ;;
        "") return 1 ;;
    esac

    if [ "${#raw}" -gt "$max_path" ]; then
        return 1
    fi

    inspect="$raw"
    case "$inspect" in
        */) inspect="${inspect%/}" ;;
    esac
    [ -n "$inspect" ] || return 1

    depth=0
    rest="$inspect"
    while [ -n "$rest" ]; do
        case "$rest" in
            */*)
                part="${rest%%/*}"
                rest="${rest#*/}"
                ;;
            *)
                part="$rest"
                rest=""
                ;;
        esac
        depth=$((depth + 1))
        if [ "$depth" -gt "$max_depth" ]; then
            return 1
        fi
        case "$part" in
            ""|"."|"..") return 1 ;;
            *[!A-Za-z0-9._-]*) return 1 ;;
            *.) return 1 ;;
        esac
        folded=$(printf '%s' "$part" | tr '[:upper:]' '[:lower:]')
        case "$folded" in
            con|prn|aux|nul|com[1-9]|lpt[1-9]|con.*|prn.*|aux.*|nul.*|com[1-9].*|lpt[1-9].*) return 1 ;;
        esac
    done

    printf '%s\n' "$inspect"
}

archive_member_is_executable_mode() {
    local mode="$1"
    case "$mode" in
        ''|*[!0-7]*) return 1 ;;
    esac
    if [ $((8#$mode & 8#111)) -ne 0 ]; then
        return 0
    fi
    return 1
}

# Identify the host tar implementation so staged evidence names the profile it
# was produced under. Inspection no longer depends on this: entry identity and
# type come from the archive headers, not from a tar rendering. Extraction of
# already-accepted canonical names still runs through tar, so the profile stays
# on the receipt as host evidence (#11508).
archive_extractor_profile() {
    local banner
    banner="$(tar --version 2>/dev/null | head -n 1)" || banner=""
    case "$banner" in
        *"GNU tar"*) printf 'gnu\n' ;;
        *bsdtar*|*libarchive*) printf 'libarchive\n' ;;
        *BusyBox*|*busybox*) printf 'busybox\n' ;;
        *) printf 'unknown\n' ;;
    esac
}

# Decode one 512-byte ustar/GNU header block at byte offset $2 of $1.
#
# Prints "<typeflag>\t<octal mode>\t<size>\t<name>" for a header block, or the
# sentinel END (end-of-archive block), BAD (short read, malformed field, or
# header checksum mismatch), or UNSAFE (a name that is empty or carries bytes
# outside printable ASCII).
#
# This is deliberately not `tar -tf` / `tar -tv`. Those are human renderings
# whose semantics differ per implementation: BusyBox tar reports a hardlink
# with a regular-file type char and strips leading `/` and `../` from the name
# it prints, so a link or an absolute-path member reads as an ordinary
# canonical file (#11508). `od -An -v -tu1 -j -N` and awk's `%c` were checked
# byte-identical on GNU and BusyBox; both are POSIX-specified, but the macOS
# one-true-awk leg is not covered by this repository's proof, so the macOS
# host remains an open residual on #11508 rather than an assumed equivalence.
#
# Numeric fields are read as octal only. GNU tar switches `size` to base-256
# past roughly 8 GiB, which this decoder reports as BAD; the uncompressed
# ceiling above rejects such an archive long before the walk, so the gap is
# unreachable rather than handled. Lifting that ceiling would make it live.
read_tar_header() {
    od -An -v -tu1 -j "$2" -N 512 "$1" 2>/dev/null | awk '
function fld(s, l,   i, r, c) {
    r = ""
    for (i = s; i < s + l; i++) {
        c = b[i + 1]
        if (c == 0) break
        r = r sprintf("%c", c)
    }
    return r
}
function octval(s,   i, v, c) {
    gsub(/^[ \t]+|[ \t]+$/, "", s)
    if (s == "") return -1
    v = 0
    for (i = 1; i <= length(s); i++) {
        c = substr(s, i, 1)
        if (c < "0" || c > "7") return -1
        v = v * 8 + (c - "0")
    }
    return v
}
function printable(s,   i, c) {
    for (i = 1; i <= length(s); i++) {
        c = substr(s, i, 1)
        if (c < " " || c > "~") return 0
        # Backslash is refused with the nonprintable bytes because a rejected
        # name is echoed into a diagnostic, and `say` renders with `printf
        # %b`, which expands backslash escapes. A name carrying \033[ would
        # otherwise emit real terminal control sequences and could clear the
        # screen and forge a success line over a failed install. No accepted
        # member may contain a backslash anyway: the path rules below allow
        # only [A-Za-z0-9._-] per component.
        if (c == "\\") return 0
    }
    return 1
}
{ for (i = 1; i <= NF; i++) b[++n] = $i }
END {
    if (n != 512) { print "BAD"; exit }
    for (i = 1; i <= 512; i++) if (b[i] != 0) { nonzero = 1; break }
    if (!nonzero) { print "END"; exit }
    # The stored checksum is computed with its own field read as spaces.
    sum = 0
    for (i = 1; i <= 512; i++) sum += (i >= 149 && i <= 156) ? 32 : b[i]
    stored = octval(fld(148, 8))
    if (stored < 0 || stored != sum) { print "BAD"; exit }
    mode = octval(fld(100, 8))
    size = octval(fld(124, 12))
    if (mode < 0 || size < 0) { print "BAD"; exit }
    name = fld(0, 100)
    # Only POSIX ustar ("ustar\0" + "00") carries a path prefix at 345. GNU
    # format ("ustar  \0") reuses that area for other metadata.
    if (fld(257, 5) == "ustar" && b[263] == 0) {
        prefix = fld(345, 155)
        if (prefix != "") name = prefix "/" name
    }
    if (name == "" || !printable(name)) { print "UNSAFE"; exit }
    t = b[157]
    typ = (t == 0) ? "0" : ((t >= 33 && t <= 126) ? sprintf("%c", t) : "?")
    printf "%s\t%o\t%d\t%s\n", typ, mode, size, name
}'
}

# True when every byte of $1 from byte offset $2 onward is NUL. Used to prove
# nothing follows the end-of-archive marker, so this walk and the host tar
# agree on where the archive stops (#11508).
archive_tail_is_zero_padding() {
    local file="$1" offset="$2" remainder
    remainder="$(tail -c "+$((offset + 1))" "$file" 2>/dev/null | tr -d '\0' | wc -c | tr -d '[:space:]')"
    case "$remainder" in
        ''|*[!0-9]*) return 1 ;;
    esac
    [ "$remainder" -eq 0 ]
}

inspect_standalone_tar_gz() {
    local archive="$1"
    local package="$2"
    local max_compressed max_uncompressed max_entries
    local compressed gzip_uncompressed
    local entry_count entries name mode size type_char normalized basename_member
    local seen_exact seen_folded folded
    local list_dir header offset data_blocks
    local bound_file gzip_err max_plus bound_status bound_bytes

    max_compressed="$(archive_safety_limit compressed "$ARCHIVE_SAFETY_MAX_COMPRESSED_BYTES")"
    max_uncompressed="$(archive_safety_limit uncompressed "$ARCHIVE_SAFETY_MAX_UNCOMPRESSED_BYTES")"
    max_entries="$(archive_safety_limit entries "$ARCHIVE_SAFETY_MAX_ENTRIES")"

    compressed="$(archive_file_bytes "$archive")"
    if [ "$compressed" -gt "$max_compressed" ]; then
        fail_archive_staging "archive compressed size $compressed exceeds policy ceiling $max_compressed"
    fi

    gzip_uncompressed="$(gzip -l "$archive" 2>/dev/null | awk 'NR==2 { print $2 }' | tr -d ',')"
    case "$gzip_uncompressed" in
        ''|*[!0-9]*) gzip_uncompressed="" ;;
    esac
    if [ -n "$gzip_uncompressed" ] && [ "$gzip_uncompressed" -gt "$max_uncompressed" ]; then
        fail_archive_staging "archive uncompressed size $gzip_uncompressed exceeds policy ceiling $max_uncompressed"
    fi

    list_dir="${STAGING_ROOT:-${TMPDIR:-/tmp}}"
    bound_file="${list_dir}/.bounded-tar"
    gzip_err="${list_dir}/.bounded-tar.err"
    max_plus=$((max_uncompressed + 1))
    set +e
    gzip -dc "$archive" 2>"$gzip_err" | head -c "$max_plus" > "$bound_file"
    bound_status=$?
    set -e
    bound_bytes="$(archive_file_bytes "$bound_file")"
    if [ "$bound_bytes" -gt "$max_uncompressed" ]; then
        fail_archive_staging "archive uncompressed size $bound_bytes exceeds policy ceiling $max_uncompressed"
    fi
    if [ "$bound_bytes" -eq 0 ]; then
        fail_archive_staging "malformed release archive: gzip decompress failed$(archive_command_diagnostic "$gzip_err" "$archive")"
    fi
    if [ "$bound_status" -ne 0 ] && [ "$bound_status" -ne 141 ]; then
        fail_archive_staging "malformed release archive: gzip decompress failed$(archive_command_diagnostic "$gzip_err" "$archive")"
    fi
    # Walk the archive headers directly and collect every entry before any
    # membership rule runs. Each step consumes exactly one header block plus
    # that member's data blocks, so the traversal cannot be steered by a tar
    # rendering, and the whole-archive entry ceiling is decided before a single
    # odd member can preempt it with its own diagnostic.
    entries=""
    entry_count=0
    offset=0
    while :; do
        # `|| header=""` keeps a failing reader (a missing `od`, a pipeline
        # error under `set -o pipefail`) from aborting the installer at the
        # assignment, so the empty case below can fail closed with a
        # diagnostic instead of the run dying mid-inspection with none.
        header="$(read_tar_header "$bound_file" "$offset")" || header=""
        case "$header" in
            END)
                # A zero block ends the archive for this walk, but readers
                # disagree about a *lone* one: GNU tar and bsdtar stop, while
                # BusyBox tar skips it and keeps reading headers. Stopping here
                # without checking what follows would let a member hidden after
                # a single zero block be invisible to this classifier yet fully
                # visible to the host `tar` that extracts, and `tar -xO`
                # concatenates every entry matching the requested name — so a
                # second `perllsp` past the marker would land in the staged
                # file. That is the exact classifier/extractor disagreement
                # this rewrite exists to remove, reintroduced at the end of the
                # archive instead of at an entry (#11508).
                #
                # Every real producer zero-pads to EOF, so requiring the
                # remainder to be zero costs nothing and restores agreement.
                if ! archive_tail_is_zero_padding "$bound_file" "$offset"; then
                    fail_archive_staging "archive continues past its end-of-archive marker at offset $offset"
                fi
                break
                ;;
            '')
                # The decoder itself did not run — a missing or failing `od`
                # or `awk`, not a verdict about the archive. Instrument
                # failure is not evidence of safety, so it fails closed with
                # its own diagnostic rather than falling through as an
                # unreadable entry or, worse, an empty one.
                fail_archive_staging "unable to read archive headers at offset $offset: the host od/awk reader produced no output"
                ;;
            BAD)
                fail_archive_staging "malformed release archive: unreadable tar header at offset $offset"
                ;;
            UNSAFE)
                fail_archive_staging "unsafe archive member path: nonportable bytes in the header at offset $offset"
                ;;
        esac

        entry_count=$((entry_count + 1))
        entries="${entries}${header}"$'\n'

        IFS=$'\t' read -r type_char mode size name <<EOF
$header
EOF
        # Regular files, the contiguous variant, and the PAX/GNU extended
        # records all carry data blocks. Links, directories, and specials do
        # not. Extended records are walked over rather than skipped over, so
        # the entry that follows one is still seen and classified.
        #
        # POSIX requires a zero size on the types that carry no data. Trusting
        # the typeflag alone would let a directory declaring a nonzero size
        # swallow the headers that follow it, so this walk and a conformant
        # reader would disagree about which entries the archive holds — the
        # divergence this classifier exists to remove. Refuse instead.
        case "$type_char" in
            0|7|x|g|L|K)
                data_blocks=$(( (size + 511) / 512 ))
                ;;
            S)
                # GNU sparse. Its size field counts only the stored bytes, and
                # a long sparse map continues into further blocks, so the
                # entry's extent cannot be derived from this header alone.
                # Refuse it by name rather than guessing an extent or
                # mislabelling it as a dataless type that declared a size.
                fail_archive_staging "sparse archive entries are not accepted: $name"
                ;;
            *)
                if [ "$size" -ne 0 ]; then
                    fail_archive_staging "archive entry declares data on a type that carries none: $name"
                fi
                data_blocks=0
                ;;
        esac
        offset=$(( offset + 512 + data_blocks * 512 ))

        # A header ceiling independent of the policy ceiling: stop walking a
        # hostile archive rather than reading it to the end to report on it.
        if [ "$entry_count" -gt "$max_entries" ]; then
            break
        fi
    done

    rm -f "$bound_file" "$gzip_err"

    if [ "$entry_count" -gt "$max_entries" ]; then
        fail_archive_staging "archive entry count $entry_count exceeds policy ceiling $max_entries"
    fi

    seen_exact=$'\n'
    seen_folded=$'\n'
    ACCEPTED_MEMBERS=""

    while IFS=$'\t' read -r type_char mode size name; do
        [ -n "$name" ] || continue

        # An extended record's name is a placeholder ("././@PaxHeader"), not a
        # member path, so it is refused on its type before path rules run.
        # 'x'/'g' are PAX extended headers and 'L'/'K' are GNU long name/link
        # records; all four can rewrite the following entry's path, so they
        # fail closed rather than being interpreted.
        case "$type_char" in
            x|g|L|K)
                fail_archive_staging "extended archive headers are not accepted: $type_char record"
                ;;
        esac

        normalized="$(normalize_archive_member_path "$name")" || fail_archive_staging "unsafe archive member path: $name"
        case "$seen_exact" in
            *$'\n'"$normalized"$'\n'*) fail_archive_staging "duplicate archive member: $normalized" ;;
        esac
        seen_exact="${seen_exact}${normalized}"$'\n'
        folded=$(printf '%s' "$normalized" | tr '[:upper:]' '[:lower:]')
        case "$seen_folded" in
            *$'\n'"$folded"$'\n'*) fail_archive_staging "case-fold collision: $normalized" ;;
        esac
        seen_folded="${seen_folded}${folded}"$'\n'

        # POSIX ustar typeflags. '0' and '\0' are regular files, '7' is the
        # contiguous-file variant, '5' is a directory, '1'/'2' are hard and
        # symbolic links, and '3'-'6' are devices, FIFOs, and sockets.
        case "$type_char" in
            5)
                if [ "$normalized" != "$package" ]; then
                    fail_archive_staging "unexpected directory member: $normalized"
                fi
                continue
                ;;
            1|2)
                fail_archive_staging "archive links are not accepted: $normalized"
                ;;
            0|7)
                ;;
            *)
                fail_archive_staging "special archive entry type is not accepted: $normalized"
                ;;
        esac

        case "$normalized" in
            "$package"/*) ;;
            *) fail_archive_staging "member is outside the package directory: $normalized" ;;
        esac

        basename_member="${normalized##*/}"
        if archive_member_is_executable_mode "$mode"; then
            case "$basename_member" in
                perllsp|perl-dap) ;;
                *) fail_archive_staging "unexpected executable member: $normalized" ;;
            esac
        fi
        case "$basename_member" in
            perl-lsp|perl-lsp.exe|perllsp.exe)
                fail_archive_staging "unexpected executable member: $normalized"
                ;;
            perllsp|perl-dap|README.md|LICENSE-APACHE|LICENSE-MIT|SHA256SUMS.txt)
                if [ "$normalized" != "${package}/${basename_member}" ]; then
                    fail_archive_staging "required member has a noncanonical path: $normalized"
                fi
                ;;
            *)
                fail_archive_staging "unexpected archive member: $normalized"
                ;;
        esac

        # The header's declared size is not trusted as the resource bound: a
        # false header would understate it. gzip -l and the capped gzip -dc
        # above bound expansion before the walk, and the named extract is
        # capped with head -c against the remaining uncompressed budget.
        ACCEPTED_MEMBERS="${ACCEPTED_MEMBERS}${normalized}"$'\n'
    done <<EOF
${entries}
EOF

    required_missing=""
    for file_ok in perllsp perl-dap README.md LICENSE-APACHE LICENSE-MIT SHA256SUMS.txt; do
        case "$ACCEPTED_MEMBERS" in
            *"$package/$file_ok"$'\n'*) ;;
            *) required_missing="${required_missing}${file_ok} " ;;
        esac
    done
    if [ -n "$required_missing" ]; then
        fail_archive_staging "missing required member: $required_missing"
    fi
}

emit_archive_safety_receipt() {
    local archive="$1" package="$2" sha_tool="$3"
    local archive_hash member_line member basename_member member_hash
    local receipt extractor

    archive_hash="$(calculate_sha256 "$sha_tool" "$archive")" || fail_archive_staging "unable to digest verified archive"
    # The extractor profile is host evidence, not an input to admission:
    # inspection classified these members from the archive headers, so the
    # receipt records which tar staged them rather than which tar was trusted.
    extractor="$(archive_extractor_profile)"
    receipt="archive_safety_receipt policy=${ARCHIVE_SAFETY_POLICY_ID} layout=posix_nested_v1 extractor=${extractor} archive_sha256=${archive_hash} members="
    member_line=""
    for member in perllsp perl-dap README.md LICENSE-APACHE LICENSE-MIT SHA256SUMS.txt; do
        member_hash="$(calculate_sha256 "$sha_tool" "${STAGING_ROOT}/${package}/${member}")" || fail_archive_staging "unable to digest staged member $member"
        if [ -n "$member_line" ]; then
            member_line="${member_line},"
        fi
        member_line="${member_line}${member}:${member_hash}"
    done
    receipt="${receipt}${member_line}"
    case "$receipt" in
        */tmp/*|*/var/*|*"$TMPDIR"*) fail_archive_staging "archive safety receipt contained a private path" ;;
    esac
    info "$receipt"
}

extract_archive() {
    local package member dest actual_total sz sha_tool leftover
    local max_uncompressed max_entry cap remain extract_err extract_status

    info "inspecting release archive"
    [ -n "${ARCHIVE_PATH:-}" ] || err "archive path is not set"
    [ -n "${TMPDIR:-}" ] || err "staging parent is not set"
    [ -f "$ARCHIVE_PATH" ] || err "verified archive is missing"

    package="${BIN_NAME}-${VERSION_NUM}-${TARGET}"
    max_uncompressed="$(archive_safety_limit uncompressed "$ARCHIVE_SAFETY_MAX_UNCOMPRESSED_BYTES")"
    max_entry="$(archive_safety_limit entry "$ARCHIVE_SAFETY_MAX_ENTRY_BYTES")"

    STAGING_ROOT="$(mktemp -d "${TMPDIR}/perl-lsp-stage.XXXXXX")" || err "unable to create private staging root"
    inspect_standalone_tar_gz "$ARCHIVE_PATH" "$package"

    mkdir -p "${STAGING_ROOT}/${package}"
    extract_err="${STAGING_ROOT}/.extract.err"
    actual_total=0
    while IFS= read -r member; do
        [ -n "$member" ] || continue
        dest="${STAGING_ROOT}/${member}"
        remain=$((max_uncompressed - actual_total))
        cap="$max_entry"
        if [ "$remain" -lt "$cap" ]; then
            cap="$remain"
        fi
        if [ "$cap" -le 0 ]; then
            fail_archive_staging "archive uncompressed size $actual_total exceeds policy ceiling $max_uncompressed"
        fi
        mkdir -p "$(dirname "$dest")"
        set +e
        tar -xOzf "$ARCHIVE_PATH" -- "$member" 2>"$extract_err" | head -c $((cap + 1)) > "$dest"
        extract_status=$?
        set -e
        if [ -L "$dest" ] || [ ! -f "$dest" ]; then
            fail_archive_staging "staged member is not a regular file: $member"
        fi
        sz="$(archive_file_bytes "$dest")"
        if [ "$sz" -gt "$max_entry" ]; then
            fail_archive_staging "archive entry size $sz exceeds policy ceiling $max_entry"
        fi
        if [ "$sz" -gt "$cap" ]; then
            fail_archive_staging "archive uncompressed size $((actual_total + sz)) exceeds policy ceiling $max_uncompressed"
        fi
        if [ "$extract_status" -ne 0 ] && [ "$extract_status" -ne 141 ]; then
            fail_archive_staging "failed to extract accepted member $member$(archive_command_diagnostic "$extract_err" "$ARCHIVE_PATH")"
        fi
        actual_total=$((actual_total + sz))
        if [ "$actual_total" -gt "$max_uncompressed" ]; then
            fail_archive_staging "archive uncompressed size $actual_total exceeds policy ceiling $max_uncompressed"
        fi
    done <<EOF
${ACCEPTED_MEMBERS}
EOF
    rm -f "$extract_err"

    leftover="$(find "$STAGING_ROOT" \( -type l -o -type b -o -type c -o -type p -o -type s \) 2>/dev/null || true)"
    if [ -n "$leftover" ]; then
        fail_archive_staging "staging contained a non-regular entry after extract"
    fi

    EXTRACT_DIR="${STAGING_ROOT}/${package}"
    if [ ! -f "${EXTRACT_DIR}/${BIN_NAME}" ] || [ ! -f "${EXTRACT_DIR}/${DAP_BIN_NAME}" ]; then
        fail_archive_staging "expected directory not found after extraction: ${package}
The release archive may have an unexpected layout."
    fi

    chmod 0755 "${EXTRACT_DIR}/${BIN_NAME}" "${EXTRACT_DIR}/${DAP_BIN_NAME}"
    chmod 0644 "${EXTRACT_DIR}/README.md" "${EXTRACT_DIR}/LICENSE-APACHE" \
        "${EXTRACT_DIR}/LICENSE-MIT" "${EXTRACT_DIR}/SHA256SUMS.txt"

    sha_tool="$(select_sha256_tool)" || fail_archive_staging "SHA-256 tool is required to bind staged member identities"
    emit_archive_safety_receipt "$ARCHIVE_PATH" "$package" "$sha_tool"
    info "staged accepted topology members"
}

# ── Source build ───────────────────────────────────────────────────────────────

# #8367: validate an explicit source-mode VERSION before the value reaches
# cargo's argv. A VERSION like `--target` must never be able to pose as a
# cargo flag, and a two-component or non-numeric spec must be rejected with a
# typed reason instead of a confusing cargo resolution error. Semver core is
# X.Y.Z with numeric components; an optional prerelease/build suffix
# (-alpha.1, +build.7) is accepted with its restricted alphabet.
validate_source_version_spec() {
    local _spec="$1"
    case "$_spec" in
        *[!0-9A-Za-z.+_-]*)
            err "invalid VERSION=$_spec for source mode: expected 'latest' or a semver like v0.12.0 (allowed characters: digits, letters, '.', '+', '-', '_')"
            ;;
    esac
    local _core="$_spec"
    case "$_spec" in
        *-*) _core="${_spec%%-*}" ;;
        *+*) _core="${_spec%%+*}" ;;
    esac
    local _major="${_core%%.*}"
    local _rest="${_core#*.}"
    local _minor="${_rest%%.*}"
    local _patch="${_rest#*.}"
    if [ "$_minor" = "$_rest" ]; then
        err "invalid VERSION=$_spec for source mode: expected a full X.Y.Z semver (v0.12.0), got fewer than three numeric components"
    fi
    case "$_major" in ""|*[!0-9]*) err "invalid VERSION=$_spec for source mode: major version component must be numeric" ;; esac
    case "$_minor" in ""|*[!0-9]*) err "invalid VERSION=$_spec for source mode: minor version component must be numeric" ;; esac
    case "$_patch" in ""|*[!0-9]*) err "invalid VERSION=$_spec for source mode: patch version component must be numeric" ;; esac
}

# #8367: bind the staged binary to the requested registry subject before any
# promotion can observe EXTRACT_DIR. For an exact request the binary must
# report exactly that version; for explicit-latest the binary must report some
# parseable version and it is surfaced as the resolved registry subject. The
# version token is extracted as a whole whitespace-separated field so a
# requested 0.12.0 can never be satisfied by a binary reporting 10.12.03.
verify_source_install_identity() {
    local _bin="${TMPDIR}/install-root/bin/${BIN_NAME}"
    if [ ! -f "$_bin" ]; then
        err "source build reported success but staged no ${BIN_NAME} binary at ${TMPDIR}/install-root/bin"
    fi
    local _resolved _resolved_ver
    if ! _resolved="$("$_bin" --version 2>/dev/null)"; then
        err "source build staged ${BIN_NAME}, but it failed to report a version (--version exited non-zero)"
    fi
    _resolved_ver="$(printf '%s' "$_resolved" | tr ' \t' '\n' \
        | grep -E '^[0-9]+(\.[0-9]+){1,3}([-+][0-9A-Za-z.-]+)?$' | tail -n 1 || true)"
    if [ -z "$_resolved_ver" ]; then
        err "source build staged ${BIN_NAME}, but its version output '${_resolved}' contains no parseable version token"
    fi
    case "$VERSION" in
        latest|"")
            info "resolved registry subject: ${BIN_NAME} ${_resolved_ver}"
            ;;
        *)
            if [ "$_resolved_ver" != "$VERSION_NUM" ]; then
                err "source build identity mismatch: requested ${BIN_NAME} ${VERSION_NUM} but the staged binary reports ${_resolved_ver}. Refusing to promote a different registry subject."
            fi
            info "staged ${BIN_NAME} identity verified: ${_resolved_ver}"
            ;;
    esac
}

build_from_source() {
    need_cmd cargo

    # Toolchain guard (#12593): the source build parses edition-2024 manifests;
    # refuse a stale non-rustup cargo before any build work. The prebuilt
    # download path above does not need cargo, so the guard lives here. In the
    # standalone remote bootstrap (the root install.sh runs this file without
    # its scripts/ siblings) the library cannot be sourced, so an inline
    # floor check refuses the same confusing pre-1.85 failures instead of
    # silently skipping the guard; from 1.85 up to the workspace rust-version,
    # cargo's own rust-version enforcement reports the requirement cleanly.
    _guard_lib="$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh"
    if [ -f "$_guard_lib" ]; then
        # shellcheck source=lib/cargo-toolchain-guard.sh
        . "$_guard_lib" && cargo_toolchain_guard
    else
        _guard_version="$(cargo --version 2>/dev/null | awk '{for (i=1; i<=NF; i++) if ($i ~ /^[0-9]+\.[0-9]+(\.[0-9]+)?/) {print $i; exit}}')"
        _guard_version="${_guard_version%%-*}"
        _guard_major="${_guard_version%%.*}"
        _guard_minor="${_guard_version#*.}"
        _guard_minor="${_guard_minor%%.*}"
        if [ -z "$_guard_major" ] || [ "$_guard_major" -lt 1 ] || { [ "$_guard_major" -eq 1 ] && [ "$_guard_minor" -lt 85 ]; }; then
            err "cargo-toolchain-guard: REFUSED: cargo ${_guard_version:-unknown} predates edition-2024 support (needs >= 1.85; this workspace pins rustc 1.95). A stale cargo reports this as 'feature edition2024 is required' -- a toolchain problem, not a manifest problem. Install a current toolchain (https://rustup.rs)."
        fi
    fi
    unset _guard_lib _guard_version _guard_major _guard_minor

    local _target_arg=()
    local _version_arg=()

    if [ -n "${TARGET:-}" ]; then
        need_cmd rustup
        info "building from source for target: $TARGET"
        rustup target add "$TARGET"
        _target_arg=(--target "$TARGET")
    else
        info "building from source for host target"
    fi

    # #8367: an explicit VERSION request must resolve that exact registry
    # subject or fail; only an explicit `latest` request may float. Earlier
    # this path ran a bare `cargo install perllsp`, so a user pinning
    # VERSION=v0.12.0 with BUILD_FROM_SOURCE=1 silently got the latest
    # crates.io subject instead.
    case "$VERSION" in
        latest|"")
            info "source mode: no version pin requested; the registry's latest perllsp will be resolved and its identity reported"
            ;;
        v[0-9]*|[0-9]*)
            validate_source_version_spec "$VERSION_NUM"
            _version_arg=(--version "$VERSION_NUM")
            info "building from source for the exact registry subject: perllsp $VERSION_NUM"
            ;;
        *)
            err "invalid VERSION='$VERSION' for source mode: expected 'latest' or a semver like v0.12.0"
            ;;
    esac

    # `${_target_arg[@]+"${_target_arg[@]}"}` — a bare `"${_target_arg[@]}"`
    # would abort under `set -u` on bash < 4.4 (macOS /bin/bash 3.2) whenever
    # TARGET is unset, which is the default host-target source build.
    if ! cargo install perllsp --locked \
        ${_version_arg[@]+"${_version_arg[@]}"} \
        ${_target_arg[@]+"${_target_arg[@]}"} \
        --root "$TMPDIR/install-root"; then
        if [ "${#_version_arg[@]}" -gt 0 ]; then
            err "cargo could not build/install perllsp $VERSION_NUM from the registry (see the cargo output above: the exact requested version may not exist on crates.io, or its lockfile is incompatible). No existing installation was modified."
        fi
        err "cargo failed to build/install perllsp from source (see the cargo output above). No existing installation was modified."
    fi

    verify_source_install_identity

    EXTRACT_DIR="${TMPDIR}/install-root/bin"
}

# ── Product-unit promotion ─────────────────────────────────────────────────────
# Readers of PATH-visible names and of .perl-lsp/current observe one complete
# unit. Current is a symlink replaced by rename(2); PATH names are stable
# relative links into current, so they cannot be updated member-by-member.

product_store_dir() {
    printf '%s\n' "${INSTALL_DIR}/.perl-lsp"
}

maybe_inject_install_fault() {
    local _barrier="$1" _pid
    if [ "${PERL_LSP_INSTALL_FAULT:-}" = "signal_${_barrier}" ]; then
        # Test-only: deliver TERM so the selector-window handlers run.
        # Use BASHPID in this shell. Do not capture it from a helper via $(),
        # which would be a different process. Bash 3.2 has no BASHPID; $$
        # there is the parent harness, so ask a child for PPID instead.
        if [ -n "${BASHPID:-}" ]; then
            _pid="$BASHPID"
        else
            _pid="$(sh -c 'echo $PPID')"
        fi
        kill -s TERM "$_pid"
        return 0
    fi
    if [ "${PERL_LSP_INSTALL_FAULT:-}" = "$_barrier" ]; then
        err "injected product-unit fault: $_barrier"
    fi
}

maybe_observe_product_unit() {
    local _barrier="$1"
    if [ "${PERL_LSP_INSTALL_OBSERVE:-}" != "$_barrier" ]; then
        return 0
    fi
    if [ -z "${PERL_LSP_INSTALL_OBSERVE_FILE:-}" ]; then
        err "PERL_LSP_INSTALL_OBSERVE_FILE is required for observation barrier $_barrier"
    fi
    # Keep the first hit. Post-commit selector repair must not overwrite the
    # pre-commit between_path_members snapshot.
    if [ -e "$PERL_LSP_INSTALL_OBSERVE_FILE" ]; then
        return 0
    fi
    {
        observe_current_product_unit
        observe_path_visible_product_unit
    } > "$PERL_LSP_INSTALL_OBSERVE_FILE"
    if grep -q 'state=mixed' "$PERL_LSP_INSTALL_OBSERVE_FILE"; then
        err "path-visible product unit became mixed at $_barrier"
    fi
}

hash_product_member() {
    local _tool
    _tool="$(select_sha256_tool)" || err "SHA-256 tool is required to bind product-unit identity"
    calculate_sha256 "$_tool" "$1"
}

product_unit_candidate_id() {
    local _disposition="$1" _server_hash="$2" _dap_hash="$3" _tmp _tool _id
    _tmp="$(mktemp)"
    {
        printf '%s\0' "perl-lsp-swarm:standalone-product-unit.v1"
        printf '%s\0' "$_disposition"
        printf '%s\0' "$_server_hash"
        printf '%s\0' "$_dap_hash"
    } > "$_tmp"
    _tool="$(select_sha256_tool)" || err "SHA-256 tool is required to bind product-unit identity"
    _id="$(calculate_sha256 "$_tool" "$_tmp")" || return
    rm -f "$_tmp"
    printf '%s\n' "$_id"
}

write_product_unit_manifest() {
    local _dir="$1" _disposition="$2" _id="$3" _server_hash="$4" _dap_hash="$5"
    cat > "${_dir}/product_unit.v1" <<EOF
schema=standalone_product_unit.v1
disposition=${_disposition}
candidate_id=${_id}
server_sha256=${_server_hash}
dap_sha256=${_dap_hash}
EOF
}

classify_staged_product_unit() {
    local _src="$1" _mode="$2"
    local _server="${_src}/${BIN_NAME}"
    local _dap="${_src}/${DAP_BIN_NAME}"
    if [ ! -f "$_server" ] || [ -L "$_server" ]; then
        err "staged product unit is missing a regular perllsp member"
    fi
    if [ "$_mode" = "source" ]; then
        printf '%s\n' "advanced_source_server_only"
        return 0
    fi
    if [ ! -f "$_dap" ] || [ -L "$_dap" ]; then
        err "archive product unit requires a complete perllsp/perl-dap pair"
    fi
    printf '%s\n' "archive_pair_required"
}

atomic_symlink_replace() {
    local _link="$1" _target="$2"
    local _tmp="${_link}.tmp.$$"
    rm -f "$_tmp"
    ln -s "$_target" "$_tmp"
    # GNU mv follows a symlink-to-directory destination unless -T is given, which
    # would move the new pointer into the old candidate instead of replacing it.
    if mv -T "$_tmp" "$_link" 2>/dev/null; then
        return 0
    fi
    # BSD/macOS mv has no -T. rename(2) replaces a symlink without following it
    # and without an unlink gap. Perl is present on the supported POSIX hosts.
    if command -v perl >/dev/null 2>&1 \
        && perl -e 'rename($ARGV[0], $ARGV[1]) or exit 1' -- "$_tmp" "$_link"
    then
        return 0
    fi
    rm -f "$_tmp"
    err "atomic current-pointer replace requires GNU mv -T or perl rename"
}

publish_immutable_candidate() {
    local _src="$1" _disposition="$2" _allow_fault="${3:-1}"
    local _store _server_src _server_hash _dap_src _dap_hash="-" _id _dest _attempt _existing_dap
    _store="$(product_store_dir)"
    _server_src="${_src}/${BIN_NAME}"
    _server_hash="$(hash_product_member "$_server_src")" || return
    _dap_src="${_src}/${DAP_BIN_NAME}"
    if [ "$_disposition" = "archive_pair_required" ]; then
        _dap_hash="$(hash_product_member "$_dap_src")" || return
    fi
    _id="$(product_unit_candidate_id "$_disposition" "$_server_hash" "$_dap_hash")" || return
    _dest="${_store}/candidates/${_id}"
    mkdir -p "${_store}/candidates" "${_store}/attempts"
    if [ -d "$_dest" ]; then
        if [ "$(hash_product_member "${_dest}/${BIN_NAME}")" != "$_server_hash" ]; then
            err "immutable candidate already exists with different perllsp bytes"
        fi
        if [ "$_disposition" = "archive_pair_required" ]; then
            _existing_dap="$(hash_product_member "${_dest}/${DAP_BIN_NAME}")" || return
            if [ "$_existing_dap" != "$_dap_hash" ]; then
                err "immutable candidate already exists with different perl-dap bytes"
            fi
        fi
        printf '%s\n' "$_id"
        return 0
    fi
    if [ "$_allow_fault" = "1" ]; then
        maybe_inject_install_fault "before_publish"
    fi
    _attempt="$(mktemp -d "${_store}/attempts/att.XXXXXX")"
    cp "$_server_src" "${_attempt}/${BIN_NAME}"
    chmod 755 "${_attempt}/${BIN_NAME}"
    if [ "$_disposition" = "archive_pair_required" ]; then
        cp "$_dap_src" "${_attempt}/${DAP_BIN_NAME}"
        chmod 755 "${_attempt}/${DAP_BIN_NAME}"
    fi
    write_product_unit_manifest "$_attempt" "$_disposition" "$_id" "$_server_hash" "$_dap_hash"
    mv "$_attempt" "$_dest"
    printf '%s\n' "$_id"
}

commit_current_selection() {
    local _id="$1" _allow_fault="${2:-1}"
    local _store _current _old
    _store="$(product_store_dir)"
    _current="${_store}/current"
    if [ "$_allow_fault" = "1" ]; then
        maybe_inject_install_fault "before_commit"
    fi
    if [ -L "$_current" ]; then
        _old="$(readlink "$_current")"
        atomic_symlink_replace "${_store}/previous" "$_old"
    fi
    atomic_symlink_replace "$_current" "candidates/${_id}"
    if [ "$_allow_fault" = "1" ]; then
        maybe_inject_install_fault "after_commit"
    fi
}

ensure_path_visible_selectors() {
    local _allow_fault="${1:-1}"
    local _incoming_pair="${2:-0}"
    local _existing_only="${3:-0}"
    local _rel=".perl-lsp/current"
    local _store _want_dap=0
    _store="$(product_store_dir)"
    if [ "$_allow_fault" = "1" ]; then
        maybe_inject_install_fault "before_selectors"
    fi
    if [ "$_existing_only" != "1" ] || [ -e "${INSTALL_DIR}/${BIN_NAME}" ] || [ -L "${INSTALL_DIR}/${BIN_NAME}" ]; then
        atomic_symlink_replace "${INSTALL_DIR}/${BIN_NAME}" "${_rel}/${BIN_NAME}"
    fi
    maybe_observe_product_unit "between_path_members"
    if [ "$_incoming_pair" = "1" ]; then
        # The staged incoming unit carries a DAP, so the DAP selector is
        # written here, before current switches: the commit then publishes the
        # whole pair to PATH at once instead of relying on a post-commit
        # selector repair that a failure could leave unfinished. The selector
        # target is relative and retargets atomically at the commit.
        _want_dap=1
    elif [ -f "${_store}/current/${DAP_BIN_NAME}" ]; then
        _want_dap=1
    elif [ ! -e "${_store}/current" ] && [ -n "${EXTRACT_DIR:-}" ] && [ -f "${EXTRACT_DIR}/${DAP_BIN_NAME}" ]; then
        _want_dap=1
    fi
    if [ "$_want_dap" = "1" ] && { [ "$_existing_only" != "1" ] || [ -e "${INSTALL_DIR}/${DAP_BIN_NAME}" ] || [ -L "${INSTALL_DIR}/${DAP_BIN_NAME}" ]; }; then
        atomic_symlink_replace "${INSTALL_DIR}/${DAP_BIN_NAME}" "${_rel}/${DAP_BIN_NAME}"
    elif [ "$_existing_only" != "1" ] && { [ -L "${INSTALL_DIR}/${DAP_BIN_NAME}" ] || [ -f "${INSTALL_DIR}/${DAP_BIN_NAME}" ]; }; then
        # A pre-existing regular file here is stale selector residue from an
        # earlier install layout; leaving it would keep PATH pointed at an
        # adapter unrelated to the selected server unit.
        rm -f "${INSTALL_DIR}/${DAP_BIN_NAME}"
    elif [ -d "${INSTALL_DIR}/${DAP_BIN_NAME}" ]; then
        err "stale PATH selector is a directory: ${INSTALL_DIR}/${DAP_BIN_NAME}"
    fi
}

path_visible_member_hash() {
    local _path="$1"
    if [ ! -e "$_path" ]; then
        printf '%s\n' "-"
        return 0
    fi
    hash_product_member "$_path"
}

observe_current_product_unit() {
    local _store _current _id _manifest _disposition="unknown" _server="-" _dap="-"
    _store="$(product_store_dir)"
    _current="${_store}/current"
    if [ ! -L "$_current" ]; then
        printf 'state=none\n'
        return 0
    fi
    _id="$(readlink "$_current")"
    _id="${_id##*/}"
    _manifest="${_current}/product_unit.v1"
    if [ -f "$_manifest" ]; then
        _disposition="$(awk -F= '/^disposition=/ {print $2; exit}' "$_manifest")"
    fi
    if [ -f "${_current}/${BIN_NAME}" ]; then
        _server="$(hash_product_member "${_current}/${BIN_NAME}")"
    fi
    if [ -f "${_current}/${DAP_BIN_NAME}" ]; then
        _dap="$(hash_product_member "${_current}/${DAP_BIN_NAME}")"
    fi
    printf 'state=selected disposition=%s candidate_id=%s server_sha256=%s dap_sha256=%s\n' \
        "$_disposition" "$_id" "$_server" "$_dap"
}

observe_path_visible_product_unit() {
    local _server _dap _server_link _dap_link _server_dir="" _dap_dir=""
    local _cur _cur_server="-" _cur_dap="-"
    _server="$(path_visible_member_hash "${INSTALL_DIR}/${BIN_NAME}")"
    _dap="$(path_visible_member_hash "${INSTALL_DIR}/${DAP_BIN_NAME}")"
    if [ -L "${INSTALL_DIR}/${BIN_NAME}" ]; then
        _server_link="$(readlink "${INSTALL_DIR}/${BIN_NAME}")"
        _server_dir="$(dirname "$_server_link")"
    fi
    if [ -L "${INSTALL_DIR}/${DAP_BIN_NAME}" ]; then
        _dap_link="$(readlink "${INSTALL_DIR}/${DAP_BIN_NAME}")"
        _dap_dir="$(dirname "$_dap_link")"
    fi
    if [ -n "$_dap_dir" ] && [ -n "$_server_dir" ] && [ "$_server_dir" != "$_dap_dir" ]; then
        printf 'state=mixed server_sha256=%s dap_sha256=%s\n' "$_server" "$_dap"
        return 0
    fi
    _cur="$(observe_current_product_unit)"
    _cur_server="$(printf '%s\n' "$_cur" | sed -n 's/.*server_sha256=\([^ ]*\).*/\1/p')"
    _cur_dap="$(printf '%s\n' "$_cur" | sed -n 's/.*dap_sha256=\([^ ]*\).*/\1/p')"
    if [ "$_cur_server" != "-" ] && [ "$_cur_dap" != "-" ]; then
        if { [ "$_server" != "-" ] && [ "$_dap" = "-" ]; } \
            || { [ "$_server" = "-" ] && [ "$_dap" != "-" ]; }; then
            printf 'state=mixed server_sha256=%s dap_sha256=%s\n' "$_server" "$_dap"
            return 0
        fi
    fi
    if [ "$_server" != "-" ] && [ "$_dap" != "-" ]; then
        if { [ "$_server" = "$_cur_server" ] && [ "$_dap" != "$_cur_dap" ]; } \
            || { [ "$_server" != "$_cur_server" ] && [ "$_dap" = "$_cur_dap" ]; }; then
            printf 'state=mixed server_sha256=%s dap_sha256=%s\n' "$_server" "$_dap"
            return 0
        fi
    fi
    printf 'state=path_visible server_sha256=%s dap_sha256=%s\n' "$_server" "$_dap"
}

legacy_regular_product_dir() {
    local _tmp
    if [ -L "${INSTALL_DIR}/${BIN_NAME}" ] || [ ! -f "${INSTALL_DIR}/${BIN_NAME}" ]; then
        return 1
    fi
    _tmp="$(mktemp -d)"
    cp "${INSTALL_DIR}/${BIN_NAME}" "${_tmp}/${BIN_NAME}"
    if [ -f "${INSTALL_DIR}/${DAP_BIN_NAME}" ] && [ ! -L "${INSTALL_DIR}/${DAP_BIN_NAME}" ]; then
        cp "${INSTALL_DIR}/${DAP_BIN_NAME}" "${_tmp}/${DAP_BIN_NAME}"
        printf '%s %s\n' "$_tmp" "archive_pair_required"
    else
        printf '%s %s\n' "$_tmp" "historical_server_only"
    fi
}

promote_legacy_layout_if_needed() {
    local _legacy _disposition _id _tmp
    _legacy="$(legacy_regular_product_dir)" || return 0
    _tmp="${_legacy%% *}"
    _disposition="${_legacy#* }"
    _id="$(publish_immutable_candidate "$_tmp" "$_disposition" 0)" || return
    rm -rf "$_tmp"
    commit_current_selection "$_id" 0 || return
    ensure_path_visible_selectors 0 || return
}

# ── Install ────────────────────────────────────────────────────────────────────

install_binaries() {
    local _mode="${1:-${INSTALL_MODE:-release}}"
    local _disposition _id _store _previous="none" _receipt _server_hash _dap_hash="-" _incoming_pair

    mkdir -p "$INSTALL_DIR"

    if [ ! -w "$INSTALL_DIR" ]; then
        err "install directory is not writable: $INSTALL_DIR
Try one of:
  sudo INSTALL_DIR=$INSTALL_DIR bash scripts/install.sh
  INSTALL_DIR=\$HOME/.local/bin bash scripts/install.sh"
    fi

    _disposition="$(classify_staged_product_unit "$EXTRACT_DIR" "$_mode")" || return
    _store="$(product_store_dir)"
    mkdir -p "$_store"

    promote_legacy_layout_if_needed || return

    _id="$(publish_immutable_candidate "$EXTRACT_DIR" "$_disposition")" || return
    _incoming_pair=0
    if [ "$_disposition" = "archive_pair_required" ]; then
        _incoming_pair=1
    fi
    _plsp_rollback_server=""
    _plsp_rollback_dap=""
    if [ ! -e "${INSTALL_DIR}/${BIN_NAME}" ] && [ ! -L "${INSTALL_DIR}/${BIN_NAME}" ]; then
        _plsp_rollback_server="${INSTALL_DIR}/${BIN_NAME}"
    fi
    if [ ! -e "${INSTALL_DIR}/${DAP_BIN_NAME}" ] && [ ! -L "${INSTALL_DIR}/${DAP_BIN_NAME}" ]; then
        _plsp_rollback_dap="${INSTALL_DIR}/${DAP_BIN_NAME}"
    fi
    # Pair selectors are created before current flips so the commit publishes
    # both PATH names together. If commit fails closed, drop names this attempt
    # created so a first install does not leave dangling commands. err() and
    # INT/TERM/HUP run the same rollback so injected faults and signals do not
    # replace main's TMPDIR EXIT trap. After current names this candidate,
    # rollback becomes a no-op so a deferred signal cannot unpublish PATH.
    _plsp_incoming_id="$_id"
    arm_new_path_selector_signal_rollback
    if ! ensure_path_visible_selectors 1 "$_incoming_pair" 0; then
        rollback_new_path_selectors
        disarm_new_path_selector_signal_rollback
        _plsp_incoming_id=""
        return 1
    fi
    if ! commit_current_selection "$_id"; then
        rollback_new_path_selectors
        disarm_new_path_selector_signal_rollback
        _plsp_incoming_id=""
        return 1
    fi
    _plsp_rollback_server=""
    _plsp_rollback_dap=""
    _plsp_incoming_id=""
    disarm_new_path_selector_signal_rollback
    ensure_path_visible_selectors || return

    if [ -L "${_store}/previous" ]; then
        _previous="$(readlink "${_store}/previous")"
        _previous="${_previous##*/}"
    fi
    _server_hash="$(hash_product_member "${_store}/current/${BIN_NAME}")" || return
    if [ -f "${_store}/current/${DAP_BIN_NAME}" ]; then
        _dap_hash="$(hash_product_member "${_store}/current/${DAP_BIN_NAME}")" || return
    fi
    _receipt="product_unit_receipt disposition=${_disposition} candidate_id=${_id} previous=${_previous} server_sha256=${_server_hash} dap_sha256=${_dap_hash} state=selected"
    case "$_receipt" in
        *"${INSTALL_DIR}"*|*"${EXTRACT_DIR}"*)
            err "product-unit receipt contained a private path"
            ;;
    esac
    info "$_receipt"
    info "installed: $INSTALL_DIR/$BIN_NAME"
    if [ "$_dap_hash" != "-" ]; then
        info "installed: $INSTALL_DIR/$DAP_BIN_NAME"
    fi
}

# ── Post-install checks ────────────────────────────────────────────────────────

verify_install() {
    local _bin="$INSTALL_DIR/$BIN_NAME"
    if [ ! -x "$_bin" ]; then
        err "installed binary not executable: $_bin"
    fi

    local _got_version
    if _got_version="$("$_bin" --version 2>&1)"; then
        info "verified: $_got_version"
    else
        warn "could not run '$BIN_NAME --version'; the binary may require a restart to load shared libraries"
    fi
}

check_path() {
    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*)
            info "$INSTALL_DIR is in PATH"
            ;;
        *)
            warn "$INSTALL_DIR is not in PATH"
            say ""
            say "Add it by appending one of these lines to your shell's startup file:"
            say ""
            say "  bash/zsh:  export PATH=\"\$PATH:$INSTALL_DIR\""
            say "  fish:      fish_add_path $INSTALL_DIR"
            say ""
            say "Then reload your shell, or run:  export PATH=\"\$PATH:$INSTALL_DIR\""
            ;;
    esac
}

configure_claude() {
    if [ "$WITH_CLAUDE" != "1" ]; then
        return 0
    fi

    # User-scoped Claude reconciliation must not run as root. Elevated installs
    # (`sudo INSTALL_DIR=...`) remain supported for the binary stage only.
    if [ "$(id -u)" -eq 0 ]; then
        CLAUDE_SETUP_RESULT="skipped_elevated"
        warn "skipping Claude reconciliation under elevated privileges; install kept the binary and the invoking user should rerun '$BIN_NAME setup claude' without sudo"
        return 2
    fi

    local _bin="$INSTALL_DIR/$BIN_NAME"
    local _status=0
    info "reconciling Claude Code through '$BIN_NAME setup claude'"

    # Invoke the exact verified binary by absolute path because this installer process may
    # still have a stale PATH. This is composition only and MUST NOT be interpreted as
    # fresh-process PATH proof; #7832/#7746 own that separate receipt.
    "$_bin" setup claude || _status=$?

    case "$_status" in
        0)
            CLAUDE_SETUP_RESULT="complete"
            info "Claude integration reconciled"
            ;;
        2)
            CLAUDE_SETUP_RESULT="action_required"
            warn "perllsp installed successfully, but Claude integration still requires an action; the binary has been preserved"
            ;;
        *)
            CLAUDE_SETUP_RESULT="failed"
            warn "perllsp installed successfully, but Claude reconciliation failed; rerun '$BIN_NAME setup claude' after resolving the reported problem"
            ;;
    esac

    return "$_status"
}

# ── Main ───────────────────────────────────────────────────────────────────────

main() {
    if [ "$PRINT_TARGET" != "1" ]; then
        say ""
        say "Perl LSP installer"
        say "=================="
        say ""
    fi

    detect_platform

    if [ "$PRINT_TARGET" = "1" ]; then
        if [ "$INSTALL_MODE" = "release" ]; then
            say "$TARGET"
        elif [ -n "${TARGET:-}" ]; then
            say "source:$TARGET"
        else
            say "source"
        fi
        exit 0
    fi

    need_cmd curl
    need_cmd tar

    resolve_version
    TMPDIR="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$TMPDIR'" EXIT

    if [ "$INSTALL_MODE" = "release" ]; then
        # Archive inspection classifies entries from the ustar headers rather
        # than from a tar listing, so the release path needs `od` as well as
        # `tar` (#11508). A source build never inspects an archive, so the
        # requirement stays inside this branch instead of gating both modes.
        need_cmd od
        download_and_verify
        extract_archive
    else
        build_from_source
    fi
    install_binaries
    verify_install
    check_path

    local _combined_exit=0
    configure_claude || _combined_exit=$?

    say ""
    say "Done. ${BIN_NAME} ${VERSION_NUM} installed to ${INSTALL_DIR}/${BIN_NAME}"
    if [ "$WITH_CLAUDE" = "1" ]; then
        say "Claude integration: ${CLAUDE_SETUP_RESULT}"
    fi
    say ""
    say "Get started:"
    say "  VS Code:  install the Perl LSP extension from the marketplace"
    say "  Vim/Neovim: add perllsp to your LSP config"
    say "  Other:    configure to use '${INSTALL_DIR}/${BIN_NAME} --stdio'"
    if [ "$WITH_CLAUDE" = "1" ] && [ "$CLAUDE_SETUP_RESULT" != "complete" ]; then
        say "  Claude:   rerun '${BIN_NAME} setup claude' after the reported action is resolved"
    fi
    say ""
    say "Docs: https://github.com/$REPO"
    say ""

    return "$_combined_exit"
}

# Internal proof seam: tests may source the functions without performing an
# install. Ordinary execution remains unchanged.
if [ "${PERL_LSP_INSTALLER_LIBRARY_ONLY:-0}" != "1" ]; then
    main "$@"
fi
