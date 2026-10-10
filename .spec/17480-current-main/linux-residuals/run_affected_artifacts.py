import hashlib,importlib.util,json,os,subprocess,time
from pathlib import Path
P=Path('/workspace/perl-helper-current-proof');R=Path('/workspace/perl-helper-current-main');rows=[]
for line in (P/'affected-xtask-tests.stdout').read_text().splitlines():
 try:d=json.loads(line)
 except ValueError:continue
 if d.get('reason')=='compiler-artifact' and d.get('executable'):rows.append(d)
normal=next(Path(d['executable']) for d in rows if d['target']['name']=='xtask' and not d['profile']['test'])
assert hashlib.sha256(normal.read_bytes()).hexdigest()==hashlib.sha256((P/'immutable-xtask').read_bytes()).hexdigest(),'Compiled CLI changed; reconcile before tests'
spec=importlib.util.spec_from_file_location('owner',R/'scripts/cargo_admitted.py');owner=importlib.util.module_from_spec(spec);spec.loader.exec_module(owner);tree=owner.ClippyTree();records=[]
try:
 for d in rows:
  name=d['target']['name']
  if not d['profile']['test']:continue
  if name=='xtask':filters=['tasks::gates::','tasks::ci_scope::','tasks::workflow_policy_lint::','tasks::workflow_trigger_lint::','tasks::targeted_checks::']
  elif name in ['ci_subject','change_set_cli','ci_scope_tests','workspace_doctor_inventory_contract','workspace_doctor_inventory_falsifiers']:filters=['']
  else:continue
  executable=Path(d['executable']);dest=P/('immutable-test-'+name);dest.write_bytes(executable.read_bytes());dest.chmod(0o700)
  for i,f in enumerate(filters):
   start=time.monotonic();run=subprocess.run([str(dest),f,'--test-threads=2'],cwd=R,env=dict(os.environ,CARGO_BIN_EXE_xtask=str(normal)),capture_output=True)
   label='affected-'+name+'-'+str(i);(P/(label+'.stdout')).write_bytes(run.stdout);(P/(label+'.stderr')).write_bytes(run.stderr);records.append({'name':name,'filter':f,'exit':run.returncode,'seconds':time.monotonic()-start,'test_binary_sha256':hashlib.sha256(dest.read_bytes()).hexdigest(),'cli_bound_path':str(normal),'cli_sha256':hashlib.sha256(normal.read_bytes()).hexdigest()});assert run.returncode==0,(name,f,run.stdout[-3000:],run.stderr[-3000:])
finally:
 settlement=tree.finish();(P/'affected-artifact-owner-settlement.json').write_text(json.dumps(settlement,indent=2)+'\n');(P/'affected-artifact-executions.json').write_text(json.dumps(records,indent=2)+'\n');assert settlement['tree_settled']
print('Affected artifact execution PASS',len(records))
