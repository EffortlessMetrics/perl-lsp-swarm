import copy
import hashlib
import json
import shutil
import subprocess
import tempfile
import importlib.util
from pathlib import Path
import unittest
import hosted_queue_admission as q

H, T = "a"*40, "b"*40
ROOT = Path(__file__).resolve().parents[2]
class Fake:
    def __init__(self):
        self.repo = {"id": 12, "node_id":"R_12", "full_name":q.REPO,"default_branch":"main"}
        self.pr = {"number":3,"state":"open","draft":False,"head":{"sha":H},
                   "base":{"ref":"main","repo":{"id":12}},"user":{"login":"author"}}
        self.branch = {"name":"main","protected":True,"commit":{"sha":T}}
        self.ref = {"ref":"refs/heads/main","object":{"sha":T,"type":"commit"}}
        self.reviews = [{"state":"COMMENTED","body":"fixture"}]
        self.thread_rows=[]
        self.role="write"
        self.calls=[]
        self.move=False
    def get(self,path):
        self.calls.append(path)
        if path.endswith('/pulls/3'):
            if self.move and self.calls.count(path)>1:
                return dict(self.pr,head={"sha":"c"*40})
            return copy.deepcopy(self.pr)
        if '/collaborators/' in path:
            return {"permission":self.role,"user":{"login":path.split('/')[-2]}}
        if path.endswith('/branches/main'):return self.branch
        if path.endswith('/git/ref/heads/main'):return self.ref
        if path.endswith('/'):return self.repo
        raise AssertionError(path)
    def pages(self,path):return copy.deepcopy(self.reviews)
    def threads(self,number):return copy.deepcopy(self.thread_rows)

class Admission(unittest.TestCase):
    def setUp(self):
        self.api=Fake()
        self.event={"action":"labeled","label":{"name":"queue-admission"},"repository":self.api.repo,
                    "pull_request":self.api.pr,"sender":{"type":"User","login":"requester"}}
        self.env={"GITHUB_EVENT_NAME":"pull_request_target","GITHUB_REPOSITORY":q.REPO,
                  "GITHUB_WORKFLOW_SHA":T,"GITHUB_WORKFLOW_REF":q.REPO+'/'+q.WORKFLOW+'@refs/heads/main',
                  "GITHUB_ACTOR":"requester","GITHUB_TRIGGERING_ACTOR":"requester",
                  "GITHUB_RUN_ID":"41","GITHUB_RUN_ATTEMPT":"1"}
    def run_admission(self):
        # Isolate transport controls. Canonical digest/parser is the unchanged
        # repository implementation, not a new fabricated review algorithm.
        return q.observe(self.api,self.event,self.env,review=lambda *a:{"reviewer":"reviewer"})
    def test_admission_not_product_proof(self):
        r=self.run_admission();self.assertEqual(r['head_sha'],H)
        self.assertEqual(r['phase'],'admission_only');self.assertFalse(r['product_tests_executed'])
    def test_ordinary_events_refuse(self):
        for action in ['opened','synchronize','ready_for_review','converted_to_draft','edited']:
            with self.subTest(action=action):
                self.event['action']=action
                with self.assertRaises(RuntimeError):self.run_admission()
    def test_non_target_refuses(self):
        for event in ['pull_request','workflow_dispatch','merge_group','push']:
            with self.subTest(event=event):
                self.env['GITHUB_EVENT_NAME']=event
                with self.assertRaises(RuntimeError):self.run_admission()
    def test_moved_head_refuses(self):
        self.api.move=True
        with self.assertRaisesRegex(RuntimeError,'moved'):self.run_admission()
    def test_draft_closed_wrong_base_refuse(self):
        for field,value in [('draft',True),('state','closed'),('base',{'ref':'other','repo':{'id':12}})]:
            with self.subTest(field=field):
                old=self.api.pr[field];self.api.pr[field]=value
                with self.assertRaises(RuntimeError):self.run_admission()
                self.api.pr[field]=old
    def test_authority_refuses(self):
        for role in ['read','triage','none']:
            with self.subTest(role=role):
                self.api.role=role
                with self.assertRaises(RuntimeError):self.run_admission()
    def test_rerun_actor_refuses(self):
        self.env['GITHUB_TRIGGERING_ACTOR']='other'
        with self.assertRaises(RuntimeError):self.run_admission()
    def test_unresolved_refuses(self):
        self.api.thread_rows=[{'isResolved':False}]
        with self.assertRaises(RuntimeError):self.run_admission()
    def test_adverse_review_refuses(self):
        self.api.reviews=[{'state':'CHANGES_REQUESTED','user':{'login':'reviewer','type':'User'},'submitted_at':'2026-11-12T00:00:00Z'}]
        with self.assertRaises(RuntimeError):self.run_admission()
    def test_unprotected_or_changed_source_refuses(self):
        self.api.branch['protected']=False
        with self.assertRaises(RuntimeError):self.run_admission()
        self.api.branch['protected']=True;self.api.ref['object']['sha']='c'*40
        with self.assertRaises(RuntimeError):self.run_admission()
    def test_wrong_workflow_refuses(self):
        self.env['GITHUB_WORKFLOW_REF']=q.REPO+'/'+q.WORKFLOW+'@refs/pull/3/merge'
        with self.assertRaises(RuntimeError):self.run_admission()
    def test_shared_publishing_account_is_not_a_separate_veto(self):
        self.api.pr['user']['login']='reviewer'
        self.assertEqual(self.run_admission()['review']['reviewer'],'reviewer')

class ReviewConvergence(unittest.TestCase):
    def row(self,state,time):
        return {'state':state,'user':{'login':'reviewer','type':'User'},'submitted_at':time}
    def test_later_approval_supersedes_request(self):
        rows=[self.row('CHANGES_REQUESTED','2026-11-12T00:00:00Z'),self.row('APPROVED','2026-11-12T01:00:00Z')]
        self.assertTrue(q.review_converged(rows));self.assertTrue(q.review_converged(list(reversed(rows))))
    def test_comment_does_not_clear_request(self):
        rows=[self.row('CHANGES_REQUESTED','2026-11-12T00:00:00Z'),self.row('COMMENTED','2026-11-12T01:00:00Z')]
        self.assertFalse(q.review_converged(rows))
    def test_dismissed_request_not_active(self):
        self.assertTrue(q.review_converged([self.row('DISMISSED','2026-11-12T00:00:00Z')]))
    def test_equal_timestamp_conflict_refuses(self):
        rows=[self.row('CHANGES_REQUESTED','2026-11-12T00:00:00Z'),self.row('APPROVED','2026-11-12T00:00:00Z')]
        with self.assertRaises(RuntimeError):q.review_converged(rows)

class CanonicalSharedPublisher(unittest.TestCase):
    """Exercise production observe AND canonical parser/digest on real Git objects.

    Only authenticated GitHub read transport is simulated. Independent review
    authorship remains the repository review process; no new attestation field
    is invented to turn an account identity into evidence of independence.
    """
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='admission-canonical-')
        self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)
        scripts=self.root/'scripts/ci';scripts.mkdir(parents=True)
        shutil.copyfile(ROOT/'scripts/ci/check-pr-semantic-review-currentness.py',scripts/'check-pr-semantic-review-currentness.py')
        self.git('init','-q');self.git('config','user.name','Fixture')
        self.git('config','user.email','fixture@example.invalid')
        (self.root/'subject.txt').write_text('base\n')
        self.git('add','subject.txt');self.git('commit','-qm','base');self.base=self.git('rev-parse','HEAD')
        (self.root/'subject.txt').write_text('material change\n')
        self.git('add','subject.txt');self.git('commit','-qm','head');self.head=self.git('rev-parse','HEAD')
        self.api=Fake();self.api.pr['head']['sha']=self.head
        self.api.pr['user']['login']='publisher'
        self.api.reviews=[self.review(self.head,self.base)]
        self.event={'action':'labeled','label':{'name':'queue-admission'},'repository':self.api.repo,
                    'pull_request':copy.deepcopy(self.api.pr),'sender':{'type':'User','login':'requester'}}
        self.env={'GITHUB_EVENT_NAME':'pull_request_target','GITHUB_REPOSITORY':q.REPO,
                  'GITHUB_WORKFLOW_SHA':T,'GITHUB_WORKFLOW_REF':q.REPO+'/'+q.WORKFLOW+'@refs/heads/main',
                  'GITHUB_ACTOR':'requester','GITHUB_TRIGGERING_ACTOR':'requester','ADMISSION_SOURCE_SHA':T,
                  'GITHUB_RUN_ID':'41','GITHUB_RUN_ATTEMPT':'1'}
    def git(self,*args):
        return subprocess.check_output(['git','-C',str(self.root),*args],stderr=subprocess.STDOUT,text=True).strip()
    def review(self,head,base):
        raw=subprocess.check_output(['git','-C',str(self.root),'diff','--binary','--full-index','--no-ext-diff',base,head,'--'])
        marker={'pr':3,'head':head,'merge_base':base,'subject_sha256':hashlib.sha256(raw).hexdigest(),'result':'REVIEW_CURRENT'}
        body='\n'.join(['## Review scope','Cumulative exact subject.','## Evidence and falsifiers','Read-only adversarial review.',
                         '## What this establishes','Reviewed candidate.','## Residual risk / not proved','No native activation.',
                         '## No material findings','## Substantive review result','- REVIEW_CURRENT',
                         '<!-- semantic-review:v1 '+json.dumps(marker)+' -->'])
        return {'user':{'login':'publisher','type':'User'},'state':'COMMENTED','body':body,'commit_id':head,
                'submitted_at':'2026-11-12T00:00:00Z'}
    def observe(self):
        return q.observe(self.api,self.event,self.env,root=self.root)
    def test_shared_login_valid_canonical_review_passes(self):
        receipt=self.observe()
        self.assertEqual(receipt['review']['classification'],'REVIEW_CURRENT')
        self.assertEqual(receipt['review']['reviewed_head'],self.head)
        self.assertEqual(receipt['review']['reviewer'],self.api.pr['user']['login'])
        self.assertFalse(receipt['product_tests_executed'])
    def test_missing_review_refuses(self):
        self.api.reviews=[]
        with self.assertRaisesRegex(RuntimeError,'exact-H substantive review'):self.observe()
    def test_stale_review_refuses(self):
        self.api.reviews=[self.review(self.base,self.base)]
        with self.assertRaisesRegex(RuntimeError,'exact-H substantive review'):self.observe()
    def test_invalid_digest_refuses(self):
        body=self.api.reviews[0]['body'];start=body.index('"subject_sha256": "')+len('"subject_sha256": "')
        self.api.reviews[0]['body']=body[:start]+'0'*64+body[start+64:]
        with self.assertRaisesRegex(RuntimeError,'exact-H substantive review'):self.observe()
    def test_reviewer_without_write_permission_refuses(self):
        original=self.api.get
        def get(path):
            value=original(path)
            if '/collaborators/publisher/' in path:value['permission']='read'
            return value
        self.api.get=get
        with self.assertRaisesRegex(RuntimeError,'write authority'):self.observe()
    def test_adverse_review_still_refuses(self):
        self.api.reviews[0]['state']='CHANGES_REQUESTED'
        with self.assertRaisesRegex(RuntimeError,'change request'):self.observe()

if __name__=='__main__':unittest.main()
