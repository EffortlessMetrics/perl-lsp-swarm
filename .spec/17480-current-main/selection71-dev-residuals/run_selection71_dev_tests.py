import hashlib,importlib.util,json,os,re,subprocess,time
from pathlib import Path
P=Path('/workspace/perl-helper-current-proof');R=Path('/workspace/perl-helper-current-main')
expected='71a69a477665e98dd11d1be163a4d08cea51ef64'
def git(*a):return subprocess.check_output(['git',*a],cwd=R,text=True).strip()
assert git('rev-parse','HEAD')==expected and not git('status','--porcelain')
art=[]
for line in (P/'selection71-dev-no-run.stdout').read_text().splitlines():
 try:d=json.loads(line)
 except ValueError:continue
 if d.get('reason')=='compiler-artifact':art.append(d)
assert any(json.loads(l).get('reason')=='build-finished' and json.loads(l)['success'] for l in (P/'selection71-dev-no-run.stdout').read_text().splitlines() if l.startswith('{'))
def pick(name,test):
 matches=[d for d in art if d['target']['name']==name and d['profile']['test']==test and d.get('executable')]
 assert len(matches)==1,(name,test,len(matches))
 return matches[0]
normal=pick('xtask',False)
tests={n:pick(n,True) for n in ['xtask','ci_subject','ci_scope_tests']}
admitted_line=next(l for l in (P/'selection71-dev-admitted-preflight.log').read_text().splitlines() if l.startswith('cargo-admitted preflight: '))
admitted=json.loads(admitted_line.split(': ',1)[1]);assert admitted['verdict']=='PASS'
scope=admitted['scope']
env=dict(os.environ,CARGO_NET_OFFLINE='true',RUSTUP_AUTO_INSTALL='0',CARGO_BIN_EXE_xtask=normal['executable'],CARGO_TARGET_DIR=scope['resources']['target'],CARGO_BUILD_BUILD_DIR=scope['resources']['build'],TMPDIR=scope['resources']['temp'],TEMP=scope['resources']['temp'],TMP=scope['resources']['temp'])
groups={
 'xtask':[
 'tasks::workflow_policy_lint::tests::'+n for n in ['shipped_workflows_enumerate_xtask_cli_wiring','runner_mismatch_skipped_for_stale_ref_in_multi_job_workflow','shipped_tree_has_no_undeclared_or_orphaned_self_hosted_capacity','shipped_profiles_are_current_today']
 ]+['tasks::gates::tests::'+n for n in ['plan_gates_nightly_tier_never_selects_a_commit_tier_gate','plan_gates_all_tier_selects_the_commit_tier_gate','plan_pr_fast_gates_falls_back_broadly_when_explicit_base_does_not_resolve','agent_receipt_builder_preserves_scope_status_and_plan_contract','static_gate_plan_threads_staged_tree_oid_into_agent_receipt','static_gate_plan_leaves_staged_tree_oid_none_when_not_staged']],
 'ci_subject':['ci_workflow_routes_scope_gate_contract_and_windows_cache_inputs_through_subject'],
 'ci_scope_tests':['test_ci_scope_auto_base_no_warning','test_ci_scope_diff_class_is_valid_value','test_ci_scope_each_lane_has_reason_field','test_ci_scope_help_shows_base_flag','test_ci_scope_help_shows_format_flag','test_ci_scope_invalid_explicit_base_fails_closed','test_ci_scope_empty_diff_has_no_selected_lanes','test_ci_scope_json_output_is_valid_schema_v2','test_ci_scope_text_output_is_readable']}
hashes={n:hashlib.sha256(Path(a['executable']).read_bytes()).hexdigest() for n,a in dict(tests,normal_xtask=normal).items()}
(P/'selection71-dev-bound-artifacts.json').write_text(json.dumps({'source':expected,'profile':'default cargo-test/dev','normal':normal,'tests':tests,'sha256':hashes,'selectors':groups,'fresh_artifact_packages':sorted(set(d['package_id'] for d in art if not d['fresh'])),'artifact_count':len(art)},indent=2)+'\n')
spec=importlib.util.spec_from_file_location('owner',R/'scripts/cargo_admitted.py');owner=importlib.util.module_from_spec(spec);spec.loader.exec_module(owner)
tree=owner.ClippyTree();records=[];error=None
try:
 probe=subprocess.run([normal['executable'],'ci-scope','--base','HEAD~1','--format','json'],cwd=R,env=env,capture_output=True)
 (P/'selection71-dev-reason-caller.stdout').write_bytes(probe.stdout);(P/'selection71-dev-reason-caller.stderr').write_bytes(probe.stderr)
 assert probe.returncode==0,probe.stderr
 scope_output=json.loads(probe.stdout);assert isinstance(scope_output.get('selected_lanes'),list)
 assert all(isinstance(lane.get('reason'),str) and lane['reason'] for lane in scope_output['selected_lanes'])
 (P/'selection71-dev-reason-caller.json').write_text(json.dumps({'exit':probe.returncode,'selected_lane_count':len(scope_output['selected_lanes']),'argv':[normal['executable'],'ci-scope','--base','HEAD~1','--format','json'],'cli_sha256':hashes['normal_xtask'],'source':expected},indent=2)+'\n')
 for name,names in groups.items():
  for selector in names:
   start=time.monotonic();run=subprocess.run([tests[name]['executable'],selector,'--exact','--test-threads=1'],cwd=R,env=env,capture_output=True)
   idx=len(records);label='selection71-dev-test-'+str(idx)
   (P/(label+'.stdout')).write_bytes(run.stdout);(P/(label+'.stderr')).write_bytes(run.stderr)
   summary=re.search(rb'test result: ok\. 1 passed; 0 failed;',run.stdout)
   record={'harness':name,'selector':selector,'exit':run.returncode,'one_test_passed':bool(summary),'seconds':time.monotonic()-start,'stdout':label+'.stdout','stderr':label+'.stderr'}
   records.append(record);print(name,selector,'PASS' if run.returncode==0 and summary else 'FAIL',flush=True)
   if run.returncode!=0 or not summary:raise AssertionError(record)
finally:
 settlement=tree.finish();(P/'selection71-dev-runtime-settlement.json').write_text(json.dumps(settlement,indent=2)+'\n')
 (P/'selection71-dev-runtime-results.json').write_text(json.dumps({'source':expected,'profile':'default cargo-test/dev','bound_cli':normal['executable'],'cwd':str(R),'records':records,'head_after':git('rev-parse','HEAD'),'dirty_after':git('status','--porcelain')},indent=2)+'\n')
 assert settlement['tree_settled'],settlement
assert len(records)==20 and git('rev-parse','HEAD')==expected and not git('status','--porcelain')
assert all(hashlib.sha256(Path(a['executable']).read_bytes()).hexdigest()==hashes[n] for n,a in dict(tests,normal_xtask=normal).items())
print('20/20 exact current-source executions PASS; original runtime owner settled',flush=True)
