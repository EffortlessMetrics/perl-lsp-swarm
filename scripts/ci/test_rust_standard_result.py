"""Negative controls for the owned-runner result policy and GET transport."""
import hashlib
import json
import os
import subprocess
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from scripts.ci.github_read import read, NoRedirect
from scripts.ci.rust_standard_result import policy_environment

ROOT = Path(__file__).resolve().parents[2]

class TransportTests(unittest.TestCase):
    def setUp(self):
        self.env = patch.dict(os.environ, {'GITHUB_REPOSITORY':'owner/repo'})
        self.env.start(); self.addCleanup(self.env.stop)
    def test_single_read_has_no_mutation_or_external_endpoint(self):
        observed=[]
        self.assertEqual(read('repos/owner/repo/git/ref/heads/main',fetch=lambda path: observed.append(path) or {'object':{'sha':'a'*40}})['object']['sha'],'a'*40)
        self.assertEqual(len(observed),1)
        for path in ('https://evil.test/', 'repos/other/repo/git/ref/heads/main', 'repos/owner/repo/actions/runs/12/cancel', 'repos/owner/repo/contents/secret'):
            with self.assertRaises(ValueError): read(path,fetch=lambda _: self.fail('invalid endpoint reached transport'))
        with self.assertRaises(ValueError): NoRedirect().redirect_request(None,None,None,None,None,None)
    def test_pages_complete_and_shapes_preserved(self):
        pages=[{'total_count':101,'check_runs':[{'id':n} for n in range(1,101)]},{'total_count':101,'check_runs':[{'id':101}]}]
        observed=[]
        def fetch(path):
            observed.append(path)
            return pages[len(observed)-1]
        self.assertEqual(read('repos/owner/repo/commits/'+ 'a'*40 +'/check-runs?per_page=100',paginate=True,fetch=fetch),pages)
        self.assertTrue(observed[1].endswith('page=2'))
    def test_missing_duplicate_and_changing_pages_rejected(self):
        first={'total_count':101,'workflow_runs':[{'id':n} for n in range(1,101)]}
        for final in ({'total_count':102,'workflow_runs':[{'id':101}]}, {'total_count':101,'workflow_runs':[{'id':1}]}, {'total_count':101,'workflow_runs':[]}):
            pages=iter((first,final))
            with self.assertRaises(ValueError):
                read('repos/owner/repo/actions/workflows/ci.yml/runs?head_sha='+ 'a'*40,paginate=True,fetch=lambda _:next(pages))

class PolicyTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.event=Path(self.temp.name)/'event.json'
        self.event.write_text(json.dumps({'pull_request':{'draft':False,'head':{'sha':'b'*40}}}))
        evidence={'schema':'em-ci-rust-result.v1','repository':'owner/repo','sha':'a'*40,'run_id':'123','run_attempt':'2','profile':'standard','result':'success','proof_outcome':'success','route':'self_hosted','infrastructure_failure':'','proof_entered_at':'2026-10-02T00:00:00Z'}
        self.env={'EM_CI_SELECTED_PROOF_RESULT':'success','EM_CI_EVIDENCE':json.dumps(evidence),'GITHUB_REPOSITORY':'owner/repo','GITHUB_SHA':'a'*40,'GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'2','GITHUB_EVENT_NAME':'pull_request','GITHUB_EVENT_PATH':str(self.event)}
    def test_distinct_tested_merge_and_check_subject(self):
        result=policy_environment(self.env)
        self.assertEqual(result['CANDIDATE_SHA'],'b'*40)
        self.assertEqual(result['EFFECTIVE_WORKFLOW_TREE'],'a'*40)
        self.assertNotIn('EM_CI_CALL_RESULT',result)
    def test_missing_failed_and_stale_proof_rejected(self):
        for changes in ({'EM_CI_SELECTED_PROOF_RESULT':'failure'},{'EM_CI_SELECTED_PROOF_RESULT':''},{'GITHUB_RUN_ATTEMPT':'3'},{'GITHUB_SHA':'c'*40},{'GITHUB_EVENT_NAME':'workflow_run'}):
            with self.assertRaises(ValueError):policy_environment(dict(self.env,**changes))
    def test_draft_cannot_create_result_policy_verdict(self):
        self.event.write_text(json.dumps({'pull_request':{'draft':True,'head':{'sha':'b'*40}}}))
        with self.assertRaises(ValueError):policy_environment(self.env)
    def test_extracted_main_red_policy_changes_only_transport(self):
        # Golden digest of the extracted shell after transport-only substitution,
        # independently byte-compared to consumer ac2cddf1 before workflow wiring.
        actual=(ROOT/'.ci/rust-main-red-probe.sh').read_text().split('\n',2)[2]
        self.assertEqual(hashlib.sha256(actual.encode()).hexdigest(),
                         'efb096913970ca7fc6fcb604eafb589a8943aefee30fa424b134dcbd1be3811e')
        self.assertNotIn('gh api',actual)
    def test_missing_tools_fail_before_bootstrap_or_dependency_setup(self):
        for missing in ('python3', 'git', 'uv', 'jq', 'base64'):
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as directory:
                binary = Path(directory)
                for name in ('python3', 'git', 'uv', 'jq', 'base64'):
                    if name == missing:
                        continue
                    executable = binary / name
                    executable.write_text('#!/bin/sh\nexit 99\n')
                    executable.chmod(0o755)
                result = subprocess.run(['/bin/bash', str(ROOT/'.ci/rust-standard-result.sh')],
                    env={'PATH':directory, 'EM_CI_SELECTED_PROOF_RESULT':'success'},
                    text=True, capture_output=True, check=False)
                self.assertEqual(result.returncode, 78, result.stderr)
                self.assertIn('required policy tool unavailable: '+missing, result.stderr)

    def test_all_existing_contracts_still_run_before_policy(self):
        script=(ROOT/'.ci/rust-standard-result.sh').read_text()
        for name in ('rustfmt_check','rustfmt_required_workflow','rust_small_route_contract','rust_small_evidence','hosted_formatter_producers','rust_small_probe_gate','main_red_refusal'):
            self.assertLess(script.index('test_'+name+'.py'),script.index('python3 scripts/ci/rust_standard_result.py'))
        self.assertIn("'PyYAML==6.0.2'",script)
        self.assertIn('uv venv',script)

if __name__=='__main__':unittest.main()
