"""Bounded source/entrypoint recurrence controls for #16802.

These checks cover the declared xtask/manual/npm/just front doors. They are not
whole-program analysis, Rust execution, or a ban on arbitrary installed tools.
Canonical workflow publishers and read-only package rehearsals remain separate.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
PUBLISH = "xtask/src/tasks/publish.rs"
MAIN = "xtask/src/main.rs"
MANUAL = "scripts/publish-new-crates-manually.sh"
MANIFEST = "vscode-extension/package.json"
REFUSAL = "vscode-extension/scripts/refuse-local-publication.cjs"
DECLARED = (PUBLISH, MAIN, MANUAL, MANIFEST, REFUSAL, "justfile")
MUTATION = re.compile(
    r"\b(?:cargo(?:\s+\+\S+)?\s+publish|(?:@vscode/)?vsce\s+publish|"
    r"ovsx\s+publish|docker\s+push|gh\s+release\s+(?:create|upload)|"
    r"npm\s+publish|twine\s+upload|gem\s+push)\b"
)


def function_body(source: str, name: str) -> str:
    match = re.search(r"(?ms)^pub fn " + name + r"\([^\n]*\)[^\n]*\{\n(.*?)^\}", source)
    return match.group(1) if match else ""


def violations(sources: dict[str, str]) -> list[str]:
    errors = []
    for name in DECLARED:
        if name not in sources:
            errors.append(f"missing declared surface: {name}")
    if errors:
        return errors
    crates = function_body(sources[PUBLISH], "publish_crates")
    refusal = re.match(r'\s*if !dry_run \{\s*bail!\(.*?\);\s*\}', crates, re.S)
    if not refusal or "return Ok(" in refusal.group(0):
        errors.append("crates: public mode must refuse before metadata or commands")
    if not all(value in crates for value in ("#13768 / #6055", "release-orchestration.yml", "does not establish release readiness")):
        errors.append("crates: canonical transaction/rehearsal guidance missing")
    for command in re.findall(r'Command::new\("cargo"\).*?\.output\(\)', crates, re.S):
        args = re.search(r'\.args\(\[([^]]*)\]\)', command)
        if not args or re.findall(r'"([^"]+)"', args.group(1)) != ["publish", "--no-verify", "--dry-run"]:
            errors.append("crates: only the fixed non-publishing rehearsal is admitted")
    vscode = function_body(sources[PUBLISH], "publish_vscode")
    if not vscode.lstrip().startswith("bail!(") or any(value in vscode for value in ("Command::", "VSCE_PAT", "OVSX_PAT", "std::env", "return Ok(")):
        errors.append("vscode: direct route must unconditionally refuse without credentials or subprocesses")
    if not re.search(r"\bPublishVscode\s*,", sources[MAIN]):
        errors.append("vscode: token-bearing command interface is not admitted")
    manual = "\n".join(line for line in sources[MANUAL].splitlines() if not line.lstrip().startswith("#"))
    if not re.search(r"(?m)^exit [1-9][0-9]*\s*$", manual) or "$" in manual or "`" in manual or MUTATION.search(manual):
        errors.append("manual: retired entrypoint must refuse without dynamic or publisher commands")
    node = sources[REFUSAL]
    if "process.exitCode = 1;" not in node or any(value in node for value in ("require(", "import ", "process.env", "process.argv", "child_process", "fetch(")):
        errors.append("npm: refusal entrypoint must not use credentials, arguments or subprocesses")
    try:
        scripts = json.loads(sources[MANIFEST])["scripts"]
    except (KeyError, TypeError, ValueError):
        return errors + ["npm: scripts manifest is unavailable"]
    for name in ("publish", "publish:openvsx"):
        if scripts.get(name) != "node ./scripts/refuse-local-publication.cjs":
            errors.append(f"npm: {name} must retain its non-publishing refusal entrypoint")
    for name, command in scripts.items():
        if MUTATION.search(command):
            errors.append(f"npm: {name} reintroduces a local publisher")
    for line in sources["justfile"].splitlines():
        if line[:1].isspace() and not line.lstrip().startswith("#") and MUTATION.search(line):
            errors.append("just: recipe reintroduces a local publisher")
    return errors


class ContainmentPolicyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.sources = {name: (ROOT / name).read_text(encoding="utf-8") for name in DECLARED}

    def test_existing_candidate_meta_runner_includes_this_guard(self):
        from scripts.ci.test_candidate_meta_self_tests import SELF_TESTS, candidate_self_test_script
        path = "scripts/ci/test_local_publication_containment.py"
        self.assertIn(Path(path), SELF_TESTS)
        self.assertIn(path, candidate_self_test_script())

    def test_current_declared_surfaces(self):
        self.assertEqual(violations(self.sources), [])

    def test_reverted_live_crate_route_and_zero_exit_are_rejected(self):
        for replacement in ('if false {', 'if !dry_run { return Ok(());'):
            sources = self.sources | {PUBLISH: self.sources[PUBLISH].replace('if !dry_run {', replacement, 1)}
            self.assertTrue(violations(sources))
        sources = self.sources | {PUBLISH: self.sources[PUBLISH].replace('["publish", "--no-verify", "--dry-run"]', '["publish", "--no-verify"]')}
        self.assertTrue(violations(sources))

    def test_reintroduced_token_and_ambient_pat_routes_are_rejected(self):
        sources = self.sources | {MAIN: self.sources[MAIN].replace('    PublishVscode,', '    PublishVscode { token: Option<String> },')}
        self.assertTrue(violations(sources))
        sources = self.sources | {REFUSAL: 'if (process.env.VSCE_PAT) require("child_process").spawnSync("vsce", ["publish"]);'}
        self.assertTrue(violations(sources))

    def test_reverted_manual_route_is_rejected(self):
        for source in ('cargo publish --no-verify --allow-dirty\n', 'exit 0\n', 'if [ "$DRY_RUN" = true ]; then exit 1; fi\ncargo publish\n'):
            self.assertTrue(violations(self.sources | {MANUAL: source}))

    def test_new_aliases_cannot_restore_publishers_or_ambient_build_then_publish(self):
        for command in ('cargo publish', 'cargo publish --no-verify', 'npm exec --offline --no -- @vscode/vsce publish',
                        'ovsx publish candidate.vsix', 'docker push image', 'gh release create tag',
                        'gh release upload tag artifact', 'npm publish', 'twine upload dist/*',
                        'npm run build && vsce publish --pat dummy'):
            with self.subTest(command=command):
                manifest = json.loads(self.sources[MANIFEST])
                manifest['scripts']['new-publisher-alias'] = command
                self.assertTrue(violations(self.sources | {MANIFEST: json.dumps(manifest)}))
                self.assertTrue(violations(self.sources | {'justfile': self.sources['justfile'] + '\nnew-publisher-alias:\n    ' + command + '\n'}))

    def test_canonical_workflow_and_read_only_rehearsal_controls_remain_allowed(self):
        manifest = json.loads(self.sources[MANIFEST])
        manifest['scripts']['inspect-candidate'] = 'npm exec --offline --no -- @vscode/vsce package'
        sources = self.sources | {MANIFEST: json.dumps(manifest),
            '.github/workflows/publish-extension.yml': 'npm exec --offline --no -- @vscode/vsce publish --packagePath sealed.vsix',
            '.github/workflows/publish-crates.yml': 'cargo publish --no-verify',
            'docs/historical-evidence.md': 'Retired unsafe example: vsce publish --pat dummy'}
        self.assertEqual(violations(sources), [])


class RefusalExecutionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='local-publication-refusal-')
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        self.bin = self.work / 'bin'
        self.bin.mkdir()
        self.log = self.work / 'publisher-probe.txt'
        bash_candidate = Path(os.environ.get('ProgramFiles', r'C:\Program Files')) / 'Git/bin/bash.exe'
        self.bash = str(bash_candidate) if os.name == 'nt' and bash_candidate.is_file() else shutil.which('bash')
        self.node = shutil.which('node')
        self.assertIsNotNone(self.bash, 'existing Bash required for refusal proof')
        self.assertIsNotNone(self.node, 'existing Node required for refusal proof')
        for command in ('cargo', 'npm', 'npx', 'vsce', 'ovsx', 'docker', 'gh'):
            shim = self.bin / command
            shim.write_text('#!/bin/sh\nprintf "invoked\\n" >> "$PUBLICATION_PROBE_LOG"\nexit 97\n', encoding='utf-8')
            shim.chmod(0o755)
            (self.bin / (command + '.cmd')).write_text('@echo invoked>>"%PUBLICATION_PROBE_LOG%"\r\n@exit /b 97\r\n', encoding='utf-8')
        # Deliberately construct an allowlisted environment; never inherit/read tokens.
        self.env = {'PATH': str(self.bin), 'PUBLICATION_PROBE_LOG': self.log.as_posix(),
                    'HOME': str(self.work), 'TMP': str(self.work), 'TEMP': str(self.work)}
        if os.name == 'nt':
            self.env['SystemRoot'] = os.environ['SystemRoot']

    def run_command(self, command, extra=None):
        return subprocess.run(command, cwd=self.work, env=self.env | (extra or {}),
                              stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=15)

    def test_inert_publisher_probe_detects_attempted_execution(self):
        result = self.run_command([self.bash, '--noprofile', '--norc', '-c', 'cargo publish --no-verify'])
        self.assertEqual(result.returncode, 97, result.stderr)
        self.assertEqual(self.log.read_text().strip(), 'invoked')

    def test_manual_entrypoint_refuses_all_modes_before_any_publisher(self):
        for dry_run in ('true', 'false', ''):
            with self.subTest(dry_run=dry_run):
                result = self.run_command([self.bash, '--noprofile', '--norc', str(ROOT / MANUAL), '--yes'],
                                          {'DRY_RUN': dry_run, 'CARGO_REGISTRY_TOKEN': 'inert-fixture'})
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('#13768 / #6055', result.stderr)
                self.assertFalse(self.log.exists(), 'a publisher or build command was invoked')

    def test_npm_run_alias_targets_refuse_argv_and_ambient_dummy_pat(self):
        scripts = json.loads((ROOT / MANIFEST).read_text())['scripts']
        for name in ('publish', 'publish:openvsx'):
            command = shlex.split(scripts[name])
            self.assertEqual(command[0], 'node')
            entrypoint = (ROOT / 'vscode-extension' / command[1]).resolve()
            self.assertEqual(entrypoint, (ROOT / REFUSAL).resolve())
            for extra in ({}, {'VSCE_PAT': 'inert-fixture', 'OVSX_PAT': 'inert-fixture'}):
                with self.subTest(alias=name, ambient_dummy_pat=bool(extra)):
                    result = self.run_command([self.node, str(entrypoint), '--token', 'inert-fixture', '--pat', 'inert-fixture'], extra)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn('#13768 / #6055', result.stderr)
                    self.assertFalse(self.log.exists(), 'a publisher or build command was invoked')


if __name__ == '__main__':
    unittest.main()
