#!/usr/bin/env bash
set -euo pipefail

# This is a fixture for the declarative GitHub path filter. It does not claim
# to simulate GitHub event delivery. It proves that the workflow keeps the
# control-plane allowlist explicit and excludes ordinary product sources.

workflow='.github/workflows/agent-flow-control-plane.yml'

test -f "$workflow"

required_paths=(
  'AGENTS.md'
  'CLAUDE.md'
  'docs/agents/**'
  'docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md'
  'docs/specs/README.md'
  'docs/INDEX.md'
  '.agents/skills/**'
  '.claude/skills/**'
  '.claude/settings.json'
  '.codex/**'
  'xtask/src/main.rs'
  'xtask/src/tasks/agent_flow.rs'
  'xtask/src/tasks/mod.rs'
  'xtask/tests/agent_merge_review_backstop.rs'
  'xtask/tests/pr_convergence_contract.rs'
  'scripts/tests/test-agent-flow-control-plane-trigger.sh'
  '.github/workflows/agent-flow-control-plane.yml'
)

for path in "${required_paths[@]}"; do
  grep -Fq "      - '$path'" "$workflow"
done

# Negative controls: an ordinary product edit is not in the trigger contract.
if grep -Fq "      - 'crates/**'" "$workflow" || grep -Fq "      - '**'" "$workflow"; then
  echo 'control-plane workflow must not include a broad product glob' >&2
  exit 1
fi

if grep -Fq "      - 'crates/perl-parser" "$workflow"; then
  echo 'ordinary Rust product paths must not trigger agent-flow compilation' >&2
  exit 1
fi

echo 'agent-flow control-plane trigger fixtures passed'
