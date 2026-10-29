#!/usr/bin/env bash
set -euo pipefail

# Git hooks export repository and command-config overrides. Keep the canonical
# proof's Git subprocesses bound to this checkout and its normal hygiene policy.
git_local_env="$(git rev-parse --local-env-vars)"
readarray -t git_local_env_names <<< "$git_local_env"
unset "${git_local_env_names[@]}"
if [[ -n "${EM_CI_LANE:-}" && "$EM_CI_LANE" != perl-rust-small ]]; then
  echo '::error::Rust Small received an unrelated artifact lane' >&2
  exit 64
fi

# One canonical proof entry point for every governed Rust Small route (#17018).
workspace="${GITHUB_WORKSPACE:-$PWD}"
if ! git config --global --get-all safe.directory | grep -Fx -- "$workspace" > /dev/null; then
  git config --global --add safe.directory "$workspace"
fi
cargo fmt --all -- --check
# Keep the original product status when retaining a genuine failure receipt.
set +e
cargo run -p perl-rust-small-verifier --bin perl-rust-small-proof --locked
product_status=$?
set -e
if [[ "${EM_CI_LANE:-}" == perl-rust-small ]]; then
  if ! python3 -I scripts/ci/export_rust_small_receipt.py; then
    if (( product_status == 0 )); then
      echo '::error::successful Rust Small proof lacks required canonical custody' >&2
      exit 1
    fi
    echo '::warning::failure receipt unavailable; original product failure retained' >&2
  fi
fi
exit "$product_status"
