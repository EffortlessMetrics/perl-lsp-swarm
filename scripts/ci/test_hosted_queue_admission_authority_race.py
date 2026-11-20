import unittest
import hosted_queue_admission as q
import test_hosted_queue_admission as existing

class AuthorityRace(unittest.TestCase):
    def setUp(self):
        self.fixture=existing.CanonicalSharedPublisher();self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
    def invoke(self,who=None,when=None):
        f=self.fixture;revoked=False;reads=[];pages_count=0
        original_get,original_pages=f.api.get,f.api.pages
        def get(path):
            value=original_get(path)
            if '/collaborators/' in path:
                login=path.split('/')[-2];reads.append(login)
                if revoked and login==who:value['permission']='read'
            return value
        def pages(path):
            nonlocal pages_count,revoked
            pages_count+=1
            if when=='final_reviews' and pages_count==2:revoked=True
            return original_pages(path)
        def review(*args):
            nonlocal revoked
            result=q.canonical_review(*args)
            if when=='during_review':revoked=True
            return result
        f.api.get,f.api.pages=get,pages
        result=q.observe(f.api,f.event,f.env,root=f.root,review=review)
        return result,reads
    def test_requester_revoked_during_review_refuses(self):
        with self.assertRaisesRegex(RuntimeError,'write authority'):self.invoke('requester','during_review')
    def test_reviewer_revoked_after_first_permission_read_refuses(self):
        with self.assertRaisesRegex(RuntimeError,'write authority'):self.invoke('publisher','final_reviews')
    def test_reviewer_revoked_during_review_still_refuses(self):
        with self.assertRaisesRegex(RuntimeError,'write authority'):self.invoke('publisher','during_review')
    def test_current_shared_publisher_still_passes_and_both_rechecked(self):
        result,reads=self.invoke()
        self.assertEqual(result['phase'],'admission_only')
        self.assertEqual(result['review']['reviewer'],self.fixture.api.pr['user']['login'])
        self.assertEqual(reads,['requester','publisher','requester','publisher'])

if __name__=='__main__':unittest.main()
