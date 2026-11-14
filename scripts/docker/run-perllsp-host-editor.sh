#!/usr/bin/env bash
# Host-editor same-path Docker launcher for perllsp (#8987).
#
# Requires an absolute workspace path matching the editor rootUri namespace.
# Native Windows shells are unsupported; use WSL or Dev Containers instead.

set -euo pipefail

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  cat <<'EOF'
Usage: run-perllsp-host-editor.sh [perllsp-args...]

Runs the published perllsp image with same-path bind mounts so host file://
URIs resolve inside the container at identical absolute paths.

Environment:
  PERLLSP_IMAGE   Container image (default from policy/docker-workspace-path.v1.toml)
  WORKSPACE_ROOT  Absolute workspace path (default: absolute path of current directory)

Example:
  WORKSPACE_ROOT="$PWD" PERLLSP_IMAGE=ghcr.io/effortlessmetrics/perl-lsp-perl:0.18.0 \
    ./scripts/docker/run-perllsp-host-editor.sh
EOF
  exit 0
fi

WORKSPACE_ROOT="${WORKSPACE_ROOT:-$PWD}"
if [[ "$WORKSPACE_ROOT" != /* ]]; then
  echo "run-perllsp-host-editor: WORKSPACE_ROOT must be absolute: $WORKSPACE_ROOT" >&2
  exit 2
fi

IMAGE="${PERLLSP_IMAGE:-ghcr.io/effortlessmetrics/perl-lsp-perl:latest}"
exec docker run --rm -i --init \
  --mount "type=bind,src=${WORKSPACE_ROOT},dst=${WORKSPACE_ROOT},readonly" \
  --workdir "${WORKSPACE_ROOT}" \
  "${IMAGE}" \
  --stdio \
  "$@"
