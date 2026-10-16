#!/usr/bin/env bash
# Inactive owning validator adapter; central import must establish custody first.
set -euo pipefail
[[ -n "${EM_CI_ARTIFACT_DIR:-}" && -n "${RUNNER_TEMP:-}" ]]
receipt="$EM_CI_ARTIFACT_DIR/rust-small-proof.json"
[[ -f "$receipt" && ! -L "$receipt" ]]

# This command validates the canonical retained receipt; it runs no product
# steps. Cargo/build scripts must not inherit the policy's metadata credential.
(
    unset GH_TOKEN GITHUB_TOKEN GH_ENTERPRISE_TOKEN GITHUB_ENTERPRISE_TOKEN
    export RUSTUP_TOOLCHAIN=1.95.0 RUSTUP_AUTO_INSTALL=0
    export CARGO_TARGET_DIR="$RUNNER_TEMP/perl-receipt-validator-target"
    export CARGO_BUILD_JOBS=2
    # Guard the exact sanitized environment used by the verifier, including its
    # version probe. Do not expose policy credentials to either Cargo invocation.
    script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
    # shellcheck source=../lib/cargo-toolchain-guard.sh
    . "$script_dir/../lib/cargo-toolchain-guard.sh"
    cargo_toolchain_guard
    cargo run -p xtask --locked -- rust-small-proof --verify-receipt "$receipt"
)

# A coherent failure receipt can pass the Rust shape verifier. The completed
# policy still requires actual product success, rather than diagnostic validity.
python3 -I - "$receipt" <<'PY'
import json
import pathlib
import sys
receipt = json.loads(pathlib.Path(sys.argv[1]).read_bytes())
if receipt.get('result') != 'success':
    raise SystemExit('canonical receipt is diagnostic, not successful proof')
PY
