"""Concrete callback transport/projection; verifier executable is synthetic here.

The real Rust binary remains the semantic boundary. No product compilation or
claim of canonical semantic execution is made by these compiler-free controls.
"""
from dataclasses import replace
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

from scripts.ci import rust_small_canonical_owner as canonical
from scripts.ci import test_qualification_runtime as runtime_fixture


class CanonicalOwnerTests(unittest.TestCase):
    def setUp(self):
        self.runtime = runtime_fixture.RuntimeCompositionTests()
        self.runtime.setUp()
        self.addCleanup(self.runtime.doCleanups)
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.observation = self.root / 'observation.json'
        self.verifier = self.root / 'perl-rust-small-verifier'
        self.make_verifier()
        self.receipt = self.runtime.packet / 'rust-small-proof.json'
        self.plan = self.runtime.plan
        self.selected = replace(self.runtime.selected, selected=('compile', 'tests', 'hygiene'))
        self.write_receipt()

    def make_verifier(self, code=0):
        self.verifier.write_text('#!' + sys.executable + '\nimport os,sys,json\n'
            'from pathlib import Path\n'
            f'Path({str(self.observation)!r}).write_text(json.dumps({{"argv":sys.argv[1:],"cwd":os.getcwd(),'
            '"sensitive":any(k in os.environ for k in ("GH_TOKEN","GITHUB_TOKEN","GH_ENTERPRISE_TOKEN","GITHUB_ENTERPRISE_TOKEN","GIT_DIR")),'
            '"toolchain":os.environ.get("RUSTUP_TOOLCHAIN"),"auto_install":os.environ.get("RUSTUP_AUTO_INSTALL")}))\n'
            f'sys.exit({code})\n')
        self.verifier.chmod(0o755)
        self.owner = canonical.CanonicalOwner(verifier=self.verifier,
            verifier_sha256=hashlib.sha256(self.verifier.read_bytes()).hexdigest(),
            control_sha=self.runtime.plan['control'], workspace=self.runtime.bridge.fixture.workspace)

    def write_receipt(self, *, result='success', outcomes=('ok', 'ok', 'ok')):
        receipt = {'schema_version': 'rust_small_proof.v1',
            'subject': {'git_sha': self.plan['head'], 'worktree_dirty': False},
            'result': result, 'steps': [{'name': name, 'argv': ['cargo', name], 'outcome': outcome,
                                      'exit_code': 0 if outcome == 'ok' else 1}
                for name, outcome in zip(self.selected.selected, outcomes)]}
        self.receipt.write_text(json.dumps(receipt))

    def test_actual_process_invokes_only_owner_cli_in_h_checkout_without_credentials(self):
        with mock.patch.dict(os.environ, {'GH_TOKEN': 'private', 'GITHUB_TOKEN': 'private',
                                         'GIT_DIR': '/untrusted/repository'}):
            proof = self.owner(self.receipt, self.plan, self.selected)
        observed = json.loads(self.observation.read_bytes())
        self.assertEqual(observed['argv'], ['--worktree-root', str(self.runtime.bridge.fixture.workspace), 'rust-small-proof', '--verify-receipt', str(self.receipt)])
        self.assertEqual(Path(observed['cwd']), self.runtime.bridge.fixture.workspace)
        self.assertFalse(observed['sensitive'])
        self.assertEqual((observed['toolchain'], observed['auto_install']), ('1.95.0', '0'))
        self.assertEqual(proof.disposition, 'verified')
        self.assertEqual(proof.executed, self.selected.selected)
        self.assertEqual(proof.canonical_receipt_sha256, hashlib.sha256(self.receipt.read_bytes()).hexdigest())
        self.assertTrue(proof.producer_reference.endswith('/runs/123/attempts/2#job-201'))

    def test_zero_verifier_exit_preserves_coherent_product_failure(self):
        self.write_receipt(result='product_failure', outcomes=('ok', 'product_failure', 'not_run'))
        proof = self.owner(self.receipt, self.plan, self.selected)
        self.assertEqual(proof.disposition, 'failure')
        self.assertEqual(proof.executed, ('compile', 'tests'))
        self.assertEqual(proof.failed, ('tests',))
        self.runtime.owners.selection_owner = lambda plan: self.selected
        self.runtime.owners.canonical_owner = self.owner
        prepared = self.runtime.owners.prepare_publication(123, 2)
        self.assertEqual(prepared['payload']['conclusion'], 'failure')
        self.assertFalse(prepared['published'])

    def test_semantic_rejection_or_incomplete_proof_is_not_product_success(self):
        self.make_verifier(code=1)
        self.assertEqual(self.owner(self.receipt, self.plan, self.selected).disposition, 'unavailable')
        self.make_verifier()
        for result, outcome in (('not_completed', 'not_completed'), ('instrument_failure', 'instrument_failure')):
            self.write_receipt(result=result, outcomes=('ok', outcome, 'not_run'))
            self.assertEqual(self.owner(self.receipt, self.plan, self.selected).disposition, 'unavailable')

    def test_tool_control_and_installer_identity_cannot_move(self):
        self.owner.control_sha = '0' * 40
        with self.assertRaises(ValueError): self.owner(self.receipt, self.plan, self.selected)
        self.owner.control_sha = self.plan['control']
        self.verifier.write_text(self.verifier.read_text() + '# changed\n')
        with self.assertRaises(ValueError): self.owner(self.receipt, self.plan, self.selected)
        self.assertFalse(self.observation.exists())

    def test_selected_order_and_native_s_custody_are_not_candidate_claims(self):
        with self.assertRaises(ValueError):
            self.owner(self.receipt, self.plan, replace(self.selected, selected=('tests', 'compile', 'hygiene')))
        context = self.runtime.packet / 'em-ci-job-context.json'
        raw = context.read_bytes()
        value = json.loads(raw)
        value['sha'] = self.plan['head']
        context.write_text(json.dumps(value))
        with self.assertRaises(ValueError): self.owner(self.receipt, self.plan, self.selected)
        context.write_bytes(raw)

    def test_receipt_or_native_context_mutation_during_verification_refuses(self):
        original_run = canonical.subprocess.run
        for changed in (self.receipt, self.runtime.packet / 'em-ci-job-context.json'):
            original = changed.read_bytes()
            def run(*args, **kwargs):
                result = original_run(*args, **kwargs)
                changed.write_bytes(original + b' ')
                return result
            with self.subTest(path=changed.name), mock.patch.object(canonical.subprocess, 'run', side_effect=run), self.assertRaises(ValueError):
                self.owner(self.receipt, self.plan, self.selected)
            changed.write_bytes(original)

    def test_receipt_or_context_changed_then_restored_cannot_hide_mutation(self):
        original_run = canonical.subprocess.run
        for changed in (self.receipt, self.runtime.packet / 'em-ci-job-context.json'):
            original = changed.read_bytes()
            def run(*args, **kwargs):
                result = original_run(*args, **kwargs)
                changed.write_bytes(original + b' ')
                changed.write_bytes(original)
                return result
            with self.subTest(path=changed.name), mock.patch.object(canonical.subprocess, 'run', side_effect=run), self.assertRaises(ValueError):
                self.owner(self.receipt, self.plan, self.selected)

    def test_concrete_callback_feeds_actual_custody_runtime_without_typed_proof_fixture(self):
        self.runtime.owners.selection_owner = lambda plan: self.selected
        self.runtime.owners.canonical_owner = self.owner
        result = self.runtime.owners.prepare_publication(123, 2)
        self.assertEqual(result['status'], 'PREPARED_NOT_PUBLISHED')
        self.assertEqual(result['payload']['head_sha'], self.plan['head'])
        self.assertTrue(all(method == 'GET' for method, _ in self.runtime.api.calls))


if __name__ == '__main__': unittest.main()
