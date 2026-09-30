#!/usr/bin/env bash
set -euo pipefail

# Preparatory entry point for #17018. Active Rust Small routes retain their
# existing wiring until the reusable workflow exposes safe failover evidence.
workspace="${GITHUB_WORKSPACE:-$PWD}"
if ! git config --global --get-all safe.directory | grep -Fx -- "$workspace" > /dev/null; then
  git config --global --add safe.directory "$workspace"
fi
cargo fmt --all -- --check
cargo run -p xtask --locked -- rust-small-proof
