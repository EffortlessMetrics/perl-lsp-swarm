#!/usr/bin/env bash
# scripts/lib/cargo-local-config.sh
#
# Per-checkout machine settings live in `.cargo/config.local.toml` (gitignored).
# Cargo does not discover that filename through hierarchical lookup. Workspace
# `.cargo/config.toml` optional-includes the same file for direct `cargo`
# invocations; repository wrappers call this helper to pass `--config` when the
# file exists.

cargo_local_config_prepare() {
  local root="${1:?worktree root required}"
  CARGO_LOCAL_CONFIG_ARGS=()
  local local_config="${root}/.cargo/config.local.toml"
  if [[ -f "$local_config" ]]; then
    CARGO_LOCAL_CONFIG_ARGS=(--config "$local_config")
  fi
}
