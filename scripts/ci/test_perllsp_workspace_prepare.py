"""Actual owner/capture composition; recorded Cargo, real executable controls."""
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


adapter = load("perllsp_prepare", Path(os.environ.get('PERLLSP_TEST_ADAPTER', str(ROOT / "scripts/ci/perllsp_workspace_prepare.py"))))
fixture = load("perllsp_owner_fixture", ROOT / "scripts/tests/test_cargo_admitted_nested.py")


class HandoffControls(unittest.TestCase):
    def setUp(self):
        self.subject = fixture.NestedTests()
        self.subject.setUp()
        self.addCleanup(self.subject.doCleanups)
        self.enterContext(patch.object(adapter, "owner", fixture.a))
        self.enterContext(patch("sys.stdout", io.StringIO()))
        self.enterContext(patch("sys.stderr", io.StringIO()))
        self.env = self.subject.env
        self.binary = self.subject.paths["target"] / "debug/perllsp"
        self.binary.parent.mkdir()
        self.binary.write_text("#!/bin/sh\n[ \"$1\" = --version ]\n")
        self.binary.chmod(0o755)
        root = self.subject.worktree
        self.artifact = {"reason": "compiler-artifact", "package_id": "path+file://fixture#perllsp@0.1.0",
                         "manifest_path": str(root / "crates/perllsp/Cargo.toml"),
                         "target": {"kind": ["bin"], "name": "perllsp",
                                    "src_path": str(root / "crates/perllsp/src/main.rs")},
                         "profile": {"test": False, "debug_assertions": True},
                         "executable": str(self.binary), "filenames": [str(self.binary)], "fresh": True}

    def log(self, artifact=None):
        return json.dumps(artifact or self.artifact) + '\n' + json.dumps({"reason": "build-finished", "success": True}) + '\n'

    def capture(self):
        return adapter.capture(self.log(), self.env)

    def test_exact_artifact_path_and_preserved_build_semantics(self):
        args = list(fixture.a.NESTED_COMMANDS["perllsp-build"])
        self.assertEqual(args, ["build", "-p", "perllsp", "--locked", "--message-format=json"])
        child = self.capture()
        self.assertEqual(adapter.validate(child, "debug"), self.binary)
        receipt, _ = fixture.a.bounded_json(json.loads(child[adapter.HANDOFF])["path"])
        self.assertEqual(receipt["binding"]["descriptor"]["marker_identity"], self.subject.receipt["marker_identity"])
        self.assertTrue(receipt["cargo_artifact"]["fresh"])  # current Cargo no-op is legitimate

    def test_missing_duplicate_failed_terminal_and_malformed_log_refuse(self):
        for log in ["", json.dumps(self.artifact), self.log().replace('"success": true', '"success": false'),
                    self.log().replace('"success": true', '"success": 1'),
                    json.dumps(self.artifact) + '\n' + self.log(), self.log() + '{}\n', '{broken\n']:
            # Keep each fault independent even when a mutant wrongly writes
            # a receipt. Otherwise the next case only detects file collision.
            (self.subject.paths['temp'] / ('perllsp-handoff-' + str(os.getpid()) + '.json')).unlink(missing_ok=True)
            with self.subTest(log=log), self.assertRaises(fixture.a.Denied):
                adapter.capture(log, self.env)

    def test_wrong_package_target_test_root_profile_and_suffix_refuse(self):
        for mutation in [lambda m: m.update(manifest_path='/other/Cargo.toml'),
                         lambda m: m['target'].update(name='other'),
                         lambda m: m['target'].update(kind=['lib']),
                         lambda m: m['target'].update(src_path='/other/main.rs'),
                         lambda m: m['profile'].update(test=True),
                         lambda m: m.update(executable=str(self.binary.parent.parent / 'release/perllsp')),
                         lambda m: m.update(executable=str(self.binary.with_suffix('.exe'))),
                         lambda m: m.update(filenames=[])]:
            m = copy.deepcopy(self.artifact)
            mutation(m)
            with self.assertRaises(fixture.a.Denied):
                adapter.capture(self.log(m), self.env)

    def test_malformed_artifact_fields_refuse(self):
        for key in ['target', 'profile', 'filenames', 'executable']:
            message = copy.deepcopy(self.artifact)
            message[key] = None
            with self.subTest(key=key), self.assertRaises(fixture.a.Denied):
                adapter.capture(self.log(message), self.env)

    def test_missing_malformed_and_changed_receipt_refuse_before_probe(self):
        child = self.capture()
        missing = dict(child); missing.pop(adapter.HANDOFF)
        malformed = dict(child); malformed[adapter.HANDOFF] = '{}'
        for env in [missing, malformed]:
            with self.assertRaises(fixture.a.Denied):
                adapter.validate(env, 'debug', probe=lambda *a, **k: self.fail('cannot probe'))
        path = Path(json.loads(child[adapter.HANDOFF])["path"])
        path.write_text(path.read_text() + ' ')
        with self.assertRaises(fixture.a.Denied):
            adapter.validate(child, 'debug', probe=lambda *a, **k: self.fail('cannot probe'))

    def test_wrong_profile_path_and_interpreter_refuse_before_probe(self):
        child = self.capture()
        for key in ['PERL_LSP_BIN', adapter.PYTHON, 'CARGO_TARGET_DIR', 'CARGO_BUILD_BUILD_DIR']:
            env = dict(child); env[key] = '/wrong'
            with self.subTest(key=key), self.assertRaises(fixture.a.Denied):
                adapter.validate(env, 'debug', probe=lambda *a, **k: self.fail('cannot probe'))
        with self.assertRaises(fixture.a.Denied):
            adapter.validate(child, 'release', probe=lambda *a, **k: self.fail('cannot probe'))

    def test_stale_source_configuration_tool_and_original_marker_refuse(self):
        child = self.capture()
        for name, kwargs in [('nested_source', {'return_value': {'head': 'other'}}),
                             ('nested_configuration', {'return_value': [{'different': True}]}),
                             ('revalidate_clippy_toolchain', {'side_effect': fixture.a.Denied('changed tool')})]:
            with patch.object(fixture.a, name, **kwargs), self.assertRaises(fixture.a.Denied):
                adapter.validate(child, 'debug', probe=lambda *a, **k: self.fail('cannot probe'))
        self.subject.marker.rename(self.subject.marker.with_name('original-retained'))
        self.subject.marker.mkdir()
        with self.assertRaises(fixture.a.Denied):
            adapter.validate(child, 'debug', probe=lambda *a, **k: self.fail('cannot probe'))

    def test_intact_receipt_file_cannot_claim_other_subject_or_owner(self):
        child = self.capture()
        path = Path(json.loads(child[adapter.HANDOFF])["path"])
        original = json.loads(path.read_text())
        mutations = [lambda b: b['plan']['source'].update(head='other-source'),
                     lambda b: b['descriptor']['resources'].update(target='/sibling/target'),
                     lambda b: b['descriptor']['resources'].update(build='/sibling/build'),
                     lambda b: b['descriptor'].update(marker_identity=[1, 2]),
                     lambda b: b.update(platform='win32'),
                     lambda b: b.update(profile='release'),
                     lambda b: b['python'].update(sha256='wrong-interpreter'),
                     lambda b: b['validator'].update(sha256='wrong-validator')]
        for mutation in mutations:
            receipt = copy.deepcopy(original)
            mutation(receipt['binding'])
            path.write_text(json.dumps(receipt))
            changed = dict(child, **{adapter.HANDOFF: json.dumps(fixture.a.file_subject(path))})
            with self.assertRaises(fixture.a.Denied):
                adapter.validate(changed, 'debug', probe=lambda *a, **k: self.fail('cannot probe'))

    def test_mutation_and_nonexecutable_refuse(self):
        child = self.capture()
        self.binary.write_text('#!/bin/sh\nexit 0\n')
        with self.assertRaises(fixture.a.Denied):
            adapter.validate(child, 'debug', probe=lambda *a, **k: self.fail('cannot probe'))

    def test_nonexecutable_file_refuses_before_probe(self):
        self.binary.chmod(0o644)
        with self.assertRaises(fixture.a.Denied):
            self.capture()

    def test_unspawnable_artifact_and_failed_version_refuse(self):
        for text in ['invalid executable format', '#!/bin/sh\nexit 7\n']:
            self.binary.write_text(text)
            with self.assertRaises(fixture.a.Denied):
                self.capture()
            handoff = self.subject.paths['temp'] / ('perllsp-handoff-' + str(os.getpid()) + '.json')
            handoff.unlink()

    def test_both_platform_artifact_contracts_without_native_windows_claim(self):
        binding = adapter.context(self.env)
        for platform, suffix in [('linux', ''), ('win32', '.exe')]:
            binding['platform'] = platform
            artifact = copy.deepcopy(self.artifact)
            path = self.binary.with_name('perllsp' + suffix)
            artifact.update(executable=str(path), filenames=[str(path)])
            self.assertEqual(adapter.artifact_path(artifact, binding), path)
            artifact['executable'] += '.wrong'
            with self.assertRaises(fixture.a.Denied):
                adapter.artifact_path(artifact, binding)
        child = self.capture()
        with patch.object(adapter.sys, 'platform', 'win32'), self.assertRaises(fixture.a.Denied):
            adapter.validate(child, 'debug', probe=lambda *a, **k: self.fail('wrong host'))

    def test_mode_membership_and_build_failure_never_launch_runtime(self):
        calls = []
        def fail_build(args, **kwargs):
            calls.append(args)
            return subprocess.CompletedProcess(args, 101, '', 'failed')
        self.assertEqual(adapter.run('--runtime', self.env, fail_build), 101)
        self.assertEqual(len(calls), 1)
        calls.clear()
        self.subject.plan['request']['rows'].remove('routed-runtime'); self.subject.save()
        with self.assertRaises(fixture.a.Denied):
            adapter.run('--runtime', self.env, fail_build)
        self.assertEqual(calls, [])

    def test_success_pipeline_binds_exact_path_and_preserves_runtime_output(self):
        for mode, row in [('--compile', 'routed-compile'), ('--runtime', 'routed-runtime')]:
            calls = []
            def invoke(args, **kw):
                calls.append((args, kw))
                if len(calls) == 1:
                    return subprocess.CompletedProcess(args, 0, self.log(), '')
                self.assertEqual(kw['env']['PERL_LSP_BIN'], str(self.binary))
                self.assertNotIn('capture_output', kw)
                self.assertEqual('--no-run' in args, row == 'routed-compile')
                return subprocess.CompletedProcess(args, 0)
            self.assertEqual(adapter.run(mode, self.env, invoke), 0)
            self.assertEqual(len(calls), 2)
            (self.subject.paths['temp'] / ('perllsp-handoff-' + str(os.getpid()) + '.json')).unlink()

    def test_last_stage_marker_replacement_is_not_success(self):
        calls = []
        def invoke(args, **kw):
            calls.append(args)
            if len(calls) == 1:
                return subprocess.CompletedProcess(args, 0, self.log(), '')
            self.subject.marker.rename(self.subject.marker.with_name('original-retained'))
            self.subject.marker.mkdir()
            return subprocess.CompletedProcess(args, 0)
        with self.assertRaises(fixture.a.Denied):
            adapter.run('--runtime', self.env, invoke)
        self.assertEqual(len(calls), 2)

    def test_successful_probe_cannot_hide_during_probe_artifact_mutation(self):
        child = self.capture()
        def probe(*args, **kwargs):
            self.binary.write_text('#!/bin/sh\nexit 0\n')
            return subprocess.CompletedProcess(args, 0)
        with self.assertRaises(fixture.a.Denied):
            adapter.validate(child, 'debug', probe=probe)

    def test_successful_probe_cannot_hide_during_probe_marker_replacement(self):
        child = self.capture()
        def probe(*args, **kwargs):
            self.subject.marker.rename(self.subject.marker.with_name('original-retained'))
            self.subject.marker.mkdir()
            return subprocess.CompletedProcess(args, 0)
        with self.assertRaises(fixture.a.Denied):
            adapter.validate(child, 'debug', probe=probe)

    def test_real_cli_missing_admission_is_refusal(self):
        result = subprocess.run([os.sys.executable, '-I', str(ROOT / 'scripts/ci/perllsp_workspace_prepare.py'),
                                 '--resolve', 'debug'], env={}, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, '')
        self.assertIn('perllsp preparation refused', result.stderr)


if __name__ == '__main__':
    unittest.main()
