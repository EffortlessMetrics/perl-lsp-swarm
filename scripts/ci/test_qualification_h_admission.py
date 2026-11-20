"""Perl join controls; optional pinned Hermes source exercises the real reader.

HERMES_SOURCE_ROOT is a test-only source location, not a production load path.
Run the integration class against Hermes #64's exact reviewed source; no API,
credentials, Admission.execute or publisher mutation is used by these tests.
"""
from copy import deepcopy
from dataclasses import replace
import datetime as dt
import importlib
import os
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace
import unittest

from scripts.ci import candidate_readiness as readiness
from scripts.ci import qualification_h_admission as join
from scripts.ci.test_candidate_readiness import ExistingOwners, PLAN

HERMES_HEAD = '914087d2b973936815c2146009cad6d2bcfc323a'
HERMES_ROOT = os.environ.get('HERMES_SOURCE_ROOT')
G_OBLIGATIONS = ('Compile All Targets (bit-rot guard)', 'Conflict marker check',
                 'validate-title', 'Perl LSP Rust Small Result', 'ripr+ New Gap Gate')


class JoinTests(unittest.TestCase):
    def setUp(self):
        self.plan = deepcopy(PLAN)
        self.owners = ExistingOwners()
        self.owners.selected = replace(self.owners.selected, integration_obligations=G_OBLIGATIONS)
        self.request = {'request_id': 'test-only', 'producer_generation_sha256': '9' * 64}
        self.reader_calls = 0
        self.reader = SimpleNamespace(observe=self.read, current=lambda *_: None)

    def authorization(self, evidence):
        return join.Authorization(evidence, deepcopy(self.request), 4242, 'CI Lifecycle Qualification',
                                  'synthetic authenticated owner', 'synthetic mapping owner')

    def read(self, authorization):
        self.reader_calls += 1
        return {'phase': 'H_ADMITTED', 'check_id': 123}

    def observe(self, **kwargs):
        return join.observe(lambda: self.plan, self.owners,
                            **dict(authorization_owner=self.authorization, hermes=self.reader, **kwargs))

    def test_default_missing_authority_is_not_ready_by_shape(self):
        result = join.observe(lambda: self.plan, self.owners)
        self.assertEqual(result['readiness']['classification'], 'READY')
        self.assertEqual(result['status'], 'NOT_PROVEN')
        self.assertEqual(self.reader_calls, 0)

    def test_synthetic_positive_is_conditional_observation_only(self):
        result = self.observe()
        self.assertEqual(result['status'], 'H_AUTHORITY_OBSERVED')
        self.assertEqual(result['readiness']['integration_obligations'], list(G_OBLIGATIONS))
        self.assertEqual(result['effects'], [])
        for field in ('publication_authorized', 'execution_authorized', 'merge_authorized', 'g_qualification_proven'):
            self.assertIs(result[field], False)

    def test_additional_G_obligations_are_preserved_without_fixed_inventory(self):
        obligations = (*G_OBLIGATIONS, 'Gate Enforcement Contract', 'source_commit_api_check')
        self.owners.selected = replace(self.owners.selected, integration_obligations=obligations)
        result = self.observe()
        self.assertEqual(result['status'], 'H_AUTHORITY_OBSERVED')
        self.assertEqual(result['readiness']['integration_obligations'], list(obligations))

    def test_missing_mismatched_or_json_authority_refuses_before_read(self):
        for change in (None, {}, 'READY', 'hash-only',
                       {'reference': ''}, {'mapping_reference': ''}, {'app_id': True},
                       {'evidence': replace(join.capture(lambda: self.plan, self.owners), subject={})}):
            with self.subTest(change=change):
                def owner(evidence):
                    if isinstance(change, dict) and change:
                        return replace(self.authorization(evidence), **change)
                    return change
                result = join.observe(lambda: self.plan, self.owners, authorization_owner=owner, hermes=self.reader)
                self.assertEqual(result['status'], 'NOT_PROVEN')
        self.assertEqual(self.reader_calls, 0)

    def test_subject_and_portfolio_in_place_changes_are_detected(self):
        for field in ('head', 'tree', 'base', 'control', 'request_key', 'portfolio_sha256', 'policy_sha256'):
            with self.subTest(field=field):
                self.setUp()
                self.reader.observe = lambda _: self.plan.update({field: 'changed'})
                self.assertEqual(self.observe()['status'], 'NOT_PROVEN')

    def test_changed_nonadverse_findings_and_selected_commands_are_detected(self):
        for change in ('finding', 'commands', 'review'):
            with self.subTest(change=change):
                self.setUp()
                def read(_):
                    if change == 'finding':
                        self.owners.findings = (readiness.IntegrationFinding(PLAN['request_key'], 'pending', 'new-observation'),)
                    elif change == 'commands':
                        self.owners.selected = replace(self.owners.selected, selected=('different-command',))
                        self.owners.proof = replace(self.owners.proof, executed=('different-command',))
                    else:
                        self.owners.semantic = replace(self.owners.semantic, reference='new-review')
                    return {}
                self.reader.observe = read
                self.assertEqual(self.observe()['reason'], 'subject_review_proof_or_findings_changed')

    def test_late_adverse_review_or_findings_are_not_hidden_by_same_head(self):
        for change in ('semantic', 'native', 'finding'):
            with self.subTest(change=change):
                self.setUp()
                def read(_):
                    if change == 'finding':
                        self.owners.findings = (readiness.IntegrationFinding(PLAN['request_key'], 'unresolved_failure', 'failure'),)
                    else:
                        setattr(self.owners, change, replace(getattr(self.owners, change), classification='NOT_READY'))
                    return {}
                self.reader.observe = read
                self.assertEqual(self.observe()['status'], 'NOT_PROVEN')

    def test_initial_adverse_or_untrusted_evidence_keeps_existing_disposition(self):
        for state, expected in [('failure', 'NOT_READY'), ('untrusted', 'UNTRUSTED_REPLAY_REQUIRED')]:
            self.owners.proof = replace(self.owners.proof, disposition=state)
            self.assertEqual(self.observe()['status'], expected)
        self.assertEqual(self.reader_calls, 0)

    def test_authority_mutation_and_revocation_are_detected(self):
        saved = self.authorization(join.capture(lambda: self.plan, self.owners))
        self.authorization = lambda _: saved
        self.reader.observe = lambda _: saved.request.update(producer_generation_sha256='8' * 64)
        self.assertEqual(self.observe()['reason'], 'h_authorization_changed')

    def test_unavailable_or_uncertain_readback_is_not_retried(self):
        calls = []
        def read(_):
            calls.append('read')
            raise RuntimeError('uncertain former publication')
        self.reader.observe = read
        self.assertEqual(self.observe()['status'], 'NOT_PROVEN')
        self.assertEqual(calls, ['read'])


@unittest.skipUnless(HERMES_ROOT, 'actual Hermes integration requires HERMES_SOURCE_ROOT at pinned #64')
class ActualHermesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        root = Path(HERMES_ROOT).resolve()
        head = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
        if head != HERMES_HEAD:
            raise RuntimeError('Hermes test dependency is not the pinned source')
        dirty = subprocess.check_output(['git', '-C', str(root), 'status', '--porcelain',
                                         '--untracked-files=all'], text=True)
        if dirty:
            raise RuntimeError('Hermes test dependency is not clean')
        old_path = sys.path[:]
        old_modules = {key: value for key, value in sys.modules.items()
                       if key == 'hosted_queue_admission' or key.startswith('hosted_queue_admission.')}
        def restore():
            sys.path[:] = old_path
            for key in list(sys.modules):
                if key == 'hosted_queue_admission' or key.startswith('hosted_queue_admission.'):
                    del sys.modules[key]
            sys.modules.update(old_modules)
        cls.addClassCleanup(restore)
        for key in old_modules:
            del sys.modules[key]
        sys.path.insert(0, str(root))
        cls.model = importlib.import_module('hosted_queue_admission.model')
        cls.publisher_type = importlib.import_module('hosted_queue_admission.checks').CheckPublisher
        for key, module in sys.modules.items():
            if key == 'hosted_queue_admission' or key.startswith('hosted_queue_admission.'):
                if not Path(module.__file__).resolve().is_relative_to(root):
                    raise RuntimeError('Hermes test import escaped pinned source')

    def setUp(self):
        self.now = dt.datetime(2026, 11, 13, tzinfo=dt.timezone.utc)
        self.plan = deepcopy(PLAN)
        self.policy = dict(schema_version=1, enabled=True, repository=PLAN['repository'], repository_id=7,
                           base_branch='main', workflow_path='.github/workflows/admission.yml',
                           publisher_app_id=4242, check_name='CI Lifecycle Qualification',
                           allowed_actors=['actor'], allowed_reviewers=['reviewer'],
                           allowed_review_dispositions=['REVIEW_CURRENT'], proof_profiles=['normal'],
                           producer_generation_sha256='9' * 64, max_admission_seconds=3600,
                           max_stack_size=3, settlement_attempts=1, settlement_interval_seconds=0)
        self.member = dict(pull_number=3, head_sha=PLAN['head'], head_ref='feature', review_id=17,
                           review_body_sha256='3' * 64, pull_body_sha256='4' * 64)
        self.request = dict(schema_id=self.model.SCHEMA, schema_version=1, request_id='source-test',
                            repository=PLAN['repository'], selected_pull_number=3, expected_base_sha=PLAN['base'],
                            members=[self.member], proof_profile='normal', producer_generation_sha256='9' * 64,
                            expires_at=(self.now + dt.timedelta(minutes=30)).isoformat())
        evidence = join.capture(lambda: self.plan, ExistingOwners())
        self.auth = join.Authorization(evidence, self.request, 4242, self.policy['check_name'],
                                       'synthetic authenticated owner', 'synthetic mapping owner')
        self.receipt = dict(self.request, phase='H_ADMITTED', authority_sha256=self.model.sha256(self.request),
                            base_sha_at_admission=PLAN['base'], member_pull_number=3, member_head_sha=PLAN['head'])
        publisher = self.publisher_type(self.policy, None)
        self.check = dict(id=55, name=self.policy['check_name'], head_sha=PLAN['head'], app={'id': 4242},
                          external_id=publisher.external_id('source-test', 3), status='completed', conclusion='success',
                          output=publisher.output(self.receipt))
        self.rows = [self.check]
        self.calls = []
        self.reader = join.HermesReadback(self.policy, model=self.model, publisher_type=self.publisher_type,
                                        read=self.read, now=lambda: self.now)

    def read(self, path):
        self.calls.append(path)
        return SimpleNamespace(data=({'total_count': len(self.rows), 'check_runs': deepcopy(self.rows)}
                                     if '?' in path else deepcopy(self.check)))

    def test_real_Hermes_conditional_verifier_is_consumed(self):
        result = self.reader.observe(self.auth)
        self.assertEqual(result['receipt']['phase'], 'H_ADMITTED')
        self.assertEqual(result['check_id'], 55)
        self.assertEqual(len(self.calls), 2)
        joined = join.observe(lambda: self.plan, ExistingOwners(),
                              authorization_owner=lambda evidence: replace(self.auth, evidence=evidence),
                              hermes=self.reader)
        self.assertEqual(joined['status'], 'H_AUTHORITY_OBSERVED')
        self.assertEqual(joined['readback']['check_id'], 55)
        self.assertFalse(joined['publication_authorized'])

    def test_wrong_or_missing_native_identity_and_phase_refuse(self):
        for update in ({'app': {'id': 15368}}, {'head_sha': '0' * 40}, {'name': 'validate-title'},
                       {'external_id': 'copied'}, {'status': 'in_progress', 'conclusion': None},
                       {'conclusion': 'failure'}, {'id': True}):
            with self.subTest(update=update):
                self.setUp()
                self.check.update(update)
                with self.assertRaises(ValueError):
                    self.reader.observe(self.auth)
        for phase in ('H_ENQUEUED', 'H_LANDED', 'G_PROOF_OBSERVED', 'H_WITHDRAWN'):
            self.setUp()
            self.receipt['phase'] = phase
            self.check['output']['text'] = self.model.canonical(self.receipt).decode()
            with self.subTest(phase=phase), self.assertRaises(ValueError):
                self.reader.observe(self.auth)

    def test_exact_ID_readback_missing_duplicate_and_substitution_refuse(self):
        self.rows = [deepcopy(self.check)]
        self.check['id'] = 99
        with self.assertRaisesRegex(ValueError, 'substituted'):
            self.reader.observe(self.auth)
        for count in (0, 2):
            self.setUp()
            self.rows = [self.check] * count
            with self.subTest(count=count), self.assertRaises(ValueError):
                self.reader.observe(self.auth)

    def test_generation_request_expiry_and_policy_are_bound(self):
        for field, value in [('producer_generation_sha256', '8' * 64), ('request_id', 'other'),
                             ('expected_base_sha', '0' * 40), ('proof_profile', 'unknown'),
                             ('expires_at', self.now.isoformat())]:
            self.setUp()
            self.auth.request[field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.reader.observe(self.auth)
        self.setUp()
        original = self.read
        def expiring(path):
            result = original(path)
            if '?' not in path:
                self.now += dt.timedelta(hours=1)
            return result
        self.reader.read = expiring
        with self.assertRaises(ValueError):
            self.reader.observe(self.auth)

    def test_capability_refuses_a_publisher_attempting_mutation(self):
        real = self.publisher_type
        for method, path in [('POST', '/repos/owner/repo/check-runs'), ('GET', '/installation/repositories'),
                             ('GET', '55'), ('GET', '/repos/other/repo/check-runs/55')]:
            class Mutating(real):
                def list_checks(instance, head):
                    instance.request(method, path)
                    return []
            self.reader.publisher_type = Mutating
            with self.subTest(method=method, path=path), self.assertRaises(ValueError):
                self.reader.observe(self.auth)
        self.assertEqual(self.calls, [])

    def test_closing_owner_delay_cannot_return_expired_authority(self):
        calls = []
        def authorize(evidence):
            calls.append('authority')
            if len(calls) == 2:
                self.now += dt.timedelta(hours=1)
            return replace(self.auth, evidence=evidence)
        result = join.observe(lambda: self.plan, ExistingOwners(),
                              authorization_owner=authorize, hermes=self.reader)
        self.assertEqual(calls, ['authority', 'authority'])
        self.assertEqual(result['status'], 'NOT_PROVEN')

    def test_closing_policy_change_cannot_return_old_authority(self):
        calls = []
        def authorize(evidence):
            calls.append('authority')
            if len(calls) == 2:
                self.reader.policy['allowed_reviewers'] = ['different-reviewer']
            return replace(self.auth, evidence=evidence)
        result = join.observe(lambda: self.plan, ExistingOwners(),
                              authorization_owner=authorize, hermes=self.reader)
        self.assertEqual(result['status'], 'NOT_PROVEN')


if __name__ == '__main__':
    unittest.main()
