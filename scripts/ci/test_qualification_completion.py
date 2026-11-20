"""Manifest producer/consumer contract using synthetic native GET boundaries."""
from dataclasses import replace
import io
import json
import unittest
from unittest import mock
import zipfile

from scripts.ci import qualification_completion as completion
from scripts.ci import qualification_publication as publication
from scripts.ci import test_qualification_publication as fixture


class CompletionTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixture.PublicationTests()
        self.fixture.setUp()
        self.adapter, self.api, self.proof = self.fixture.adapter, self.fixture.api, self.fixture.proof
        self.get = self.api.get
        prepared = publication.prepare(self.adapter, 21, 1, self.proof)
        self.check = {key: prepared['payload'][key] for key in
                      ('name', 'head_sha', 'external_id', 'details_url', 'status', 'conclusion')}
        self.check.update(id=45, app={'id': 15368})

    def native_get(self, path):
        if path.endswith('/check-runs/45'):
            self.api.calls.append(('GET', path))
            return dict(self.check)
        return self.get(path)

    def manifest(self, *, check_id=45, proof=None):
        with mock.patch.object(self.api, 'get', side_effect=self.native_get):
            return completion.build(self.adapter, self.proof if proof is None else proof, check_run_id=check_id)

    def inspect_manifest(self, manifest, conclusion='success'):
        data = io.BytesIO()
        with zipfile.ZipFile(data, 'w') as archive:
            archive.writestr('qualification-result.json', json.dumps(manifest))
        self.api.rows[0].update(status='completed', conclusion=conclusion)
        with mock.patch.object(self.api, 'get', side_effect=self.native_get), \
             mock.patch.object(self.api, 'get_bytes', return_value=data.getvalue()):
            return self.adapter.inspect(21, 1)

    def test_absent_head_publication_retains_actual_evidence_but_not_qualification(self):
        manifest = self.manifest(check_id=None)
        self.assertEqual(manifest['result'], 'not_proven')
        self.assertEqual((manifest['selected'], manifest['executed'], manifest['failed']), (10, 10, 0))
        self.assertIsNone(manifest['check_run_id'])
        self.assertEqual(self.inspect_manifest(manifest)['qualification_result'], 'NOT_PROVEN')
        self.assertFalse(any(call[0] != 'GET' for call in self.api.calls))

    def test_exact_native_readback_completes_existing_inspect_contract_without_write(self):
        manifest = self.manifest()
        self.assertEqual(manifest['result'], 'success')
        self.assertEqual(manifest['check_run_id'], 45)
        self.assertEqual(manifest['head'], self.proof.head)
        result = self.inspect_manifest(manifest)
        self.assertEqual(result['status'], 'SUCCESS')
        self.assertEqual(result['qualification_result'], 'SUCCESS')
        self.assertFalse(any(call[0] != 'GET' for call in self.api.calls))

    def test_real_product_failure_manifest_remains_failure_in_consumer(self):
        self.check['conclusion'] = 'failure'
        manifest = self.manifest(proof=replace(self.proof, result='product_failure', executed=3, failed=1))
        self.assertEqual(manifest['result'], 'product_failure')
        result = self.inspect_manifest(manifest, conclusion='failure')
        self.assertEqual(result['status'], 'PRODUCT_OR_TEST_FAILURE')
        self.assertEqual(result['qualification_result'], 'FAILURE')

    def test_wrong_h_app_context_request_or_attempt_cannot_become_success(self):
        original = dict(self.check)
        for change in ({'head_sha': self.proof.control}, {'app': {'id': 9}},
                       {'name': 'validate-title'}, {'external_id': 'candidate-authored'},
                       {'details_url': original['details_url'].replace('/attempts/1', '/attempts/2')},
                       {'status': 'in_progress'}, {'conclusion': 'skipped'}):
            self.check = {**original, **change}
            with self.subTest(change=change), self.assertRaises(ValueError): self.manifest()

    def test_native_attempt_move_during_check_readback_refuses(self):
        def get(path):
            result = self.native_get(path)
            if path.endswith('/check-runs/45'):
                self.api.rows[0]['run_attempt'] = 2
            return result
        with mock.patch.object(self.api, 'get', side_effect=get), self.assertRaises(ValueError):
            completion.build(self.adapter, self.proof, check_run_id=45)

    def test_other_run_after_check_readback_cannot_complete(self):
        check_read = False
        def get(path):
            nonlocal check_read
            row = self.native_get(path)
            if path.endswith('/check-runs/45'):
                check_read = True
            if check_read and path.endswith('/actions/runs/21'):
                return dict(row, id=22)
            return row
        with mock.patch.object(self.api, 'get', side_effect=get), self.assertRaises(ValueError):
            completion.build(self.adapter, self.proof, check_run_id=45)

    def test_candidate_json_or_boolean_check_identity_never_certifies(self):
        with self.assertRaises(ValueError): completion.build(self.adapter, {'result': 'success'})
        with self.assertRaises(ValueError): self.manifest(check_id=True)


if __name__ == '__main__': unittest.main()
