from dataclasses import replace
import unittest
from unittest import mock
from scripts.ci import qualification_publication as publisher
from scripts.ci.test_request_qualification import GitHub, SUBJECT, adapter, ready


class ScopedGitHub(GitHub):
    def __init__(self):
        super().__init__()
        self.subject = replace(SUBJECT, repository=publisher.REPOSITORY, check_name=publisher.CONTEXT)
    def get(self, path):
        result = super().get(path.replace(publisher.REPOSITORY, 'owner/repo'))
        if path == 'repos/' + publisher.REPOSITORY:
            result['full_name'] = publisher.REPOSITORY
        return result


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.api = ScopedGitHub()
        self.adapter = adapter.Adapter(self.api, self.api.subject, ready)
        run = self.api.run()
        run['status'] = 'in_progress'
        self.api.rows = [run]
        plan = self.adapter.snapshot()
        fields = {key: plan[key] for key in ('request_key', 'head', 'tree', 'base', 'control',
                  'portfolio_sha256', 'policy_sha256')}
        self.proof = publisher.VerifiedQualification(**fields, run_id=21, run_attempt=1,
            selected=10, executed=10, failed=0, result='success',
            canonical_receipt_sha256='9' * 64, authenticated_producer_reference='canonical-native-import')

    def prepare(self, proof=None):
        return publisher.prepare(self.adapter, 21, 1, self.proof if proof is None else proof)

    def test_prepare_exact_h_app_context_external_run_attempt_without_write(self):
        result = self.prepare()
        self.assertEqual(result['status'], 'PREPARED_NOT_PUBLISHED')
        self.assertFalse(result['published'])
        self.assertEqual(result['effects'], [])
        self.assertEqual(result['payload']['head_sha'], self.proof.head)
        self.assertNotEqual(result['payload']['head_sha'], self.proof.control)
        self.assertEqual(result['app_id'], 15368)
        self.assertTrue(result['payload']['external_id'].endswith(':21:1'))
        self.assertTrue(result['payload']['details_url'].endswith('/runs/21/attempts/1'))
        self.assertFalse(any(call[0] != 'GET' for call in self.api.calls))
        # Existing transport still refuses any publication mutation.
        with self.assertRaises(ValueError):
            adapter.GitHub().dispatch(result['endpoint'], result['payload'])

    def test_candidate_json_or_rehearsal_cannot_claim_verification(self):
        for proof in ({'result': 'success', 'trusted': True}, replace(self.proof, result='not_proven'),
                      replace(self.proof, selected=0, executed=0),
                      replace(self.proof, canonical_receipt_sha256=''),
                      replace(self.proof, authenticated_producer_reference='')):
            with self.subTest(proof=proof), self.assertRaises(ValueError): self.prepare(proof)

    def test_subject_control_portfolio_policy_and_attempt_cannot_be_swapped(self):
        for key in ('request_key', 'head', 'tree', 'base', 'control', 'portfolio_sha256', 'policy_sha256'):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.prepare(replace(self.proof, **{key: 'foreign'}))
        for key in ('run_id', 'run_attempt'):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.prepare(replace(self.proof, **{key: 2}))
        with self.assertRaises(ValueError):
            self.prepare(replace(self.proof, run_attempt=True))

    def test_wrong_app_context_native_source_event_and_completed_attempt_refuse(self):
        for changes in ({'app_id': 9}, {'check_name': 'validate-title'}):
            bad = adapter.Adapter(self.api, replace(self.api.subject, **changes), ready)
            with self.assertRaises(ValueError): publisher.prepare(bad, 21, 1, self.proof)
        for changes in ({'head_sha': self.proof.head}, {'event': 'pull_request'},
                        {'run_attempt': 2}, {'status': 'completed', 'conclusion': 'success'}):
            self.api.rows = [{**self.api.run(), 'status': 'in_progress', **changes}]
            with self.subTest(changes=changes), self.assertRaises(ValueError): self.prepare()

    def test_native_attempt_or_terminal_state_changed_during_final_acquisition_refuses(self):
        for change in ({'run_attempt': 2}, {'status': 'completed', 'conclusion': 'cancelled'}):
            self.setUp()
            original_get = self.api.get
            observed = {'native_run': False}
            def get(path):
                if path == self.adapter.root and observed['native_run']:
                    self.api.rows[0].update(change)
                result = original_get(path)
                if path.endswith('/actions/runs/21'):
                    observed['native_run'] = True
                return result
            with self.subTest(change=change), mock.patch.object(self.api, 'get', side_effect=get), self.assertRaises(ValueError):
                self.prepare()

    def test_complete_success_only_and_real_partial_product_failure_preserved(self):
        for changes in ({'executed': 9}, {'failed': 1}, {'selected': True}, {'executed': False},
                        {'selected': True, 'executed': True}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                self.prepare(replace(self.proof, **changes))
        failure = self.prepare(replace(self.proof, result='product_failure', executed=3, failed=1))
        self.assertEqual(failure['payload']['conclusion'], 'failure')


if __name__ == '__main__': unittest.main()
