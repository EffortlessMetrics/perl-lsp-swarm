import os,sys,json,time,hashlib,subprocess,threading,stat
from pathlib import Path
D=Path('/workspace/scratch/artifact-retention'); repo=Path('/workspace/perl-lsp-swarm')
env=os.environ.copy(); env.update(CARGO_HOME='/workspace/shared/cargo',RUSTUP_HOME='/workspace/shared/rustup',PATH='/workspace/shared/cargo/bin:'+env['PATH'],RUSTUP_TOOLCHAIN='1.95.0',RUSTUP_AUTO_INSTALL='0',DEVPLANE=str(D/'devplane'),TMPDIR=str(D/'temp'),CARGO_BUILD_JOBS='2',CARGO_ADMITTED_CHILD_DISCOVERY='proc-parent-pidfd')
for k in ('MIN_FREE_GB','MAX_USED_PCT','CARGO_STORAGE_POLICY','CARGO_EXPECTED_GROWTH_GB','CARGO_STORAGE_BUDGET_EVIDENCE'): env.pop(k,None)
args=['test','-p','perl-test-must','--lib','--profile','agent','--locked','--offline','--message-format=json']
def call(argv): return subprocess.run(argv,cwd=repo,env=env,capture_output=True,text=True)
p=call(['scripts/cargo-admitted','--preflight',*args]); (D/'default-preflight.log').write_text(p.stderr); assert p.returncode==75,p
scope=json.loads(next(x.split(': ',1)[1] for x in p.stderr.splitlines() if x.startswith('cargo-admitted preflight:')))['scope']; roots={k:Path(v) for k,v in scope['resources'].items()}
budget={'schema_version':1,'scope':scope,'reserve_bytes':26*2**30,'expected_growth_bytes':2**30,'basis':'Independent reviewed dependency-free perl-test-must lib-only offline pinned1.95 agent/two-jobs; 1GiB conservative operator total-growth estimate; prior #3230 comment6104276397 retained7.3MB, not peak guarantee. No concurrent builds; host reserve26GiB.'}
(D/'budget.json').write_text(json.dumps(budget,indent=2)+'\n')
p=call(['scripts/cargo-admitted','--preflight','--budget-file',str(D/'budget.json'),*args]); (D/'budget-preflight.log').write_text(p.stdout+p.stderr); assert p.returncode==0,p.stderr
for c in [['rustc','-vV'],['cargo','-vV'],['python3','--version']]:
 p=call(c); (D/(c[0]+'-version.txt')).write_text(p.stdout+p.stderr); assert p.returncode==0

def snapshot(selected,hashes=False):
 seen=set(); out={}; files=[]; errors=[]
 for name in selected:
  root=roots[name]; totals={'logical':0,'allocated':0,'entries':0,'exists':root.exists()}
  paths=[root]
  if root.exists():
   for base,dirs,names in os.walk(root,followlinks=False,onerror=lambda e:errors.append(str(e))): paths.extend(Path(base)/n for n in dirs+names)
  for path in paths:
   try: s=path.lstat()
   except FileNotFoundError:
    if path!=root: errors.append('vanished:'+str(path))
    continue
   key=(s.st_dev,s.st_ino)
   if key in seen: continue
   seen.add(key); totals['logical']+=s.st_size; totals['allocated']+=s.st_blocks*512; totals['entries']+=1
   if hashes and stat.S_ISREG(s.st_mode):
    files.append({'root':name,'path':str(path.relative_to(root)),'size':s.st_size,'allocated':s.st_blocks*512,'dev':s.st_dev,'inode':s.st_ino,'sha256':hashlib.sha256(path.read_bytes()).hexdigest()})
  out[name]=totals
 return {'roots':out,'files':files,'errors':errors,'time':time.monotonic(),'free':os.statvfs(D).f_bavail*os.statvfs(D).f_frsize}
# Endpoint roots union deduplicates in target/build/cache/temp traversal order; hashes target/build only below.
source=repo/'crates/perl-test-must/src/lib.rs'; original=source.read_bytes(); rows=[]
(D/'original-lib.rs').write_bytes(original)
for label in ['cold','warm','edit1','revert1','edit2','revert2','edit3','revert3','final-warm']:
 if label.startswith('edit'):
  marker=' [retention-'+label[-1]+']'; changed=original.decode().replace('panic!("{helper}: {context}: {detail}")','panic!("{helper}: {context}: {detail}'+marker+'")').replace('panic!("{helper}: {detail}")','panic!("{helper}: {detail}'+marker+'")'); assert changed.encode()!=original; source.write_text(changed)
 elif label.startswith('revert'): source.write_bytes(original)
 before=snapshot(list(roots)); before['files']=snapshot(['target','build'],True)['files']; samples=[]; stop=threading.Event(); failure=[]
 def sample():
  try:
   while not stop.is_set():
    t=time.monotonic(); s=snapshot(['target','build','temp']); s['scan_seconds']=time.monotonic()-t; samples.append(s); stop.wait(.02)
  except BaseException as e: failure.append(repr(e))
 observer=threading.Thread(target=sample); observer.start(); start=time.monotonic()
 p=call(['scripts/cargo-admitted','--budget-file',str(D/'budget.json'),*args]); elapsed=time.monotonic()-start; stop.set(); observer.join()
 (D/(label+'.stdout')).write_text(p.stdout); (D/(label+'.stderr')).write_text(p.stderr)
 after=snapshot(list(roots)); after['files']=snapshot(['target','build'],True)['files']; units=[]
 for line in p.stdout.splitlines():
  try: msg=json.loads(line)
  except ValueError: continue
  if msg.get('reason')=='compiler-artifact': units.append(msg)
 settlement=json.loads(next(x.split(': ',1)[1] for x in p.stderr.splitlines() if x.startswith('cargo-admitted Cargo settlement:')))
 row={'label':label,'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'exit':p.returncode,'elapsed':elapsed,'before':before,'after':after,'samples':samples,'observer_errors':failure,'units':units,'settlement':settlement}; rows.append(row); (D/'results.json').write_text(json.dumps({'scope':scope,'argv':args,'rows':rows},indent=2)+'\n')
 print(label,p.returncode,round(elapsed,3),{k:(v['logical'],v['allocated']) for k,v in after['roots'].items()},'samples',len(samples),'fresh',[u['fresh'] for u in units],flush=True)
 assert not failure and not before['errors'] and not after['errors']
 assert p.returncode==0 and settlement['tree_settled'] and settlement['lease_released'],p.stderr
assert source.read_bytes()==original
