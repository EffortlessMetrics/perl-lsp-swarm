import hashlib, importlib.util, json, os, shutil, subprocess, time
from pathlib import Path
P=Path('/workspace/perl-helper-current-proof'); ROOT=Path('/workspace/perl-helper-current-main'); env=os.environ.copy()
BINS={name:P/('immutable-'+name) for name in ['xtask','perl-ci-hygiene']}
records=[]
def execute(label,args,cwd,expected=0,extra=None):
 start=time.monotonic();r=subprocess.run([str(v) for v in args],cwd=cwd,env=extra or env,capture_output=True)
 (P/(label+'.stdout')).write_bytes(r.stdout);(P/(label+'.stderr')).write_bytes(r.stderr)
 records.append({'label':label,'argv':[str(v) for v in args],'cwd':str(cwd),'exit':r.returncode,'seconds':time.monotonic()-start,'stdout_sha256':hashlib.sha256(r.stdout).hexdigest()})
 assert r.returncode==expected,(label,r.returncode,r.stderr[-2000:]);return r

def git(root,*args):return subprocess.check_output(['git','-C',str(root),*args],env=env).decode().strip()
# Exact source/binary identity is saved before any fixture metadata change.
rows=[]
for line in (P/'paired-build.stdout').read_text().splitlines():
 try:r=json.loads(line)
 except ValueError:continue
 if r.get('reason')=='compiler-artifact':rows.append(r)
for name,dest in BINS.items():
 source=next(Path(r['executable']) for r in rows if r['target']['name']==name and r.get('executable'))
 shutil.copy2(source,dest)
(P/'paired-binaries.json').write_text(json.dumps({name:{'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()} for name,path in BINS.items()},indent=2)+'\n')
spec=importlib.util.spec_from_file_location('owned',ROOT/'scripts/cargo_admitted.py');owner=importlib.util.module_from_spec(spec);spec.loader.exec_module(owner)
tree=owner.ClippyTree();original=(ROOT/'.git').read_bytes();shared_config=Path(git(ROOT,'rev-parse','--path-format=absolute','--git-common-dir'))/'config';shared_hash=hashlib.sha256(shared_config.read_bytes()).hexdigest();switched=False
try:
 fixture=P/'CLI fixture with spaces';shutil.rmtree(fixture,ignore_errors=True);fixture.mkdir();(fixture/'crates/directory-name/src').mkdir(parents=True)
 git(fixture,'init','-q');git(fixture,'config','user.email','proof@example.invalid');git(fixture,'config','user.name','Paired CLI proof')
 (fixture/'Cargo.toml').write_text('[workspace]\nmembers=["crates/directory-name"]\nresolver="2"\n');(fixture/'crates/directory-name/Cargo.toml').write_text('[package]\nname="actual-package-name"\nversion="0.1.0"\nedition="2021"\n');(fixture/'crates/directory-name/src/lib.rs').write_text('pub fn base() {}\n');(fixture/'old.rs').write_text(''.join('line'+str(i)+'\n' for i in range(100)));(fixture/'delete.md').write_text('delete\n')
 git(fixture,'add','.');git(fixture,'commit','-qm','base');base=git(fixture,'rev-parse','HEAD');git(fixture,'update-ref','refs/remotes/origin/main',base)
 git(fixture,'mv','old.rs','renamed.rs');(fixture/'delete.md').unlink();(fixture/'docs').mkdir();(fixture/'docs/notes.md').write_text('docs\n');(fixture/'crates/directory-name/src/lib.rs').write_text('pub fn changed() {}\n');git(fixture,'add','-A');git(fixture,'commit','-qm','mixed rename deletion');head=git(fixture,'rev-parse','HEAD')
 for fmt in ['paths','json']:
  for base_arg in [base,'auto']:
   args=['change-set','--base',base_arg,'--head',head,'--format',fmt,'--root',fixture]
   a=execute('hygiene-'+fmt+'-'+base_arg[:8],[BINS['perl-ci-hygiene'],*args],fixture);b=execute('xtask-'+fmt+'-'+base_arg[:8],[BINS['xtask'],*args],fixture);assert a.stdout==b.stdout
   if fmt=='paths':assert a.stdout==b'crates/directory-name/src/lib.rs\ndelete.md\ndocs/notes.md\nrenamed.rs\n',a.stdout
 hostile=dict(env,GIT_DIR='/nonexistent/foreign.git',GIT_WORK_TREE='/nonexistent/foreign',GIT_COMMON_DIR='/nonexistent/common')
 args=['change-set','--base',base,'--head',head,'--format','paths','--root',fixture]
 a=execute('hostile-hygiene',[BINS['perl-ci-hygiene'],*args],fixture,extra=hostile);b=execute('hostile-xtask',[BINS['xtask'],*args],fixture,extra=hostile);assert a.stdout==b.stdout
 for label,args in [('invalid-base',['change-set','--base','missing-proof-base','--head',head,'--format','paths','--root',fixture]),('invalid-format',['change-set','--base',base,'--head',head,'--format','pathss','--root',fixture])]:
  a=execute(label+'-hygiene',[BINS['perl-ci-hygiene'],*args],fixture,1);b=execute(label+'-xtask',[BINS['xtask'],*args],fixture,1);assert a.stdout==b.stdout==b''
  # Both frontends print equivalent resolution diagnostics; their top-level framing differs.
  assert ('missing-proof-base' if label=='invalid-base' else 'pathss').encode() in a.stderr and ('missing-proof-base' if label=='invalid-base' else 'pathss').encode() in b.stderr
 for spelling in ['crates/directory-name','crates\\directory-name\\']:
  a=execute('pkg-hygiene-'+str(len(spelling)),[BINS['perl-ci-hygiene'],'resolve-package-name',spelling],fixture);b=execute('pkg-xtask-'+str(len(spelling)),[BINS['xtask'],'resolve-package-name',spelling],fixture);assert a.stdout==b.stdout==b'actual-package-name\n'
 for name in BINS:assert execute('unknown-pkg-'+name,[BINS[name],'resolve-package-name','crates/unknown'],fixture,1).stdout==b''
 (fixture/'Cargo.toml').rename(fixture/'Cargo.toml.saved')
 for name in BINS:assert execute('missing-metadata-'+name,[BINS[name],'resolve-package-name','crates/directory-name'],fixture,1).stdout==b''
 (fixture/'Cargo.toml.saved').rename(fixture/'Cargo.toml')
 # Legacy implicit root remains the compiled checkout; lightweight implicit root remains cwd.
 a=execute('xtask-default-root',[BINS['xtask'],'change-set','--base','1f973039d812fe080abddc44e461112d7b95a6fa','--head','HEAD','--format','paths'],fixture)
 b=execute('hygiene-explicit-compiled-root',[BINS['perl-ci-hygiene'],'change-set','--base','1f973039d812fe080abddc44e461112d7b95a6fa','--head','HEAD','--format','paths','--root',ROOT],fixture);assert a.stdout==b.stdout
 a=execute('hygiene-default-fixture-root',[BINS['perl-ci-hygiene'],'change-set','--base',base,'--head',head,'--format','paths'],fixture);assert a.stdout==b'crates/directory-name/src/lib.rs\ndelete.md\ndocs/notes.md\nrenamed.rs\n'
 # Isolate install side effects: compiled ROOT unchanged, independent Git metadata, no sibling trees.
 clone=P/'install-fixture-checkout';shutil.rmtree(clone,ignore_errors=True);execute('fixture-clone',['git','clone','--no-checkout','--shared','/workspace/perl-lsp-swarm',clone],P)
 execute('fixture-head',['git','-C',clone,'update-ref','refs/heads/proof-helper','42ea165fec26d75644d687090876eaa1c6dae4bc'],P);execute('fixture-symbolic-head',['git','-C',clone,'symbolic-ref','HEAD','refs/heads/proof-helper'],P)
 (ROOT/'.git').unlink();shutil.move(clone/'.git',ROOT/'.git');switched=True
 trees=git(ROOT,'worktree','list','--porcelain');assert trees.count('worktree ')==1 and 'worktree '+str(ROOT) in trees,trees
 assert git(ROOT,'rev-parse','HEAD')=='42ea165fec26d75644d687090876eaa1c6dae4bc'
 def budget(command):
  args=['run','--locked','-p','perl-ci-hygiene','--',command];pre=subprocess.run([str(ROOT/'scripts/cargo-admitted'),'--preflight',*args],cwd=ROOT,env=env,capture_output=True,text=True)
  prefix='cargo-admitted preflight: ';row=json.loads(next(l[len(prefix):] for l in pre.stderr.splitlines() if l.startswith(prefix)));path=P/(command+'-budget.json');path.write_text(json.dumps({'schema_version':1,'scope':row['scope'],'reserve_bytes':8*1024**3,'expected_growth_bytes':16*1024**3,'basis':'Same exact hygiene source as paired build; package-only admitted helper run for isolated install/currentness fixture.70-package normal/build graph; no product build, prior observed narrow outputs under1GiB final allocation not peak. Conservative16GiB aggregate growth8GiB reserve estimate, default40GiB unchanged.'},indent=2)+'\n');return path
 def hooks():
  return {f.name:{'sha256':hashlib.sha256(f.read_bytes()).hexdigest(),'mode':f.stat().st_mode,'mtime_ns':f.stat().st_mtime_ns} for f in (ROOT/'.githooks').iterdir()}
 assert not (ROOT/'.githooks').exists()
 execute('direct-check-missing',[BINS['perl-ci-hygiene'],'check-githooks'],ROOT,1)
 execute('xtask-check-missing',[BINS['xtask'],'ci-hygiene','check-githooks','--budget-file',budget('check-githooks')],fixture,1)
 execute('xtask-install',[BINS['xtask'],'ci-hygiene','install-githooks','--budget-file',budget('install-githooks')],fixture);compat={k:v['sha256'] for k,v in hooks().items()}
 assert (ROOT/'.githooks/pre-push').read_bytes()==(ROOT/'hooks/pre-push').read_bytes()+b'\n'
 before=hooks();execute('direct-check-current',[BINS['perl-ci-hygiene'],'check-githooks'],ROOT);execute('xtask-check-current',[BINS['xtask'],'ci-hygiene','check-githooks','--budget-file',budget('check-githooks')],fixture);assert hooks()==before
 hook=ROOT/'.githooks/pre-push';hook.write_bytes(hook.read_bytes()+b'\n# stale proof\n');before=hooks()
 execute('direct-check-stale',[BINS['perl-ci-hygiene'],'check-githooks'],ROOT,1);execute('xtask-check-stale',[BINS['xtask'],'ci-hygiene','check-githooks','--budget-file',budget('check-githooks')],fixture,1);assert hooks()==before
 execute('direct-install',[BINS['perl-ci-hygiene'],'install-githooks'],ROOT);assert {k:v['sha256'] for k,v in hooks().items()}==compat
 hook.chmod(0o644);before=hooks();execute('direct-check-nonexec',[BINS['perl-ci-hygiene'],'check-githooks'],ROOT,1);execute('xtask-check-nonexec',[BINS['xtask'],'ci-hygiene','check-githooks','--budget-file',budget('check-githooks')],fixture,1);assert hooks()==before
 execute('direct-reinstall',[BINS['perl-ci-hygiene'],'install-githooks'],ROOT);shutil.copytree(ROOT/'.githooks',P/'installed-fixture-hooks');(P/'installed-byte-comparison.json').write_text(json.dumps({'compatibility_install_sha256':compat,'direct_install_sha256':{k:v['sha256'] for k,v in hooks().items()},'all_checks_read_only':True,'fixture_worktree_list':trees},indent=2)+'\n')
finally:
 if switched:
  shutil.rmtree(ROOT/'.git');(ROOT/'.git').write_bytes(original)
 assert hashlib.sha256(shared_config.read_bytes()).hexdigest()==shared_hash,'Shared Git config must remain untouched'
 settlement=tree.finish();(P/'paired-cli-owner-settlement.json').write_text(json.dumps(settlement,indent=2)+'\n');assert settlement['tree_settled'],settlement
 (P/'paired-cli-records.json').write_text(json.dumps(records,indent=2)+'\n')
print('PAIRED CLI PROOF PASS',len(records),'actual invocations; shared/old metadata untouched; native Windows NOT_PROVEN')
