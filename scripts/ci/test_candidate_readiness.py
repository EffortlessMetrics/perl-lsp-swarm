"""Existing-authority join controls; no fixture can authorize production dispatch."""
from dataclasses import replace
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest import mock

from scripts.ci import candidate_readiness as admission

KEY = 'a' * 64
PLAN = {'request_key': KEY, 'head': 'b' * 40, 'tree': 'c' * 40, 'base': 'd' * 40,
        'control': 'e' * 40, 'portfolio_sha256': 'f' * 64, 'policy_sha256': '1' * 64,
        'repository': 'owner/repo', 'repository_id': 7, 'pr': 3}


class ExistingOwners:
    """Trusted in-process test boundaries, never production receipt files."""
    def __init__(self):
        self.semantic = admission.Review(KEY, 'REVIEW_CURRENT', 'canonical-review', 'exact subject')
        self.native = admission.Review(KEY, 'CONVERGED', 'canonical-native', 'no current finding')
        self.selected = admission.Selection(KEY, PLAN['portfolio_sha256'], PLAN['policy_sha256'],
            ('cargo-current', 'adopted-ripr'), ('Rust Small', 'RIPR'), 'existing selector')
        self.proof = admission.CommittedProof(KEY, 'verified', self.selected.selected, (), '2' * 64,
            'native authenticated import + canonical owner verification')
        self.findings = ()
    def semantic_review(self, plan): return self.semantic
    def native_review(self, plan): return self.native
    def selection(self, plan): return self.selected
    def committed_proof(self, plan, selected): return self.proof
    def integration_findings(self, plan): return self.findings


class JoinTests(unittest.TestCase):
    def setUp(self): self.owners = ExistingOwners()
    def result(self): return admission.evaluate(PLAN, self.owners)

    def test_current_cargo_and_adopted_ripr_without_future_ci_is_entry_only(self):
        with mock.patch.object(admission.subprocess, 'run', side_effect=AssertionError('join executed a command')):
            result = self.result()
        self.assertEqual(result['classification'], 'READY')
        self.assertEqual(result['integration_obligations'], ['Rust Small', 'RIPR'])
        self.assertFalse(result['execution_authorized'])
        self.assertFalse(result['merge_authorized'])
        self.assertNotIn('nextest', json.dumps(result))
        self.assertEqual(result['schema_version'], 'candidate_readiness.v1')

    def test_no_future_tool_or_not_adopted_ripr_promotion_is_invented(self):
        self.owners.selected = replace(self.owners.selected, selected=('cargo-current',))
        self.owners.proof = replace(self.owners.proof, executed=('cargo-current',))
        self.assertEqual(self.result()['classification'], 'READY')

    def test_missing_adopted_obligation_never_passes(self):
        self.owners.proof = replace(self.owners.proof, executed=('cargo-current',))
        self.assertEqual(self.result()['classification'], 'NOT_PROVEN')

    def test_later_adverse_review_or_native_finding_same_head_refuses(self):
        self.owners.semantic = replace(self.owners.semantic, classification='NOT_READY', reason='later adverse review')
        self.assertEqual(self.result()['classification'], 'NOT_READY')
        self.owners.semantic = replace(self.owners.semantic, classification='REVIEW_CURRENT')
        self.owners.native = replace(self.owners.native, classification='NOT_READY')
        self.assertEqual(self.result()['classification'], 'NOT_READY')

    def test_bound_subject_selection_policy_and_proof_cannot_move(self):
        for field, value in (('subject_key', 'x'), ('portfolio_sha256', '3' * 64),
                             ('policy_sha256', '4' * 64), ('integration_obligations', ())):
            with self.subTest(field=field):
                owners = ExistingOwners()
                owners.selected = replace(owners.selected, **{field: value})
                self.assertEqual(admission.evaluate(PLAN, owners)['classification'], 'NOT_PROVEN')
        self.owners.proof = replace(self.owners.proof, subject_key='staged-tree-is-not-commit')
        self.assertEqual(self.result()['classification'], 'NOT_PROVEN')

    def test_untrusted_receipt_requires_replay_and_json_cannot_assert_ready(self):
        self.owners.proof = replace(self.owners.proof, disposition='untrusted')
        self.assertEqual(self.result()['classification'], 'UNTRUSTED_REPLAY_REQUIRED')
        self.owners.proof = {'disposition': 'verified', 'trusted': True, 'pass': True}
        self.assertEqual(self.result()['classification'], 'NOT_PROVEN')

    def test_failed_product_and_complete_bounded_gap_set_are_preserved(self):
        self.owners.proof = replace(self.owners.proof, disposition='failure', failed=('cargo-current',))
        self.owners.native = replace(self.owners.native, classification='NOT_PROVEN')
        result = self.result()
        self.assertEqual(result['classification'], 'NOT_READY')
        self.assertEqual(len(result['gaps']), 2)
        self.assertIsNotNone(result['first_actionable_gap'])

    def test_missing_or_pending_qualification_is_not_entry_cycle(self):
        self.owners.findings = tuple(admission.IntegrationFinding(KEY, state, 'current-native-observation')
                                     for state in ('pending', 'absent', 'repaired'))
        self.assertEqual(self.result()['classification'], 'READY')
        self.owners.findings += (admission.IntegrationFinding(KEY, 'unresolved_failure', 'actual failed proof'),)
        self.assertEqual(self.result()['classification'], 'NOT_READY')

    def test_unknown_integration_failure_disposition_does_not_disappear(self):
        self.owners.findings = (admission.IntegrationFinding(KEY, 'unknown', 'native-check'),)
        self.assertEqual(self.result()['classification'], 'NOT_PROVEN')

    def test_empty_selection_needs_positive_owner_inapplicability(self):
        self.owners.selected = replace(self.owners.selected, selected=())
        self.assertEqual(self.result()['classification'], 'NOT_PROVEN')
        self.owners.selected = replace(self.owners.selected, inapplicable_reason='owner verified no applicable committed command')
        self.assertEqual(self.result()['classification'], 'READY')
        self.owners.selected = replace(self.owners.selected, selected=('cargo-current',))
        self.assertEqual(self.result()['classification'], 'NOT_PROVEN')

    def test_malformed_typed_observations_cannot_authorize(self):
        for updates in ({'integration_obligations': 'Rust Small'},
                        {'integration_obligations': ('same', 'same')},
                        {'inapplicable_reason': True, 'selected': ()}):
            self.owners = ExistingOwners()
            self.owners.selected = replace(self.owners.selected, **updates)
            self.assertEqual(self.result()['classification'], 'NOT_PROVEN')
        for updates in ({'failed': False}, {'executed': 'cargo-current'}):
            self.owners = ExistingOwners()
            self.owners.proof = replace(self.owners.proof, **updates)
            self.assertEqual(self.result()['classification'], 'NOT_PROVEN')

    def test_missing_ownership_and_duplicate_or_unexecuted_selection_refuses(self):
        self.owners.selected = None
        self.assertEqual(self.result()['classification'], 'NOT_PROVEN')
        self.owners = ExistingOwners()
        self.owners.selected = replace(self.owners.selected, selected=('cargo-current', 'cargo-current'))
        self.assertEqual(self.result()['classification'], 'NOT_PROVEN')
        for updates in ({'executed': ()}, {'failed': ('cargo-current',)},
                        {'producer_reference': ''}, {'canonical_receipt_sha256': ''}):
            self.owners = ExistingOwners()
            self.owners.proof = replace(self.owners.proof, **updates)
            self.assertEqual(self.result()['classification'], 'NOT_PROVEN')


class ExistingAdapterTests(unittest.TestCase):
    def test_live_canonical_review_adapter_rejects_fixture_mode_or_wrong_head(self):
        authority = admission.ExistingControlAuthorities(Path.cwd())
        for row in ({'acquisition_mode': 'fixture', 'current_head': PLAN['head'], 'classification': 'REVIEW_CURRENT'},
                    {'acquisition_mode': 'gh_live', 'current_head': 'wrong', 'classification': 'REVIEW_CURRENT'}):
            with mock.patch.object(authority, 'command', return_value=row):
                self.assertEqual(authority.semantic_review(PLAN).classification, 'NOT_PROVEN')

    def test_actual_wrapper_prefix_and_schema_are_consumed(self):
        row = {'wrapper_schema_version': 'pr_review_convergence.v2', 'headRefOid': PLAN['head'],
               'native_review_facts_converged': False, 'is_draft': True,
               'pending_reviewers': [], 'current_change_requests': [], 'review_decision': '',
               'unresolved_active': 0, 'unresolved_outdated': 0, 'unresolved_total': 0,
               'resolved_without_disposition': 0}
        completed = subprocess.CompletedProcess([], 0, 'OK native facts\n' + json.dumps(row), '')
        authority = admission.ExistingControlAuthorities(Path.cwd())
        with mock.patch.object(admission.subprocess, 'run', return_value=completed):
            self.assertEqual(authority.native_review(PLAN).classification, 'CONVERGED')
        for change in ({'unresolved_total': 1, 'unresolved_active': 1},
                       {'current_change_requests': ['reviewer']}, {'review_decision': 'CHANGES_REQUESTED'},
                       {'resolved_without_disposition': 1}):
            with mock.patch.object(authority, 'command', return_value={**row, **change}):
                self.assertEqual(authority.native_review(PLAN).classification, 'NOT_READY')
        with mock.patch.object(authority, 'command', return_value={**row, 'pending_reviewers': ['reviewer']}):
            self.assertEqual(authority.native_review(PLAN).classification, 'NOT_PROVEN')
        with mock.patch.object(authority, 'command', return_value={**row, 'unresolved_total': 1}):
            self.assertEqual(authority.native_review(PLAN).classification, 'NOT_PROVEN')
        for variable in ('SEMANTIC_REVIEW_TEST_FIXTURE', 'CONVERGENCE_TEST_FIXTURE_DIR'):
            with mock.patch.dict(os.environ, {variable: ''}), self.assertRaises(ValueError):
                authority.command(['bash', 'scripts/ci/check-pr-review-convergence'])

    def test_authority_instrument_failure_exit_cannot_look_successful(self):
        authority = admission.ExistingControlAuthorities(Path.cwd())
        for code in (2, -9):
            completed = subprocess.CompletedProcess([], code, '{"classification":"REVIEW_CURRENT"}', '')
            with mock.patch.object(admission.subprocess, 'run', return_value=completed), self.assertRaises(ValueError):
                authority.command(['python3', 'canonical-owner'])
        row = {'acquisition_mode': 'gh_live', 'current_head': PLAN['head'],
               'classification': 'REVIEW_CURRENT', '_authority_exit_code': 1}
        with mock.patch.object(authority, 'command', return_value=row):
            self.assertEqual(authority.semantic_review(PLAN).classification, 'NOT_PROVEN')

    def test_concrete_owner_ports_reject_empty_json_applicability(self):
        source = ExistingOwners()
        for supplied in ({}, [], '', None):
            authority = admission.ExistingControlAuthorities(Path.cwd(),
                selection_owner=source.selection, committed_owner=source.committed_proof,
                applicability_owner=lambda plan: supplied)
            with mock.patch.object(authority, 'semantic_review', return_value=source.semantic), \
                 mock.patch.object(authority, 'native_review', return_value=source.native):
                self.assertEqual(admission.evaluate(PLAN, authority)['classification'], 'NOT_PROVEN')
        authority = admission.ExistingControlAuthorities(Path.cwd(),
            selection_owner=source.selection, committed_owner=source.committed_proof,
            applicability_owner=lambda plan: ())
        with mock.patch.object(authority, 'semantic_review', return_value=source.semantic), \
             mock.patch.object(authority, 'native_review', return_value=source.native):
            self.assertEqual(admission.evaluate(PLAN, authority)['classification'], 'READY')

    def test_actual_default_adapter_cannot_certify_missing_proof_ports(self):
        authority = admission.ExistingControlAuthorities(Path.cwd())
        with mock.patch.object(authority, 'semantic_review', return_value=ExistingOwners().semantic), \
             mock.patch.object(authority, 'native_review', return_value=ExistingOwners().native):
            result = admission.evaluate(PLAN, authority)
        self.assertEqual(result['classification'], 'NOT_PROVEN')
        self.assertIn('existing_selection_interface_unavailable', [g['code'] for g in result['gaps']])

    def test_source_guard_strips_git_overrides_and_credentials_before_first_probe(self):
        calls = []
        def command(argv, **kwargs):
            calls.append((argv, kwargs))
            return subprocess.CompletedProcess(argv, 0, PLAN['control'] + '\n', '')
        with mock.patch.dict(os.environ, {'GIT_DIR': '/foreign/repo', 'GH_TOKEN': 'private'}), \
             mock.patch.object(admission.subprocess, 'run', side_effect=command), \
             mock.patch.object(admission, 'ExistingControlAuthorities', return_value=ExistingOwners()):
            admission.production(PLAN)
        for argv, kwargs in calls:
            if argv[0] == 'git':
                self.assertTrue('GIT_DIR' not in kwargs.get('env', os.environ), 'Git override reached source guard')
                self.assertTrue('GH_TOKEN' not in kwargs.get('env', os.environ), 'Metadata token reached source guard')

    def test_untracked_control_file_refuses_before_any_authority_acquisition(self):
        def command(argv, **kwargs):
            output = PLAN['control'] + '\n' if 'rev-parse' in argv else ''
            if 'status' in argv:
                output = '?? scripts/ci/shadow_module.py\n'
            return subprocess.CompletedProcess(argv, 0, output, '')
        with mock.patch.object(admission.subprocess, 'run', side_effect=command), \
             mock.patch.object(admission, 'ExistingControlAuthorities', return_value=ExistingOwners()) as owner:
            result = admission.production(PLAN)
        self.assertEqual(result['classification'], 'NOT_PROVEN')
        owner.assert_not_called()

    def test_production_requires_clean_control_source(self):
        result = admission.production(PLAN)
        self.assertEqual(result['classification'], 'NOT_PROVEN')
        self.assertIn('exact clean control', result['reason'])


if __name__ == '__main__': unittest.main()
