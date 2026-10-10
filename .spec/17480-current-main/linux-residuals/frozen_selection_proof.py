import hashlib,importlib.util,json,os,shutil,subprocess,time
from pathlib import Path
P=Path('/workspace/perl-helper-current-proof');R=Path('/workspace/perl-helper-current-main');base='1f973039d812fe080abddc44e461112d7b95a6fa';candidate='ce99f24f7df1f8561ecd28cda57b055094d2920a'
def git(*a):return subprocess.check_output(['git','-C',str(R),*a],text=True).strip()
original=(R/'.git').read_bytes();common=Path(git('rev-parse','--path-format=absolute','--git-common-dir'));config=hashlib.sha256((common/'config').read_bytes()).hexdigest()
fixture=P/'frozen-selection-original-metadata';subprocess.run(['git','clone','--no-checkout','--shared','/workspace/perl-lsp-swarm',str(fixture)],check=True,capture_output=True)
spec=importlib.util.spec_from_file_location('owner',R/'scripts/cargo_admitted.py');owner=importlib.util.module_from_spec(spec);spec.loader.exec_module(owner);tree=owner.ClippyTree();switched=False
try:
 (R/'.git').unlink();shutil.move(fixture/'.git',R/'.git');switched=True
 git('update-ref','refs/heads/frozen-proof',base);git('symbolic-ref','HEAD','refs/heads/frozen-proof');git('read-tree',candidate)
 paths=git('diff','--cached','--name-only').splitlines();assert len(paths)==43,len(paths)
 frozen=git('write-tree');assert frozen==git('rev-parse',candidate+'^{tree}')
 for label,negative in [('selection-original',False),('selection-original-conflict-negative',True)]:
  if negative:
   blob=subprocess.check_output(['git','-C',str(R),'hash-object','-w','--stdin'],input=b'<<<<<<< proof\nconflict\n=======\nother\n>>>>>>> proof\n').decode().strip();git('update-index','--add','--cacheinfo','100644,'+blob+',proof-conflict.rs')
  receipt=P/('frozen-'+label+'-receipt.json');start=time.monotonic();run=subprocess.run([str(P/'immutable-xtask'),'gates','--tier','commit','--staged','--receipt','--receipt-path',str(receipt)],cwd=R,capture_output=True)
  (P/('frozen-'+label+'.stdout')).write_bytes(run.stdout);(P/('frozen-'+label+'.stderr')).write_bytes(run.stderr)
  (P/('frozen-'+label+'-execution.json')).write_text(json.dumps({'exit':run.returncode,'seconds':time.monotonic()-start,'base':base,'candidate':candidate,'index_tree':git('write-tree'),'changed_paths':git('diff','--cached','--name-only').splitlines()},indent=2)+'\n')
  assert receipt.exists()
  if negative:assert run.returncode!=0
finally:
 if switched:shutil.rmtree(R/'.git');(R/'.git').write_bytes(original)
 assert hashlib.sha256((common/'config').read_bytes()).hexdigest()==config
 settlement=tree.finish();(P/'frozen-selection-owner-settlement.json').write_text(json.dumps(settlement,indent=2)+'\n');assert settlement['tree_settled']
print('Frozen nonempty candidate and conflict falsifier receipts saved; shared metadata restored')
