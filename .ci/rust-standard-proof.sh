#!/usr/bin/env bash
set -euo pipefail

git config --global --add safe.directory "${GITHUB_WORKSPACE:-$PWD}" || true
cargo fmt --all -- --check
cargo run -p xtask --locked -- rust-small-proof
