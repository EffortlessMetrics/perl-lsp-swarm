#!/usr/bin/env python3
"""Materialize the permanent MiniMax compile check and its exact ratchet baseline."""

from __future__ import annotations

import json
from pathlib import Path

WORKFLOW_PATH = Path(".github/workflows/minimax-agent-compile-check.yml")
BASELINE_PATH = Path(".ci/workflow-security-baseline.json")

PERMANENT_WORKFLOW = '''name: MiniMax Agent Compile Check

on:
  pull_request:
    branches: [main]
    types: [opened, synchronize, reopened]
    paths:
      - '.github/workflows/minimax-coding-agent.md'
      - '.github/workflows/minimax-coding-agent.lock.yml'
      - '.github/workflows/minimax-agent-compile-check.yml'
      - '.github/aw/actions-lock.json'
      - '.gitattributes'
      - '.github/dependabot.yml'
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: minimax-agent-compile-check-${{ github.event.pull_request.number || github.run_id }}
  cancel-in-progress: false

jobs:
  compile-check:
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    steps:
      - name: Checkout candidate
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          fetch-depth: 0
          persist-credentials: false

      - name: Install checksum-pinned gh-aw compiler
        shell: bash
        run: |
          set -euo pipefail
          compiler="${RUNNER_TEMP:?}/gh-aw-v0.88.7"
          curl --fail --silent --show-error --location --retry 3 --retry-all-errors \
            --connect-timeout 15 --max-time 180 \
            https://github.com/github/gh-aw/releases/download/v0.88.7/linux-amd64 \
            --output "$compiler"
          printf '%s  %s\n' \
            37faaaa95f622b910568bc878452f6036f01e951380fdfc41441944a95da43bf \
            "$compiler" | sha256sum --check --status
          chmod 0755 "$compiler"
          "$compiler" --version
          echo "GH_AW_COMPILER=$compiler" >> "$GITHUB_ENV"

      - name: Verify generated authority is current
        shell: bash
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          set -euo pipefail
          "$GH_AW_COMPILER" compile minimax-coding-agent \
            --strict --verbose --approve --no-check-update
          git diff --exit-code -- \
            .github/workflows/minimax-coding-agent.lock.yml \
            .github/aw/actions-lock.json \
            .gitattributes \
            .github/dependabot.yml
'''


def main() -> None:
    WORKFLOW_PATH.write_text(PERMANENT_WORKFLOW, encoding="utf-8")

    with BASELINE_PATH.open(encoding="utf-8") as handle:
        baseline = json.load(handle)
    findings = baseline.get("findings", baseline.get("allowed_findings"))
    if not isinstance(findings, list):
        raise SystemExit("baseline findings are missing")

    target = {
        "path": ".github/workflows/minimax-agent-compile-check.yml",
        "rule": "pr_write_permission",
        "evidence": "contents: write",
    }
    matching_indexes = [
        index
        for index, item in enumerate(findings)
        if all(item.get(key) == value for key, value in target.items())
    ]
    if len(matching_indexes) != 1:
        raise SystemExit(
            f"expected one temporary compile-check finding, found {len(matching_indexes)}"
        )
    del findings[matching_indexes[0]]
    if len(findings) != 87:
        raise SystemExit(f"expected 87 retained findings, found {len(findings)}")

    BASELINE_PATH.write_text(
        json.dumps(baseline, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
