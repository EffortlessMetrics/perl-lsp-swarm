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
err()     { say "${RED}error:${NC} $1" >&2; exit 1; }

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

# ── Extract ────────────────────────────────────────────────────────────────────

extract_archive() {
    info "extracting archive"
    tar -xzf "$ARCHIVE_PATH" -C "$TMPDIR"

    if [ ! -d "$EXTRACT_DIR" ]; then
        err "expected directory not found after extraction: $EXTRACT_DIR
The release archive may have an unexpected layout."
    fi
}

# ── Source build ───────────────────────────────────────────────────────────────

build_from_source() {
    need_cmd cargo

    local _target_arg=()

    if [ -n "${TARGET:-}" ]; then
        need_cmd rustup
        info "building from source for target: $TARGET"
        rustup target add "$TARGET"
        _target_arg=(--target "$TARGET")
    else
        info "building from source for host target"
    fi

    # `${_target_arg[@]+"${_target_arg[@]}"}` — a bare `"${_target_arg[@]}"`
    # would abort under `set -u` on bash < 4.4 (macOS /bin/bash 3.2) whenever
    # TARGET is unset, which is the default host-target source build.
    cargo install perllsp --locked ${_target_arg[@]+"${_target_arg[@]}"} \
        --root "$TMPDIR/install-root"

    EXTRACT_DIR="${TMPDIR}/install-root/bin"
}

# ── Install ────────────────────────────────────────────────────────────────────

install_binaries() {
    local _src_bin="${EXTRACT_DIR}/${BIN_NAME}"
    if [ ! -f "$_src_bin" ]; then
        err "binary not found in archive: $_src_bin"
    fi

    mkdir -p "$INSTALL_DIR"

    # Verify we can write to the install directory.
    if [ ! -w "$INSTALL_DIR" ]; then
        err "install directory is not writable: $INSTALL_DIR
Try one of:
  sudo INSTALL_DIR=$INSTALL_DIR bash scripts/install.sh
  INSTALL_DIR=\$HOME/.local/bin bash scripts/install.sh"
    fi

    info "installing $BIN_NAME to $INSTALL_DIR"
    cp "$_src_bin" "$INSTALL_DIR/$BIN_NAME"
    chmod 755 "$INSTALL_DIR/$BIN_NAME"
    info "installed: $INSTALL_DIR/$BIN_NAME"

    # Install perl-dap companion binary if present (ships since v0.9.1).
    local _src_dap="${EXTRACT_DIR}/${DAP_BIN_NAME}"
    if [ -f "$_src_dap" ]; then
        info "installing $DAP_BIN_NAME to $INSTALL_DIR"
        cp "$_src_dap" "$INSTALL_DIR/$DAP_BIN_NAME"
        chmod 755 "$INSTALL_DIR/$DAP_BIN_NAME"
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
