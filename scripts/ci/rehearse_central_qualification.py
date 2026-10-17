"""Compiler-free cross-repository fixture rehearsal; never product qualification.

Usage: python3 -B -m scripts.ci.rehearse_central_qualification /path/to/em-ci
The supplied central checkout contributes its existing native fixture/code loader.
Only canonical semantic verification is synthetic. No network or publisher write.
"""
import importlib.util, json, hashlib, tempfile, sys
from pathlib import Path
from scripts.ci.test_qualification_runtime import RuntimeCompositionTests
if len(sys.argv) != 2:
 raise SystemExit('usage: python3 -B -m scripts.ci.rehearse_central_qualification /path/to/em-ci')
spec=importlib.util.spec_from_file_location('central', Path(sys.argv[1]).resolve() / 'tests/rust-perl-artifact-contract.py')
central=importlib.util.module_from_spec(spec); spec.loader.exec_module(central)
t=RuntimeCompositionTests(); t.setUp()
try:
 with tempfile.TemporaryDirectory() as directory:
  helper=central.QualificationSubjectHooks(); f=helper.fixture(directory)
  # Real request adapter fixture owns PR/H/B; native API envelope owns S.
  f.run['repository']['id']=f.run['head_repository']['id']=7
  f.artifact['workflow_run'].update(repository_id=7,head_repository_id=7)
  for name in ('subject','evidence','run','producer','policy','check','artifact','context'):
   value=getattr(f,name)
   value=json.loads(json.dumps(value).replace('a'*40,t.plan['control']))
   setattr(f,name,value)
  f.plan=t.plan
  f.context['qualification_subject']={k:t.plan[k] for k in ('head','tree','base','control','request_key')}
  f.evidence['sha']=t.plan['head']
  receipt=json.loads(f.files['rust-small-proof.json']); receipt['subject']['git_sha']=t.plan['head']
  raw=json.dumps(receipt).encode(); f.files['rust-small-proof.json']=raw
  manifest=json.loads(f.files['perl-rust-small-export.json']); manifest['sha']=t.plan['control']
  manifest['member'].update(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest())
  f.files['perl-rust-small-export.json']=json.dumps(manifest).encode();f.write_zip()
  env=helper.environment(f,GITHUB_REPOSITORY_ID='7')
  captured=central.QualificationSubjectHooks.CAPTURE['capture_context'](env,acquire_plan=t.owners.acquire)
  assert captured['qualification_subject']==f.context['qualification_subject']
  def imported(*,acquire_plan):
   root=helper.imported(f,acquire_plan)
   helper.custody(f,root,acquire_plan)
   return root
  t.owners.native_import=imported
  def semantic(receipt,plan,selection):
   # Semantic product verification remains synthetic; real canonical verifier not run.
   from scripts.ci.candidate_readiness import CommittedProof
   return CommittedProof(plan['request_key'],'verified',selection.selected,(),
      hashlib.sha256(receipt.read_bytes()).hexdigest(),
      f'https://github.com/{plan["repository"]}/actions/runs/123/attempts/2#job-201')
  t.owners.canonical_owner=semantic
  result=t.owners.prepare_publication(123,2)
  assert result['status']=='PREPARED_NOT_PUBLISHED' and result['payload']['head_sha']==t.plan['head']
  assert all(method=='GET' for method,_ in t.api.calls)
  f.check['head_sha']=t.plan['head']
  try:t.owners.prepare_publication(123,2)
  except ValueError:pass
  else:raise AssertionError('native check relabelled H accepted')
  print('PASS actual central capture/import/custody -> actual H/S runtime -> publication preparation; native H relabel refused; semantic callback synthetic; no publication')
finally:t.doCleanups()
