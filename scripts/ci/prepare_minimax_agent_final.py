#!/usr/bin/env python3
"""Prepare the reviewed final gh-aw canary authority for one bootstrap run.

This file is deliberately temporary. The bootstrap workflow executes it, uploads
its outputs as inert Git blobs, and the final commit deletes both this generator
and the write-capable bootstrap workflow.
"""

from __future__ import annotations

from pathlib import Path

ROOT = Path(".")
SCANNER_PATH = ROOT / "scripts/ci/workflow_security_ratchet.py"
TESTS_PATH = ROOT / "scripts/ci/test_workflow_security_ratchet.py"
LEDGER_PATH = ROOT / ".ci/policies/action-pin-provenance.toml"
PERMANENT_WORKFLOW_PATH = ROOT / ".github/workflows/minimax-agent-compile-check.yml"
TEMPORARY_WORKFLOW_PATH = ROOT / ".github/workflows/compile-minimax-agent-candidate.yml"
THIS_PATH = ROOT / "scripts/ci/prepare_minimax_agent_final.py"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one anchor, found {count}")
    return text.replace(old, new, 1)


def patch_scanner() -> None:
    scanner = SCANNER_PATH.read_text(encoding="utf-8")
    scanner = replace_once(
        scanner,
        'SECRET_RE = re.compile(r"\\$\\{\\{\\s*secrets\\.")\nCARGO_INSTALL_RE =',
        'SECRET_RE = re.compile(r"\\$\\{\\{\\s*secrets\\.")\n'
        'GH_AW_PUSH_TOKEN = "${{ secrets.GH_AW_GITHUB_TOKEN || secrets.GITHUB_TOKEN }}"\n'
        'GH_AW_CONFIGURE_GIT_COMMAND = '
        'r\'bash "${RUNNER_TEMP}/gh-aw/actions/configure_git_credentials.sh"\'\n'
        'CARGO_INSTALL_RE =',
        "gh-aw constants",
    )

    helper = r'''

def _gh_aw_headers(lines: Sequence[str]) -> tuple[dict[str, object], dict[str, object]] | None:
    metadata: dict[str, object] | None = None
    manifest: dict[str, object] | None = None
    for line in lines[:16]:
        try:
            if line.startswith("# gh-aw-metadata:"):
                candidate = json.loads(line.split(":", 1)[1].strip())
                if isinstance(candidate, dict):
                    metadata = candidate
            elif line.startswith("# gh-aw-manifest:"):
                candidate = json.loads(line.split(":", 1)[1].strip())
                if isinstance(candidate, dict):
                    manifest = candidate
        except json.JSONDecodeError:
            return None
    if metadata is None or manifest is None:
        return None
    return metadata, manifest


def _containing_job(
    lines: Sequence[str], index: int
) -> tuple[str, int, int] | None:
    jobs_start: int | None = None
    for cursor in range(index, -1, -1):
        parsed = _parse_key_line(lines[cursor])
        if parsed and parsed.indent == 0:
            if parsed.key == "jobs":
                jobs_start = cursor
            break
    if jobs_start is None:
        return None

    job_name: str | None = None
    job_start: int | None = None
    for cursor in range(jobs_start + 1, index + 1):
        parsed = _parse_key_line(lines[cursor])
        if parsed and parsed.indent == 0:
            return None
        if parsed and parsed.indent == 2 and not parsed.list_item:
            job_name = parsed.key
            job_start = cursor
    if job_name is None or job_start is None:
        return None

    job_end = len(lines)
    for cursor in range(job_start + 1, len(lines)):
        parsed = _parse_key_line(lines[cursor])
        if parsed and (
            parsed.indent == 0
            or (parsed.indent == 2 and not parsed.list_item)
        ):
            job_end = cursor
            break
    if not (job_start <= index < job_end):
        return None
    return job_name, job_start, job_end


def _job_permissions(
    lines: Sequence[str], start: int, end: int
) -> dict[str, str] | None:
    for cursor in range(start + 1, end):
        parsed = _parse_key_line(lines[cursor])
        if not parsed or parsed.indent != 4 or parsed.key != "permissions":
            continue
        if parsed.value:
            return {"__scalar__": _strip_scalar(parsed.value)}
        permissions: dict[str, str] = {}
        for child_index in range(cursor + 1, end):
            child = _parse_key_line(lines[child_index])
            if child and child.indent <= 4:
                break
            if child and child.indent == 6 and child.key in PERMISSION_KEYS:
                permissions[child.key] = _strip_scalar(child.value)
        return permissions
    return None


def _step_bounds(
    lines: Sequence[str], index: int, limit: int
) -> tuple[int, int, int] | None:
    parsed = _parse_key_line(lines[index])
    if parsed is None:
        return None
    step_indent = parsed.indent
    step_start = index
    if not parsed.list_item:
        for cursor in range(index - 1, -1, -1):
            candidate = _parse_key_line(lines[cursor])
            if candidate and candidate.indent < step_indent:
                return None
            if candidate and candidate.list_item and candidate.indent == step_indent:
                step_start = cursor
                break
        else:
            return None
    step_end = limit
    for cursor in range(step_start + 1, limit):
        candidate = _parse_key_line(lines[cursor])
        if candidate and (
            candidate.indent < step_indent
            or (candidate.list_item and candidate.indent == step_indent)
        ):
            step_end = cursor
            break
    return step_start, step_end, step_indent


def _step_values(lines: Sequence[str], start: int, end: int) -> dict[str, str]:
    values: dict[str, str] = {}
    for cursor in range(start, end):
        parsed = _parse_key_line(lines[cursor])
        if parsed:
            values[parsed.key] = _strip_scalar(parsed.value)
    return values


def _is_gh_aw_safe_outputs_checkout(
    lines: Sequence[str], relative: str, use_index: int, action: str
) -> bool:
    if not relative.endswith(".lock.yml"):
        return False
    headers = _gh_aw_headers(lines)
    if headers is None:
        return False
    metadata, manifest = headers
    if metadata.get("schema_version") != "v4" or metadata.get("strict") is not True:
        return False
    if manifest.get("version") != 1:
        return False

    external = EXTERNAL_ACTION_RE.fullmatch(action)
    actions = manifest.get("actions")
    if external is None or not isinstance(actions, list):
        return False
    checkout_sha = external.group(2)
    checkout_manifested = False
    setup_manifested = False
    for item in actions:
        if not isinstance(item, dict):
            continue
        repo = item.get("repo")
        sha = item.get("sha")
        if repo == "actions/checkout" and sha == checkout_sha:
            checkout_manifested = True
        if (
            repo == "github/gh-aw-actions/setup"
            and isinstance(sha, str)
            and FULL_SHA_RE.fullmatch(sha)
        ):
            setup_manifested = True
    if not checkout_manifested or not setup_manifested:
        return False

    bounds = _containing_job(lines, use_index)
    if bounds is None:
        return False
    job_name, job_start, job_end = bounds
    if job_name != "safe_outputs":
        return False
    if _job_permissions(lines, job_start, job_end) != {
        "contents": "write",
        "issues": "write",
        "pull-requests": "write",
    }:
        return False
    job_lines = {line.strip() for line in lines[job_start:job_end]}
    if not {"- activation", "- agent", "- detection"}.issubset(job_lines):
        return False
    job_gate = any(
        parsed
        and parsed.indent == 4
        and parsed.key == "if"
        and "needs.agent.result != 'skipped'" in parsed.value
        and "needs.detection.result == 'success'" in parsed.value
        for parsed in (
            _parse_key_line(line) for line in lines[job_start:job_end]
        )
    )
    if not job_gate:
        return False

    checkout_bounds = _step_bounds(lines, use_index, job_end)
    if checkout_bounds is None:
        return False
    checkout_start, checkout_end, step_indent = checkout_bounds
    checkout = _step_values(lines, checkout_start, checkout_end)
    if checkout.get("persist-credentials") != "true":
        return False
    if checkout.get("token") != GH_AW_PUSH_TOKEN:
        return False
    checkout_gate = checkout.get("if", "")
    if (
        "needs.agent.result != 'skipped'" not in checkout_gate
        or "create_pull_request" not in checkout_gate
    ):
        return False

    if checkout_end >= job_end:
        return False
    next_parsed = _parse_key_line(lines[checkout_end])
    if not next_parsed or not next_parsed.list_item or next_parsed.indent != step_indent:
        return False
    configure_bounds = _step_bounds(lines, checkout_end, job_end)
    if configure_bounds is None:
        return False
    configure_start, configure_end, _ = configure_bounds
    configure = _step_values(lines, configure_start, configure_end)
    return (
        configure.get("name") == "Configure Git credentials"
        and configure.get("if") == checkout_gate
        and configure.get("GIT_TOKEN") == GH_AW_PUSH_TOKEN
        and configure.get("run") == GH_AW_CONFIGURE_GIT_COMMAND
    )
'''
    scanner = replace_once(
        scanner,
        "    return True\n\n\ndef _cargo_install_pin_surface",
        "    return True\n" + helper + "\n\ndef _cargo_install_pin_surface",
        "gh-aw safe-output helper",
    )
    scanner = replace_once(
        scanner,
        '    value = parsed.value.strip()\n    return value.startswith(("*", "&", "{"))',
        '    value = parsed.value.strip()\n'
        '    if parsed.key == "permissions" and value == "{}":\n'
        "        return False\n"
        '    return value.startswith(("*", "&", "{"))',
        "explicit deny-all permissions",
    )
    scanner = replace_once(
        scanner,
        "                    and _checkout_persists(lines, index, parsed.indent)\n"
        "                ):",
        "                    and _checkout_persists(lines, index, parsed.indent)\n"
        "                    and not _is_gh_aw_safe_outputs_checkout(\n"
        "                        lines, relative, index, action\n"
        "                    )\n"
        "                ):",
        "checkout exception call",
    )
    SCANNER_PATH.write_text(scanner, encoding="utf-8")


def patch_tests() -> None:
    tests = TESTS_PATH.read_text(encoding="utf-8")
    fixture_helper = r'''

    def gh_aw_safe_outputs_workflow(
        self,
        *,
        detection_gate: bool = True,
        push_token: str | None = None,
    ) -> str:
        checkout_sha = "a" * 40
        setup_sha = "b" * 40
        token = push_token or ratchet.GH_AW_PUSH_TOKEN
        metadata = json.dumps(
            {"schema_version": "v4", "strict": True},
            separators=(",", ":"),
        )
        manifest = json.dumps(
            {
                "version": 1,
                "actions": [
                    {"repo": "actions/checkout", "sha": checkout_sha},
                    {"repo": "github/gh-aw-actions/setup", "sha": setup_sha},
                ],
            },
            separators=(",", ":"),
        )
        condition = "needs.agent.result != 'skipped'"
        if detection_gate:
            condition += " && needs.detection.result == 'success'"
        return (
            f"# gh-aw-metadata: {metadata}\n"
            f"# gh-aw-manifest: {manifest}\n"
            "name: generated\n"
            "on: [workflow_dispatch]\n"
            "permissions: {}\n"
            "jobs:\n"
            "  safe_outputs:\n"
            "    needs:\n"
            "      - activation\n"
            "      - agent\n"
            "      - detection\n"
            f"    if: {condition}\n"
            "    permissions:\n"
            "      contents: write\n"
            "      issues: write\n"
            "      pull-requests: write\n"
            "    runs-on: ubuntu-latest\n"
            "    steps:\n"
            "      - name: Checkout repository\n"
            f"        if: {condition} && contains(needs.agent.outputs.output_types, 'create_pull_request')\n"
            f"        uses: actions/checkout@{checkout_sha}\n"
            "        with:\n"
            "          persist-credentials: true\n"
            f"          token: {token}\n"
            "      - name: Configure Git credentials\n"
            f"        if: {condition} && contains(needs.agent.outputs.output_types, 'create_pull_request')\n"
            "        env:\n"
            f"          GIT_TOKEN: {token}\n"
            f"        run: {ratchet.GH_AW_CONFIGURE_GIT_COMMAND}\n"
        )
'''
    tests = replace_once(
        tests,
        "    def test_detects_mutable_external_action_in_composite_action",
        fixture_helper + "\n    def test_detects_mutable_external_action_in_composite_action",
        "test fixture helper",
    )
    policy_tests = r'''

    def test_explicit_empty_permissions_mapping_is_clean(self) -> None:
        self.write(
            ".github/workflows/deny-all.yml",
            "name: deny all\non: [workflow_dispatch]\npermissions: {}\njobs: {}\n",
        )
        self.assertNotIn("unsupported_security_yaml_indirection", self.rules())

    def test_nonempty_permissions_flow_map_remains_rejected(self) -> None:
        self.write(
            ".github/workflows/flow-permissions.yml",
            "name: flow\non: [workflow_dispatch]\npermissions: {contents: write}\njobs: {}\n",
        )
        self.assertIn("unsupported_security_yaml_indirection", self.rules())

    def test_accepts_strict_gh_aw_safe_output_publisher_checkout(self) -> None:
        self.write(
            ".github/workflows/generated.lock.yml",
            self.gh_aw_safe_outputs_workflow(),
        )
        rules = self.rules()
        self.assertNotIn("unsupported_security_yaml_indirection", rules)
        self.assertNotIn("checkout_persists_credentials_on_write_surface", rules)

    def test_gh_aw_publisher_checkout_requires_detection_gate(self) -> None:
        self.write(
            ".github/workflows/generated.lock.yml",
            self.gh_aw_safe_outputs_workflow(detection_gate=False),
        )
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_gh_aw_publisher_checkout_requires_canonical_push_token(self) -> None:
        self.write(
            ".github/workflows/generated.lock.yml",
            self.gh_aw_safe_outputs_workflow(
                push_token="$" + "{{ secrets.OTHER_TOKEN }}"
            ),
        )
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())
'''
    tests = replace_once(
        tests,
        "    def test_detects_floating_cargo_install_in_list_form_run",
        policy_tests + "\n    def test_detects_floating_cargo_install_in_list_form_run",
        "policy regression tests",
    )
    TESTS_PATH.write_text(tests, encoding="utf-8")


def write_compile_check() -> None:
    PERMANENT_WORKFLOW_PATH.write_text(
        '''name: MiniMax Agent Compile Check

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
''',
        encoding="utf-8",
    )


def update_pin_ledger() -> None:
    ledger = LEDGER_PATH.read_text(encoding="utf-8")
    marker = (
        "action = 'github/gh-aw-actions/setup'\n"
        "sha = '5e508589e03a7757a7e05b26e834292f5445bfb6'"
    )
    if marker not in ledger:
        ledger += '''

# Verified against upstream on 2026-09-16 via the GitHub refs API:
#   refs/tags/v7.0.0 -> lightweight tag on commit 820762786026740c76f36085b0efc47a31fe5020
[[pin]]
action = 'actions/setup-node'
sha = '820762786026740c76f36085b0efc47a31fe5020'
kind = 'release_tag'
value = 'v7.0.0'

# Verified against upstream on 2026-09-16 via the GitHub refs API:
#   refs/tags/v9.0.0 -> annotated tag d746ffe35508b1917358783b479e04febd2b8f71
#   -> commit 3a2844b7e9c422d3c10d287c895573f7108da1b3
[[pin]]
action = 'actions/github-script'
sha = '3a2844b7e9c422d3c10d287c895573f7108da1b3'
kind = 'release_tag'
value = 'v9.0.0'

# Verified against upstream on 2026-09-16 via the GitHub refs API:
#   refs/tags/v0.88.7 -> annotated tag 0c4566ff6a133b81c2b1ecd6559623e1313a1927
#   -> commit 5e508589e03a7757a7e05b26e834292f5445bfb6
[[pin]]
action = 'github/gh-aw-actions/setup'
sha = '5e508589e03a7757a7e05b26e834292f5445bfb6'
kind = 'release_tag'
value = 'v0.88.7'
'''
    LEDGER_PATH.write_text(ledger, encoding="utf-8")


def main() -> None:
    patch_scanner()
    patch_tests()
    write_compile_check()
    update_pin_ledger()
    TEMPORARY_WORKFLOW_PATH.unlink()
    THIS_PATH.unlink()


if __name__ == "__main__":
    main()
