#!/usr/bin/env bash
# Wrapper for the #7832 Windows fresh-process PATH oracle self-test.
# The discriminating cases live in scripts/windows_fresh_path_oracle.py --self-test
# so the same proof runs on Linux CI and Windows developer hosts.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORACLE="$ROOT/scripts/windows_fresh_path_oracle.py"

[[ -f "$ORACLE" ]] || {
    echo "missing windows_fresh_path_oracle.py" >&2
    exit 1
}

if command -v python3 >/dev/null 2>&1; then
    PYTHON=(python3)
elif command -v python >/dev/null 2>&1; then
    PYTHON=(python)
else
    echo "python3/python is required for the fresh-process PATH oracle" >&2
    exit 1
fi

exec "${PYTHON[@]}" "$ORACLE" --self-test
