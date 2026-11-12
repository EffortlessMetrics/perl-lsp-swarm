import copy
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
    def test_self_review_refuses(self):
        self.api.pr['user']['login']='reviewer'
        with self.assertRaises(RuntimeError):self.run_admission()

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

if __name__=='__main__':unittest.main()
