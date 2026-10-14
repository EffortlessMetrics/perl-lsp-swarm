#!/usr/bin/env bash
# Inactive owning validator adapter; central import must establish custody first.
set -euo pipefail
[[ -n "${EM_CI_ARTIFACT_DIR:-}" && -n "${RUNNER_TEMP:-}" ]]
receipt="$EM_CI_ARTIFACT_DIR/rust-small-proof.json"
[[ -f "$receipt" && ! -L "$receipt" ]]

# This command validates the canonical retained receipt; it runs no product
# steps. Cargo/build scripts must not inherit the policy's metadata credential.
env -u GH_TOKEN -u GITHUB_TOKEN -u GH_ENTERPRISE_TOKEN -u GITHUB_ENTERPRISE_TOKEN \
    RUSTUP_TOOLCHAIN=1.95.0 RUSTUP_AUTO_INSTALL=0 \
    CARGO_TARGET_DIR="$RUNNER_TEMP/perl-receipt-validator-target" \
    CARGO_BUILD_JOBS=2 \
    cargo run -p xtask --locked -- rust-small-proof --verify-receipt "$receipt"

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
