#!/usr/bin/env python3
"""Execute the owned-policy admission boundary before the main-red probe."""
import itertools
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from scripts.ci import rust_standard_result as policy

ROOT = Path(__file__).resolve().parents[2]

class RustSmallProbeGateTests(unittest.TestCase):
    def test_actual_gate_rejects_draft_absent_failed_and_stale_proof(self):
        with tempfile.TemporaryDirectory() as directory:
            event_file = Path(directory) / 'event.json'
            for event, draft, result, evidence_mode in itertools.product(
                    ('pull_request', 'merge_group', 'workflow_dispatch', 'push'),
                    (False, True), ('success', 'failure', 'cancelled', 'skipped', ''),
                    ('valid', 'missing', 'stale', 'failed')):
                with self.subTest(event=event, draft=draft, result=result, evidence=evidence_mode):
                    event_file.write_text(json.dumps({'pull_request': {'draft': draft, 'head': {'sha': 'b'*40}},
                                                     'merge_group': {'head_sha': 'a'*40}}))
                    evidence = {'schema':'em-ci-rust-result.v1','repository':'owner/repo','sha':'a'*40,
                                'run_id':'123','run_attempt':'2','profile':'standard','result':'success',
                                'proof_outcome':'success','route':'self_hosted','infrastructure_failure':'',
                                'proof_entered_at':'2026-10-02T00:00:00Z'}
                    if evidence_mode == 'stale': evidence['run_attempt'] = '1'
                    if evidence_mode == 'failed': evidence['proof_outcome'] = 'failure'
                    env = {'EM_CI_SELECTED_PROOF_RESULT':result,
                           'EM_CI_EVIDENCE':json.dumps(evidence) if evidence_mode != 'missing' else '',
                           'GITHUB_REPOSITORY':'owner/repo','GITHUB_SHA':'a'*40,
                           'GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'2','GITHUB_EVENT_NAME':event,
                           'GITHUB_EVENT_PATH':str(event_file)}
                    admitted = not (event == 'pull_request' and draft) and result == 'success' and evidence_mode == 'valid'
                    with patch.dict(os.environ, env, clear=True), patch.object(policy.subprocess, 'run') as run, patch('sys.stderr'):
                        run.return_value.returncode = 1
                        self.assertEqual(policy.main(), 1)
                        self.assertEqual(run.call_count, int(admitted))
                        if admitted:
                            self.assertEqual(run.call_args.args[0], ['bash', '.ci/rust-main-red-probe.sh'])
                            self.assertFalse(run.call_args.kwargs['check'])

    def test_refusal_exit_status_propagates(self):
        with patch.object(policy, 'policy_environment', return_value={'subject':'validated'}), patch.object(policy.subprocess, 'run') as run:
            for status in (0, 1, 2, 43):
                run.return_value.returncode = status
                self.assertEqual(policy.main(), status)

    def test_policy_follows_prevention_suite(self):
        wrapper = (ROOT / '.ci/rust-standard-result.sh').read_text()
        self.assertLess(wrapper.index('scripts/ci/test_rust_small_probe_gate.py'),
                        wrapper.index('python3 scripts/ci/rust_standard_result.py'))
        self.assertIn('set -euo pipefail', wrapper)

if __name__ == '__main__': unittest.main()
