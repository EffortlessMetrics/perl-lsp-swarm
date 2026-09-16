#!/usr/bin/env python3
"""Temporarily harden the gh-aw publisher ratchet and regenerate its baseline."""

from __future__ import annotations

from pathlib import Path

ROOT = Path(".")
SCANNER_PATH = ROOT / "scripts/ci/workflow_security_ratchet.py"
TESTS_PATH = ROOT / "scripts/ci/test_workflow_security_ratchet.py"
THIS_PATH = ROOT / "scripts/ci/harden_gh_aw_ratchet.py"

LOCK_PATH = ".github/workflows/minimax-coding-agent.lock.yml"
LOCK_DIGEST = "e2a2042de450de733abd8b07853fceb5af0167920fde149042c146da185932f1"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one anchor, found {count}")
    return text.replace(old, new, 1)


def patch_scanner() -> None:
    scanner = SCANNER_PATH.read_text(encoding="utf-8")
    if "GH_AW_LOCK_FILE_DIGESTS" not in scanner:
        scanner = replace_once(
            scanner,
            "GH_AW_CONFIGURE_GIT_COMMAND = "
            "r'bash \\\"${RUNNER_TEMP}/gh-aw/actions/configure_git_credentials.sh\\\"'\n",
            "GH_AW_CONFIGURE_GIT_COMMAND = "
            "r'bash \\\"${RUNNER_TEMP}/gh-aw/actions/configure_git_credentials.sh\\\"'\n"
            "# Normalized-LF digest of the complete reviewed strict compiler output.\n"
            "# The persisted-credential exception applies only while the entire lock file\n"
            "# remains byte-for-byte equivalent after splitlines() normalization.\n"
            "GH_AW_LOCK_FILE_DIGESTS = {\n"
            f'    "{LOCK_PATH}": "{LOCK_DIGEST}",\n'
            "}\n",
            "exact lock digest",
        )

    start = scanner.index("def _gh_aw_headers(")
    end = scanner.index("def _cargo_install_pin_surface", start)
    replacement = '''def _gh_aw_headers(
    lines: Sequence[str],
) -> tuple[dict[str, object], dict[str, object]] | None:
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


def _normalized_lines_digest(lines: Sequence[str]) -> str:
    return hashlib.sha256("\\n".join(lines).encode("utf-8")).hexdigest()


def _manifest_action_sha(
    manifest: dict[str, object], repository: str
) -> str | None:
    actions = manifest.get("actions")
    if not isinstance(actions, list):
        return None
    matches = [
        item.get("sha")
        for item in actions
        if isinstance(item, dict) and item.get("repo") == repository
    ]
    if len(matches) != 1 or not isinstance(matches[0], str):
        return None
    return matches[0]


def _is_gh_aw_safe_outputs_checkout(
    lines: Sequence[str], relative: str, use_index: int, action: str
) -> bool:
    expected_digest = GH_AW_LOCK_FILE_DIGESTS.get(relative)
    if expected_digest is None:
        return False
    if _normalized_lines_digest(lines) != expected_digest:
        return False

    headers = _gh_aw_headers(lines)
    if headers is None:
        return False
    metadata, manifest = headers
    if metadata != {
        "schema_version": "v4",
        "frontmatter_hash": "a9350cf6a6b596635b33a5199de7e5689bc44818394e69d72684e034d307ce08",
        "body_hash": "cdc940a963151e984eebadb6fbb0b21430c045598de29328b00111d9d29bb577",
        "compiler_version": "v0.88.7",
        "strict": True,
        "agent_id": "claude",
        "agent_model": "MiniMax-M3",
        "engine_versions": {"claude": "2.1.247"},
    }:
        return False
    if manifest.get("version") != 1:
        return False

    external = EXTERNAL_ACTION_RE.fullmatch(action)
    if external is None or external.group(1) != "actions/checkout":
        return False
    checkout_sha = _manifest_action_sha(manifest, "actions/checkout")
    setup_sha = _manifest_action_sha(manifest, "github/gh-aw-actions/setup")
    if checkout_sha != external.group(2):
        return False
    if setup_sha is None or FULL_SHA_RE.fullmatch(setup_sha) is None:
        return False

    bounds = _containing_job(lines, use_index)
    return bounds is not None and bounds[0] == "safe_outputs"


'''
    scanner = scanner[:start] + replacement + scanner[end:]
    SCANNER_PATH.write_text(scanner, encoding="utf-8")


def patch_tests() -> None:
    tests = TESTS_PATH.read_text(encoding="utf-8")
    helper_start = tests.index("    def gh_aw_safe_outputs_workflow(")
    helper_end = tests.index(
        "    def test_detects_mutable_external_action_in_composite_action", helper_start
    )
    helper = '''    def gh_aw_safe_outputs_workflow(
        self,
        *,
        detection_gate: bool = True,
        push_token: str | None = None,
        extra_step: str = "",
    ) -> str:
        checkout_sha = "a" * 40
        setup_sha = "b" * 40
        token = push_token or ratchet.GH_AW_PUSH_TOKEN
        metadata = json.dumps(
            {
                "schema_version": "v4",
                "frontmatter_hash": "a9350cf6a6b596635b33a5199de7e5689bc44818394e69d72684e034d307ce08",
                "body_hash": "cdc940a963151e984eebadb6fbb0b21430c045598de29328b00111d9d29bb577",
                "compiler_version": "v0.88.7",
                "strict": True,
                "agent_id": "claude",
                "agent_model": "MiniMax-M3",
                "engine_versions": {"claude": "2.1.247"},
            },
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
            f"# gh-aw-metadata: {metadata}\\n"
            f"# gh-aw-manifest: {manifest}\\n"
            "name: generated\\n"
            "on: [workflow_dispatch]\\n"
            "permissions: {}\\n"
            "jobs:\\n"
            "  safe_outputs:\\n"
            "    needs:\\n"
            "      - activation\\n"
            "      - agent\\n"
            "      - detection\\n"
            f"    if: {condition}\\n"
            "    permissions:\\n"
            "      contents: write\\n"
            "      issues: write\\n"
            "      pull-requests: write\\n"
            "    runs-on: ubuntu-latest\\n"
            "    steps:\\n"
            "      - name: Checkout repository\\n"
            f"        if: {condition} && contains(needs.agent.outputs.output_types, 'create_pull_request')\\n"
            f"        uses: actions/checkout@{checkout_sha}\\n"
            "        with:\\n"
            "          persist-credentials: true\\n"
            f"          token: {token}\\n"
            "      - name: Configure Git credentials\\n"
            f"        if: {condition} && contains(needs.agent.outputs.output_types, 'create_pull_request')\\n"
            "        env:\\n"
            f"          GIT_TOKEN: {token}\\n"
            f"        run: {ratchet.GH_AW_CONFIGURE_GIT_COMMAND}\\n"
            f"{extra_step}"
        )

    def approve_gh_aw_fixture(self, relative: str, content: str) -> None:
        previous = ratchet.GH_AW_LOCK_FILE_DIGESTS.get(relative)
        ratchet.GH_AW_LOCK_FILE_DIGESTS[relative] = ratchet._normalized_lines_digest(
            content.splitlines()
        )

        def restore() -> None:
            if previous is None:
                ratchet.GH_AW_LOCK_FILE_DIGESTS.pop(relative, None)
            else:
                ratchet.GH_AW_LOCK_FILE_DIGESTS[relative] = previous

        self.addCleanup(restore)

'''
    tests = tests[:helper_start] + helper + tests[helper_end:]

    policy_start = tests.index(
        "    def test_explicit_empty_permissions_mapping_is_clean"
    )
    policy_end = tests.index(
        "    def test_detects_floating_cargo_install_in_list_form_run", policy_start
    )
    policy = '''    def test_explicit_empty_permissions_mapping_is_clean(self) -> None:
        self.write(
            ".github/workflows/deny-all.yml",
            "name: deny all\\non: [workflow_dispatch]\\npermissions: {}\\njobs: {}\\n",
        )
        self.assertNotIn("unsupported_security_yaml_indirection", self.rules())

    def test_nonempty_permissions_flow_map_remains_rejected(self) -> None:
        self.write(
            ".github/workflows/flow-permissions.yml",
            "name: flow\\non: [workflow_dispatch]\\npermissions: {contents: write}\\njobs: {}\\n",
        )
        self.assertIn("unsupported_security_yaml_indirection", self.rules())

    def test_accepts_exact_reviewed_gh_aw_lock(self) -> None:
        relative = ".github/workflows/generated.lock.yml"
        content = self.gh_aw_safe_outputs_workflow()
        self.approve_gh_aw_fixture(relative, content)
        self.write(relative, content)
        rules = self.rules()
        self.assertNotIn("unsupported_security_yaml_indirection", rules)
        self.assertNotIn("checkout_persists_credentials_on_write_surface", rules)

    def test_any_gh_aw_lock_change_restores_persisted_credential_finding(self) -> None:
        relative = ".github/workflows/generated.lock.yml"
        reviewed = self.gh_aw_safe_outputs_workflow()
        self.approve_gh_aw_fixture(relative, reviewed)
        changed = reviewed.replace("name: generated", "name: changed", 1)
        self.write(relative, changed)
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_extra_gh_aw_publisher_step_restores_finding(self) -> None:
        relative = ".github/workflows/generated.lock.yml"
        reviewed = self.gh_aw_safe_outputs_workflow()
        self.approve_gh_aw_fixture(relative, reviewed)
        changed = self.gh_aw_safe_outputs_workflow(
            extra_step="      - name: Unreviewed push\\n        run: git push origin HEAD\\n"
        )
        self.write(relative, changed)
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_gh_aw_publisher_checkout_requires_detection_gate(self) -> None:
        relative = ".github/workflows/generated.lock.yml"
        reviewed = self.gh_aw_safe_outputs_workflow()
        self.approve_gh_aw_fixture(relative, reviewed)
        self.write(relative, self.gh_aw_safe_outputs_workflow(detection_gate=False))
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_gh_aw_publisher_checkout_requires_canonical_push_token(self) -> None:
        relative = ".github/workflows/generated.lock.yml"
        reviewed = self.gh_aw_safe_outputs_workflow()
        self.approve_gh_aw_fixture(relative, reviewed)
        self.write(
            relative,
            self.gh_aw_safe_outputs_workflow(
                push_token="$" + "{{ secrets.OTHER_TOKEN }}"
            ),
        )
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_gh_aw_lock_allowance_is_path_bound(self) -> None:
        reviewed_path = ".github/workflows/reviewed.lock.yml"
        other_path = ".github/workflows/other.lock.yml"
        content = self.gh_aw_safe_outputs_workflow()
        self.approve_gh_aw_fixture(reviewed_path, content)
        self.write(other_path, content)
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())

'''
    tests = tests[:policy_start] + policy + tests[policy_end:]
    TESTS_PATH.write_text(tests, encoding="utf-8")


def main() -> None:
    patch_scanner()
    patch_tests()
    THIS_PATH.unlink()


if __name__ == "__main__":
    main()
