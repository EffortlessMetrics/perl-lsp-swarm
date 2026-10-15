#!/usr/bin/env bash
# Repository policy for the pinned, isolated central result_script interface.
set -euo pipefail
[[ "${EM_CI_SELECTED_PROOF_RESULT:-}" == success ]]
for tool in python3 git uv jq base64; do
  command -v "$tool" || { echo "::error::required policy tool unavailable: $tool" >&2; exit 78; }
done
[[ "${EM_CI_LANE:-}" == perl-rust-small ]]
[[ -f "${EM_CI_ARTIFACT_DIR:?}/rust-small-proof.json" ]]
# The pinned central job already authenticated/imported these bytes and ran the
# actual Rust owner before this policy; do not start another cold verification.
[[ "$(printf ZW0tY2k= | base64 --decode)" == em-ci ]]
jq -en 'true' >/dev/null
# The immutable runner contains uv and Python. Isolate the pinned test dependency
# from the checkout and from any persistent runner Python environment.
policy_env=$(mktemp -d "${RUNNER_TEMP:?}/rust-result-python.XXXXXX")
trap 'rm -rf -- "$policy_env"' EXIT
export UV_CACHE_DIR="$policy_env/cache"
export UV_NO_CONFIG=1
export UV_PYTHON_DOWNLOADS=never
uv venv --python python3 "$policy_env/venv"
uv pip install --python "$policy_env/venv/bin/python" --only-binary=:all: --no-deps 'PyYAML==6.0.2'
export PATH="$policy_env/venv/bin:$PATH"
python3 -m unittest \
  scripts/ci/test_rustfmt_check.py \
  scripts/ci/test_rustfmt_required_workflow.py \
  scripts/ci/test_rust_small_route_contract.py \
  scripts/ci/test_rust_small_evidence.py \
  scripts/ci/test_export_rust_small_receipt.py \
  scripts/ci/test_rust_small_packet_wiring.py \
  scripts/ci/test_hosted_formatter_producers.py \
  scripts/ci/test_rust_small_probe_gate.py \
  scripts/ci/test_main_red_refusal.py \
  scripts/ci/test_rust_standard_result.py
python3 scripts/ci/rust_standard_result.py
